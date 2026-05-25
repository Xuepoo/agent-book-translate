//! Progress reporting and usage accounting for translation runs.

use crate::job::{JobState, JobStatus, JobStore};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

impl TokenUsage {
    pub fn from_response(raw: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(raw).ok()?;
        let usage = value.get("usage")?;
        Some(Self {
            prompt_tokens: usage
                .get("prompt_tokens")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            completion_tokens: usage
                .get("completion_tokens")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            total_tokens: usage
                .get("total_tokens")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressEvent {
    Started {
        total_text_files: usize,
        total_chunks: usize,
        completed_chunks: usize,
        completed_text_files: usize,
    },
    FileStarted {
        file_name: String,
    },
    FileFinished,
    RequestFinished {
        usage: TokenUsage,
        retries: u64,
    },
    ChunkFinished,
    ChunkFailed {
        error: String,
    },
    Completed,
    Paused,
    Failed {
        error: String,
    },
}

pub trait ProgressReporter {
    fn on_event(&self, event: ProgressEvent);
}

#[derive(Debug, Default)]
pub struct NoopProgressReporter;

impl ProgressReporter for NoopProgressReporter {
    fn on_event(&self, _event: ProgressEvent) {}
}

#[derive(Debug, Default)]
pub struct TerminalProgressReporter {
    bar: Mutex<Option<ProgressBar>>,
}

impl TerminalProgressReporter {
    pub fn new() -> Self {
        Self::default()
    }

    fn with_bar(&self, f: impl FnOnce(&ProgressBar)) {
        if let Ok(guard) = self.bar.lock()
            && let Some(bar) = guard.as_ref()
        {
            f(bar);
        }
    }
}

impl ProgressReporter for TerminalProgressReporter {
    fn on_event(&self, event: ProgressEvent) {
        match event {
            ProgressEvent::Started {
                total_chunks,
                completed_chunks,
                ..
            } => {
                let bar = ProgressBar::new(total_chunks as u64);
                let style = ProgressStyle::with_template(
                    "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} chunks {msg}",
                )
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("=>-");
                bar.set_style(style);
                bar.set_position(completed_chunks as u64);
                if let Ok(mut guard) = self.bar.lock() {
                    *guard = Some(bar);
                }
            }
            ProgressEvent::FileStarted { file_name } => {
                self.with_bar(|bar| bar.set_message(file_name));
            }
            ProgressEvent::RequestFinished { usage, retries } => {
                self.with_bar(|bar| {
                    bar.set_message(format!(
                        "tokens={} prompt={} completion={} retries={}",
                        usage.total_tokens, usage.prompt_tokens, usage.completion_tokens, retries
                    ));
                });
            }
            ProgressEvent::ChunkFinished => {
                self.with_bar(|bar| bar.inc(1));
            }
            ProgressEvent::Completed => {
                self.with_bar(|bar| bar.finish_with_message("completed"));
            }
            ProgressEvent::Paused => {
                self.with_bar(|bar| bar.finish_with_message("paused"));
            }
            ProgressEvent::Failed { error } | ProgressEvent::ChunkFailed { error } => {
                self.with_bar(|bar| bar.abandon_with_message(format!("failed: {error}")));
            }
            ProgressEvent::FileFinished => {}
        }
    }
}

#[derive(Debug, Clone)]
pub struct JobProgressReporter {
    store: JobStore,
    job_id: String,
}

impl JobProgressReporter {
    pub fn new(store: JobStore, job_id: String) -> Self {
        Self { store, job_id }
    }

    fn update_state(&self, update: impl FnOnce(&mut JobState)) {
        let Ok(mut state) = self.store.load(&self.job_id) else {
            return;
        };
        update(&mut state);
        let _ = self.store.save(&state);
    }
}

impl ProgressReporter for JobProgressReporter {
    fn on_event(&self, event: ProgressEvent) {
        self.update_state(|state| match event {
            ProgressEvent::Started {
                total_text_files,
                total_chunks,
                completed_chunks,
                completed_text_files,
            } => {
                state.status = JobStatus::Running;
                state.metrics.total_text_files = total_text_files;
                state.metrics.total_chunks = total_chunks;
                state.metrics.completed_chunks = completed_chunks;
                state.metrics.completed_text_files = completed_text_files;
            }
            ProgressEvent::FileStarted { file_name } => {
                state.current_file = Some(file_name);
            }
            ProgressEvent::FileFinished => {
                state.metrics.completed_text_files += 1;
            }
            ProgressEvent::RequestFinished { usage, retries } => {
                state.metrics.request_count += 1;
                state.metrics.retry_count += retries;
                state.metrics.prompt_tokens += usage.prompt_tokens;
                state.metrics.completion_tokens += usage.completion_tokens;
                state.metrics.total_tokens += usage.total_tokens;
            }
            ProgressEvent::ChunkFinished => {
                state.metrics.completed_chunks += 1;
            }
            ProgressEvent::ChunkFailed { error } => {
                state.metrics.failed_chunks += 1;
                state.last_error = Some(error);
            }
            ProgressEvent::Completed => {
                state.status = JobStatus::Completed;
            }
            ProgressEvent::Paused => {
                state.status = JobStatus::Paused;
            }
            ProgressEvent::Failed { error } => {
                state.status = JobStatus::Failed;
                state.last_error = Some(error);
            }
        });
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemoryProgressReporter {
    events: Rc<RefCell<Vec<ProgressEvent>>>,
}

impl MemoryProgressReporter {
    pub fn events(&self) -> Vec<ProgressEvent> {
        self.events.borrow().clone()
    }
}

impl ProgressReporter for MemoryProgressReporter {
    fn on_event(&self, event: ProgressEvent) {
        self.events.borrow_mut().push(event);
    }
}
