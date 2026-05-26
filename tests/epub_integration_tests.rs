//! Off-line end-to-end EPUB integration tests.
//!
//! These tests dynamically generate a synthetic micro-EPUB, mount an in-process
//! mock HTTP server using `wiremock`, run the translation engine, and verify
//! the generated translated EPUB prose as well as checkpoint pause/resume idempotency.

use agent_book_translate::config::AppConfig;
use agent_book_translate::core::engine::{JobControl, run_with_progress_and_control};
use agent_book_translate::core::parser::parse_epub;
use agent_book_translate::core::progress::JobProgressReporter;
use agent_book_translate::db::checkpoint::open_checkpoint_db;
use agent_book_translate::job::{JobState, JobStore};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zip::ZipWriter;
use zip::write::FileOptions;

// ── Synthetic EPUB Helper ──────────────────────────────────────────────────

fn create_synthetic_epub(path: &Path, chapter_html: &str) -> std::io::Result<()> {
    let file = File::create(path)?;
    let mut writer = ZipWriter::new(file);

    // 1. mimetype (Stored)
    let stored_opts: FileOptions<'static, ()> =
        FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    writer.start_file("mimetype", stored_opts)?;
    writer.write_all(b"application/epub+zip")?;

    // 2. META-INF/container.xml (Deflated)
    let deflated_opts: FileOptions<'static, ()> =
        FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    writer.start_file("META-INF/container.xml", deflated_opts)?;
    writer.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
    )?;

    // 3. OEBPS/content.opf
    writer.start_file("OEBPS/content.opf", deflated_opts)?;
    writer.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="pub-id" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Synthetic</dc:title>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="chapter1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="chapter1"/>
  </spine>
</package>"#,
    )?;

    // 4. OEBPS/chapter1.xhtml
    writer.start_file("OEBPS/chapter1.xhtml", deflated_opts)?;
    writer.write_all(chapter_html.as_bytes())?;

    writer.finish()?;
    Ok(())
}

fn make_openai_response(content: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "chatcmpl-synthetic",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": content},
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 20,
            "completion_tokens": 10,
            "total_tokens": 30
        }
    })
}

fn make_config_for(base_url: &str) -> AppConfig {
    AppConfig {
        base_url: base_url.to_string(),
        api_key: "mock-key".to_string(),
        concurrency: 1,
        ..AppConfig::default()
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_e2e_successful_epub_translation_mocked() {
    let temp = TempDir::new().unwrap();
    let epub_in = temp.path().join("input.epub");
    let epub_out = temp.path().join("output.epub");

    let chapter_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Chapter 1</title></head>
<body>
  <h1>The Beginning</h1>
  <p>Once upon a time, there was a little rust compiler.</p>
</body>
</html>"#;

    create_synthetic_epub(&epub_in, chapter_content).unwrap();

    // Spawn Mock Server
    let server = MockServer::start().await;

    // Mock for "The Beginning" chunk
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(wiremock::matchers::body_string_contains("The Beginning"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(make_openai_response(r#"{"translation": "开始"}"#)),
        )
        .mount(&server)
        .await;

    // Mock for the paragraph chunk
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(wiremock::matchers::body_string_contains("Once upon a time"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(make_openai_response(
                r#"{"translation": "很久以前，有一个小小的 Rust 编译器。"}"#,
            )),
        )
        .mount(&server)
        .await;

    let config = make_config_for(&server.uri());
    let store = JobStore::new(temp.path().to_path_buf());
    let job_id = "job-e2e-success".to_string();

    let state = JobState::new(job_id.clone(), epub_in.clone(), epub_out.clone());
    store.save(&state).unwrap();

    let reporter = JobProgressReporter::new(store.clone(), job_id.clone());
    let job_control = JobControl {
        store: store.clone(),
        job_id: job_id.clone(),
    };

    // Run translation
    run_with_progress_and_control(&epub_in, &epub_out, &config, &reporter, Some(job_control))
        .await
        .unwrap();

    // Verify Output EPUB structure
    let files = parse_epub(&epub_out).unwrap();
    let chapter_entry = files
        .iter()
        .find(|e| e.name == "OEBPS/chapter1.xhtml")
        .unwrap();
    let rendered_html = String::from_utf8(chapter_entry.data.clone()).unwrap();

    assert!(rendered_html.contains("很久以前，有一个小小的 Rust 编译器。"));
    assert!(rendered_html.contains("开始"));
}

#[tokio::test]
async fn test_e2e_resume_resilience_on_crashes_mocked() {
    let temp = TempDir::new().unwrap();
    let epub_in = temp.path().join("input.epub");
    let epub_out = temp.path().join("output.epub");

    // Micro-epub containing two text entries
    let file = File::create(&epub_in).unwrap();
    let mut writer = ZipWriter::new(file);
    let stored_opts: FileOptions<'static, ()> =
        FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated_opts: FileOptions<'static, ()> =
        FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    writer.start_file("mimetype", stored_opts).unwrap();
    writer.write_all(b"application/epub+zip").unwrap();

    writer
        .start_file("OEBPS/chapter1.xhtml", deflated_opts)
        .unwrap();
    writer.write_all(br#"<p>Original chunk 1</p>"#).unwrap();

    writer
        .start_file("OEBPS/chapter2.xhtml", deflated_opts)
        .unwrap();
    writer.write_all(br#"<p>Original chunk 2</p>"#).unwrap();
    writer.finish().unwrap();

    // Spawn Mock Server
    let server = MockServer::start().await;

    // First request succeeds, second request crashes with 500 Internal Error
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(make_openai_response(r#"{"translation": "译文 1"}"#)),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let config = make_config_for(&server.uri());
    let store = JobStore::new(temp.path().to_path_buf());
    let job_id = "job-resume-test".to_string();

    let state = JobState::new(job_id.clone(), epub_in.clone(), epub_out.clone());
    store.save(&state).unwrap();

    let reporter = JobProgressReporter::new(store.clone(), job_id.clone());
    let job_control = JobControl {
        store: store.clone(),
        job_id: job_id.clone(),
    };

    // First run: expect to fail at chapter 2
    let run1 = run_with_progress_and_control(
        &epub_in,
        &epub_out,
        &config,
        &reporter,
        Some(job_control.clone()),
    )
    .await;

    assert!(run1.is_err());

    // Verify first chunk was checkpointed
    let checkpoint_path = store.checkpoint_path(&job_id);
    let conn = open_checkpoint_db(&checkpoint_path).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT translated_text FROM chunk_progress WHERE chapter_id = 'OEBPS/chapter1.xhtml'",
        )
        .unwrap();
    let text1: String = stmt.query_row([], |r| r.get(0)).unwrap();
    assert_eq!(text1, "译文 1");

    // Second Mock mount: respond successfully for chunk 2
    let server2 = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(make_openai_response(r#"{"translation": "译文 2"}"#)),
        )
        .mount(&server2)
        .await;

    let config2 = make_config_for(&server2.uri());

    // Resume execution
    run_with_progress_and_control(&epub_in, &epub_out, &config2, &reporter, Some(job_control))
        .await
        .unwrap();

    // Verify final EPUB contains BOTH translations correctly
    let files = parse_epub(&epub_out).unwrap();
    let entry1 = files
        .iter()
        .find(|e| e.name == "OEBPS/chapter1.xhtml")
        .unwrap();
    let entry2 = files
        .iter()
        .find(|e| e.name == "OEBPS/chapter2.xhtml")
        .unwrap();

    let html1 = String::from_utf8(entry1.data.clone()).unwrap();
    let html2 = String::from_utf8(entry2.data.clone()).unwrap();

    assert!(html1.contains("译文 1"));
    assert!(html2.contains("译文 2"));
}
