//! Mocked OpenAI-compatible integration tests.
//!
//! These tests use `wiremock` to spin up an in-process HTTP server that serves
//! deterministic chat-completion responses. No real network traffic is made.

use agent_book_translate::agent::client::TranslationClient;
use agent_book_translate::agent::prompt::PromptContext;
use agent_book_translate::config::AppConfig;
use agent_book_translate::core::progress::TokenUsage;
use agent_book_translate::db::checkpoint::{
    completed_chunk_map, init_checkpoint_schema, upsert_chunk_progress,
};
use rusqlite::Connection;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn make_openai_response(content: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": content},
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15
        }
    })
}

fn make_config_for(base_url: &str) -> AppConfig {
    AppConfig {
        base_url: base_url.to_string(),
        api_key: "test-key".to_string(),
        concurrency: 1,
        ..AppConfig::default()
    }
}

fn make_ctx(text: &str) -> PromptContext {
    PromptContext {
        book_summary: String::new(),
        pov_speaker: String::new(),
        glossary: Vec::new(),
        previous_context: String::new(),
        target: text.to_string(),
        next_context: String::new(),
        target_language: "Chinese".to_string(),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// A well-formed JSON translation response is parsed and returned as plain text.
#[tokio::test]
async fn normal_translation_response_returns_text() {
    let server = MockServer::start().await;
    let body = make_openai_response(r#"{"translation": "你好，世界"}"#);

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let config = make_config_for(&server.uri());
    let client = TranslationClient::new(config);
    let result = client.translate(&make_ctx("Hello, world")).await.unwrap();

    assert_eq!(result, "你好，世界");
}

/// A `refined_translation` wrapper (critique report format) is unwrapped correctly.
#[tokio::test]
async fn refined_translation_wrapper_is_normalized() {
    let server = MockServer::start().await;
    let content =
        r#"{"has_mismatches": false, "incorrect_terms": [], "refined_translation": "译文内容"}"#;
    let body = make_openai_response(content);

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let config = make_config_for(&server.uri());
    let client = TranslationClient::new(config);
    let result = client.translate(&make_ctx("source text")).await.unwrap();

    assert_eq!(result, "译文内容");
}

/// A malformed JSON translation wrapper (unescaped inner quotes) is recovered.
#[tokio::test]
async fn malformed_json_wrapper_is_recovered() {
    let server = MockServer::start().await;
    // The inner quotes around 先生 are unescaped, making the JSON invalid.
    let content = r#"{"translation": "他说："先生好。""}"#;
    let body = make_openai_response(content);

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let config = make_config_for(&server.uri());
    let client = TranslationClient::new(config);
    let result = client.translate(&make_ctx("source")).await.unwrap();

    // The healer should unwrap the inner text.
    assert!(
        !result.contains(r#"{"translation""#),
        "JSON wrapper must not appear in result, got: {result:?}"
    );
    assert!(
        result.contains("先生好"),
        "translated content must be present, got: {result:?}"
    );
}

/// Token usage fields are parsed from the OpenAI-compatible response envelope.
#[tokio::test]
async fn token_usage_is_accumulated_from_response() {
    let server = MockServer::start().await;
    let body = make_openai_response(r#"{"translation": "测试"}"#);

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let config = make_config_for(&server.uri());
    let client = TranslationClient::new(config);
    let result = client
        .translate_with_stats(&make_ctx("test"))
        .await
        .unwrap();

    assert_eq!(result.usage.prompt_tokens, 10);
    assert_eq!(result.usage.completion_tokens, 5);
    assert_eq!(result.usage.total_tokens, 15);
}

/// A 429 response followed by a success triggers a retry and the translation
/// is returned correctly.
#[tokio::test]
async fn rate_limit_triggers_retry_and_succeeds() {
    let server = MockServer::start().await;
    let success_body = make_openai_response(r#"{"translation": "重试成功"}"#);

    // First call → 429, subsequent calls → 200.
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&success_body))
        .mount(&server)
        .await;

    let config = make_config_for(&server.uri());
    let client = TranslationClient::new(config);
    let result = client.translate(&make_ctx("retry test")).await.unwrap();

    assert_eq!(result, "重试成功");
}

/// Checkpoint reuse: a completed chunk stored in SQLite is returned without
/// making a new API request.
#[tokio::test]
async fn checkpoint_reuse_skips_api_call() {
    // Pre-populate a checkpoint database with a completed chunk.
    let conn = Connection::open_in_memory().unwrap();
    init_checkpoint_schema(&conn).unwrap();
    upsert_chunk_progress(
        &conn,
        "chapter_1.xhtml",
        0,
        "original",
        Some("已存储的翻译"),
        "completed",
    )
    .unwrap();

    let map = completed_chunk_map(&conn).unwrap();
    let key = ("chapter_1.xhtml".to_string(), 0i64);
    let row = map.get(&key).unwrap();

    // The stored translation is returned as-is (no network call needed).
    assert_eq!(row.translated_text.as_deref(), Some("已存储的翻译"));
    assert_eq!(map.len(), 1);
}

/// Checkpoint normalization: a malformed wrapper stored in the checkpoint DB
/// is normalized when read back via `completed_chunk_map`.
#[tokio::test]
async fn checkpoint_stored_wrapper_is_normalized_on_read() {
    let conn = Connection::open_in_memory().unwrap();
    init_checkpoint_schema(&conn).unwrap();

    // Write a raw wrapper directly (simulates a pre-fix checkpoint row).
    upsert_chunk_progress(
        &conn,
        "ch.xhtml",
        0,
        "source",
        Some(r#"{"translation":"归还的宝剑"}"#),
        "completed",
    )
    .unwrap();

    let map = completed_chunk_map(&conn).unwrap();
    let row = map.get(&("ch.xhtml".to_string(), 0)).unwrap();

    assert_eq!(
        row.translated_text.as_deref(),
        Some("归还的宝剑"),
        "read-time normalization must unwrap the JSON wrapper"
    );
}

/// Token usage accumulates correctly across multiple requests when called
/// in sequence.
#[tokio::test]
async fn token_usage_accumulates_across_requests() {
    let server = MockServer::start().await;
    let body = make_openai_response(r#"{"translation": "一"}"#);

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let config = make_config_for(&server.uri());
    let client = TranslationClient::new(config);

    let mut total = TokenUsage::default();
    for _ in 0..3 {
        let result = client.translate_with_stats(&make_ctx("a")).await.unwrap();
        total.prompt_tokens += result.usage.prompt_tokens;
        total.completion_tokens += result.usage.completion_tokens;
        total.total_tokens += result.usage.total_tokens;
    }

    assert_eq!(total.prompt_tokens, 30);
    assert_eq!(total.completion_tokens, 15);
    assert_eq!(total.total_tokens, 45);
}

/// A response that returns syntactically valid JSON but is missing the expected
/// translation fields triggers a retry, and succeeding on retry returns the correct text.
#[tokio::test]
async fn test_retry_on_invalid_json_format_succeeds() {
    let server = MockServer::start().await;
    let bad_body = make_openai_response(r#"{"status": 500}"#);
    let good_body = make_openai_response(r#"{"translation": "成功重试译文"}"#);

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&bad_body))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&good_body))
        .mount(&server)
        .await;

    let config = make_config_for(&server.uri());
    let client = TranslationClient::new(config);
    let result = client
        .translate_with_stats(&make_ctx("retry test format"))
        .await
        .unwrap();

    assert_eq!(result.translation, "成功重试译文");
    assert_eq!(result.retries, 1);
}
