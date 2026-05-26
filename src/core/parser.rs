//! EPUB ZIP parser & HTML AST scraper.

use crate::error::{AppError, Result};
use scraper::{ElementRef, Html, Selector};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use zip::write::FileOptions;
use zip::{ZipArchive, ZipWriter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChunk {
    pub source_path: String,
    pub node_id: Option<String>,
    pub text: String,
}

impl TextChunk {
    pub fn with_source_path(mut self, source_path: impl Into<String>) -> Self {
        self.source_path = source_path.into();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedChunk {
    pub file_name: String,
    pub chunk_index: usize,
    pub original: String,
    pub translated: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpubEntry {
    pub name: String,
    pub data: Vec<u8>,
    pub is_text: bool,
}

pub fn extract_text_chunks(document: &Html) -> Vec<TextChunk> {
    let mut chunks = Vec::new();
    let selector = Selector::parse("p, h1, h2, h3, li").expect("valid selector");
    for (index, element) in document.select(&selector).enumerate() {
        let text = flatten_inline_text(&element);
        if !text.trim().is_empty() {
            chunks.push(TextChunk {
                source_path: String::new(),
                node_id: Some(format!("{}-{index}", element.value().name())),
                text,
            });
        }
    }
    chunks
}

pub fn extract_and_flatten_text(document: &Html) -> String {
    extract_text_chunks(document)
        .into_iter()
        .map(|chunk| chunk.text)
        .collect::<Vec<_>>()
        .join("")
}

pub fn render_bilingual_node(original_html: &str, translated: &str) -> String {
    let mut output = String::new();
    output.push_str(original_html);
    if !translated.trim().is_empty() {
        output.push_str(r#"<p class="translation">"#);
        output.push_str(translated);
        output.push_str("</p>");
    }
    output
}

pub fn render_translation_node(original_html: &str, translated: &str) -> String {
    let escaped_translation = escape_text(translated);
    if translated.trim().is_empty() {
        String::new()
    } else if let Some((start, end)) = find_first_block_tag(original_html) {
        let mut rendered = String::with_capacity(original_html.len() + escaped_translation.len());
        rendered.push_str(&original_html[..start]);
        rendered.push_str(&escaped_translation);
        rendered.push_str(&original_html[end..]);
        rendered
    } else {
        escaped_translation
    }
}

pub fn render_file_from_chunks(original_html: &str, chunks: &[RenderedChunk]) -> String {
    let mut rendered = original_html.to_string();
    let block_spans = find_translatable_block_spans(original_html);
    let mut ordered = chunks.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|chunk| Reverse(chunk.chunk_index));

    for chunk in ordered {
        if chunk.translated.trim().is_empty() {
            continue;
        }
        if let Some(span) = block_spans.get(chunk.chunk_index) {
            rendered.replace_range(
                span.inner_start..span.inner_end,
                &escape_text(&chunk.translated),
            );
            continue;
        }
        if let Some(updated) = replace_nth_occurrence(
            &rendered,
            &chunk.original,
            &escape_text(&chunk.translated),
            chunk.chunk_index,
        ) {
            rendered = updated;
        }
    }

    rendered
}

pub fn parse_epub(input: &Path) -> Result<Vec<EpubEntry>> {
    let file = File::open(input)?;
    let mut archive = ZipArchive::new(file).map_err(|e| AppError::Parse(e.to_string()))?;
    let mut files = Vec::new();

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| AppError::Parse(e.to_string()))?;
        let name = entry.name().to_string();
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        let is_text = name.ends_with(".xhtml") || name.ends_with(".html") || name.ends_with(".htm");
        files.push(EpubEntry {
            name,
            data,
            is_text,
        });
    }

    Ok(files)
}

pub fn write_epub(
    output: &Path,
    files: &[EpubEntry],
    rendered: &HashMap<String, String>,
) -> Result<()> {
    let file = File::create(output)?;
    let mut writer = ZipWriter::new(file);
    let options: FileOptions<'static, ()> =
        FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    writer
        .start_file("mimetype", options)
        .map_err(|e| AppError::Parse(e.to_string()))?;
    writer
        .write_all(b"application/epub+zip")
        .map_err(AppError::from)?;

    let deflated: FileOptions<'static, ()> =
        FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for entry in files {
        if entry.name == "mimetype" {
            continue;
        }
        let content = rendered
            .get(&entry.name)
            .map(String::as_bytes)
            .unwrap_or(&entry.data);
        writer
            .start_file(&entry.name, deflated)
            .map_err(|e| AppError::Parse(e.to_string()))?;
        writer.write_all(content)?;
    }

    writer
        .finish()
        .map_err(|e| AppError::Parse(e.to_string()))?;
    Ok(())
}

fn flatten_inline_text(element: &ElementRef<'_>) -> String {
    let mut text = String::new();
    for child in element.children() {
        if let Some(text_node) = child.value().as_text() {
            text.push_str(text_node);
        } else if let Some(child_el) = ElementRef::wrap(child) {
            match child_el.value().name() {
                "rt" | "img" | "svg" | "figure" | "video" => continue,
                "ruby" => text.push_str(&flatten_inline_text(&child_el)),
                _ => text.push_str(&flatten_inline_text(&child_el)),
            }
        }
    }
    text
}

fn find_first_block_tag(original_html: &str) -> Option<(usize, usize)> {
    original_html.find('>').map(|start| {
        let end = original_html.rfind('<').unwrap_or(original_html.len());
        (start + 1, end)
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BlockSpan {
    inner_start: usize,
    inner_end: usize,
}

fn find_translatable_block_spans(html: &str) -> Vec<BlockSpan> {
    let mut spans = Vec::new();
    let mut cursor = 0usize;

    while let Some(relative_start) = html[cursor..].find('<') {
        let tag_start = cursor + relative_start;
        let Some((tag_name, tag_end, self_closing)) = parse_start_tag(html, tag_start) else {
            cursor = tag_start + 1;
            continue;
        };
        if !is_translatable_block(&tag_name) || self_closing {
            cursor = tag_end;
            continue;
        }
        if let Some((closing_start, _closing_end)) =
            find_matching_close_tag(html, tag_end, &tag_name)
        {
            spans.push(BlockSpan {
                inner_start: tag_end,
                inner_end: closing_start,
            });
        }
        cursor = tag_end;
    }

    spans
}

fn parse_start_tag(html: &str, tag_start: usize) -> Option<(String, usize, bool)> {
    let after_open = tag_start.checked_add(1)?;
    let first = html.as_bytes().get(after_open).copied()? as char;
    if !first.is_ascii_alphabetic() {
        return None;
    }

    let mut name_end = after_open;
    for (offset, ch) in html[after_open..].char_indices() {
        if !(ch.is_ascii_alphanumeric() || ch == '-' || ch == ':') {
            name_end = after_open + offset;
            break;
        }
    }
    if name_end == after_open {
        return None;
    }

    let tag_end = find_tag_end(html, name_end)? + 1;
    let before_end = html[..tag_end].trim_end();
    Some((
        html[after_open..name_end].to_ascii_lowercase(),
        tag_end,
        before_end.ends_with("/>"),
    ))
}

fn find_matching_close_tag(html: &str, from: usize, tag_name: &str) -> Option<(usize, usize)> {
    let mut cursor = from;
    let mut depth = 1usize;

    while let Some(relative_start) = html[cursor..].find('<') {
        let tag_start = cursor + relative_start;
        if is_close_tag(html, tag_start, tag_name) {
            let tag_end = find_tag_end(html, tag_start)? + 1;
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some((tag_start, tag_end));
            }
            cursor = tag_end;
            continue;
        }

        if let Some((nested_name, tag_end, self_closing)) = parse_start_tag(html, tag_start) {
            if nested_name == tag_name && !self_closing {
                depth += 1;
            }
            cursor = tag_end;
        } else {
            cursor = tag_start + 1;
        }
    }

    None
}

fn is_close_tag(html: &str, tag_start: usize, tag_name: &str) -> bool {
    let Some(rest) = html.get(tag_start + 2..) else {
        return false;
    };
    html[tag_start..].starts_with("</")
        && rest
            .get(..tag_name.len())
            .map(|name| name.eq_ignore_ascii_case(tag_name))
            .unwrap_or(false)
}

fn find_tag_end(html: &str, from: usize) -> Option<usize> {
    let mut quote = None;
    for (offset, ch) in html[from..].char_indices() {
        match (quote, ch) {
            (Some(active), current) if active == current => quote = None,
            (None, '"' | '\'') => quote = Some(ch),
            (None, '>') => return Some(from + offset),
            _ => {}
        }
    }
    None
}

fn is_translatable_block(tag_name: &str) -> bool {
    matches!(tag_name, "p" | "h1" | "h2" | "h3" | "li")
}

fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn replace_nth_occurrence(
    haystack: &str,
    needle: &str,
    replacement: &str,
    nth: usize,
) -> Option<String> {
    if needle.is_empty() {
        return None;
    }

    let mut start_at = 0usize;
    let mut occurrence = 0usize;
    while let Some(relative) = haystack[start_at..].find(needle) {
        let match_start = start_at + relative;
        if occurrence == nth {
            let mut output = String::with_capacity(
                haystack.len().saturating_sub(needle.len()) + replacement.len(),
            );
            output.push_str(&haystack[..match_start]);
            output.push_str(replacement);
            output.push_str(&haystack[match_start + needle.len()..]);
            return Some(output);
        }
        occurrence += 1;
        start_at = match_start + needle.len();
    }

    None
}
