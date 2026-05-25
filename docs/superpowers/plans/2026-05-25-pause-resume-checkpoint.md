# Pause/Resume Checkpoint Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add manual pause/resume controls with durable chunk checkpoints so long EPUB translations can stop safely and continue from the same `job_id` after interruption.

**Architecture:** Keep `JobState` as the user-visible control plane and add a per-job SQLite checkpoint DB for chunk-level recovery. The engine will translate chunks in deterministic order, persist each completed chunk immediately, and rebuild EPUB output from persisted chunk data instead of a one-shot string replace. `pause` will be cooperative: the worker finishes the current chunk, marks the job paused, and exits cleanly.

**Tech Stack:** Rust 2024, Clap subcommands, Serde JSON, rusqlite, tokio, indicatif, zip, scraper, XDG state paths via `dirs`.

---

### Task 1: Pause/Resume State and Checkpoints

**Files:**
- Modify: `src/job/mod.rs`
- Modify: `src/db/checkpoint.rs`
- Modify: `src/lib.rs`
- Modify: `tests/job_tests.rs`
- Modify: `tests/db_tests.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn paused_state_round_trip_persists() {
    let dir = tempfile::tempdir().unwrap();
    let store = JobStore::new(dir.path().to_path_buf());
    let mut state = JobState::new("job-paused".to_string(), "input.epub".into(), "output.epub".into());
    state.status = JobStatus::Paused;
    state.last_error = Some("paused by user".to_string());
    store.save(&state).unwrap();
    let loaded = store.load("job-paused").unwrap();
    assert_eq!(loaded.status, JobStatus::Paused);
    assert_eq!(loaded.last_error.as_deref(), Some("paused by user"));
}

#[test]
fn checkpoint_completed_chunk_is_reused() {
    let conn = Connection::open_in_memory().unwrap();
    init_checkpoint_schema(&conn).unwrap();
    upsert_chunk_progress(
        &conn,
        "chapter-1",
        0,
        "Call me Ishmael.",
        Some("叫我以实玛利。"),
        "completed",
    )
    .unwrap();
    let completed = list_completed_chunks(&conn).unwrap();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].translated_text.as_deref(), Some("叫我以实玛利。"));
}
```

- [ ] **Step 2: Run the focused tests and verify they fail**

Run: `cargo test paused_state_round_trip_persists checkpoint_completed_chunk_is_reused -- --nocapture`
Expected: FAIL because `Paused` and checkpoint recovery behavior are not fully wired yet.

- [ ] **Step 3: Implement the minimal state and checkpoint changes**

```rust
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
```

```rust
pub fn open_checkpoint_db(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    init_checkpoint_schema(&conn)?;
    Ok(conn)
}
```

```rust
pub fn completed_chunk_map(conn: &Connection) -> Result<HashMap<(String, i64), ChunkProgress>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT chapter_id, chunk_index, original_text, translated_text, state
        FROM chunk_progress
        WHERE state = 'completed'
        ORDER BY chapter_id, chunk_index
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ChunkProgress {
            chapter_id: row.get(0)?,
            chunk_index: row.get(1)?,
            original_text: row.get(2)?,
            translated_text: row.get(3)?,
            state: row.get(4)?,
        })
    })?;

    let mut map = HashMap::new();
    for row in rows {
        let chunk = row?;
        map.insert((chunk.chapter_id.clone(), chunk.chunk_index), chunk);
    }
    Ok(map)
}
```

- [ ] **Step 4: Run the focused tests again and verify they pass**

Run: `cargo test paused_state_round_trip_persists checkpoint_completed_chunk_is_reused -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/job/mod.rs src/db/checkpoint.rs src/lib.rs tests/job_tests.rs tests/db_tests.rs
git commit -m "feat: add pause state and checkpoint accessors"
```

### Task 2: Deterministic Chunk Resume and Pause-Aware Engine

**Files:**
- Modify: `src/core/parser.rs`
- Modify: `src/core/engine.rs`
- Modify: `src/core/progress.rs`
- Modify: `src/agent/client.rs`
- Modify: `tests/parser_tests.rs`
- Modify: `tests/progress_tests.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn duplicate_source_chunks_keep_stable_identity() {
    let raw_xhtml = r#"<div><p>Repeat me.</p><p>Repeat me.</p></div>"#;
    let document = Html::parse_document(raw_xhtml);
    let chunks = extract_text_chunks(&document);
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].text, "Repeat me.");
    assert_eq!(chunks[1].text, "Repeat me.");
    assert_eq!(chunks[0].node_id.as_deref(), Some("p-0"));
    assert_eq!(chunks[1].node_id.as_deref(), Some("p-1"));
}

#[test]
fn resume_rendering_uses_checkpoint_data() {
    let rendered = render_file_from_checkpoints(
        "chapter.xhtml",
        vec![
            RenderedChunk {
                file_name: "chapter.xhtml".to_string(),
                chunk_index: 0,
                original: "Repeat me.".to_string(),
                translated: "重复我".to_string(),
            },
            RenderedChunk {
                file_name: "chapter.xhtml".to_string(),
                chunk_index: 1,
                original: "Repeat me.".to_string(),
                translated: "再重复我".to_string(),
            },
        ],
    );
    assert!(rendered.contains("重复我"));
    assert!(rendered.contains("再重复我"));
}
```

- [ ] **Step 2: Run the focused tests and verify they fail**

Run: `cargo test duplicate_source_chunks_keep_stable_identity resume_rendering_uses_checkpoint_data -- --nocapture`
Expected: FAIL because the engine still relies on a naive string replacement path and has no pause check.

- [ ] **Step 3: Implement deterministic chunk rendering and pause checks**

```rust
pub struct RenderedChunk {
    pub file_name: String,
    pub chunk_index: usize,
    pub original: String,
    pub translated: String,
}
```

```rust
if matches!(state.status, JobStatus::Pausing) {
    state.status = JobStatus::Paused;
    state.last_error = Some("paused by user".to_string());
    store.save(&state)?;
    return Ok(());
}
```

```rust
let completed_map = completed_chunk_map(&conn)?;
let key = (file_name.clone(), chunk_index as i64);
if let Some(existing) = completed_map.get(&key) {
    rendered_chunks.push(existing.translated_text.clone().unwrap_or_default());
    continue;
}
```

- [ ] **Step 4: Run the focused tests again and verify they pass**

Run: `cargo test duplicate_source_chunks_keep_stable_identity resume_rendering_uses_checkpoint_data -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/core/parser.rs src/core/engine.rs src/core/progress.rs src/agent/client.rs tests/parser_tests.rs tests/progress_tests.rs
git commit -m "feat: make translation chunks resumable"
```

### Task 3: CLI Pause/Resume Commands

**Files:**
- Modify: `src/main.rs`
- Modify: `README.md`
- Modify: `tests/job_tests.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn resume_rejects_completed_job() {
    let dir = tempfile::tempdir().unwrap();
    let store = JobStore::new(dir.path().to_path_buf());
    let mut state = JobState::new("job-done".to_string(), "input.epub".into(), "output.epub".into());
    state.status = JobStatus::Completed;
    store.save(&state).unwrap();
    assert!(matches!(resume_job(&store, "job-done"), Err(_)));
}
```

- [ ] **Step 2: Run the focused tests and verify they fail**

Run: `cargo test resume_rejects_completed_job -- --nocapture`
Expected: FAIL because `pause` and `resume` commands are not implemented yet.

- [ ] **Step 3: Implement the CLI commands**

```rust
CommandKind::Pause(JobIdArgs),
CommandKind::Resume(JobIdArgs),
```

```rust
fn pause(job_id: &str) -> Result<()> {
    let store = JobStore::xdg()?;
    let mut state = store.load(job_id)?;
    state.status = JobStatus::Pausing;
    state.last_error = Some("pause requested by user".to_string());
    store.save(&state)?;
    Ok(())
}
```

```rust
fn resume(job_id: &str) -> Result<()> {
    let store = JobStore::xdg()?;
    let state = store.load(job_id)?;
    if state.status == JobStatus::Completed {
        return Err(AppError::Config(format!("job already completed: {job_id}")));
    }
    if matches!(state.status, JobStatus::Paused | JobStatus::Failed | JobStatus::Pausing) {
        return start_or_resume_existing_job(&store, job_id, state);
    }
    Err(AppError::Config(format!("job is not resumable: {job_id}")))
}
```

- [ ] **Step 4: Run the focused tests again and verify they pass**

Run: `cargo test resume_rejects_completed_job -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Update the README**

Document `pause <job_id>` and `resume <job_id>` alongside `start`, `status`, and `logs`, and note that pause takes effect after the current chunk.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs README.md tests/job_tests.rs
git commit -m "feat: add pause resume cli"
```

### Task 4: End-to-End Verification

**Files:**
- Modify tests only if verification reveals a real regression.

- [ ] **Step 1: Run formatting and test suites**

Run:
`cargo fmt --all -- --check`
`cargo test`
`cargo clippy --all-targets -- -D warnings`
`pre-commit run --all-files`

- [ ] **Step 2: Run a real EPUB resume regression in podman**

Use the existing Fedora container, `moby_dick_melville_2701.epub`, and the Xiaomi endpoint with `mimo-v2.5-pro`. Start a job, pause it after a few chunks, resume it, and verify the output EPUB with `unzip -t`.

- [ ] **Step 3: Inspect sampled XHTML output**

Use `unzip -p` on a few translated XHTML files and verify:
- Chinese translation is present
- no JSON response wrapper leaked into the EPUB
- repeated source text did not collapse into the wrong translation
- the pause/resume boundary did not corrupt EPUB structure

- [ ] **Step 4: Commit verification-only changes if needed**

```bash
git add <any files changed by verification>
git commit -m "test: verify pause resume recovery"
```
