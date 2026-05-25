use agent_book_translate::db::checkpoint::{
    completed_chunk_map, init_checkpoint_schema, list_completed_chunks, upsert_chunk_progress,
};
use agent_book_translate::db::series::{
    init_series_schema, insert_entity, search_series_entities, update_entity,
};
use rusqlite::Connection;

#[test]
fn checkpoint_persistence_on_shutdown() {
    let conn = Connection::open_in_memory().unwrap();
    init_checkpoint_schema(&conn).unwrap();
    for i in 0..3 {
        upsert_chunk_progress(
            &conn,
            "ch_1",
            i,
            &format!("orig_{i}"),
            Some(&format!("trans_{i}")),
            "completed",
        )
        .unwrap();
    }
    let completed = list_completed_chunks(&conn).unwrap();
    assert_eq!(completed.len(), 3);
}

#[test]
fn checkpoint_completed_chunk_is_reused() {
    let conn = Connection::open_in_memory().unwrap();
    init_checkpoint_schema(&conn).unwrap();
    upsert_chunk_progress(
        &conn,
        "ch_2",
        4,
        "original text",
        Some("translated text"),
        "completed",
    )
    .unwrap();

    let completed = completed_chunk_map(&conn).unwrap();
    let key = ("ch_2".to_string(), 4);
    let row = completed.get(&key).unwrap();

    assert_eq!(completed.len(), 1);
    assert_eq!(row.chapter_id, "ch_2");
    assert_eq!(row.chunk_index, 4);
    assert_eq!(row.original_text, "original text");
    assert_eq!(row.translated_text.as_deref(), Some("translated text"));
    assert_eq!(row.state, "completed");
}

#[test]
fn fts5_entity_matching() {
    let conn = Connection::open_in_memory().unwrap();
    init_series_schema(&conn).unwrap();
    insert_entity(
        &conn,
        "Jon Snow",
        "琼恩·雪诺",
        "Character",
        "Bastard of Winterfell",
    )
    .unwrap();
    let results = search_series_entities(&conn, "Jon").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].translated_name, "琼恩·雪诺");
}

#[test]
fn fts5_update_trigger_sync() {
    let conn = Connection::open_in_memory().unwrap();
    init_series_schema(&conn).unwrap();
    insert_entity(&conn, "Cersei", "瑟曦", "Character", "Queen").unwrap();
    update_entity(
        &conn,
        "Cersei",
        "瑟曦·兰尼斯特",
        "Character",
        "Queen of the Seven Kingdoms",
    )
    .unwrap();
    let results = search_series_entities(&conn, "Cersei").unwrap();
    assert_eq!(results[0].translated_name, "瑟曦·兰尼斯特");
}
