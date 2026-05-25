use agent_book_translate::core::progress::{
    JobProgressReporter, ProgressEvent, ProgressReporter, TokenUsage,
};
use agent_book_translate::job::{JobState, JobStatus, JobStore};
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
