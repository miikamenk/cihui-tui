use crate::language::Language;
use crate::transcription::{TranscriptionLanguage, WhisperModelSize};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TranslationService {
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
    #[serde(default)]
    pub transcription_language: TranscriptionLanguage,
    #[serde(default)]
    pub transcription_device: Option<String>,
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

impl Default for TranslationService {
    fn default() -> Self {
        TranslationService::Auto
    }
}

impl Default for Config {
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
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path()?;

        if !config_path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&config_path)?;
        let config: Config = serde_json::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path()?;

        // Create config directory if it doesn't exist
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(self)?;
        fs::write(&config_path, content)?;
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
