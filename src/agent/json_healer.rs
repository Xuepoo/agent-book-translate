//! Fault-tolerant JSON repair & fallback for LLM responses.

use crate::agent::prompt::CritiqueReport;
use crate::error::{AppError, Result};
use regex::Regex;

pub fn heal_and_parse_json(raw: &str) -> Result<CritiqueReport> {
    let cleaned = strip_markdown(raw).trim().to_string();
    parse_candidate(&cleaned)
        .or_else(|_| {
            extract_json_object(&cleaned).and_then(|candidate| parse_candidate(&candidate))
        })
        .or_else(|_| {
            let fixed = balance_braces(&cleaned);
            parse_candidate(&fixed)
        })
}

fn parse_candidate(candidate: &str) -> Result<CritiqueReport> {
    serde_json::from_str(candidate).map_err(AppError::from)
}

fn strip_markdown(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("```") {
        let without_open = trimmed
            .strip_prefix("```json")
            .or_else(|| trimmed.strip_prefix("```"))
            .unwrap_or(trimmed);
        without_open
            .strip_suffix("```")
            .unwrap_or(without_open)
            .trim()
            .to_string()
    } else {
        trimmed.to_string()
    }
}

fn extract_json_object(raw: &str) -> Result<String> {
    let re = Regex::new(r"\{[^{}]*\}").map_err(|e| AppError::Parse(e.to_string()))?;
    re.find(raw)
        .map(|m| m.as_str().to_string())
        .ok_or_else(|| AppError::Parse("no JSON object found".to_string()))
}

fn balance_braces(raw: &str) -> String {
    let mut result = raw.to_string();
    let open = result.chars().filter(|c| *c == '{').count();
    let close = result.chars().filter(|c| *c == '}').count();
    for _ in 0..open.saturating_sub(close) {
        result.push('}');
    }
    result
}
