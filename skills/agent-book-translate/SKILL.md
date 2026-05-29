---
name: agent-book-translate
description: CLI control guide for the agent-book-translate tool. Use when you need to trigger, monitor, pause, resume, or run quality assurance checks on translation jobs for EPUB books.
---

# Agent Book Translate Tool Guide

`agent-book-translate` is a high-performance, concurrent, and highly fault-tolerant EPUB book translation utility driven by Large Language Models (OpenAI-compatible endpoints). It features SQLite-backed chunk-level checkpointing, automated recovery/retries, robust QA checks, and process-level crash resilience.

---

## Environment & Setup

The tool respects configuration values in the following order of precedence:
`CLI Flags > Environment Variables > Config File (TOML) > Default Fallbacks`

### Environment Variables
- `XIAOMI_API_KEY` (or custom provider key): Your LLM API bearer token.
- `XDG_STATE_HOME`: Path override for the jobs and database repository. Defaults to `~/.local/state/agent-book-translate/`.

### Default Config Path
- `~/.config/agent-book-translate/config.toml` (XDG-compliant config file)

---

## Command Reference

Run the executable directly (e.g., `./target/debug/agent-book-translate` or the release target):

### 1. Starting translations
- **Foreground (blocking with live progress bar):**
  ```bash
  agent-book-translate translate --input <input.epub> --output <output.epub> --language <zh|ja|de|...> [--concurrency <1-5>] [--verbose]
  ```
- **Background (non-blocking process spawn):**
  ```bash
  agent-book-translate start --input <input.epub> --output <output.epub> --language <zh|ja|de|...> [--concurrency <1-5>]
  ```

### 2. Managing Job Lifecycle
- **List all jobs:**
  ```bash
  agent-book-translate list
  ```
- **Inspect job details & metrics (chunks completed, token usage, elapsed time, errors):**
  ```bash
  agent-book-translate status <job_id>
  ```
- **Pause a running job gracefully:**
  ```bash
  agent-book-translate pause <job_id>
  ```
- **Resume/Recover a paused or failed job:**
  ```bash
  agent-book-translate resume <job_id>
  ```
  *Note:* PID-based staleness checks allow immediate resume upon crashes or CLI termination. If a job is reported as running but the process is dead, the tool auto-takes over safely. Use `--force` only to override actively running processes.
- **Inspect background job logs:**
  ```bash
  agent-book-translate logs <job_id>
  ```

### 3. Verification & Maintenance
- **Quality Assurance Scan (Crucial Gate):**
  Inspects the integrity of the target EPUB and scans XHTML content to ensure no JSON wrappers (e.g., `{"translation": ...}`) have leaked into the prose text.
  ```bash
  agent-book-translate qa <output.epub>
  ```
- **Checkpoint SQLite Repair:**
  Rewrites and normalizes lingering JSON wrappers directly in the database.
  ```bash
  agent-book-translate migrate-checkpoint <~/.local/state/agent-book-translate/checkpoints/JOB_ID.sqlite3>
  ```

---

## Best Practices & Heuristics for Agents

1. **Job Recovery:** If a background run fails or stalls (e.g., due to network disconnection or API rate limits), always call `agent-book-translate status <job_id>` to read the `last_error` and then call `agent-book-translate resume <job_id>` to pick up right from where it left off.
2. **Quality Gates:** NEVER consider a translation successful until you run `agent-book-translate qa <output.epub>` and receive `[PASS] all QA checks passed`.
3. **Concurrency & Rate Limits:** While a concurrency limit of `3` is functional, high RPM API limits (like `mimo-v2.5-pro` under multiple parallel jobs) can easily cause rate limiting. A single-job **`concurrency = 2`** is a highly recommended and safe default. Adjust concurrency according to your API provider's RPM limit to minimize retries and optimize overall speed.
