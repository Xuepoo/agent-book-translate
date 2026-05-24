//! Format routing via Calibre CLI (`ebook-convert`).

use crate::error::{AppError, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn has_calibre() -> bool {
    Command::new("ebook-convert")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

pub fn convert_to_epub(input: &Path, output: &Path) -> Result<PathBuf> {
    if input
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("epub"))
        .unwrap_or(false)
    {
        return Ok(input.to_path_buf());
    }

    if !has_calibre() {
        return Err(AppError::UnsupportedFormat(format!(
            "ebook-convert is required to convert {}",
            input.display()
        )));
    }

    let status = Command::new("ebook-convert")
        .arg(input)
        .arg(output)
        .status()?;

    if status.success() {
        Ok(output.to_path_buf())
    } else {
        Err(AppError::Command(format!(
            "ebook-convert failed for {}",
            input.display()
        )))
    }
}

pub fn convert_from_epub(input: &Path, output: &Path) -> Result<()> {
    if output
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("epub"))
        .unwrap_or(false)
    {
        std::fs::copy(input, output)?;
        return Ok(());
    }

    if !has_calibre() {
        return Err(AppError::UnsupportedFormat(format!(
            "ebook-convert is required to convert {}",
            output.display()
        )));
    }

    let status = Command::new("ebook-convert")
        .arg(input)
        .arg(output)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(AppError::Command(format!(
            "ebook-convert failed for {}",
            output.display()
        )))
    }
}
