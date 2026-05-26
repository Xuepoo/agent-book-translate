//! FTS5 vectorless entity retrieval engine for series-wide glossary.

use crate::error::Result;
use rusqlite::{Connection, params};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesEntity {
    pub id: i64,
    pub original_name: String,
    pub translated_name: String,
    pub category: String,
    pub profile: String,
}

pub fn init_series_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS series_entities (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            original_name TEXT UNIQUE NOT NULL,
            translated_name TEXT NOT NULL,
            category TEXT NOT NULL,
            profile TEXT NOT NULL
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS series_entities_fts USING fts5(
            original_name,
            category,
            profile,
            content='series_entities',
            content_rowid='id'
        );

        CREATE TRIGGER IF NOT EXISTS tbl_ai AFTER INSERT ON series_entities BEGIN
          INSERT INTO series_entities_fts(rowid, original_name, category, profile)
          VALUES (new.id, new.original_name, new.category, new.profile);
        END;

        CREATE TRIGGER IF NOT EXISTS tbl_ad AFTER DELETE ON series_entities BEGIN
          INSERT INTO series_entities_fts(series_entities_fts, rowid, original_name, category, profile)
          VALUES('delete', old.id, old.original_name, old.category, old.profile);
        END;

        CREATE TRIGGER IF NOT EXISTS tbl_au AFTER UPDATE ON series_entities BEGIN
          INSERT INTO series_entities_fts(series_entities_fts, rowid, original_name, category, profile)
          VALUES('delete', old.id, old.original_name, old.category, old.profile);
          INSERT INTO series_entities_fts(rowid, original_name, category, profile)
          VALUES (new.id, new.original_name, new.category, new.profile);
        END;
        "#,
    )?;
    Ok(())
}

pub fn insert_entity(
    conn: &Connection,
    original_name: &str,
    translated_name: &str,
    category: &str,
    profile: &str,
) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO series_entities (original_name, translated_name, category, profile)
        VALUES (?1, ?2, ?3, ?4)
        "#,
        params![original_name, translated_name, category, profile],
    )?;
    Ok(())
}

pub fn update_entity(
    conn: &Connection,
    original_name: &str,
    translated_name: &str,
    category: &str,
    profile: &str,
) -> Result<()> {
    conn.execute(
        r#"
        UPDATE series_entities
        SET translated_name = ?1, category = ?2, profile = ?3
        WHERE original_name = ?4
        "#,
        params![translated_name, category, profile, original_name],
    )?;
    Ok(())
}

pub fn search_series_entities(conn: &Connection, query: &str) -> Result<Vec<SeriesEntity>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT se.id, se.original_name, se.translated_name, se.category, se.profile
        FROM series_entities_fts fts
        JOIN series_entities se ON se.id = fts.rowid
        WHERE series_entities_fts MATCH ?1
        ORDER BY bm25(series_entities_fts)
        LIMIT 20
        "#,
    )?;

    let rows = stmt.query_map(params![query], |row| {
        Ok(SeriesEntity {
            id: row.get(0)?,
            original_name: row.get(1)?,
            translated_name: row.get(2)?,
            category: row.get(3)?,
            profile: row.get(4)?,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}
