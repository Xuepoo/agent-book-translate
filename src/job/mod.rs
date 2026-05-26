//! Job state persistence for foreground and background translation runs.

pub mod control;

use crate::error::{AppError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Running,
    Pausing,
    Paused,
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

/// After this many seconds without a heartbeat update, a Running job is
/// considered stale (process died or machine was shut down).
pub const STALE_THRESHOLD_SECS: i64 = 120;

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
    /// Periodically updated by the running translation process. Used to detect
    /// stale Running jobs after a crash or power loss.
    #[serde(default)]
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub pid: Option<u32>,
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
            last_heartbeat_at: None,
            pid: None,
            metrics: JobMetrics::default(),
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }

    pub fn update_heartbeat(&mut self) {
        self.last_heartbeat_at = Some(Utc::now());
    }

    /// Returns true when the job status is Running but the process is no longer
    /// alive. Leverages PID /proc checking on Linux, with heartbeat fallback.
    pub fn is_stale_running(&self) -> bool {
        if self.status != JobStatus::Running {
            return false;
        }

        // 1. PID-based check (highly reliable on Linux)
        if let Some(pid) = self.pid {
            let pid_path = format!("/proc/{}", pid);
            if !std::path::Path::new(&pid_path).exists() {
                // Recorded process is definitely dead
                return true;
            }
            // If the pid directory exists, it's alive, so NOT stale.
            return false;
        }

        // 2. Heartbeat-based fallback
        match self.last_heartbeat_at {
            Some(ts) => {
                let age = (Utc::now() - ts).num_seconds();
                age > STALE_THRESHOLD_SECS
            }
            None => true,
        }
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

    pub fn checkpoints_dir(&self) -> PathBuf {
        self.root.join("checkpoints")
    }

    pub fn log_path(&self, job_id: &str) -> PathBuf {
        self.logs_dir().join(format!("{job_id}.log"))
    }

    pub fn checkpoint_path(&self, job_id: &str) -> PathBuf {
        self.checkpoints_dir().join(format!("{job_id}.sqlite3"))
    }

    pub fn path_for(&self, job_id: &str) -> PathBuf {
        self.jobs_dir().join(format!("{job_id}.json"))
    }

    pub fn save(&self, state: &JobState) -> Result<()> {
        fs::create_dir_all(self.jobs_dir())?;
        let mut state = state.clone();
        state.touch();
        let raw = serde_json::to_vec_pretty(&state)?;
        write_atomic(&self.path_for(&state.job_id), &raw)
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

    pub fn ensure_checkpoint_dir(&self) -> Result<()> {
        fs::create_dir_all(self.checkpoints_dir())?;
        Ok(())
    }
}

fn read_state_file(path: &Path) -> Result<JobState> {
    let raw = fs::read_to_string(path)?;
    serde_json::from_str(&raw).map_err(AppError::from)
}

fn write_atomic(path: &Path, raw: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Config(format!("invalid job state path: {}", path.display())))?;
    let mut tmp = NamedTempFile::new_in(parent)?;
    tmp.write_all(raw)?;
    tmp.flush()?;
    tmp.persist(path)
        .map(|_| ())
        .map_err(|err| err.error.into())
}
