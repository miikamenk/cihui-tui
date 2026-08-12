use cpal::traits::{DeviceTrait, HostTrait};
use cpal::SampleFormat;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

const WHISPER_SAMPLE_RATE: u32 = 16000;

/// Handle to a running transcription session
pub struct TranscriptionHandle {
    pub transcription_task: tokio::task::JoinHandle<()>,
    pub audio_thread: Option<std::thread::JoinHandle<()>>,
    shutdown_signal: Arc<AtomicBool>,
}

impl TranscriptionHandle {
    /// Signal the transcription session to stop and clean up resources
    pub fn stop(&self) {
        self.shutdown_signal.store(true, Ordering::SeqCst);
    }

    /// Stop and wait for cleanup
    pub fn stop_and_wait(self) {
        self.stop();
        self.transcription_task.abort();
        if let Some(audio_thread) = self.audio_thread {
            let _ = audio_thread.join();
        }
    }
}

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

/// List input devices using cpal first (for device name matching), with fallback to pactl
pub fn list_input_devices() -> Vec<AudioDevice> {
    // First try cpal device enumeration for better name compatibility
    let host = cpal::default_host();
    let mut devices = Vec::new();

    if let Ok(input_devices) = host.input_devices() {
        for device in input_devices {
            if let Ok(name) = device.name() {
                // cpal device names might need cleaning up
                let description = name.clone();
                devices.push(AudioDevice { name, description });
            }
        }
    }

    if !devices.is_empty() {
        return devices;
    }

    // Fallback to pactl
    if let Some(pactl_devices) = list_devices_pactl() {
        return pactl_devices;
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
    let host = cpal::default_host();
    host.default_input_device().and_then(|d| d.name().ok())
}

fn model_repo_file(model: WhisperModelSize) -> (&'static str, &'static str) {
    // Returns (repo_id, filename) on HuggingFace
    match model {
        WhisperModelSize::Tiny => ("ggerganov/whisper.cpp", "ggml-tiny.bin"),
        WhisperModelSize::Base => ("ggerganov/whisper.cpp", "ggml-base.bin"),
        WhisperModelSize::Medium => ("ggerganov/whisper.cpp", "ggml-medium.bin"),
        WhisperModelSize::LargeV3Turbo => ("ggerganov/whisper.cpp", "ggml-large-v3-turbo.bin"),
    }
}

fn whisper_language_code(lang: TranscriptionLanguage) -> Option<&'static str> {
    match lang {
        TranscriptionLanguage::English => Some("en"),
        TranscriptionLanguage::Chinese => Some("zh"),
        TranscriptionLanguage::Auto => None,
    }
}

/// Download model from HuggingFace if needed, return local path
fn download_model(
    model_size: WhisperModelSize,
    tx: &mpsc::Sender<TranscriptionEvent>,
) -> anyhow::Result<PathBuf> {
    let (repo, filename) = model_repo_file(model_size);

    let _ = tx.try_send(TranscriptionEvent::ModelLoading {
        progress: 0.0,
        status: format!("Downloading {}...", filename),
    });

    let api = hf_hub::api::sync::Api::new()?;
    let repo = api.model(repo.to_string());
    let path = repo.get(filename)?;

    let _ = tx.try_send(TranscriptionEvent::ModelLoading {
        progress: 1.0,
        status: "Loading model...".into(),
    });

    Ok(path)
}

pub async fn load_model(
    model_size: WhisperModelSize,
    _language: TranscriptionLanguage,
    tx: mpsc::Sender<TranscriptionEvent>,
) -> anyhow::Result<Arc<WhisperContext>> {
    let ctx = tokio::task::spawn_blocking(move || -> anyhow::Result<Arc<WhisperContext>> {
        let model_path = download_model(model_size, &tx)?;

        let _ = tx.try_send(TranscriptionEvent::ModelLoading {
            progress: 0.5,
            status: "Loading model into memory...".into(),
        });

        let ctx = WhisperContext::new_with_params(
            model_path.to_str().unwrap(),
            WhisperContextParameters::default(),
        )
        .map_err(|e| anyhow::anyhow!("Failed to load whisper model: {}", e))?;

        let _ = tx.try_send(TranscriptionEvent::ModelReady);
        Ok(Arc::new(ctx))
    })
    .await??;

    Ok(ctx)
}

pub fn start_transcription(
    ctx: Arc<WhisperContext>,
    language: TranscriptionLanguage,
    device_name: Option<String>,
    tx: mpsc::Sender<TranscriptionEvent>,
) -> TranscriptionHandle {
    let lang_code = whisper_language_code(language);

    // Audio capture uses cpal which is !Send, so run the capture setup
    // on a dedicated thread and communicate via shared buffer
    let audio_buf: Arc<std::sync::Mutex<Vec<f32>>> =
        Arc::new(std::sync::Mutex::new(Vec::with_capacity(32000)));
    let sample_rate_holder: Arc<std::sync::Mutex<u32>> =
        Arc::new(std::sync::Mutex::new(WHISPER_SAMPLE_RATE));

    // Shutdown signal for the audio thread
    let shutdown_signal: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    // Start audio capture on a std thread (not tokio) since cpal Stream is !Send
    let audio_buf_capture = audio_buf.clone();
    let sample_rate_capture = sample_rate_holder.clone();
    let tx_err = tx.clone();
    let device_name_capture = device_name.clone();
    let shutdown_signal_capture = shutdown_signal.clone();
    let audio_thread = std::thread::spawn(move || {
        if let Err(e) = run_audio_capture(
            audio_buf_capture,
            sample_rate_capture,
            device_name_capture,
            shutdown_signal_capture,
        ) {
            let _ = tx_err.try_send(TranscriptionEvent::Error(format!("Audio error: {}", e)));
        }
    });

    // Transcription loop on tokio
    let handle = tokio::spawn(async move {
        let chunk_duration_secs = 2.0;

        loop {
            tokio::time::sleep(std::time::Duration::from_secs_f64(chunk_duration_secs)).await;

            let sample_rate = *sample_rate_holder.lock().unwrap();
            let samples = {
                let mut buf = audio_buf.lock().unwrap();
                std::mem::take(&mut *buf)
            };

            if samples.is_empty() {
                continue;
            }

            // Resample to 16kHz if needed
            let samples = if sample_rate != WHISPER_SAMPLE_RATE {
                resample(&samples, sample_rate, WHISPER_SAMPLE_RATE)
            } else {
                samples
            };

            // Simple VAD: check RMS energy
            let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
            let is_voice = rms > 0.005;
            let _ = tx.send(TranscriptionEvent::VadActivity(is_voice)).await;

            if !is_voice {
                continue;
            }

            // Run whisper inference in blocking thread
            let ctx = ctx.clone();
            let tx = tx.clone();
            tokio::task::spawn_blocking(move || {
                let mut state = match ctx.create_state() {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = tx.try_send(TranscriptionEvent::Error(format!(
                            "Failed to create state: {}",
                            e
                        )));
                        return;
                    }
                };

                let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
                params.set_language(lang_code);
                params.set_print_progress(false);
                params.set_print_realtime(false);
                params.set_print_timestamps(false);
                params.set_no_context(true);
                params.set_single_segment(false);

                if let Err(e) = state.full(params, &samples) {
                    let _ =
                        tx.try_send(TranscriptionEvent::Error(format!("Inference error: {}", e)));
                    return;
                }

                let n_segments = state.full_n_segments();

                for i in 0..n_segments {
                    if let Some(segment) = state.get_segment(i) {
                        if let Ok(text) = segment.to_str() {
                            let text = text.trim().to_string();
                            if !text.is_empty() {
                                let _ = tx.try_send(TranscriptionEvent::Segment(text));
                            }
                        }
                    }
                }
            })
            .await
            .ok();
        }
    });

    TranscriptionHandle {
        transcription_task: handle,
        audio_thread: Some(audio_thread),
        shutdown_signal,
    }
}

fn run_audio_capture(
    audio_buf: Arc<std::sync::Mutex<Vec<f32>>>,
    sample_rate_out: Arc<std::sync::Mutex<u32>>,
    device_name: Option<String>,
    shutdown_signal: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    use cpal::traits::StreamTrait;

    // Set PULSE_SOURCE environment variable BEFORE initializing cpal
    // This tells PipeWire/PulseAudio which source to use as the default
    if let Some(ref name) = device_name {
        eprintln!("[Audio] Setting PULSE_SOURCE to: {}", name);
        std::env::set_var("PULSE_SOURCE", name);
    } else {
        std::env::remove_var("PULSE_SOURCE");
    }

    let host = cpal::default_host();

    // Try to find the requested device by name, otherwise use default
    let device = if let Some(ref name) = device_name {
        eprintln!("[Audio] Looking for device: {}", name);

        // Search for device by name with more flexible matching
        let mut found_device = None;
        let name_lower = name.to_lowercase();

        if let Ok(devices) = host.input_devices() {
            let device_list: Vec<_> = devices.collect();
            eprintln!("[Audio] Found {} cpal devices", device_list.len());

            // First pass: try exact or substring match
            for d in &device_list {
                if let Ok(d_name) = d.name() {
                    eprintln!("[Audio] Checking device: {}", d_name);
                    let d_name_lower = d_name.to_lowercase();

                    // Check for exact match
                    if d_name_lower == name_lower {
                        eprintln!("[Audio] Found exact match: {}", d_name);
                        found_device = Some(d.clone());
                        break;
                    }

                    // Check if either contains the other
                    if d_name_lower.contains(&name_lower) || name_lower.contains(&d_name_lower) {
                        eprintln!("[Audio] Found substring match: {}", d_name);
                        found_device = Some(d.clone());
                        break;
                    }
                }
            }

            // Second pass: word-based matching
            if found_device.is_none() {
                let name_words: Vec<&str> = name_lower.split_whitespace().collect();
                for d in &device_list {
                    if let Ok(d_name) = d.name() {
                        let d_name_lower = d_name.to_lowercase();
                        // Check if any significant word matches
                        for word in &name_words {
                            if word.len() > 3 && d_name_lower.contains(word) {
                                eprintln!("[Audio] Found word match ({}): {}", word, d_name);
                                found_device = Some(d.clone());
                                break;
                            }
                        }
                        if found_device.is_some() {
                            break;
                        }
                    }
                }
            }
        }

        if found_device.is_none() {
            eprintln!("[Audio] No matching device found, using default");
        }

        found_device.or_else(|| host.default_input_device())
    } else {
        eprintln!("[Audio] No device specified, using default");
        host.default_input_device()
    }
    .ok_or_else(|| anyhow::anyhow!("No input device found"))?;

    let device_name_str = device.name().unwrap_or_else(|_| "unknown".to_string());
    eprintln!("[Audio] Using device: {}", device_name_str);

    let config = device.default_input_config()?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    *sample_rate_out.lock().unwrap() = sample_rate;

    let sample_format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();

    let buf = audio_buf.clone();
    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let mut buf = buf.lock().unwrap();
                for frame in data.chunks(channels) {
                    let mono = frame.iter().sum::<f32>() / channels as f32;
                    buf.push(mono);
                }
            },
            |err| eprintln!("Audio stream error: {}", err),
            None,
        )?,
        SampleFormat::I16 => {
            let buf = audio_buf;
            device.build_input_stream(
                &stream_config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let mut buf = buf.lock().unwrap();
                    for frame in data.chunks(channels) {
                        let mono = frame.iter().map(|&s| s as f32 / 32768.0).sum::<f32>()
                            / channels as f32;
                        buf.push(mono);
                    }
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )?
        }
        _ => anyhow::bail!("Unsupported sample format: {:?}", sample_format),
    };

    stream.play()?;

    // Keep thread alive while stream is running, but check for shutdown signal periodically
    while !shutdown_signal.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // Stream will be dropped here, which stops the audio capture
    Ok(())
}

/// Simple linear resampling
fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    let ratio = to_rate as f64 / from_rate as f64;
    let out_len = (samples.len() as f64 * ratio) as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_idx = i as f64 / ratio;
        let idx = src_idx as usize;
        let frac = src_idx - idx as f64;
        let s = if idx + 1 < samples.len() {
            samples[idx] as f64 * (1.0 - frac) + samples[idx + 1] as f64 * frac
        } else if idx < samples.len() {
            samples[idx] as f64
        } else {
            0.0
        };
        out.push(s as f32);
    }
    out
}

// --- Types used by app.rs and config.rs ---

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
pub enum TranscriptionLanguage {
    English,
    Chinese,
    #[default]
    Auto,
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

#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
pub enum WhisperModelSize {
    #[default]
    Tiny,
    Base,
    Medium,
    LargeV3Turbo,
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

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------- resample --

    #[test]
    fn resampling_to_the_same_rate_is_a_no_op() {
        let samples = vec![0.0, 0.25, 0.5, 0.75, 1.0];

        let out = resample(&samples, 16_000, 16_000);

        assert_eq!(out.len(), samples.len());
        for (i, (a, b)) in out.iter().zip(samples.iter()).enumerate() {
            assert!((a - b).abs() < 1e-6, "sample {i} changed: {a} vs {b}");
        }
    }

    #[test]
    fn downsampling_halves_the_sample_count() {
        // The capture device usually runs at 44.1 or 48 kHz and whisper wants
        // 16 kHz, so this path runs on every chunk of audio.
        let samples = vec![0.0; 480];

        let out = resample(&samples, 48_000, 24_000);

        assert_eq!(out.len(), 240);
    }

    #[test]
    fn upsampling_grows_the_sample_count() {
        let samples = vec![0.0; 100];

        let out = resample(&samples, 8_000, 16_000);

        assert_eq!(out.len(), 200);
    }

    #[test]
    fn resampling_48k_to_16k_gives_a_third_of_the_samples() {
        let samples = vec![0.5; 4800];

        let out = resample(&samples, 48_000, 16_000);

        assert_eq!(out.len(), 1600);
    }

    #[test]
    fn resampling_interpolates_between_neighbours() {
        // Doubling the rate should put a midpoint between each pair.
        let out = resample(&[0.0, 1.0], 8_000, 16_000);

        assert_eq!(out.len(), 4);
        assert!((out[0] - 0.0).abs() < 1e-6);
        assert!(
            (out[1] - 0.5).abs() < 1e-6,
            "expected a midpoint, got {}",
            out[1]
        );
    }

    #[test]
    fn resampling_preserves_a_constant_signal() {
        let samples = vec![0.3_f32; 1000];

        let out = resample(&samples, 44_100, 16_000);

        for (i, s) in out.iter().enumerate() {
            assert!((s - 0.3).abs() < 1e-5, "sample {i} drifted to {s}");
        }
    }

    #[test]
    fn resampling_empty_input_gives_empty_output() {
        assert!(resample(&[], 48_000, 16_000).is_empty());
    }

    #[test]
    fn resampling_a_single_sample_does_not_panic() {
        let out = resample(&[1.0], 48_000, 16_000);

        assert!(out.len() <= 1);
    }

    // ------------------------------------------------------------ mapping --

    #[test]
    fn language_codes_match_whisper_names() {
        assert_eq!(
            whisper_language_code(TranscriptionLanguage::English),
            Some("en")
        );
        assert_eq!(
            whisper_language_code(TranscriptionLanguage::Chinese),
            Some("zh")
        );
        assert_eq!(
            whisper_language_code(TranscriptionLanguage::Auto),
            None,
            "Auto must be None so whisper detects the language itself"
        );
    }

    #[test]
    fn every_model_size_maps_to_a_distinct_ggml_file() {
        let sizes = [
            WhisperModelSize::Tiny,
            WhisperModelSize::Base,
            WhisperModelSize::Medium,
            WhisperModelSize::LargeV3Turbo,
        ];

        let mut files = Vec::new();
        for size in sizes {
            let (repo, file) = model_repo_file(size);

            assert_eq!(repo, "ggerganov/whisper.cpp");
            assert!(file.starts_with("ggml-"), "{size:?} maps to {file}");
            assert!(file.ends_with(".bin"), "{size:?} maps to {file}");
            files.push(file);
        }

        files.sort_unstable();
        let count = files.len();
        files.dedup();
        assert_eq!(files.len(), count, "two model sizes share a file");
    }
}
