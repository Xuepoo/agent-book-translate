//! Async OpenAI-compatible client with rate limiting and retry.

use crate::agent::json_healer::heal_and_parse_json;
use crate::agent::prompt::{CritiqueReport, PromptContext, build_translation_prompt};
use crate::config::AppConfig;
use crate::core::progress::TokenUsage;
use crate::error::{AppError, Result};
use reqwest::StatusCode;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::time::sleep;

#[derive(Clone)]
pub struct TranslationClient {
    http: reqwest::Client,
    config: AppConfig,
    semaphore: Arc<Semaphore>,
}

impl TranslationClient {
    pub fn new(config: AppConfig) -> Self {
        let mut builder = reqwest::Client::builder();
        if let Some(proxy) = config.http_proxy.as_deref()
            && let Ok(proxy) = reqwest::Proxy::all(proxy)
        {
            builder = builder.proxy(proxy);
        }
        Self {
            http: builder.build().unwrap_or_else(|_| reqwest::Client::new()),
            semaphore: Arc::new(Semaphore::new(config.concurrency.max(1))),
            config,
        }
    }

    pub async fn translate(&self, ctx: &PromptContext) -> Result<String> {
        self.translate_with_stats(ctx)
            .await
            .map(|result| result.translation)
    }

    pub async fn translate_with_stats(&self, ctx: &PromptContext) -> Result<TranslationResult> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| AppError::Translation("semaphore closed".to_string()))?;

        let body = serde_json::json!({
            "model": self.config.default_model,
            "messages": [
                {"role": "system", "content": build_translation_prompt(ctx)},
                {"role": "user", "content": ctx.target}
            ],
            "response_format": {"type": "json_object"}
        });

        let mut delay = Duration::from_secs(1);
        let max_parse_attempts = 3usize;
        let mut total_retries = 0;

        for attempt in 1..=max_parse_attempts {
            match self.post_with_retry(body.clone()).await {
                Ok(response) => {
                    total_retries += response.retries;
                    let content = extract_content(&response.text).unwrap_or(response.text.clone());
                    let usage = TokenUsage::from_response(&response.text).unwrap_or_default();

                    if let Ok(translation) = parse_translation_content(&content) {
                        // Check for leaked JSON wrappers inside the parsed string as secondary validation
                        let leaks = [
                            "{\"translation\"",
                            "{\"role\"",
                            "refined_translation",
                            "incorrect_terms",
                        ];
                        let has_leak = leaks.iter().any(|pattern| translation.contains(pattern));
                        if !has_leak {
                            return Ok(TranslationResult {
                                translation,
                                usage,
                                retries: total_retries + (attempt - 1) as u64,
                            });
                        }
                    }
                }
                Err(e) => {
                    return Err(e);
                }
            }

            if attempt < max_parse_attempts {
                sleep(delay).await;
                delay *= 2;
            }
        }

        Err(AppError::Translation(
            "translation parsing and validation exhausted".to_string(),
        ))
    }

    async fn post_with_retry(&self, body: serde_json::Value) -> Result<TranslationResponse> {
        let mut delay = Duration::from_secs(1);
        let max_attempts = 5usize;
        for (retry_count, attempt) in (1..=max_attempts).enumerate() {
            let request = self
                .http
                .post(format!("{}/chat/completions", self.config.base_url))
                .bearer_auth(&self.config.api_key)
                .json(&body);

            match request.send().await {
                Ok(resp) if resp.status().is_success() => {
                    let text = resp.text().await.map_err(AppError::from)?;
                    return Ok(TranslationResponse {
                        text,
                        retries: retry_count as u64,
                    });
                }
                Ok(resp)
                    if resp.status().is_client_error()
                        && resp.status() != StatusCode::TOO_MANY_REQUESTS =>
                {
                    return Err(AppError::Translation(format!(
                        "client error status: {}",
                        resp.status()
                    )));
                }
                Ok(resp) if resp.status() == StatusCode::TOO_MANY_REQUESTS => {
                    if attempt == max_attempts {
                        return Err(AppError::Translation(
                            "rate limited after retries".to_string(),
                        ));
                    }
                }
                Ok(resp) => {
                    if attempt == max_attempts {
                        return Err(AppError::Translation(format!(
                            "request failed with status {}",
                            resp.status()
                        )));
                    }
                }
                Err(err) => {
                    if attempt == max_attempts {
                        return Err(AppError::Http(err));
                    }
                }
            }

            sleep(delay).await;
            delay *= 2;
        }

        Err(AppError::Translation("retry exhausted".to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationResult {
    pub translation: String,
    pub usage: TokenUsage,
    pub retries: u64,
}

struct TranslationResponse {
    text: String,
    retries: u64,
}

pub fn parse_critique(raw: &str) -> Result<CritiqueReport> {
    heal_and_parse_json(raw)
}

pub fn parse_translation_content(raw: &str) -> Result<String> {
    if let Ok(report) = heal_and_parse_json(raw) {
        return Ok(report.refined_translation);
    }

    let trimmed = strip_markdown_fence(raw);
    let value = match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(value) => value,
        Err(_) => {
            return Ok(
                unwrap_malformed_translation_object(trimmed).unwrap_or_else(|| trimmed.to_string())
            );
        }
    };

    translation_from_json_value(&value)
}

pub fn extract_content(raw: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    value
        .get("choices")?
        .get(0)?
        .get("message")?
        .get("content")?
        .as_str()
        .map(|s| s.to_string())
}

fn translation_from_json_value(value: &serde_json::Value) -> Result<String> {
    if let Some(text) = value.as_str() {
        return Ok(text.to_string());
    }

    if let Some(array) = value.as_array() {
        if !array.is_empty() {
            if let Some(text) = array[0].as_str() {
                return Ok(text.to_string());
            }
            if let Some(candidate) = array.iter().find_map(pick_translation_candidate_nested) {
                return Ok(candidate);
            }
        }
    }

    if let Some(object) = value.as_object() {
        if let Some(candidate) = pick_translation_candidate(object) {
            return Ok(candidate);
        } else {
            return Err(AppError::Translation(
                "translation response JSON object does not contain expected translation fields"
                    .to_string(),
            ));
        }
    }

    if let Some(candidate) = pick_translation_candidate_nested(value) {
        return Ok(candidate);
    }

    if !value.is_null() && !value.is_object() && !value.is_array() {
        return Ok(value.to_string());
    }

    Err(AppError::Translation(
        "translation response JSON is empty or invalid format".to_string(),
    ))
}

fn strip_markdown_fence(raw: &str) -> &str {
    let trimmed = raw.trim();
    if let Some(without_open) = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
    {
        without_open
            .strip_suffix("```")
            .unwrap_or(without_open)
            .trim()
    } else {
        trimmed
    }
}

fn unwrap_malformed_translation_object(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let inner = raw.strip_prefix('{')?.strip_suffix('}')?.trim();
    for key in ["translation", "refined_translation", "translated_text"] {
        let Some(value) = strip_malformed_field(inner, key) else {
            continue;
        };
        return Some(value.to_string());
    }
    None
}

fn strip_malformed_field<'a>(field: &'a str, key: &str) -> Option<&'a str> {
    let remainder = field
        .strip_prefix('"')?
        .strip_prefix(key)?
        .strip_prefix('"')?
        .trim_start()
        .strip_prefix(':')?
        .trim_start();

    for (open, close) in [('"', '"'), ('“', '”')] {
        if let Some(value) = remainder
            .strip_prefix(open)
            .and_then(|value| value.strip_suffix(close))
        {
            return Some(value);
        }
    }

    None
}

fn pick_translation_candidate(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Option<String> {
    let mut fallback = Vec::new();

    for (key, value) in object {
        if let Some(candidate) = value.as_str() {
            match key.as_str() {
                "refined_translation" | "translation" | "translated_text" => {
                    return Some(candidate.to_string());
                }
                "message" | "content" | "text" | "result" => {
                    fallback.push((1usize, candidate.to_string()));
                }
                "role" | "status" | "ai_persona" | "intent_detected" => {}
                _ => {
                    fallback.push((2usize, candidate.to_string()));
                }
            }
            continue;
        }

        if let Some(candidate) = pick_translation_candidate_nested(value) {
            fallback.push((3usize, candidate));
        }
    }

    fallback
        .into_iter()
        .min_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| right.1.len().cmp(&left.1.len()))
        })
        .map(|(_, candidate)| candidate)
}

fn pick_translation_candidate_nested(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.to_string()),
        serde_json::Value::Array(items) => items.iter().find_map(pick_translation_candidate_nested),
        serde_json::Value::Object(map) => pick_translation_candidate(map),
        _ => None,
    }
}
