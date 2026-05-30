# agent-book-translate

A powerful, concurrent, and highly resilient CLI ebook translator driven by OpenAI-compatible Large Language Model APIs. Designed with robust sqlite3-backed chunk checkpoints, automated API retry loops, linux-native process safety, and strict quality verification.

---

## Features

- **Robust Background Controls**: Cooperative `pause`, non-blocking `start`, and instant process-safe `resume`.
- **Linux Proc Safety**: Native `/proc/<pid>` active checking for instant crashed-job detection and collision prevention (blocking concurrent dual-writes to state).
- **Offline E2E Verification**: Rigorous quality gates validated by mock HTTP server testing.
- **Write-Time Checkpoint Shield**: Strict write-time validation filtering out lingering JSON leakage markers (`{"translation"`, `refined_translation`) before persisting to SQLite.
- **EPUB Quality Inspector**: Built-in automated QA checks verifying archive structure integrity and JSON wrapper leaks.
- **Nix & nFPM Packaging**: Built-in Nix flake compilation and fast deb/rpm packaging support.

---

## Configuration

By default, the CLI reads configuration from:
```text
$XDG_CONFIG_HOME/agent-book-translate/config.toml
```

Use `--config <FILE>` to pass an explicit TOML file for isolated runs, agent workflows, or per-provider configurations:
```bash
agent-book-translate translate \
  --config tmp/config/openai.toml \
  --input tmp/data/epub/alice_in_wonderland.epub \
  --output tmp/output/alice.zh.epub
```

### Example TOML Configuration
```toml
base_url = "https://example.invalid/v1"
api_key = "replace-with-your-token"
default_model = "gpt-4o-mini"
concurrency = 3
bilingual = false
http_proxy = "http://127.0.0.1:1080"
```

### Precedence Hierarchy
```text
CLI flags > Environment Variables > Config File > Built-in Defaults
```

### Supported Environment Variables
- `LLM_API_KEY`, `OPENAI_API_KEY`
- `LLM_BASE_URL`, `OPENAI_BASE_URL`
- `LLM_MODEL`, `OPENAI_MODEL`
- `HTTP_PROXY`, `HTTPS_PROXY`

---

## Background Control & Lifecycle

### 1. Foreground Translation (Blocking with Progress Bar)
```bash
agent-book-translate translate \
  --input input.epub \
  --output output.epub \
  --language zh \
  --concurrency 3
```

### 2. Background Translation (Non-blocking Process)
```bash
agent-book-translate start \
  --input input.epub \
  --output output.epub \
  --language zh
```
This spawns a background thread and prints a `job_id`. Use this ID to monitor or manage the job:
```bash
agent-book-translate status <job_id>
agent-book-translate logs <job_id>
agent-book-translate list
```

### 3. Graceful Pause and Instant Resume
- **Pause cooperatively** (stops safely once the current chunk finishes translating):
  ```bash
  agent-book-translate pause <job_id>
  ```
- **Resume and Recover**:
  ```bash
  agent-book-translate resume --job-id <job_id>
  ```
  *Note:* PID-based Linux proc checks guarantee that if the process is dead (crashed/terminated), the CLI immediately takes over the job safely. If the process is still running in the OS, concurrent resume writes are safely rejected to prevent state corruption.

---

## Quality Assurance & Maintenance

### 1. Built-in QA Scanner
Never release a book without verifying archive structure integrity and JSON leakage!
```bash
agent-book-translate qa <output.epub>
```
*Gated checks:*
- `[PASS] archive integrity: ok`
- `[PASS] JSON leakage checks: leakage hits = 0`

### 2. Checkpoint SQLite Repair Utility
Normalize or strip linger JSON headers directly inside the SQLite checkpoint:
```bash
agent-book-translate migrate-checkpoint <sqlite_db_path>
```

---

## Compilation & Production Packaging

### Nix Flake Sandbox Build
To compile the absolute isolated binary directly using the Nix environment:
```bash
nix build
```

### Debian & RPM Native Packaging (nFPM)
NFPM allows packaging the compiled release binary without external heavy build toolchains:
```bash
# First build release binary
cargo build --release

# Pack to deb package
nfpm pkg --packager deb --target ./agent-book-translate.deb

# Pack to rpm package
nfpm pkg --packager rpm --target ./agent-book-translate.rpm
```

---

## Best Practices & API Rate Limit Tuning

When running large translation jobs with limited API quotas (e.g., `gpt-4o-mini` or similar providers), pay attention to **RPM (Requests Per Minute)** limits. 

- **Concurrency Setting**: High concurrency (e.g., `--concurrency 3` or more) combined with multiple parallel jobs can easily hit API rate limits (RPM), resulting in a high retry rate or request failures.
- **Recommended Default**: For a stable, unattended translation flow with standard RPM limits, we recommend setting a conservative concurrency level:
  - **`concurrency = 2`** is a very stable default for single-job execution.
- **Tuning**: Adjust the `--concurrency` flag based on your provider's specific rate limits. Lower concurrency ensures fewer retries and a smoother, more efficient overall translation speed.
