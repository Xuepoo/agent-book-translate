use crate::agent::client::parse_translation_content;
use crate::db::checkpoint::open_checkpoint_db;
use crate::error::Result;
use rusqlite::params;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationResult {
    pub scanned: usize,
    pub repaired: usize,
}

pub fn migrate_checkpoint_db(db_path: &Path) -> Result<MigrationResult> {
    let conn = open_checkpoint_db(db_path)?;
    let mut updates = Vec::new();
    let mut scanned = 0;

    {
        let mut stmt = conn.prepare(
            "SELECT chapter_id, chunk_index, translated_text FROM chunk_progress WHERE state = 'completed'"
        )?;

        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            scanned += 1;
            let chapter_id: String = row.get(0)?;
            let chunk_index: i64 = row.get(1)?;
            let translated_text: Option<String> = row.get(2)?;

            if let Some(raw) = translated_text
                && let Ok(normalized) = parse_translation_content(&raw)
                && normalized != raw
            {
                updates.push((chapter_id, chunk_index, normalized));
            }
        }
    }

    let mut repaired = 0;
    for (chapter_id, chunk_index, normalized) in updates {
        conn.execute(
            "UPDATE chunk_progress SET translated_text = ?1 WHERE chapter_id = ?2 AND chunk_index = ?3",
            params![normalized, chapter_id, chunk_index],
        )?;
        repaired += 1;
    }

    Ok(MigrationResult { scanned, repaired })
}
