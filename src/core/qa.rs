//! EPUB quality assurance checks: archive integrity, JSON leakage scan,
//! and sampled XHTML content validation.

use crate::error::{AppError, Result};
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

/// JSON wrapper patterns that must not appear in rendered EPUB output.
const LEAKAGE_PATTERNS: &[&str] = &[
    r#"{"translation""#,
    r#"{"role""#,
    "refined_translation",
    "incorrect_terms",
];

#[derive(Debug, Default)]
pub struct QaReport {
    pub archive_ok: bool,
    pub leakage_hits: Vec<LeakageHit>,
    pub sample_failures: Vec<String>,
    pub text_file_count: usize,
}

#[derive(Debug, Clone)]
pub struct LeakageHit {
    pub file_name: String,
    pub pattern: String,
    pub line_number: usize,
    pub line: String,
}

impl QaReport {
    pub fn passed(&self) -> bool {
        self.archive_ok && self.leakage_hits.is_empty() && self.sample_failures.is_empty()
    }

    pub fn print_summary(&self) {
        if self.archive_ok {
            println!("[PASS] archive integrity: ok");
        } else {
            println!("[FAIL] archive integrity: corrupt or unreadable");
        }

        println!(
            "[INFO] text files: {}  leakage hits: {}  sample failures: {}",
            self.text_file_count,
            self.leakage_hits.len(),
            self.sample_failures.len()
        );

        for hit in &self.leakage_hits {
            println!(
                "[FAIL] leakage in {}:{} pattern={:?} line={:?}",
                hit.file_name, hit.line_number, hit.pattern, hit.line
            );
        }

        for failure in &self.sample_failures {
            println!("[FAIL] sample check: {failure}");
        }

        if self.passed() {
            println!("[PASS] all QA checks passed");
        } else {
            println!("[FAIL] QA checks failed");
        }
    }
}

/// Run all QA checks on an EPUB file and return a structured report.
pub fn run_epub_qa(epub_path: &Path) -> Result<QaReport> {
    let file = std::fs::File::open(epub_path)
        .map_err(|e| AppError::Config(format!("cannot open epub: {e}")))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|e| AppError::Config(format!("cannot open epub as zip: {e}")))?;

    let archive_ok = verify_archive_integrity(&mut archive);

    // Collect text file names in index order for reproducible sampling.
    let text_files = collect_text_file_names(&mut archive);

    let mut report = QaReport {
        archive_ok,
        text_file_count: text_files.len(),
        ..QaReport::default()
    };

    // ── Phase 2: JSON leakage scan ──────────────────────────────────────────
    for name in &text_files {
        let content = match read_zip_entry(&mut archive, name) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for (line_number, line) in content.lines().enumerate() {
            for pattern in LEAKAGE_PATTERNS {
                if line.contains(pattern) {
                    report.leakage_hits.push(LeakageHit {
                        file_name: name.clone(),
                        pattern: pattern.to_string(),
                        line_number: line_number + 1,
                        line: line.chars().take(120).collect(),
                    });
                }
            }
        }
    }

    // ── Phase 3: sampled content check ─────────────────────────────────────
    let sample_indices = sample_indices(text_files.len());
    for idx in sample_indices {
        let name = &text_files[idx];
        match read_zip_entry(&mut archive, name) {
            Ok(content) if content.trim().is_empty() => {
                report
                    .sample_failures
                    .push(format!("{name}: sampled file is empty"));
            }
            Err(e) => {
                report
                    .sample_failures
                    .push(format!("{name}: read error: {e}"));
            }
            Ok(_) => {}
        }
    }

    Ok(report)
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn verify_archive_integrity(archive: &mut ZipArchive<std::fs::File>) -> bool {
    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => return false,
        };
        let mut buf = Vec::new();
        if entry.read_to_end(&mut buf).is_err() {
            return false;
        }
    }
    true
}

fn collect_text_file_names(archive: &mut ZipArchive<std::fs::File>) -> Vec<String> {
    (0..archive.len())
        .filter_map(|i| {
            let entry = archive.by_index(i).ok()?;
            let name = entry.name().to_string();
            if is_text_entry(&name) {
                Some(name)
            } else {
                None
            }
        })
        .collect()
}

fn is_text_entry(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".html") || lower.ends_with(".xhtml") || lower.ends_with(".htm")
}

fn read_zip_entry(archive: &mut ZipArchive<std::fs::File>, name: &str) -> Result<String> {
    let mut entry = archive
        .by_name(name)
        .map_err(|e| AppError::Config(format!("zip entry {name}: {e}")))?;
    let mut buf = Vec::new();
    entry
        .read_to_end(&mut buf)
        .map_err(|e| AppError::Config(format!("read {name}: {e}")))?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Return the indices of the first, middle, and last text files to sample.
/// Deduplicates naturally (e.g. when count == 1 all three are the same index).
fn sample_indices(count: usize) -> Vec<usize> {
    if count == 0 {
        return vec![];
    }
    let first = 0;
    let last = count - 1;
    let mid = count / 2;
    let mut indices = vec![first, mid, last];
    indices.sort_unstable();
    indices.dedup();
    indices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_indices_single_entry() {
        assert_eq!(sample_indices(1), vec![0]);
    }

    #[test]
    fn sample_indices_two_entries() {
        let idx = sample_indices(2);
        assert!(idx.contains(&0));
        assert!(idx.contains(&1));
    }

    #[test]
    fn sample_indices_many_entries() {
        let idx = sample_indices(10);
        assert_eq!(idx[0], 0);
        assert_eq!(*idx.last().unwrap(), 9);
    }

    #[test]
    fn leakage_patterns_cover_known_wrappers() {
        let content = r#"<p>{"translation": "some text"}</p>"#;
        let hit = LEAKAGE_PATTERNS.iter().any(|p| content.contains(p));
        assert!(hit, "leakage pattern must match translation wrapper");
    }

    #[test]
    fn leakage_patterns_do_not_match_clean_content() {
        let content = "<p>这是干净的中文段落。</p>";
        let hit = LEAKAGE_PATTERNS.iter().any(|p| content.contains(p));
        assert!(!hit, "clean content must not trigger leakage patterns");
    }
}
