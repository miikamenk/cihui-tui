//! Rendering is a pure function of `App`, so a whole frame can be drawn into
//! an in-memory buffer and compared against a stored snapshot. That catches
//! layout regressions no assertion would think to check, and the snapshots
//! double as readable documentation of what each screen looks like.
//!
//! The rendered frame depends on which features are enabled - the help line
//! lists different keys, and a transcription-only build starts in a
//! different mode - so one stored snapshot cannot serve every build. These
//! run in the feature-free configuration, which is the one CI gates on.

#![cfg(not(any(feature = "ocr", feature = "transcription")))]

mod common;

use cihui_tui::language::Language;
use cihui_tui::pinyin_conv::convert_to_pinyin_lines;
use cihui_tui::ui::{draw_ui, update_pinyin_display};
use common::TestApp;
use ratatui::{
    backend::{Backend, TestBackend},
    Terminal,
};

/// Draw the app into an 100x30 buffer, the size the snapshots assume.
fn render(app: &TestApp) -> Terminal<TestBackend> {
    render_sized(app, 100, 30)
}

/// Fill the pinyin panes from the real conversion, so snapshots show what the
/// app actually produces rather than hand-written sample rows.
fn set_pinyin_from(app: &mut TestApp, text: &str) {
    update_pinyin_display(&mut app.app, convert_to_pinyin_lines(text));
}

fn render_sized(app: &TestApp, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("create terminal");
    terminal
        .draw(|f| draw_ui(f, app))
        .expect("draw must not fail");
    terminal
}

// ------------------------------------------------------------- whole frames --

#[test]
fn empty_startup_screen() {
    let app = TestApp::new();

    insta::assert_snapshot!(render(&app).backend());
}

#[test]
fn screen_with_pinyin_and_translation() {
    let mut app = TestApp::new();
    app.set_input("你好世界".to_string());
    set_pinyin_from(&mut app, "你好世界");
    app.translation = "Hello world".to_string();

    insta::assert_snapshot!(render(&app).backend());
}

#[test]
fn screen_with_multiline_input() {
    let mut app = TestApp::new();
    app.set_input("你好\n世界".to_string());
    set_pinyin_from(&mut app, "你好\n世界");

    insta::assert_snapshot!(render(&app).backend());
}

#[test]
fn screen_with_an_error_message() {
    let mut app = TestApp::new();
    app.set_input("你好".to_string());
    app.error_message = Some("Translation service unavailable".to_string());

    insta::assert_snapshot!(render(&app).backend());
}

#[test]
fn screen_while_processing() {
    let mut app = TestApp::new();
    app.set_input("你好".to_string());
    app.processing = true;

    insta::assert_snapshot!(render(&app).backend());
}

#[test]
fn screen_in_chinese_ui_language() {
    let mut app = TestApp::new();
    app.toggle_ui_language();

    insta::assert_snapshot!(render(&app).backend());
}

// ----------------------------------------------------------------- popups --

#[test]
fn settings_popup() {
    let mut app = TestApp::new();
    app.toggle_settings();

    insta::assert_snapshot!(render(&app).backend());
}

#[test]
fn settings_popup_on_the_second_row() {
    let mut app = TestApp::new();
    app.toggle_settings();
    app.settings_move_down();

    insta::assert_snapshot!(render(&app).backend());
}

#[test]
fn language_selector_popup() {
    let mut app = TestApp::new();
    app.toggle_language_selector();

    insta::assert_snapshot!(render(&app).backend());
}

#[test]
fn language_selector_filtered_by_a_search() {
    let mut app = TestApp::new();
    app.toggle_language_selector();
    for c in "finn".chars() {
        app.language_selector_search_add_char(c);
    }

    insta::assert_snapshot!(render(&app).backend());
}

#[test]
fn language_selector_with_no_matches() {
    let mut app = TestApp::new();
    app.toggle_language_selector();
    for c in "zzzz".chars() {
        app.language_selector_search_add_char(c);
    }

    insta::assert_snapshot!(render(&app).backend());
}

#[test]
fn language_selector_scrolled_into_the_list() {
    let mut app = TestApp::new();
    app.target_language = Language::German;
    app.toggle_language_selector();
    for _ in 0..40 {
        app.language_selector_move_down();
    }

    insta::assert_snapshot!(render(&app).backend());
}

// ------------------------------------------------------------ terminal size --

#[test]
fn renders_in_a_small_terminal() {
    let app = TestApp::new();

    insta::assert_snapshot!(render_sized(&app, 40, 15).backend());
}

#[test]
fn renders_in_a_wide_terminal() {
    let mut app = TestApp::new();
    app.set_input("你好".to_string());

    insta::assert_snapshot!(render_sized(&app, 160, 20).backend());
}
