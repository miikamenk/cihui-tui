//! Rendering is a pure function of `App`, so a whole frame can be drawn into
//! an in-memory buffer and compared against a stored snapshot. That catches
//! layout regressions no assertion would think to check, and the snapshots
//! double as readable documentation of what each screen looks like.

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

#[test]
fn a_tiny_terminal_does_not_panic() {
    // Popup layout does arithmetic on the popup width, so a terminal too small
    // to hold the popup must not underflow.
    let mut app = TestApp::new();
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
    let mut app = TestApp::new();
    app.toggle_settings();

    for (width, height) in [(20, 10), (10, 6), (4, 4), (1, 1)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("create terminal");
        terminal
            .draw(|f| draw_ui(f, &app))
            .unwrap_or_else(|e| panic!("drawing at {width}x{height} failed: {e}"));
    }
}

// ---------------------------------------------------------------- cursor --

/// Where the terminal cursor ends up after drawing.
fn cursor_after_render(app: &TestApp) -> (u16, u16) {
    let mut terminal = render(app);
    terminal
        .backend_mut()
        .get_cursor_position()
        .expect("reading the test backend cursor cannot fail")
        .into()
}

#[test]
fn the_cursor_starts_in_the_input_box() {
    let app = TestApp::new();

    let (x, y) = cursor_after_render(&app);

    // One cell inside the input block's border, which starts below the
    // three-row header.
    assert_eq!((x, y), (1, 4));
}

#[test]
fn ascii_advances_the_cursor_one_column_per_character() {
    let mut app = TestApp::new();
    app.set_input("abc".to_string());

    let (x, _) = cursor_after_render(&app);

    assert_eq!(x, 1 + 3);
}

#[test]
fn hanzi_advances_the_cursor_two_columns_per_character() {
    let mut app = TestApp::new();
    app.set_input("你好".to_string());

    let (x, _) = cursor_after_render(&app);

    assert_eq!(x, 1 + 4, "two double-width characters take four columns");
}

#[test]
fn full_width_punctuation_is_two_columns_wide() {
    // Regression for is_wide_char: the CJK symbols and punctuation block is
    // double-width, not just the full-width space at U+3000.
    let mut app = TestApp::new();
    app.set_input("你好，".to_string());

    let (x, _) = cursor_after_render(&app);

    assert_eq!(x, 1 + 6, "two hanzi plus a full-width comma");
}

#[test]
fn ascii_punctuation_stays_one_column_wide() {
    let mut app = TestApp::new();
    app.set_input("你好,".to_string());

    let (x, _) = cursor_after_render(&app);

    assert_eq!(x, 1 + 5, "two hanzi plus a single-width comma");
}

#[test]
fn a_newline_moves_the_cursor_to_the_next_row() {
    let mut app = TestApp::new();
    app.set_input("ab\ncd".to_string());

    let (x, y) = cursor_after_render(&app);

    assert_eq!((x, y), (1 + 2, 4 + 1));
}

#[test]
fn the_cursor_follows_a_move_to_the_start() {
    let mut app = TestApp::new();
    app.set_input("你好世界".to_string());
    app.move_cursor_to_start();

    let (x, y) = cursor_after_render(&app);

    assert_eq!((x, y), (1, 4));
}

#[test]
fn the_cursor_sits_between_characters_mid_buffer() {
    let mut app = TestApp::new();
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
    let mut app = TestApp::new();
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
    let mut app = TestApp::new();
    app.set_input("你好".to_string());
    app.processing = true;

    assert_eq!(cursor_after_render(&app), (0, 0));
}
