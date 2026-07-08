//! Lean configuration: plain TOML into serde structs with per-section
//! defaults — deliberately no `config` crate and no generic env-override
//! matrix (flags cover per-run tweaks). Precedence: defaults ->
//! `~/.config/spqx/config.toml` -> `--config`/`SPQX_CONFIG` path override.
//!
//! On first run the default config is written with a `# @schema` header and
//! `config.schema.json` (schemars draft-07) alongside, so TOML LSPs validate
//! and complete it.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::paths;

/// spqx configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Config {
    /// Model selection.
    pub model: ModelConfig,
    /// Default voice used by `say`/`read` when `--voice` is not given.
    pub voice: VoiceConfig,
    /// Audio output settings.
    pub audio: AudioConfig,
    /// Sampling parameters passed to the engine.
    pub sampling: SamplingConfig,
    /// OpenAI-compatible API server settings (`spqx api`).
    pub api: ApiConfig,
    /// Directory overrides.
    pub paths: PathsConfig,
}

/// Model selection.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ModelConfig {
    /// Hugging Face model id resolved from the local HF cache.
    pub id: String,
    /// Explicit local model directory; takes precedence over `id`.
    pub path: Option<String>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            id: "mlx-community/Qwen3-TTS-12Hz-0.6B-Base-6bit".to_string(),
            path: None,
        }
    }
}

/// Default voice.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct VoiceConfig {
    /// Registry voice name (see `spqx voices`) or a CustomVoice preset
    /// speaker (serena, vivian, uncle_fu, ryan, aiden, ono_anna, sohee,
    /// eric, dylan).
    pub default: String,
    /// Default language passed to the engine (`auto` detects).
    pub language: String,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            default: "aiden".to_string(),
            language: "auto".to_string(),
        }
    }
}

/// Audio output settings.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct AudioConfig {
    /// PCM sample rate for synthesis output.
    pub sample_rate_hz: u32,
    /// Directory for `--out` files given as bare names.
    pub output_dir: Option<String>,
    /// Play audio live by default (`--no-play` overrides per run).
    pub play: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate_hz: 24_000,
            output_dir: None,
            play: true,
        }
    }
}

/// Engine sampling parameters.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SamplingConfig {
    pub temperature: f64,
    pub top_k: i64,
}

impl Default for SamplingConfig {
    fn default() -> Self {
        Self {
            temperature: 0.9,
            top_k: 50,
        }
    }
}

/// OpenAI-compatible API server settings.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ApiConfig {
    pub ip: String,
    pub port: u16,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            ip: "127.0.0.1".to_string(),
            port: 8331,
        }
    }
}

/// Directory overrides (defaults are XDG-derived).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct PathsConfig {
    /// Voice registry directory (default: `~/.local/share/spqx/voices`).
    pub voices_dir: Option<String>,
    /// Model cache override (default: the Hugging Face cache).
    pub models_dir: Option<String>,
}

impl Config {
    /// Load the effective config. `override_path` comes from `--config` or
    /// `SPQX_CONFIG`; when absent the XDG config is used and created on
    /// first run (together with its JSON schema).
    pub fn load(override_path: Option<&Path>) -> Result<Self> {
        if let Some(path) = override_path {
            return Self::load_from(path);
        }
        let path = paths::config_file()?;
        if !path.exists() {
            Self::default().write_default(&path)?;
            return Ok(Self::default());
        }
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("reading config file {}", path.display()))?;
        toml::from_str(&content).with_context(|| format!("parsing config file {}", path.display()))
    }

    /// Write the default config with a schema header, and the schema next to
    /// it. Only overwrites the config when `path` does not exist.
    pub fn write_default(&self, path: &Path) -> Result<PathBuf> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating config directory {}", parent.display()))?;
        }
        let schema_path = paths::schema_file()?;
        fs::write(&schema_path, Self::schema_json()?)
            .with_context(|| format!("writing schema {}", schema_path.display()))?;
        if !path.exists() {
            let body = format!(
                "# @schema ./config.schema.json\n# spqx configuration — `spqx config schema` prints the JSON schema.\n\n{}",
                toml::to_string_pretty(self).context("serializing default config")?
            );
            fs::write(path, body)
                .with_context(|| format!("writing config file {}", path.display()))?;
        }
        Ok(schema_path)
    }

    pub fn schema_json() -> Result<String> {
        let schema = schemars::schema_for!(Config);
        serde_json::to_string_pretty(&schema).context("serializing config schema")
    }
}
