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
    let mut state = store.load(job_id)?;
    match state.status {
        JobStatus::Completed => {
            return Err(AppError::Config(format!("job already completed: {job_id}")));
        }
        JobStatus::Running => {
            return Err(AppError::Config(format!("job already running: {job_id}")));
        }
        JobStatus::Pending | JobStatus::Pausing | JobStatus::Paused | JobStatus::Failed => {
            state.status = JobStatus::Running;
            state.last_error = None;
        }
    }
    store.save(&state)?;
    Ok(state)
}
