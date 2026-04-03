use cpal::traits::{DeviceTrait, HostTrait};
use futures_util::StreamExt;
use kalosm::sound::*;
use tokio::sync::mpsc;

// --- Public API ---

pub enum TranscriptionEvent {
    ModelLoading { progress: f32, status: String },
    ModelReady,
    Segment(String),
    VadActivity(bool),
    Error(String),
}

/// Audio device with PipeWire/PulseAudio name and description
#[derive(Clone, Debug)]
pub struct AudioDevice {
    pub name: String,        // PulseAudio source name (for selection)
    pub description: String, // Human-readable description (for display)
}

/// List input devices using pactl (PipeWire/PulseAudio), with fallback to cpal
pub fn list_input_devices() -> Vec<AudioDevice> {
    if let Some(devices) = list_devices_pactl() {
        return devices;
    }
    // Fallback to cpal
    let host = cpal::default_host();
    let mut devices = Vec::new();
    if let Ok(input_devices) = host.input_devices() {
        for device in input_devices {
            if let Ok(name) = device.name() {
                devices.push(AudioDevice {
                    description: name.clone(),
                    name,
                });
            }
        }
    }
    devices
}

fn list_devices_pactl() -> Option<Vec<AudioDevice>> {
    let output = std::process::Command::new("pactl")
        .args(["list", "sources"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut devices = Vec::new();
    let mut current_name = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Name: ") {
            current_name = Some(trimmed.strip_prefix("Name: ").unwrap().to_string());
        } else if trimmed.starts_with("Description: ") {
            if let Some(name) = current_name.take() {
                let description = trimmed.strip_prefix("Description: ").unwrap().to_string();
                devices.push(AudioDevice { name, description });
            }
        }
    }
    if devices.is_empty() {
        None
    } else {
        Some(devices)
    }
}

pub fn default_device_name() -> Option<String> {
    // Try pactl to get default source
    if let Ok(output) = std::process::Command::new("pactl")
        .args(["get-default-source"])
        .output()
    {
        if output.status.success() {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    // Fallback to cpal
    let host = cpal::default_host();
    host.default_input_device().and_then(|d| d.name().ok())
}

fn whisper_source(model: WhisperModelSize) -> WhisperSource {
    match model {
        WhisperModelSize::Tiny => WhisperSource::Tiny,
        WhisperModelSize::Base => WhisperSource::Base,
        WhisperModelSize::Medium => WhisperSource::Medium,
        WhisperModelSize::LargeV3Turbo => WhisperSource::QuantizedLargeV3Turbo,
    }
}

fn whisper_language(lang: TranscriptionLanguage) -> Option<WhisperLanguage> {
    match lang {
        TranscriptionLanguage::English => Some(WhisperLanguage::English),
        TranscriptionLanguage::Chinese => Some(WhisperLanguage::Chinese),
        TranscriptionLanguage::Auto => None,
    }
}

pub async fn load_model(
    model_size: WhisperModelSize,
    language: TranscriptionLanguage,
    tx: mpsc::Sender<TranscriptionEvent>,
) -> anyhow::Result<Whisper> {
    let source = whisper_source(model_size);
    let lang = whisper_language(language);

    let tx_clone = tx.clone();
    let model = Whisper::builder()
        .with_source(source)
        .with_language(lang)
        .build_with_loading_handler(move |progress| match progress {
            ModelLoadingProgress::Downloading {
                source, progress, ..
            } => {
                let pct = if progress.size > 0 {
                    progress.progress as f32 / progress.size as f32
                } else {
                    0.0
                };
                let _ = tx_clone.try_send(TranscriptionEvent::ModelLoading {
                    progress: pct,
                    status: format!("Downloading {}...", source),
                });
            }
            ModelLoadingProgress::Loading { progress } => {
                let _ = tx_clone.try_send(TranscriptionEvent::ModelLoading {
                    progress,
                    status: "Loading model...".into(),
                });
            }
        })
        .await?;

    let _ = tx.send(TranscriptionEvent::ModelReady).await;
    Ok(model)
}

pub fn start_transcription(
    model: Whisper,
    device_name: Option<String>,
    tx: mpsc::Sender<TranscriptionEvent>,
) -> tokio::task::JoinHandle<()> {
    // Set PULSE_SOURCE so cpal/PipeWire picks up the selected device
    if let Some(ref name) = device_name {
        std::env::set_var("PULSE_SOURCE", name);
    } else {
        std::env::remove_var("PULSE_SOURCE");
    }

    tokio::spawn(async move {
        let mic = MicInput::default();
        let stream = mic.stream();
        let mut transcription = stream.transcribe(model);

        while let Some(segment) = transcription.next().await {
            let no_speech_prob = segment.probability_of_no_speech();
            let is_voice = no_speech_prob < 0.5;
            let _ = tx.send(TranscriptionEvent::VadActivity(is_voice)).await;

            if no_speech_prob < 0.9 {
                let text = segment.text().to_string();
                if !text.trim().is_empty() {
                    if tx.send(TranscriptionEvent::Segment(text)).await.is_err() {
                        break;
                    }
                }
            }
        }
    })
}

// --- Types used by app.rs and config.rs ---

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum TranscriptionLanguage {
    English,
    Chinese,
    Auto,
}

impl Default for TranscriptionLanguage {
    fn default() -> Self {
        Self::Auto
    }
}

impl TranscriptionLanguage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Chinese => "Chinese",
            Self::Auto => "Auto",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::English => Self::Chinese,
            Self::Chinese => Self::Auto,
            Self::Auto => Self::English,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Self::English => Self::Auto,
            Self::Chinese => Self::English,
            Self::Auto => Self::Chinese,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum WhisperModelSize {
    Tiny,
    Base,
    Medium,
    LargeV3Turbo,
}

impl Default for WhisperModelSize {
    fn default() -> Self {
        Self::Tiny
    }
}

impl WhisperModelSize {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Tiny => "Tiny",
            Self::Base => "Base",
            Self::Medium => "Medium",
            Self::LargeV3Turbo => "Large V3 Turbo",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::Tiny => Self::Base,
            Self::Base => Self::Medium,
            Self::Medium => Self::LargeV3Turbo,
            Self::LargeV3Turbo => Self::Tiny,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Self::Tiny => Self::LargeV3Turbo,
            Self::Base => Self::Tiny,
            Self::Medium => Self::Base,
            Self::LargeV3Turbo => Self::Medium,
        }
    }
}
