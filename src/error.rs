//! Unified error types for agent-book-translate.

use std::io;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("translation error: {0}")]
    Translation(String),

    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("external command failed: {0}")]
    Command(String),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, AppError>;
