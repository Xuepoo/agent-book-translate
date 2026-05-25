//! Configuration management.
//! Loads from an explicit TOML path or `$XDG_CONFIG_HOME/agent-book-translate/config.toml`.

use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AppConfig {
    pub base_url: String,
    pub api_key: String,
    pub default_model: String,
    pub concurrency: usize,
    pub bilingual: bool,
    pub max_spend_usd: Option<f64>,
    pub http_proxy: Option<String>,
    pub reasoning: ReasoningConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ReasoningConfig {
    pub enable: bool,
    pub intensity: ReasoningIntensity,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningIntensity {
    Low,
    Middle,
    High,
    Xhigh,
    Max,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            base_url: "https://openrouter.ai/api/v1".to_string(),
            api_key: String::new(),
            default_model: "mimo-v2.5-pro".to_string(),
            concurrency: 5,
            bilingual: false,
            max_spend_usd: None,
            http_proxy: None,
            reasoning: ReasoningConfig::default(),
        }
    }
}

impl Default for ReasoningConfig {
    fn default() -> Self {
        Self {
            enable: false,
            intensity: ReasoningIntensity::Low,
        }
    }
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        Self::load_from_path(None)
    }

    pub fn load_from_path(path: Option<&Path>) -> Result<Self> {
        let mut config = Self::default();
        if let Some(path) = path {
            if !path.exists() {
                return Err(AppError::Config(format!(
                    "config file does not exist: {}",
                    path.display()
                )));
            }
            config = config.merge(load_file_config(path)?);
        } else if let Some(path) = config_path()
            && path.exists()
        {
            config = config.merge(load_file_config(&path)?);
        }

        config.apply_environment_overrides();
        Ok(config)
    }

    fn apply_environment_overrides(&mut self) {
        if let Ok(value) = env::var("LLM_API_KEY")
            .or_else(|_| env::var("XIAOMI_API_KEY"))
            .or_else(|_| env::var("OPENAI_API_KEY"))
            && !value.is_empty()
        {
            self.api_key = value;
        }
        if let Ok(value) = env::var("LLM_BASE_URL").or_else(|_| env::var("XIAOMI_BASE_URL"))
            && !value.is_empty()
        {
            self.base_url = value;
        }
        if let Ok(value) = env::var("LLM_MODEL").or_else(|_| env::var("XIAOMI_MODEL"))
            && !value.is_empty()
        {
            self.default_model = value;
        }
        if let Ok(value) = env::var("HTTP_PROXY").or_else(|_| env::var("HTTPS_PROXY"))
            && !value.is_empty()
        {
            self.http_proxy = Some(value);
        }
    }

    fn merge(mut self, file_cfg: FileConfig) -> Self {
        if let Some(value) = file_cfg.base_url {
            self.base_url = value;
        }
        if let Some(value) = file_cfg.api_key {
            self.api_key = value;
        }
        if let Some(value) = file_cfg.default_model {
            self.default_model = value;
        }
        if let Some(value) = file_cfg.concurrency {
            self.concurrency = value;
        }
        if let Some(value) = file_cfg.bilingual {
            self.bilingual = value;
        }
        if let Some(value) = file_cfg.max_spend_usd {
            self.max_spend_usd = Some(value);
        }
        if let Some(value) = file_cfg.http_proxy {
            self.http_proxy = Some(value);
        }
        if let Some(value) = file_cfg.reasoning {
            self.reasoning = value.into();
        }
        self
    }
}

fn load_file_config(path: &Path) -> Result<FileConfig> {
    let raw = fs::read_to_string(path)?;
    toml::from_str(&raw)
        .map_err(|e| AppError::Config(format!("failed to parse {}: {e}", path.display())))
}

pub fn load_config_file(path: &Path) -> Result<AppConfig> {
    AppConfig::load_from_path(Some(path))
}

#[derive(Debug, Deserialize)]
struct FileConfig {
    base_url: Option<String>,
    api_key: Option<String>,
    default_model: Option<String>,
    concurrency: Option<usize>,
    bilingual: Option<bool>,
    max_spend_usd: Option<f64>,
    http_proxy: Option<String>,
    reasoning: Option<FileReasoningConfig>,
}

#[derive(Debug, Deserialize)]
struct FileReasoningConfig {
    enable: Option<bool>,
    intensity: Option<ReasoningIntensity>,
}

impl From<FileReasoningConfig> for ReasoningConfig {
    fn from(value: FileReasoningConfig) -> Self {
        Self {
            enable: value.enable.unwrap_or(false),
            intensity: value.intensity.unwrap_or(ReasoningIntensity::Low),
        }
    }
}

fn config_path() -> Option<PathBuf> {
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(dirs::config_dir)?;
    Some(base.join("agent-book-translate").join("config.toml"))
}
