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
    pub target_language: String,
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
            default_model: "gpt-4o-mini".to_string(),
            concurrency: 5,
            bilingual: false,
            max_spend_usd: None,
            http_proxy: None,
            reasoning: ReasoningConfig::default(),
            target_language: "Chinese".to_string(),
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

pub fn expand_env_vars(input: &str) -> String {
    let re =
        regex::Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}|\$([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    re.replace_all(input, |caps: &regex::Captures| {
        let var_name = caps.get(1).or(caps.get(2)).unwrap().as_str();
        env::var(var_name).unwrap_or_else(|_| caps[0].to_string())
    })
    .to_string()
}

pub fn expand_path(path: &Path) -> Result<PathBuf> {
    let s = path
        .to_str()
        .ok_or_else(|| AppError::Config("invalid UTF-8 in path".to_string()))?;

    // 1. Expand ~ to home directory
    let expanded = if s.starts_with("~/") || s == "~" {
        let home = dirs::home_dir()
            .ok_or_else(|| AppError::Config("cannot resolve home dir".to_string()))?;
        if s == "~" { home } else { home.join(&s[2..]) }
    } else {
        // 2. Expand $VAR / ${VAR}
        PathBuf::from(expand_env_vars(s))
    };

    // 3. Convert relative to absolute
    if expanded.is_absolute() {
        Ok(expanded)
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(expanded))
            .map_err(|e| AppError::Config(format!("cannot resolve cwd: {e}")))
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
        if let Ok(value) = env::var("LLM_API_KEY").or_else(|_| env::var("OPENAI_API_KEY"))
            && !value.is_empty()
        {
            self.api_key = value;
        }
        if let Ok(value) = env::var("LLM_BASE_URL").or_else(|_| env::var("OPENAI_BASE_URL"))
            && !value.is_empty()
        {
            self.base_url = value;
        }
        if let Ok(value) = env::var("LLM_MODEL").or_else(|_| env::var("OPENAI_MODEL"))
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
        if let Some(value) = file_cfg.target_language {
            self.target_language = value;
        }
        self
    }
}

fn load_file_config(path: &Path) -> Result<FileConfig> {
    let raw = fs::read_to_string(path)?;
    let mut file_cfg: FileConfig = toml::from_str(&raw)
        .map_err(|e| AppError::Config(format!("failed to parse {}: {e}", path.display())))?;

    file_cfg.base_url = file_cfg.base_url.map(|s| expand_env_vars(&s));
    file_cfg.api_key = file_cfg.api_key.map(|s| expand_env_vars(&s));
    file_cfg.default_model = file_cfg.default_model.map(|s| expand_env_vars(&s));
    file_cfg.http_proxy = file_cfg.http_proxy.map(|s| expand_env_vars(&s));
    file_cfg.target_language = file_cfg.target_language.map(|s| expand_env_vars(&s));

    Ok(file_cfg)
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
    target_language: Option<String>,
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
