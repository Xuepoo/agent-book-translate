use agent_book_translate::core::migration::migrate_checkpoint_db;
use agent_book_translate::db::checkpoint::{init_checkpoint_schema, upsert_chunk_progress};
use rusqlite::Connection;
use tempfile::NamedTempFile;

#[test]
fn test_migrate_checkpoint_db_repairs_wrappers() {
    let temp = NamedTempFile::new().unwrap();
    let conn = Connection::open(temp.path()).unwrap();
    init_checkpoint_schema(&conn).unwrap();

    // Insert malformed wrapper entry
    upsert_chunk_progress(
        &conn,
        "intro.xhtml",
        0,
        "Source text",
        Some(r#"{"translation": "已修好的译文"}"#),
        "completed",
    )
    .unwrap();
    drop(conn);

    let res = migrate_checkpoint_db(temp.path()).unwrap();
    assert_eq!(res.scanned, 1);
    assert_eq!(res.repaired, 1);

    // Verify row was fully normalized
    let conn = Connection::open(temp.path()).unwrap();
    let mut stmt = conn
        .prepare("SELECT translated_text FROM chunk_progress")
        .unwrap();
    let text: String = stmt.query_row([], |r| r.get(0)).unwrap();
    assert_eq!(text, "已修好的译文");
}
