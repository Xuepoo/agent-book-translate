use crate::error::{AppError, Result};

use super::{JobState, JobStatus, JobStore};

pub fn request_pause(store: &JobStore, job_id: &str) -> Result<JobState> {
    let mut state = store.load(job_id)?;
    match state.status {
        JobStatus::Completed => {
            return Err(AppError::Config(format!("job already completed: {job_id}")));
        }
        JobStatus::Running => {
            state.status = JobStatus::Pausing;
            state.last_error = Some("pause requested by user".to_string());
        }
        JobStatus::Pending => {
            state.status = JobStatus::Paused;
            state.last_error = Some("pause requested by user".to_string());
        }
        JobStatus::Pausing | JobStatus::Paused => {}
        JobStatus::Failed => {
            return Err(AppError::Config(format!(
                "job is failed; resume it instead: {job_id}"
            )));
        }
    }
    store.save(&state)?;
    Ok(state)
}

pub fn request_resume(store: &JobStore, job_id: &str) -> Result<JobState> {
    request_resume_inner(store, job_id, false)
}

/// Resume a job, optionally bypassing the stale-running guard.
///
/// When `force` is true the caller accepts responsibility for verifying that no
/// other process is still running the job. The function will still reject a
/// non-stale Running job when `force` is false.
pub fn request_resume_force(store: &JobStore, job_id: &str) -> Result<JobState> {
    request_resume_inner(store, job_id, true)
}

fn request_resume_inner(store: &JobStore, job_id: &str, force: bool) -> Result<JobState> {
    let mut state = store.load(job_id)?;
    match state.status {
        JobStatus::Completed => {
            return Err(AppError::Config(format!("job already completed: {job_id}")));
        }
        JobStatus::Running => {
            if force || state.is_stale_running() {
                // Safe to take over: the original process is gone.
                state.status = JobStatus::Running;
                state.last_error = None;
            } else {
                return Err(AppError::Config(format!(
                    "job already running: {job_id}; use --force to override a stale running state"
                )));
            }
        }
        JobStatus::Pending | JobStatus::Pausing | JobStatus::Paused | JobStatus::Failed => {
            state.status = JobStatus::Running;
            state.last_error = None;
        }
    }
    store.save(&state)?;
    Ok(state)
}
