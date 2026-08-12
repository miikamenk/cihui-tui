//! Snapshots of the transcription screen.
//!
//! Gated to the transcription-without-ocr configuration, which is exactly how
//! the `cihui-transcribe` binary is built. In that build the app starts in
//! transcription mode and the help line omits the key that switches back to
//! text mode, so the frames differ from the text-mode snapshots.

#![cfg(all(feature = "transcription", not(feature = "ocr")))]

mod common;

use cihui_tui::transcription::{AudioDevice, TranscriptionEvent};
use cihui_tui::ui::draw_ui;
use common::TestApp;
use ratatui::{backend::TestBackend, Terminal};

fn render(app: &TestApp) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("create terminal");
    terminal
        .draw(|f| draw_ui(f, app))
        .expect("draw must not fail");
    terminal
}

fn device(name: &str) -> AudioDevice {
    AudioDevice {
        name: name.to_string(),
        description: format!("{name} (description)"),
    }
}

#[test]
fn idle_before_the_model_is_loaded() {
    let app = TestApp::new();

    insta::assert_snapshot!(render(&app).backend());
}

#[test]
fn while_the_model_downloads() {
    let mut app = TestApp::new();
    app.apply_transcription_event(TranscriptionEvent::ModelLoading {
        progress: 0.35,
        status: "Downloading ggml-tiny.bin".to_string(),
    });

    insta::assert_snapshot!(render(&app).backend());
}

#[test]
fn ready_to_record() {
    let mut app = TestApp::new();
    app.apply_transcription_event(TranscriptionEvent::ModelReady);

    insta::assert_snapshot!(render(&app).backend());
}

#[test]
fn recording_with_voice_detected() {
    let mut app = TestApp::new();
    app.apply_transcription_event(TranscriptionEvent::ModelReady);
    app.transcription.is_recording = true;
    app.apply_transcription_event(TranscriptionEvent::VadActivity(true));

    insta::assert_snapshot!(render(&app).backend());
}

#[test]
fn with_a_transcript_and_translation() {
    let mut app = TestApp::new();
    app.apply_transcription_event(TranscriptionEvent::ModelReady);
    app.apply_transcription_event(TranscriptionEvent::Segment("你好世界".to_string()));
    app.transcription.pinyin_lines = vec!["nǐ hǎo shì jiè".to_string()];
    app.transcription.hanzi_lines = vec!["你 好  世  界".to_string()];
    app.transcription.translation = "Hello world".to_string();

    insta::assert_snapshot!(render(&app).backend());
}

#[test]
fn after_an_error() {
    let mut app = TestApp::new();
    app.transcription.is_recording = true;
    app.apply_transcription_event(TranscriptionEvent::Error("no input device".to_string()));

    insta::assert_snapshot!(render(&app).backend());
}

#[test]
fn device_selector_popup() {
    let mut app = TestApp::new();
    app.transcription.available_devices = vec![
        device("alsa_input.pci-0000_00_1f.3.analog-stereo"),
        device("alsa_input.usb-Blue_Microphones"),
        device("bluez_input.00_11_22_33_44_55"),
    ];
    app.transcription.selected_device = Some("alsa_input.usb-Blue_Microphones".to_string());
    app.transcription.device_selector_open = true;
    app.transcription.device_selector_scroll = 1;

    insta::assert_snapshot!(render(&app).backend());
}

#[test]
fn transcription_settings_popup() {
    let mut app = TestApp::new();
    app.transcription.settings_open = true;

    insta::assert_snapshot!(render(&app).backend());
}
