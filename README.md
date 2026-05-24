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
agent-book-translate \
  --config tmp/config/xiaomi.toml \
  --input tmp/data/epub/alice_in_wonderland.epub \
  --output tmp/output/alice.zh.epub
```

Example Xiaomi-compatible config:

```toml
base_url = "https://example.invalid/v1"
api_key = "replace-with-your-token"
default_model = "MiMo-V2.5"
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

## CI and release

- Pull requests run `fmt`, `clippy`, and tests on Linux, macOS, and Windows.
- Tag pushes matching `v*` build release artifacts for Linux, Windows, and macOS.
- Nix users can enter the dev shell with `nix develop`.
