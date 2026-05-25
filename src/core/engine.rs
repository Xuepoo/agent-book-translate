//! Tokio-driven translation workflow controller.

use crate::agent::client::TranslationClient;
use crate::agent::prompt::PromptContext;
use crate::config::AppConfig;
use crate::core::parser::{
    RenderedChunk, extract_text_chunks, parse_epub, render_file_from_chunks, write_epub,
};
use crate::core::progress::{NoopProgressReporter, ProgressEvent, ProgressReporter};
use crate::db::checkpoint::{completed_chunk_map, open_checkpoint_db, upsert_chunk_progress};
use crate::error::{AppError, Result};
use crate::job::{JobStatus, JobStore};
use scraper::Html;
use std::collections::HashMap;
use std::path::Path;

pub async fn run(input: &Path, output: &Path, config: &AppConfig) -> Result<()> {
    let reporter = NoopProgressReporter;
    run_with_progress_and_control(input, output, config, &reporter, None).await
}

pub async fn run_with_progress(
    input: &Path,
    output: &Path,
    config: &AppConfig,
    reporter: &dyn ProgressReporter,
) -> Result<()> {
    run_with_progress_and_control(input, output, config, reporter, None).await
}

#[derive(Debug, Clone)]
pub struct JobControl {
    pub store: JobStore,
    pub job_id: String,
}

pub async fn run_with_progress_and_control(
    input: &Path,
    output: &Path,
    config: &AppConfig,
    reporter: &dyn ProgressReporter,
    job_control: Option<JobControl>,
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

    let mut checkpoint_conn = None;
    let mut completed_map = HashMap::new();
    let mut completed_chunks = 0usize;
    if let Some(control) = job_control.as_ref() {
        control.store.ensure_checkpoint_dir()?;
        let state = control.store.load(&control.job_id)?;
        completed_chunks = state.metrics.completed_chunks;
        let checkpoint_path = control.store.checkpoint_path(&control.job_id);
        let conn = open_checkpoint_db(&checkpoint_path)?;
        completed_map = completed_chunk_map(&conn)?;
        checkpoint_conn = Some(conn);
    }

    reporter.on_event(ProgressEvent::Started {
        total_text_files: text_entries.len(),
        total_chunks,
        completed_chunks,
    });

    for entry in text_entries {
        reporter.on_event(ProgressEvent::FileStarted {
            file_name: entry.name.clone(),
        });
        let raw_html = String::from_utf8_lossy(&entry.data).to_string();
        let document = Html::parse_document(&raw_html);
        let chunks = extract_text_chunks(&document)
            .into_iter()
            .map(|chunk| chunk.with_source_path(entry.name.clone()))
            .collect::<Vec<_>>();
        let mut rendered_chunks = Vec::new();

        for (chunk_index, chunk) in chunks.iter().enumerate() {
            if let Some(control) = job_control.as_ref()
                && is_pause_requested(&control.store, &control.job_id)?
            {
                mark_paused(&control.store, &control.job_id)?;
                reporter.on_event(ProgressEvent::Paused);
                return Ok(());
            }

            let checkpoint_key = (entry.name.clone(), chunk_index as i64);
            if let Some(existing) = completed_map.get(&checkpoint_key) {
                rendered_chunks.push(RenderedChunk {
                    file_name: entry.name.clone(),
                    chunk_index,
                    original: chunk.text.clone(),
                    translated: existing.translated_text.clone().unwrap_or_default(),
                });
                continue;
            }

            if let Some(conn) = checkpoint_conn.as_ref() {
                upsert_chunk_progress(
                    conn,
                    &entry.name,
                    chunk_index as i64,
                    &chunk.text,
                    None,
                    "processing",
                )?;
            }

            let ctx = PromptContext {
                book_summary: String::new(),
                pov_speaker: String::new(),
                glossary: Vec::new(),
                previous_context: String::new(),
                target: chunk.text.clone(),
                next_context: String::new(),
            };
            match client.translate_with_stats(&ctx).await {
                Ok(result) => {
                    reporter.on_event(ProgressEvent::RequestFinished {
                        usage: result.usage,
                        retries: result.retries,
                    });
                    if let Some(conn) = checkpoint_conn.as_ref() {
                        upsert_chunk_progress(
                            conn,
                            &entry.name,
                            chunk_index as i64,
                            &chunk.text,
                            Some(&result.translation),
                            "completed",
                        )?;
                    }
                    completed_map.insert(
                        checkpoint_key,
                        crate::db::checkpoint::ChunkProgress {
                            chapter_id: entry.name.clone(),
                            chunk_index: chunk_index as i64,
                            original_text: chunk.text.clone(),
                            translated_text: Some(result.translation.clone()),
                            state: "completed".to_string(),
                        },
                    );
                    rendered_chunks.push(RenderedChunk {
                        file_name: entry.name.clone(),
                        chunk_index,
                        original: chunk.text.clone(),
                        translated: result.translation,
                    });
                    reporter.on_event(ProgressEvent::ChunkFinished);
                }
                Err(error) => {
                    if let Some(conn) = checkpoint_conn.as_ref() {
                        upsert_chunk_progress(
                            conn,
                            &entry.name,
                            chunk_index as i64,
                            &chunk.text,
                            None,
                            "pending",
                        )?;
                    }
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

        let rendered = render_file_from_chunks(&raw_html, &rendered_chunks);
        rendered_files.insert(entry.name.clone(), rendered);
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

fn is_pause_requested(store: &JobStore, job_id: &str) -> Result<bool> {
    let state = store.load(job_id)?;
    Ok(matches!(state.status, JobStatus::Pausing))
}

fn mark_paused(store: &JobStore, job_id: &str) -> Result<()> {
    let mut state = store.load(job_id)?;
    state.status = JobStatus::Paused;
    store.save(&state)?;
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
