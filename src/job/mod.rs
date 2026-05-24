//! Job state persistence for foreground and background translation runs.

use crate::error::{AppError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobMetrics {
    pub total_text_files: usize,
    pub completed_text_files: usize,
    pub total_chunks: usize,
    pub completed_chunks: usize,
    pub failed_chunks: usize,
    pub request_count: u64,
    pub retry_count: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobState {
    pub job_id: String,
    pub status: JobStatus,
    pub input: PathBuf,
    pub output: PathBuf,
    pub current_file: Option<String>,
    pub last_error: Option<String>,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metrics: JobMetrics,
}

impl JobState {
    pub fn new(job_id: String, input: PathBuf, output: PathBuf) -> Self {
        let now = Utc::now();
        Self {
            job_id,
            status: JobStatus::Pending,
            input,
            output,
            current_file: None,
            last_error: None,
            started_at: now,
            updated_at: now,
            metrics: JobMetrics::default(),
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }

    pub fn elapsed_seconds(&self) -> i64 {
        (self.updated_at - self.started_at).num_seconds().max(0)
    }
}

#[derive(Debug, Clone)]
pub struct JobStore {
    root: PathBuf,
}

impl JobStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn xdg() -> Result<Self> {
        let root = dirs::state_dir()
            .ok_or_else(|| AppError::Config("could not resolve XDG state directory".to_string()))?
            .join("agent-book-translate");
        Ok(Self::new(root))
    }

    pub fn jobs_dir(&self) -> PathBuf {
        self.root.join("jobs")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn log_path(&self, job_id: &str) -> PathBuf {
        self.logs_dir().join(format!("{job_id}.log"))
    }

    pub fn path_for(&self, job_id: &str) -> PathBuf {
        self.jobs_dir().join(format!("{job_id}.json"))
    }

    pub fn save(&self, state: &JobState) -> Result<()> {
        fs::create_dir_all(self.jobs_dir())?;
        let mut state = state.clone();
        state.touch();
        let raw = serde_json::to_vec_pretty(&state)?;
        fs::write(self.path_for(&state.job_id), raw)?;
        Ok(())
    }

    pub fn load(&self, job_id: &str) -> Result<JobState> {
        let path = self.path_for(job_id);
        if !path.exists() {
            return Err(AppError::Config(format!("job not found: {job_id}")));
        }
        let raw = fs::read_to_string(path)?;
        serde_json::from_str(&raw).map_err(AppError::from)
    }

    pub fn list(&self) -> Result<Vec<JobState>> {
        let jobs_dir = self.jobs_dir();
        if !jobs_dir.exists() {
            return Ok(Vec::new());
        }

        let mut states = Vec::new();
        for entry in fs::read_dir(jobs_dir)? {
            let entry = entry?;
            if entry.path().extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            states.push(read_state_file(&entry.path())?);
        }
        states.sort_by_key(|state| Reverse(state.updated_at));
        Ok(states)
    }

    pub fn ensure_log_dir(&self) -> Result<()> {
        fs::create_dir_all(self.logs_dir())?;
        Ok(())
    }
}

fn read_state_file(path: &Path) -> Result<JobState> {
    let raw = fs::read_to_string(path)?;
    serde_json::from_str(&raw).map_err(AppError::from)
}
