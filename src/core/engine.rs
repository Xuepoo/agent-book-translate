//! Tokio-driven translation workflow controller.

use crate::agent::client::TranslationClient;
use crate::agent::prompt::PromptContext;
use crate::config::AppConfig;
use crate::core::parser::{extract_text_chunks, parse_epub, write_epub};
use crate::core::progress::{NoopProgressReporter, ProgressEvent, ProgressReporter};
use crate::error::{AppError, Result};
use scraper::Html;
use std::collections::HashMap;
use std::path::Path;

pub async fn run(input: &Path, output: &Path, config: &AppConfig) -> Result<()> {
    let reporter = NoopProgressReporter;
    run_with_progress(input, output, config, &reporter).await
}

pub async fn run_with_progress(
    input: &Path,
    output: &Path,
    config: &AppConfig,
    reporter: &dyn ProgressReporter,
) -> Result<()> {
    let source_files = parse_epub(input)?;
    let client = TranslationClient::new(config.clone());
    let mut rendered_files = HashMap::new();
    let text_entries = source_files
        .iter()
        .filter(|entry| entry.is_text)
        .collect::<Vec<_>>();
    let total_chunks = text_entries
        .iter()
        .map(|entry| {
            let raw_html = String::from_utf8_lossy(&entry.data).to_string();
            let document = Html::parse_document(&raw_html);
            extract_text_chunks(&document).len()
        })
        .sum();

    reporter.on_event(ProgressEvent::Started {
        total_text_files: text_entries.len(),
        total_chunks,
    });

    for entry in text_entries {
        reporter.on_event(ProgressEvent::FileStarted {
            file_name: entry.name.clone(),
        });
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
            match client.translate_with_stats(&ctx).await {
                Ok(result) => {
                    reporter.on_event(ProgressEvent::RequestFinished {
                        usage: result.usage,
                        retries: result.retries,
                    });
                    translated = translated.replace(&chunk, &result.translation);
                    reporter.on_event(ProgressEvent::ChunkFinished);
                }
                Err(error) => {
                    reporter.on_event(ProgressEvent::ChunkFailed {
                        error: error.to_string(),
                    });
                    reporter.on_event(ProgressEvent::Failed {
                        error: error.to_string(),
                    });
                    return Err(error);
                }
            }
        }

        rendered_files.insert(entry.name.clone(), translated);
        reporter.on_event(ProgressEvent::FileFinished);
    }

    match write_epub(output, &source_files, &rendered_files) {
        Ok(()) => {
            reporter.on_event(ProgressEvent::Completed);
            Ok(())
        }
        Err(error) => {
            reporter.on_event(ProgressEvent::Failed {
                error: error.to_string(),
            });
            Err(error)
        }
    }
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
