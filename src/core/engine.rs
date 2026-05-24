//! Tokio-driven translation workflow controller.

use crate::agent::client::TranslationClient;
use crate::agent::prompt::PromptContext;
use crate::config::AppConfig;
use crate::core::parser::{extract_text_chunks, parse_epub, write_epub};
use crate::error::{AppError, Result};
use scraper::Html;
use std::collections::HashMap;
use std::path::Path;

pub async fn run(input: &Path, output: &Path, config: &AppConfig) -> Result<()> {
    let source_files = parse_epub(input)?;
    let client = TranslationClient::new(config.clone());
    let mut rendered_files = HashMap::new();

    for entry in source_files.iter().filter(|entry| entry.is_text) {
        let raw_html = String::from_utf8_lossy(&entry.data).to_string();
        let document = Html::parse_document(&raw_html);
        let chunks = extract_text_chunks(&document);
        let mut translated = raw_html.clone();

        for chunk in chunks {
            let ctx = PromptContext {
                book_summary: String::new(),
                pov_speaker: String::new(),
                glossary: Vec::new(),
                previous_context: String::new(),
                target: chunk.clone(),
                next_context: String::new(),
            };
            let translated_chunk = client.translate(&ctx).await?;
            translated = translated.replace(&chunk, &translated_chunk);
        }

        rendered_files.insert(entry.name.clone(), translated);
    }

    write_epub(output, &source_files, &rendered_files)?;
    Ok(())
}

pub fn validate_epub_input(input: &Path) -> Result<()> {
    if input
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("epub"))
        .unwrap_or(false)
    {
        Ok(())
    } else {
        Err(AppError::UnsupportedFormat(format!(
            "only EPUB input is supported in v0.1.0: {}",
            input.display()
        )))
    }
}
