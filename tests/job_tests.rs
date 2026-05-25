use agent_book_translate::job::{JobMetrics, JobState, JobStatus, JobStore};
use std::fs;

#[test]
fn job_state_round_trip_persists_metrics() {
    let dir = tempfile::tempdir().unwrap();
    let store = JobStore::new(dir.path().to_path_buf());
    let mut state = JobState::new(
        "job-test".to_string(),
        "input.epub".into(),
        "output.epub".into(),
    );
    state.status = JobStatus::Running;
    state.metrics.total_chunks = 10;
    state.metrics.completed_chunks = 4;
    state.metrics.request_count = 5;
    state.metrics.prompt_tokens = 100;
    state.metrics.completion_tokens = 80;
    state.metrics.total_tokens = 180;
    state.current_file = Some("chapter.xhtml".to_string());

    store.save(&state).unwrap();
    let loaded = store.load("job-test").unwrap();

    assert_eq!(loaded.job_id, "job-test");
    assert_eq!(loaded.status, JobStatus::Running);
    assert_eq!(loaded.metrics.completed_chunks, 4);
    assert_eq!(loaded.metrics.total_tokens, 180);
    assert_eq!(loaded.current_file.as_deref(), Some("chapter.xhtml"));
}

#[test]
fn paused_state_round_trip_persists() {
    let dir = tempfile::tempdir().unwrap();
    let store = JobStore::new(dir.path().to_path_buf());
    let mut state = JobState::new(
        "job-paused".to_string(),
        "input.epub".into(),
        "output.epub".into(),
    );
    state.status = JobStatus::Paused;
    state.last_error = Some("network disconnected".to_string());
    state.metrics.completed_chunks = 3;

    store.save(&state).unwrap();
    let loaded = store.load("job-paused").unwrap();

    assert_eq!(loaded.status, JobStatus::Paused);
    assert_eq!(loaded.last_error.as_deref(), Some("network disconnected"));
    assert_eq!(loaded.metrics.completed_chunks, 3);

    let jobs_dir = dir.path().join("jobs");
    let entries = fs::read_dir(&jobs_dir)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].path().file_name().and_then(|name| name.to_str()),
        Some("job-paused.json")
    );
}

#[test]
fn job_store_lists_newest_updated_first() {
    let dir = tempfile::tempdir().unwrap();
    let store = JobStore::new(dir.path().to_path_buf());
    let first = JobState::new("job-a".to_string(), "a.epub".into(), "a.zh.epub".into());
    let mut second = JobState::new("job-b".to_string(), "b.epub".into(), "b.zh.epub".into());
    second.metrics = JobMetrics {
        total_chunks: 2,
        completed_chunks: 2,
        ..JobMetrics::default()
    };

    store.save(&first).unwrap();
    store.save(&second).unwrap();

    let jobs = store.list().unwrap();
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[0].job_id, "job-b");
    assert_eq!(jobs[1].job_id, "job-a");
}

#[test]
fn elapsed_seconds_are_non_negative() {
    let state = JobState::new(
        "job-elapsed".to_string(),
        "input.epub".into(),
        "output.epub".into(),
    );

    assert!(state.elapsed_seconds() >= 0);
}
