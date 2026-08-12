//! Cursor placement in the input box, which has to account for double-width
//! characters. These run in every feature configuration, so the app's mode is
//! set explicitly rather than relying on the default for the build.

mod common;

use cihui_tui::app::AppMode;
use cihui_tui::ui::draw_ui;
use common::TestApp;
use ratatui::{
    backend::{Backend, TestBackend},
    Terminal,
};

/// An app in text-input mode regardless of which features are compiled in.
fn text_mode_app() -> TestApp {
    let mut app = TestApp::new();
    app.mode = AppMode::Normal;
    app
}

fn render_sized(app: &TestApp, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("create terminal");
    terminal
        .draw(|f| draw_ui(f, app))
        .expect("draw must not fail");
    terminal
}

// ---------------------------------------------------------------- cursor --

/// Where the terminal cursor ends up after drawing.
fn cursor_after_render(app: &TestApp) -> (u16, u16) {
    let mut terminal = render_sized(app, 100, 30);
    terminal
        .backend_mut()
        .get_cursor_position()
        .expect("reading the test backend cursor cannot fail")
        .into()
}

#[test]
fn the_cursor_starts_in_the_input_box() {
    let app = text_mode_app();

    let (x, y) = cursor_after_render(&app);

    // One cell inside the input block's border, which starts below the
    // three-row header.
    assert_eq!((x, y), (1, 4));
}

#[test]
fn ascii_advances_the_cursor_one_column_per_character() {
    let mut app = text_mode_app();
    app.set_input("abc".to_string());

    let (x, _) = cursor_after_render(&app);

    assert_eq!(x, 1 + 3);
}

#[test]
fn hanzi_advances_the_cursor_two_columns_per_character() {
    let mut app = text_mode_app();
    app.set_input("你好".to_string());

    let (x, _) = cursor_after_render(&app);

    assert_eq!(x, 1 + 4, "two double-width characters take four columns");
}

#[test]
fn full_width_punctuation_is_two_columns_wide() {
    // Regression for is_wide_char: the CJK symbols and punctuation block is
    // double-width, not just the full-width space at U+3000.
    let mut app = text_mode_app();
    app.set_input("你好，".to_string());

    let (x, _) = cursor_after_render(&app);

    assert_eq!(x, 1 + 6, "two hanzi plus a full-width comma");
}

#[test]
fn ascii_punctuation_stays_one_column_wide() {
    let mut app = text_mode_app();
    app.set_input("你好,".to_string());

    let (x, _) = cursor_after_render(&app);

    assert_eq!(x, 1 + 5, "two hanzi plus a single-width comma");
}

#[test]
fn a_newline_moves_the_cursor_to_the_next_row() {
    let mut app = text_mode_app();
    app.set_input("ab\ncd".to_string());

    let (x, y) = cursor_after_render(&app);

    assert_eq!((x, y), (1 + 2, 4 + 1));
}

#[test]
fn the_cursor_follows_a_move_to_the_start() {
    let mut app = text_mode_app();
    app.set_input("你好世界".to_string());
    app.move_cursor_to_start();

    let (x, y) = cursor_after_render(&app);

    assert_eq!((x, y), (1, 4));
}

#[test]
fn the_cursor_sits_between_characters_mid_buffer() {
    let mut app = text_mode_app();
    app.set_input("你好世界".to_string());
    app.move_cursor_to_start();
    app.move_cursor_right();

    let (x, _) = cursor_after_render(&app);

    assert_eq!(x, 1 + 2, "after exactly one double-width character");
}

#[test]
fn no_cursor_is_placed_while_a_popup_is_open() {
    // draw_input skips positioning the cursor when a popup is open, so it is
    // never moved into the input box and the backend leaves it at the origin.
    let mut app = text_mode_app();
    app.set_input("你好".to_string());
    app.toggle_settings();

    assert_eq!(
        cursor_after_render(&app),
        (0, 0),
        "the cursor must not be positioned in the input box behind a popup"
    );
}

#[test]
fn no_cursor_is_placed_while_processing() {
    let mut app = text_mode_app();
    app.set_input("你好".to_string());
    app.processing = true;

    assert_eq!(cursor_after_render(&app), (0, 0));
}

// ------------------------------------------------------------ terminal size --

#[test]
fn a_tiny_terminal_does_not_panic() {
    // Popup layout does arithmetic on the popup width, so a terminal too small
    // to hold the popup must not underflow.
    let mut app = text_mode_app();
    app.toggle_language_selector();

    for (width, height) in [(20, 10), (10, 6), (4, 4), (1, 1)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("create terminal");
        terminal
            .draw(|f| draw_ui(f, &app))
            .unwrap_or_else(|e| panic!("drawing at {width}x{height} failed: {e}"));
    }
}

#[test]
fn a_tiny_terminal_does_not_panic_with_settings_open() {
    let mut app = text_mode_app();
    app.toggle_settings();

    for (width, height) in [(20, 10), (10, 6), (4, 4), (1, 1)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("create terminal");
        terminal
            .draw(|f| draw_ui(f, &app))
            .unwrap_or_else(|e| panic!("drawing at {width}x{height} failed: {e}"));
    }
}
