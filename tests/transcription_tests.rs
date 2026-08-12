//! The transcription UI is driven entirely by `TranscriptionEvent` messages
//! from the audio and inference tasks. Folding those events into `App` is pure
//! state, so the whole state machine can be tested with no microphone, no
//! model download and no GPU.

#![cfg(feature = "transcription")]

mod common;

use cihui_tui::transcription::{TranscriptionEvent, TranscriptionLanguage, WhisperModelSize};
use common::TestApp;
use pretty_assertions::assert_eq;

// ----------------------------------------------------------- model loading --

#[test]
fn model_loading_reports_progress() {
    let mut app = TestApp::new();

    app.apply_transcription_event(TranscriptionEvent::ModelLoading {
        progress: 0.42,
        status: "Downloading ggml-tiny.bin".to_string(),
    });

    assert!(app.transcription.model_loading);
    assert_eq!(app.transcription.model_progress, 0.42);
    assert_eq!(app.transcription.status, "Downloading ggml-tiny.bin");
    assert!(!app.transcription.model_ready);
}

#[test]
fn model_ready_clears_the_loading_state() {
    let mut app = TestApp::new();
    app.apply_transcription_event(TranscriptionEvent::ModelLoading {
        progress: 0.9,
        status: "Almost there".to_string(),
    });

    app.apply_transcription_event(TranscriptionEvent::ModelReady);

    assert!(!app.transcription.model_loading);
    assert!(app.transcription.model_ready);
    assert!(
        app.transcription.status.contains("Space"),
        "the status should tell the user what to press, got {:?}",
        app.transcription.status
    );
}

#[test]
fn loading_events_do_not_ask_for_a_translation() {
    let mut app = TestApp::new();

    assert_eq!(
        app.apply_transcription_event(TranscriptionEvent::ModelReady),
        None
    );
    assert_eq!(
        app.apply_transcription_event(TranscriptionEvent::ModelLoading {
            progress: 0.1,
            status: "x".to_string(),
        }),
        None
    );
}

// --------------------------------------------------------------- segments --

#[test]
fn the_first_segment_starts_the_transcript() {
    let mut app = TestApp::new();

    let pending = app.apply_transcription_event(TranscriptionEvent::Segment("你好".to_string()));

    assert_eq!(app.transcription.transcript, "你好");
    assert_eq!(
        pending.as_deref(),
        Some("你好"),
        "a new segment needs re-translating"
    );
}

#[test]
fn later_segments_are_joined_with_a_space() {
    let mut app = TestApp::new();

    app.apply_transcription_event(TranscriptionEvent::Segment("hello".to_string()));
    app.apply_transcription_event(TranscriptionEvent::Segment("world".to_string()));

    assert_eq!(app.transcription.transcript, "hello world");
}

#[test]
fn segments_are_trimmed_before_being_appended() {
    // Whisper pads its output with spaces, which would otherwise accumulate.
    let mut app = TestApp::new();

    app.apply_transcription_event(TranscriptionEvent::Segment("  hello  ".to_string()));
    app.apply_transcription_event(TranscriptionEvent::Segment("  world  ".to_string()));

    assert_eq!(app.transcription.transcript, "hello world");
}

#[test]
fn no_second_space_is_added_after_existing_whitespace() {
    let mut app = TestApp::new();
    app.transcription.transcript = "hello ".to_string();

    app.apply_transcription_event(TranscriptionEvent::Segment("world".to_string()));

    assert_eq!(app.transcription.transcript, "hello world");
}

#[test]
fn no_space_is_added_after_a_newline() {
    let mut app = TestApp::new();
    app.transcription.transcript = "first line\n".to_string();

    app.apply_transcription_event(TranscriptionEvent::Segment("second".to_string()));

    assert_eq!(app.transcription.transcript, "first line\nsecond");
}

#[test]
fn an_empty_segment_still_requests_a_translation() {
    // Documents current behaviour: a blank segment adds a separator space and
    // still triggers a re-translation of the unchanged transcript.
    let mut app = TestApp::new();
    app.transcription.transcript = "hello".to_string();

    let pending = app.apply_transcription_event(TranscriptionEvent::Segment("   ".to_string()));

    assert_eq!(app.transcription.transcript, "hello ");
    assert!(pending.is_some());
}

#[test]
fn the_pending_text_is_the_whole_transcript_not_just_the_segment() {
    // Translation runs over everything said so far, so context is preserved.
    let mut app = TestApp::new();

    app.apply_transcription_event(TranscriptionEvent::Segment("你好".to_string()));
    let pending = app.apply_transcription_event(TranscriptionEvent::Segment("世界".to_string()));

    assert_eq!(pending.as_deref(), Some("你好 世界"));
}

// -------------------------------------------------------------------- vad --

#[test]
fn voice_activity_toggles_the_indicator() {
    let mut app = TestApp::new();
    assert!(!app.transcription.vad_active);

    app.apply_transcription_event(TranscriptionEvent::VadActivity(true));
    assert!(app.transcription.vad_active);

    app.apply_transcription_event(TranscriptionEvent::VadActivity(false));
    assert!(!app.transcription.vad_active);
}

// ------------------------------------------------------------------ error --

#[test]
fn an_error_stops_the_recording() {
    // Otherwise the UI would keep claiming to record after the audio thread
    // has died.
    let mut app = TestApp::new();
    app.transcription.is_recording = true;

    app.apply_transcription_event(TranscriptionEvent::Error("device disappeared".to_string()));

    assert!(!app.transcription.is_recording);
    assert!(
        app.transcription.status.contains("device disappeared"),
        "the status should surface the cause, got {:?}",
        app.transcription.status
    );
}

#[test]
fn an_error_does_not_discard_the_transcript() {
    let mut app = TestApp::new();
    app.apply_transcription_event(TranscriptionEvent::Segment("你好".to_string()));

    app.apply_transcription_event(TranscriptionEvent::Error("boom".to_string()));

    assert_eq!(app.transcription.transcript, "你好");
}

// -------------------------------------------------------------- sequences --

#[test]
fn a_full_session_from_loading_to_error() {
    let mut app = TestApp::new();

    app.apply_transcription_event(TranscriptionEvent::ModelLoading {
        progress: 0.5,
        status: "Loading".to_string(),
    });
    app.apply_transcription_event(TranscriptionEvent::ModelReady);
    app.transcription.is_recording = true;

    app.apply_transcription_event(TranscriptionEvent::VadActivity(true));
    app.apply_transcription_event(TranscriptionEvent::Segment("你好".to_string()));
    app.apply_transcription_event(TranscriptionEvent::Segment("世界".to_string()));
    app.apply_transcription_event(TranscriptionEvent::VadActivity(false));

    assert_eq!(app.transcription.transcript, "你好 世界");
    assert!(app.transcription.model_ready);
    assert!(!app.transcription.vad_active);

    app.apply_transcription_event(TranscriptionEvent::Error("stream closed".to_string()));

    assert!(!app.transcription.is_recording);
    assert_eq!(app.transcription.transcript, "你好 世界");
}

// ------------------------------------------------------------ enum cycling --

#[test]
fn transcription_languages_cycle_in_both_directions() {
    let all = [
        TranscriptionLanguage::English,
        TranscriptionLanguage::Chinese,
        TranscriptionLanguage::Auto,
    ];

    for lang in all {
        assert_eq!(lang.next().prev(), lang, "next then prev for {lang:?}");
        assert_eq!(lang.prev().next(), lang, "prev then next for {lang:?}");
    }
}

#[test]
fn cycling_languages_visits_every_variant() {
    let mut lang = TranscriptionLanguage::English;
    let mut seen = vec![lang.name()];

    for _ in 0..2 {
        lang = lang.next();
        seen.push(lang.name());
    }
    seen.sort_unstable();
    seen.dedup();

    assert_eq!(seen.len(), 3, "cycling should reach all three languages");
    assert_eq!(
        lang.next(),
        TranscriptionLanguage::English,
        "and then wrap around"
    );
}

#[test]
fn model_sizes_cycle_in_both_directions() {
    let all = [
        WhisperModelSize::Tiny,
        WhisperModelSize::Base,
        WhisperModelSize::Medium,
        WhisperModelSize::LargeV3Turbo,
    ];

    for size in all {
        assert_eq!(size.next().prev(), size, "next then prev for {size:?}");
        assert_eq!(size.prev().next(), size, "prev then next for {size:?}");
    }
}

#[test]
fn cycling_model_sizes_visits_every_variant_and_wraps() {
    let mut size = WhisperModelSize::Tiny;
    let mut seen = vec![size.name()];

    for _ in 0..3 {
        size = size.next();
        seen.push(size.name());
    }

    assert_eq!(
        seen,
        vec!["Tiny", "Base", "Medium", "Large V3 Turbo"],
        "sizes should cycle from smallest to largest"
    );
    assert_eq!(size.next(), WhisperModelSize::Tiny);
}

#[test]
fn defaults_are_the_cheapest_options() {
    // Tiny downloads fastest and Auto avoids asking the user to pick.
    assert_eq!(WhisperModelSize::default(), WhisperModelSize::Tiny);
    assert_eq!(
        TranscriptionLanguage::default(),
        TranscriptionLanguage::Auto
    );
}

// -------------------------------------------------------------- settings --

#[test]
fn changing_the_model_invalidates_the_loaded_one() {
    // The loaded model has to be discarded, or the app would keep using the
    // old weights after the user picked a different size.
    let mut app = TestApp::new();
    app.transcription.model_ready = true;
    app.transcription.settings_selection = 1;

    app.transcription_settings_cycle_forward();

    assert!(!app.transcription.model_ready);
    assert_eq!(app.transcription.model_size, WhisperModelSize::Base);
}

#[test]
fn changing_the_language_invalidates_the_loaded_model() {
    let mut app = TestApp::new();
    app.transcription.model_ready = true;
    app.transcription.settings_selection = 0;

    app.transcription_settings_cycle_forward();

    assert!(!app.transcription.model_ready);
}

#[test]
fn transcription_settings_are_persisted() {
    let mut app = TestApp::new();
    let path = app.config_path();
    app.transcription.settings_selection = 1;

    app.transcription_settings_cycle_forward();

    let reloaded = cihui_tui::config::Config::load_from(&path).unwrap();
    assert_eq!(reloaded.whisper_model, app.transcription.model_size);
}
