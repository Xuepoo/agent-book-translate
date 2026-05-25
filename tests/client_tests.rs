use agent_book_translate::agent::client::parse_translation_content;

#[test]
fn nested_assistant_content_is_unwrapped() {
    let raw = r#"{"role": "assistant", "content": "The Project Gutenberg™ Full License"}"#;

    let translation = parse_translation_content(raw).unwrap();

    assert_eq!(translation, "The Project Gutenberg™ Full License");
}

#[test]
fn critique_report_returns_refined_translation() {
    let raw = r#"{"has_mismatches": false, "incorrect_terms": [], "refined_translation": "译文"}"#;

    let translation = parse_translation_content(raw).unwrap();

    assert_eq!(translation, "译文");
}

#[test]
fn message_field_is_used_as_translation_fallback() {
    let raw = r#"{"status":"success","message":"你好世界","ai_persona":"helpful assistant"}"#;

    let translation = parse_translation_content(raw).unwrap();

    assert_eq!(translation, "你好世界");
}

#[test]
fn malformed_translation_wrapper_is_unwrapped() {
    let raw = r#"{"translation": "他说："你好。""}"#;

    let translation = parse_translation_content(raw).unwrap();

    assert_eq!(translation, "他说：\"你好。\"");
}
