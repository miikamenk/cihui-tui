use crate::language::Language;
#[cfg(feature = "transcription")]
use crate::transcription::{TranscriptionLanguage, WhisperModelSize};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum TranslationService {
    #[default]
    Auto,
    Google,
    MyMemory,
    LibreTranslate,
    #[serde(alias = "LocalModel")]
    LTEngine,
}

impl TranslationService {
    pub fn all() -> &'static [TranslationService] {
        &[
            TranslationService::Auto,
            TranslationService::Google,
            TranslationService::MyMemory,
            TranslationService::LibreTranslate,
            TranslationService::LTEngine,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            TranslationService::Auto => "Auto",
            TranslationService::Google => "Google Translate",
            TranslationService::MyMemory => "MyMemory",
            TranslationService::LibreTranslate => "LibreTranslate",
            TranslationService::LTEngine => "LTEngine (Local)",
        }
    }

    pub fn next(&self) -> Self {
        let all = Self::all();
        let idx = all.iter().position(|s| s == self).unwrap_or(0);
        all[(idx + 1) % all.len()]
    }

    pub fn prev(&self) -> Self {
        let all = Self::all();
        let idx = all.iter().position(|s| s == self).unwrap_or(0);
        if idx == 0 {
            all[all.len() - 1]
        } else {
            all[idx - 1]
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub target_language: Language,
    #[serde(default)]
    pub translation_service: TranslationService,
    #[serde(default = "default_local_translate_url")]
    pub local_translate_url: String,
    #[serde(default = "default_ltengine_model")]
    pub ltengine_model: String,
    #[serde(default = "default_ltengine_path")]
    pub ltengine_path: String,
    #[cfg(feature = "transcription")]
    #[serde(default)]
    pub transcription_language: TranscriptionLanguage,
    #[cfg(feature = "transcription")]
    #[serde(default)]
    pub transcription_device: Option<String>,
    #[cfg(feature = "transcription")]
    #[serde(default)]
    pub whisper_model: WhisperModelSize,
}

fn default_local_translate_url() -> String {
    "http://localhost:5050".to_string()
}

fn default_ltengine_model() -> String {
    "gemma3-4b".to_string()
}

fn default_ltengine_path() -> String {
    "ltengine".to_string()
}

impl Default for Config {
    #[cfg(feature = "transcription")]
    fn default() -> Self {
        Self {
            target_language: Language::English,
            translation_service: TranslationService::default(),
            local_translate_url: default_local_translate_url(),
            ltengine_model: default_ltengine_model(),
            ltengine_path: default_ltengine_path(),
            transcription_language: TranscriptionLanguage::default(),
            transcription_device: None,
            whisper_model: WhisperModelSize::default(),
        }
    }

    #[cfg(not(feature = "transcription"))]
    fn default() -> Self {
        Self {
            target_language: Language::English,
            translation_service: TranslationService::default(),
            local_translate_url: default_local_translate_url(),
            ltengine_model: default_ltengine_model(),
            ltengine_path: default_ltengine_path(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        Self::load_from(&Self::config_path()?)
    }

    /// Load a config from an explicit path. Returns the defaults if the file
    /// does not exist, and an error if it exists but cannot be parsed.
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::config_path()?)
    }

    /// Write the config to an explicit path, creating parent directories.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    fn config_path() -> Result<PathBuf> {
        // Get home directory from environment variable
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| anyhow::anyhow!("Could not find home directory"))?;
        let config_dir = PathBuf::from(home).join(".config").join("cihui-tui");
        Ok(config_dir.join("config.json"))
    }
}
