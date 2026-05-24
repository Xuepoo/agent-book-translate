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

        let response = self.post_with_retry(body).await?;
        let content = extract_content(&response.text).unwrap_or(response.text.clone());
        let translation = match heal_and_parse_json(&content) {
            Ok(report) => report.refined_translation,
            Err(_) => content,
        };
        Ok(TranslationResult {
            translation,
            usage: TokenUsage::from_response(&response.text).unwrap_or_default(),
            retries: response.retries,
        })
    }

    async fn post_with_retry(&self, body: serde_json::Value) -> Result<TranslationResponse> {
        let mut delay = Duration::from_secs(1);
        let max_attempts = 5usize;
        let mut retries = 0u64;

        for attempt in 1..=max_attempts {
            let request = self
                .http
                .post(format!("{}/chat/completions", self.config.base_url))
                .bearer_auth(&self.config.api_key)
                .json(&body);

            match request.send().await {
                Ok(resp) if resp.status().is_success() => {
                    let text = resp.text().await.map_err(AppError::from)?;
                    return Ok(TranslationResponse { text, retries });
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

            retries += 1;
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
