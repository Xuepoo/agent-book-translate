# Observable Jobs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add foreground progress, request/token metrics, and background job status commands for EPUB translation.

**Architecture:** Keep translation logic in `core::engine` and inject a lightweight progress reporter. Persist job state as JSON under the XDG state directory so foreground and background runs share the same status model.

**Tech Stack:** Rust 2024, Clap subcommands, Indicatif progress bars, Serde JSON, XDG state paths via `dirs`.

---

### Task 1: Metrics and Job State Types

**Files:**
- Create: `src/job/mod.rs`
- Create: `tests/job_tests.rs`
- Modify: `src/lib.rs`
- Modify: `Cargo.toml`

- [ ] Add serializable `JobStatus`, `JobState`, `JobMetrics`, and `JobStore` types.
- [ ] Store state files in `$XDG_STATE_HOME/agent-book-translate/jobs/<job_id>.json`.
- [ ] Add tests for XDG-style explicit root paths, state round trips, and elapsed seconds.

### Task 2: Engine Progress Hooks

**Files:**
- Create: `src/core/progress.rs`
- Modify: `src/core/mod.rs`
- Modify: `src/core/engine.rs`
- Modify: `src/agent/client.rs`
- Test: `tests/progress_tests.rs`

- [ ] Add `ProgressReporter` trait with no-op, terminal, and job-state implementations.
- [ ] Count total text files and chunks before translation starts.
- [ ] Update completed chunks, current file, request count, retry count, and token usage after each request.
- [ ] Parse `usage.prompt_tokens`, `usage.completion_tokens`, and `usage.total_tokens` from responses.

### Task 3: CLI Subcommands

**Files:**
- Modify: `src/main.rs`
- Modify: `README.md`

- [ ] Convert CLI to subcommands: `translate`, `start`, `status`, `list`, `logs`.
- [ ] Preserve current no-subcommand behavior by treating root flags as `translate`.
- [ ] Implement `start` as a detached child process that invokes `translate --job-id <id>`.
- [ ] Keep API keys in env/config only; never write them to job JSON or logs.

### Task 4: Verification

**Files:**
- Modify tests as needed only for behavior coverage.

- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run `cargo test`.
- [ ] Run `cargo clippy --all-targets -- -D warnings`.
- [ ] Run `pre-commit run --all-files`.
- [ ] Run a real EPUB through the new CLI in the Fedora container.
- [ ] Verify `status <job_id>`, output EPUB `unzip -t`, and sampled XHTML content.
