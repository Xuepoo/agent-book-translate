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
    if translated.trim().is_empty() {
        String::new()
    } else if let Some((start, end)) = find_first_block_tag(original_html) {
        let mut rendered = String::with_capacity(original_html.len() + translated.len());
        rendered.push_str(&original_html[..start]);
        rendered.push_str(translated);
        rendered.push_str(&original_html[end..]);
        rendered
    } else {
        translated.to_string()
    }
}

pub fn render_file_from_chunks(original_html: &str, chunks: &[RenderedChunk]) -> String {
    let mut rendered = original_html.to_string();
    let mut ordered = chunks.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|chunk| Reverse(chunk.chunk_index));

    for chunk in ordered {
        if chunk.translated.trim().is_empty() {
            continue;
        }
        if let Some(updated) = replace_nth_occurrence(
            &rendered,
            &chunk.original,
            &chunk.translated,
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
