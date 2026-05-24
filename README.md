# agent-book-translate

A lightweight, agent-driven CLI ebook translator powered by OpenAI-compatible
LLM APIs.

## Configuration

By default the CLI reads:

```text
$XDG_CONFIG_HOME/agent-book-translate/config.toml
```

Use `--config <FILE>` to pass an explicit TOML file for isolated test runs,
agent workflows, or per-provider settings:

```bash
agent-book-translate translate \
  --config tmp/config/xiaomi.toml \
  --input tmp/data/epub/alice_in_wonderland.epub \
  --output tmp/output/alice.zh.epub
```

Example Xiaomi-compatible config:

```toml
base_url = "https://example.invalid/v1"
api_key = "replace-with-your-token"
default_model = "mimo-v2.5-pro"
concurrency = 2
bilingual = false
http_proxy = "http://127.0.0.1:1080"

[reasoning]
enable = false
intensity = "low"
```

Precedence is:

```text
CLI flags > environment variables > --config/default config file > built-in defaults
```

Supported environment variables:

- `LLM_API_KEY`, `XIAOMI_API_KEY`, `OPENAI_API_KEY`
- `LLM_BASE_URL`, `XIAOMI_BASE_URL`
- `LLM_MODEL`, `XIAOMI_MODEL`
- `HTTP_PROXY`, `HTTPS_PROXY`

## Jobs and progress

Foreground translation prints a progress bar and creates a job state file:

```bash
agent-book-translate translate \
  --config tmp/config/xiaomi.toml \
  --input tmp/data/epub/alice_in_wonderland.epub \
  --output tmp/outputs/alice.zh.epub
```

Run in the background with `start`:

```bash
agent-book-translate start \
  --config tmp/config/xiaomi.toml \
  --input tmp/data/epub/alice_in_wonderland.epub \
  --output tmp/outputs/alice.zh.epub
```

The command prints a `job_id`. Use it to inspect progress:

```bash
agent-book-translate status <job_id>
agent-book-translate logs <job_id>
agent-book-translate list
```

Job state and logs are stored under:

```text
$XDG_STATE_HOME/agent-book-translate/
```

State files include progress, elapsed time, request count, retry count, and
token usage returned by the API provider. They do not store API keys or full
provider configuration.

## CI and release

- Pull requests run `fmt`, `clippy`, and tests on Linux, macOS, and Windows.
- Tag pushes matching `v*` build release artifacts for Linux, Windows, and macOS.
- Nix users can enter the dev shell with `nix develop`.
