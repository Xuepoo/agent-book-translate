use agent_book_translate::core::progress::{
    JobProgressReporter, ProgressEvent, ProgressReporter, TokenUsage,
};
use agent_book_translate::job::control::{request_resume, request_resume_force};
use agent_book_translate::job::{JobState, JobStatus, JobStore, STALE_THRESHOLD_SECS};
use chrono::Utc;
use std::path::PathBuf;

#[test]
fn token_usage_is_parsed_from_openai_compatible_response() {
    let raw = r#"{
        "choices": [{"message": {"content": "ok"}}],
        "usage": {
            "prompt_tokens": 11,
            "completion_tokens": 13,
            "total_tokens": 24
        }
    }"#;

    let usage = TokenUsage::from_response(raw).unwrap();

    assert_eq!(usage.prompt_tokens, 11);
    assert_eq!(usage.completion_tokens, 13);
    assert_eq!(usage.total_tokens, 24);
}

#[test]
fn job_progress_reporter_persists_progress_events() {
    let dir = tempfile::tempdir().unwrap();
    let store = JobStore::new(dir.path().to_path_buf());
    let state = JobState::new(
        "job-progress".to_string(),
        PathBuf::from("input.epub"),
        PathBuf::from("output.epub"),
    );
    store.save(&state).unwrap();
    let reporter = JobProgressReporter::new(store.clone(), "job-progress".to_string());

    reporter.on_event(ProgressEvent::Started {
        total_text_files: 2,
        total_chunks: 5,
        completed_chunks: 0,
        completed_text_files: 0,
    });
    reporter.on_event(ProgressEvent::FileStarted {
        file_name: "chapter.xhtml".to_string(),
    });
    reporter.on_event(ProgressEvent::RequestFinished {
        usage: TokenUsage {
            prompt_tokens: 7,
            completion_tokens: 9,
            total_tokens: 16,
        },
        retries: 1,
    });
    reporter.on_event(ProgressEvent::ChunkFinished);
    reporter.on_event(ProgressEvent::Completed);

    let loaded = store.load("job-progress").unwrap();
    assert_eq!(loaded.status, JobStatus::Completed);
    assert_eq!(loaded.metrics.total_text_files, 2);
    assert_eq!(loaded.metrics.total_chunks, 5);
    assert_eq!(loaded.metrics.completed_chunks, 1);
    assert_eq!(loaded.metrics.request_count, 1);
    assert_eq!(loaded.metrics.retry_count, 1);
    assert_eq!(loaded.metrics.total_tokens, 16);
    assert_eq!(loaded.current_file.as_deref(), Some("chapter.xhtml"));
}

#[test]
fn paused_progress_event_persists_paused_state() {
    let dir = tempfile::tempdir().unwrap();
    let store = JobStore::new(dir.path().to_path_buf());
    let state = JobState::new(
        "job-paused".to_string(),
        PathBuf::from("input.epub"),
        PathBuf::from("output.epub"),
    );
    store.save(&state).unwrap();
    let reporter = JobProgressReporter::new(store.clone(), "job-paused".to_string());

    reporter.on_event(ProgressEvent::Paused);

    let loaded = store.load("job-paused").unwrap();
    assert_eq!(loaded.status, JobStatus::Paused);
}

// ─── P1: Stale-running recovery ────────────────────────────────────────────

#[test]
fn running_job_with_no_heartbeat_is_stale() {
    let mut state = JobState::new(
        "job-stale".to_string(),
        PathBuf::from("input.epub"),
        PathBuf::from("output.epub"),
    );
    // Not running → never stale.
    assert!(!state.is_stale_running());

    state.status = JobStatus::Running;
    // Running with no heartbeat ever written → stale.
    assert!(state.is_stale_running());
}

#[test]
fn running_job_with_recent_heartbeat_is_not_stale() {
    let mut state = JobState::new(
        "job-fresh".to_string(),
        PathBuf::from("input.epub"),
        PathBuf::from("output.epub"),
    );
    state.status = JobStatus::Running;
    state.update_heartbeat();

    assert!(!state.is_stale_running());
}

#[test]
fn running_job_with_old_heartbeat_is_stale() {
    let mut state = JobState::new(
        "job-old".to_string(),
        PathBuf::from("input.epub"),
        PathBuf::from("output.epub"),
    );
    state.status = JobStatus::Running;
    // Backdate the heartbeat beyond the stale threshold.
    state.last_heartbeat_at =
        Some(Utc::now() - chrono::Duration::seconds(STALE_THRESHOLD_SECS + 1));

    assert!(state.is_stale_running());
}

#[test]
fn resume_rejects_non_stale_running_job() {
    let dir = tempfile::tempdir().unwrap();
    let store = JobStore::new(dir.path().to_path_buf());
    let mut state = JobState::new(
        "job-live".to_string(),
        PathBuf::from("input.epub"),
        PathBuf::from("output.epub"),
    );
    state.status = JobStatus::Running;
    // Fresh heartbeat – process is considered alive.
    state.update_heartbeat();
    store.save(&state).unwrap();

    let result = request_resume(&store, "job-live");
    assert!(
        result.is_err(),
        "resume must reject a non-stale running job"
    );
}

#[test]
fn resume_auto_detects_stale_running_and_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let store = JobStore::new(dir.path().to_path_buf());
    let mut state = JobState::new(
        "job-stale-auto".to_string(),
        PathBuf::from("input.epub"),
        PathBuf::from("output.epub"),
    );
    state.status = JobStatus::Running;
    state.last_heartbeat_at =
        Some(Utc::now() - chrono::Duration::seconds(STALE_THRESHOLD_SECS + 10));
    store.save(&state).unwrap();

    // Normal resume auto-detects stale and succeeds.
    let resumed = request_resume(&store, "job-stale-auto").unwrap();
    assert_eq!(resumed.status, JobStatus::Running);
}

#[test]
fn resume_force_overrides_live_running_job() {
    let dir = tempfile::tempdir().unwrap();
    let store = JobStore::new(dir.path().to_path_buf());
    let mut state = JobState::new(
        "job-force".to_string(),
        PathBuf::from("input.epub"),
        PathBuf::from("output.epub"),
    );
    state.status = JobStatus::Running;
    // Fresh heartbeat – normally would be rejected.
    state.update_heartbeat();
    store.save(&state).unwrap();

    // --force bypasses the guard unconditionally.
    let forced = request_resume_force(&store, "job-force").unwrap();
    assert_eq!(forced.status, JobStatus::Running);
}

// ─── P2: Idempotent text_files metrics ──────────────────────────────────────

#[test]
fn started_event_initializes_completed_text_files_from_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let store = JobStore::new(dir.path().to_path_buf());
    let state = JobState::new(
        "job-resume-files".to_string(),
        PathBuf::from("input.epub"),
        PathBuf::from("output.epub"),
    );
    store.save(&state).unwrap();
    let reporter = JobProgressReporter::new(store.clone(), "job-resume-files".to_string());

    // Simulate a resume where 2 out of 3 files were already done.
    reporter.on_event(ProgressEvent::Started {
        total_text_files: 3,
        total_chunks: 9,
        completed_chunks: 6,
        completed_text_files: 2,
    });
    // One new file finishes in this run.
    reporter.on_event(ProgressEvent::FileFinished);

    let loaded = store.load("job-resume-files").unwrap();
    assert_eq!(loaded.metrics.total_text_files, 3);
    assert_eq!(
        loaded.metrics.completed_text_files, 3,
        "completed_text_files must equal total after one new file completes"
    );
}

#[test]
fn completed_text_files_never_exceeds_total() {
    let dir = tempfile::tempdir().unwrap();
    let store = JobStore::new(dir.path().to_path_buf());
    let state = JobState::new(
        "job-no-overflow".to_string(),
        PathBuf::from("input.epub"),
        PathBuf::from("output.epub"),
    );
    store.save(&state).unwrap();
    let reporter = JobProgressReporter::new(store.clone(), "job-no-overflow".to_string());

    reporter.on_event(ProgressEvent::Started {
        total_text_files: 2,
        total_chunks: 4,
        completed_chunks: 4,
        // All files already done from checkpoint; no FileFinished will fire.
        completed_text_files: 2,
    });
    // Engine emits no FileFinished when all chunks come from checkpoint.

    let loaded = store.load("job-no-overflow").unwrap();
    assert_eq!(loaded.metrics.completed_text_files, 2);
    assert!(
        loaded.metrics.completed_text_files <= loaded.metrics.total_text_files,
        "completed_text_files must not exceed total_text_files"
    );
}

#[test]
fn test_pid_stale_running_detection() {
    let mut state = JobState::new(
        "job-pid-test".to_string(),
        PathBuf::from("input.epub"),
        PathBuf::from("output.epub"),
    );
    state.status = JobStatus::Running;

    // Case 1: PID is current process (definitely alive)
    state.pid = Some(std::process::id());
    // Heartbeat is old, but process is alive, so it must NOT be stale.
    state.last_heartbeat_at = Some(Utc::now() - chrono::Duration::seconds(300));
    assert!(
        !state.is_stale_running(),
        "Job must not be stale if its process is still alive"
    );

    // Case 2: PID is non-existent (dead process)
    // Most OS limit PID to 32768 or 4194304, a very high PID is safe.
    state.pid = Some(999999);
    // Heartbeat is fresh, but process is dead, so it MUST be stale.
    state.last_heartbeat_at = Some(Utc::now());
    assert!(
        state.is_stale_running(),
        "Job must be stale if its process is dead"
    );
}

#[test]
fn test_completed_text_files_bounds_protection() {
    let dir = tempfile::tempdir().unwrap();
    let store = JobStore::new(dir.path().to_path_buf());
    let state = JobState::new(
        "job-bounds-test".to_string(),
        PathBuf::from("input.epub"),
        PathBuf::from("output.epub"),
    );
    store.save(&state).unwrap();
    let reporter = JobProgressReporter::new(store.clone(), "job-bounds-test".to_string());

    reporter.on_event(ProgressEvent::Started {
        total_text_files: 2,
        total_chunks: 4,
        completed_chunks: 0,
        completed_text_files: 1,
    });

    // Fire FileFinished twice
    reporter.on_event(ProgressEvent::FileFinished);
    reporter.on_event(ProgressEvent::FileFinished);

    let loaded = store.load("job-bounds-test").unwrap();
    // completed_text_files was 1. 1 + 2 = 3, but bounds limit to 2.
    assert_eq!(loaded.metrics.completed_text_files, 2);
}
