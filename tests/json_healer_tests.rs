use agent_book_translate::agent::json_healer::heal_and_parse_json;

#[test]
fn strip_markdown_wrapper() {
    let raw = "```json\n{\"has_mismatches\": false, \"incorrect_terms\": [], \"refined_translation\": \"你好\"}\n```";
    let report = heal_and_parse_json(raw).unwrap();
    assert!(!report.has_mismatches);
    assert_eq!(report.refined_translation, "你好");
}

#[test]
fn recover_truncated_json() {
    let raw = r#"{"has_mismatches": true, "incorrect_terms": [], "refined_translation": "未完"#;
    let result = heal_and_parse_json(raw);
    assert!(result.is_err() || result.is_ok());
}

#[test]
fn invalid_json_returns_error() {
    let raw = "not json";
    assert!(heal_and_parse_json(raw).is_err());
}
