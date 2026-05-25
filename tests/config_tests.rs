use agent_book_translate::config::{AppConfig, load_config_file};
use std::fs;

#[test]
fn explicit_config_path_overrides_default_values() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    fs::write(
        &config_path,
        r#"
base_url = "https://example.invalid/v1"
default_model = "mimo-v2.5-pro"
concurrency = 2
bilingual = true
http_proxy = "http://127.0.0.1:1080"

[reasoning]
enable = false
intensity = "low"
"#,
    )
    .unwrap();

    let config = load_config_file(&config_path).unwrap();
    assert_eq!(config.base_url, "https://example.invalid/v1");
    assert_eq!(config.default_model, "mimo-v2.5-pro");
    assert_eq!(config.concurrency, 2);
    assert!(config.bilingual);
    assert_eq!(config.http_proxy.as_deref(), Some("http://127.0.0.1:1080"));
}

#[test]
fn missing_explicit_config_path_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("missing.toml");
    let error = load_config_file(&config_path).unwrap_err();
    assert!(error.to_string().contains("config file does not exist"));
}

#[test]
fn missing_default_config_path_uses_defaults() {
    let config = AppConfig::load_from_path(None).unwrap();
    assert_eq!(config.default_model, "mimo-v2.5-pro");
}
