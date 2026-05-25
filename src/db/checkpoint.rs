//! SQLite ACID task recovery mechanism.

use crate::error::Result;
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkProgress {
    pub chapter_id: String,
    pub chunk_index: i64,
    pub original_text: String,
    pub translated_text: Option<String>,
    pub state: String,
}

pub fn open_checkpoint_db(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    init_checkpoint_schema(&conn)?;
    Ok(conn)
}

pub fn init_checkpoint_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS chunk_progress (
            chapter_id TEXT NOT NULL,
            chunk_index INTEGER NOT NULL,
            original_text TEXT NOT NULL,
            translated_text TEXT,
            state TEXT CHECK(state IN ('pending', 'processing', 'completed')) DEFAULT 'pending',
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (chapter_id, chunk_index)
        );

        CREATE TABLE IF NOT EXISTS global_memory (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS local_glossary (
            original_term TEXT PRIMARY KEY,
            translated_term TEXT NOT NULL,
            category TEXT,
            profile TEXT
        );
        "#,
    )?;
    Ok(())
}

pub fn upsert_chunk_progress(
    conn: &Connection,
    chapter_id: &str,
    chunk_index: i64,
    original_text: &str,
    translated_text: Option<&str>,
    state: &str,
) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO chunk_progress (chapter_id, chunk_index, original_text, translated_text, state)
        VALUES (?1, ?2, ?3, ?4, ?5)
        ON CONFLICT(chapter_id, chunk_index) DO UPDATE SET
            original_text = excluded.original_text,
            translated_text = excluded.translated_text,
            state = excluded.state,
            updated_at = CURRENT_TIMESTAMP
        "#,
        params![
            chapter_id,
            chunk_index,
            original_text,
            translated_text,
            state
        ],
    )?;
    Ok(())
}

pub fn list_completed_chunks(conn: &Connection) -> Result<Vec<ChunkProgress>> {
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

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn completed_chunk_map(conn: &Connection) -> Result<HashMap<(String, i64), ChunkProgress>> {
    let chunks = list_completed_chunks(conn)?;
    Ok(chunks
        .into_iter()
        .map(|chunk| ((chunk.chapter_id.clone(), chunk.chunk_index), chunk))
        .collect())
}
