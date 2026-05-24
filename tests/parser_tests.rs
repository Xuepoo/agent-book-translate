use agent_book_translate::core::parser::{
    extract_and_flatten_text, extract_text_chunks, render_bilingual_node,
};
use scraper::Html;

#[test]
fn ruby_text_flattening() {
    let raw_xhtml = r#"<div class="chapter"><p>これは<ruby>漢<rt>かん</rt>字<rt>じ</rt></ruby>です。</p></div>"#;
    let document = Html::parse_document(raw_xhtml);
    assert_eq!(extract_and_flatten_text(&document), "これは漢字です。");
}

#[test]
fn bypass_images_extraction() {
    let raw_xhtml = r#"<p>Text before <img src="image.jpg" id="fig1"/> text after.</p>"#;
    let document = Html::parse_document(raw_xhtml);
    let text_chunks = extract_text_chunks(&document);
    assert_eq!(text_chunks.len(), 1);
    assert_eq!(text_chunks[0], "Text before  text after.");
}

#[test]
fn bilingual_injection_preserves_images() {
    let original_html =
        r#"<p id="p1">Text before <img src="image.jpg" id="fig1"/> text after.</p>"#;
    let output = render_bilingual_node(original_html, "翻译");
    assert!(output.contains(r#"<img src="image.jpg" id="fig1""#));
    assert!(output.contains(r#"class="translation""#));
}

#[test]
fn drop_cap_splicing() {
    let raw_xhtml = r#"<p><span class="dropcap">O</span>nce upon a time...</p>"#;
    let document = Html::parse_document(raw_xhtml);
    assert_eq!(extract_and_flatten_text(&document), "Once upon a time...");
}
