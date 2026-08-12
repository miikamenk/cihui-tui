//! Text editing in `App` tracks the cursor as a **character** index while the
//! buffer is a UTF-8 `String`, so every edit converts between the two. Chinese
//! input makes that conversion load-bearing: one hanzi is three bytes, so a
//! cursor position used as a byte offset would panic or corrupt the buffer.

mod common;

use cihui_tui::app::InputLanguage;
use common::TestApp;
use pretty_assertions::assert_eq;

/// Build an app whose input is `text` with the cursor at the end.
fn app_with_input(text: &str) -> TestApp {
    let mut test_app = TestApp::new();
    test_app.set_input(text.to_string());
    test_app
}

/// The invariant every edit must preserve.
fn assert_cursor_is_valid(app: &cihui_tui::app::App) {
    let char_count = app.input.chars().count();

    assert!(
        app.cursor_position <= char_count,
        "cursor {} is past the end of {:?} ({} chars)",
        app.cursor_position,
        app.input,
        char_count
    );
    assert!(
        app.input.is_char_boundary(app.cursor_byte_index()),
        "cursor byte index {} is not a UTF-8 boundary in {:?}",
        app.cursor_byte_index(),
        app.input
    );
}

// ----------------------------------------------------------------- insert --

#[test]
fn inserting_into_an_empty_buffer() {
    let mut app = TestApp::new();

    app.insert_char('a');

    assert_eq!(app.input, "a");
    assert_eq!(app.cursor_position, 1);
    assert_cursor_is_valid(&app);
}

#[test]
fn inserting_at_the_cursor_not_the_end() {
    let mut app = app_with_input("ac");
    app.move_cursor_left();

    app.insert_char('b');

    assert_eq!(app.input, "abc");
    assert_eq!(app.cursor_position, 2);
}

#[test]
fn inserting_hanzi_advances_by_one_character_not_three_bytes() {
    let mut app = TestApp::new();

    app.insert_char('你');
    app.insert_char('好');

    assert_eq!(app.input, "你好");
    assert_eq!(app.cursor_position, 2, "cursor counts characters");
    assert_eq!(app.cursor_byte_index(), 6, "each hanzi is three bytes");
    assert_cursor_is_valid(&app);
}

#[test]
fn inserting_between_hanzi() {
    let mut app = app_with_input("你好");
    app.move_cursor_left();

    app.insert_char('X');

    assert_eq!(app.input, "你X好");
    assert_eq!(app.cursor_position, 2);
    assert_cursor_is_valid(&app);
}

#[test]
fn inserting_a_newline_keeps_it_in_the_buffer() {
    let mut app = app_with_input("a");

    app.insert_char('\n');
    app.insert_char('b');

    assert_eq!(app.input, "a\nb");
}

// ----------------------------------------------------------------- delete --

#[test]
fn backspace_removes_the_character_before_the_cursor() {
    let mut app = app_with_input("abc");

    app.backspace();

    assert_eq!(app.input, "ab");
    assert_eq!(app.cursor_position, 2);
}

#[test]
fn backspace_at_the_start_does_nothing() {
    let mut app = app_with_input("abc");
    app.move_cursor_to_start();

    app.backspace();

    assert_eq!(app.input, "abc");
    assert_eq!(app.cursor_position, 0);
}

#[test]
fn backspace_on_an_empty_buffer_does_nothing() {
    let mut app = TestApp::new();

    app.backspace();

    assert_eq!(app.input, "");
    assert_eq!(app.cursor_position, 0);
}

#[test]
fn backspace_removes_a_whole_hanzi() {
    let mut app = app_with_input("你好");

    app.backspace();

    assert_eq!(app.input, "你", "a three-byte character goes as a unit");
    assert_eq!(app.cursor_position, 1);
    assert_cursor_is_valid(&app);
}

#[test]
fn backspace_in_the_middle_of_hanzi_text() {
    let mut app = app_with_input("你好世界");
    app.move_cursor_left();
    app.move_cursor_left();

    app.backspace();

    assert_eq!(app.input, "你世界");
    assert_eq!(app.cursor_position, 1);
    assert_cursor_is_valid(&app);
}

// ------------------------------------------------------------- select all --

#[test]
fn typing_over_a_selection_replaces_everything() {
    let mut app = app_with_input("你好世界");

    app.select_all();
    app.insert_char('x');

    assert_eq!(app.input, "x");
    assert_eq!(app.cursor_position, 1);
    assert!(!app.select_all, "the selection is consumed by the insert");
}

#[test]
fn backspace_over_a_selection_clears_everything() {
    let mut app = app_with_input("你好世界");
    app.translation = "hello world".to_string();

    app.select_all();
    app.backspace();

    assert_eq!(app.input, "");
    assert_eq!(app.cursor_position, 0);
    assert!(!app.select_all);
    assert_eq!(app.translation, "", "clearing also drops derived output");
}

#[test]
fn moving_the_cursor_cancels_the_selection() {
    let mut app = app_with_input("abc");

    app.select_all();
    app.move_cursor_left();

    assert!(!app.select_all);
    assert_eq!(app.input, "abc", "moving must not delete the text");
}

#[test]
fn select_all_puts_the_cursor_at_the_end() {
    let mut app = app_with_input("你好");
    app.move_cursor_to_start();

    app.select_all();

    assert_eq!(app.cursor_position, 2);
}

// ----------------------------------------------------------- cursor moves --

#[test]
fn horizontal_movement_stops_at_both_ends() {
    let mut app = app_with_input("ab");

    app.move_cursor_to_start();
    app.move_cursor_left();
    assert_eq!(app.cursor_position, 0, "cannot move left of the start");

    app.move_cursor_to_end();
    app.move_cursor_right();
    assert_eq!(app.cursor_position, 2, "cannot move right of the end");
}

#[test]
fn move_to_end_counts_characters_not_bytes() {
    let mut app = app_with_input("你好世界");

    app.move_cursor_to_end();

    assert_eq!(app.cursor_position, 4);
    assert_eq!(app.cursor_byte_index(), 12);
}

#[test]
fn moving_up_keeps_the_column() {
    let mut app = app_with_input("abcd\nefgh");
    // Cursor is at the end, column 4 of line 1.

    app.move_cursor_up();

    assert_eq!(app.cursor_position, 4, "column 4 of line 0");
}

#[test]
fn moving_up_from_the_first_line_does_nothing() {
    let mut app = app_with_input("abc");
    app.move_cursor_to_start();

    app.move_cursor_up();

    assert_eq!(app.cursor_position, 0);
}

#[test]
fn moving_down_from_the_last_line_does_nothing() {
    let mut app = app_with_input("abc");

    app.move_cursor_down();

    assert_eq!(app.cursor_position, 3);
}

#[test]
fn moving_onto_a_shorter_line_clamps_to_its_end() {
    let mut app = app_with_input("abcdef\nxy");
    app.move_cursor_to_start();
    for _ in 0..6 {
        app.move_cursor_right();
    }
    // Column 6 of line 0, but line 1 only has two characters.

    app.move_cursor_down();

    assert_eq!(app.cursor_position, 9, "clamped to the end of \"xy\"");
    assert_cursor_is_valid(&app);
}

#[test]
fn moving_up_onto_a_shorter_line_clamps_to_its_end() {
    let mut app = app_with_input("xy\nabcdef");
    // Cursor at the end: column 6 of line 1.

    app.move_cursor_up();

    assert_eq!(app.cursor_position, 2, "clamped to the end of \"xy\"");
}

#[test]
fn vertical_movement_round_trips_on_equal_length_lines() {
    let mut app = app_with_input("abcd\nefgh");
    app.move_cursor_to_start();
    app.move_cursor_right();
    app.move_cursor_right();
    let start = app.cursor_position;

    app.move_cursor_down();
    app.move_cursor_up();

    assert_eq!(app.cursor_position, start);
}

#[test]
fn vertical_movement_works_with_hanzi_lines() {
    let mut app = app_with_input("你好世界\n中文");

    app.move_cursor_up();

    assert_eq!(app.cursor_position, 2, "clamped to two characters");
    assert_cursor_is_valid(&app);
}

// ------------------------------------------------------------ word delete --

#[test]
fn deleting_a_word_removes_the_word_before_the_cursor() {
    let mut app = app_with_input("hello world");

    app.delete_word_backwards();

    assert_eq!(app.input, "hello ");
    assert_eq!(app.cursor_position, 6);
}

#[test]
fn deleting_a_word_skips_trailing_whitespace_first() {
    let mut app = app_with_input("hello world   ");

    app.delete_word_backwards();

    assert_eq!(app.input, "hello ");
}

#[test]
fn deleting_a_word_at_the_start_does_nothing() {
    let mut app = app_with_input("hello");
    app.move_cursor_to_start();

    app.delete_word_backwards();

    assert_eq!(app.input, "hello");
}

#[test]
fn deleting_a_word_of_hanzi_removes_the_whole_run() {
    // Chinese is written without spaces, so the whole run is one "word".
    let mut app = app_with_input("你好世界");

    app.delete_word_backwards();

    assert_eq!(app.input, "");
    assert_eq!(app.cursor_position, 0);
    assert_cursor_is_valid(&app);
}

#[test]
fn deleting_a_word_over_a_selection_clears_everything() {
    let mut app = app_with_input("hello world");

    app.select_all();
    app.delete_word_backwards();

    assert_eq!(app.input, "");
}

// -------------------------------------------------------- delete to start --

#[test]
fn deleting_to_the_start_keeps_what_follows_the_cursor() {
    let mut app = app_with_input("hello world");
    app.move_cursor_to_start();
    for _ in 0..6 {
        app.move_cursor_right();
    }

    app.delete_to_start();

    assert_eq!(app.input, "world");
    assert_eq!(app.cursor_position, 0);
}

#[test]
fn deleting_to_the_start_with_hanzi() {
    let mut app = app_with_input("你好世界");
    app.move_cursor_to_start();
    app.move_cursor_right();
    app.move_cursor_right();

    app.delete_to_start();

    assert_eq!(app.input, "世界");
    assert_eq!(app.cursor_position, 0);
    assert_cursor_is_valid(&app);
}

#[test]
fn deleting_to_the_start_at_the_start_does_nothing() {
    let mut app = app_with_input("abc");
    app.move_cursor_to_start();

    app.delete_to_start();

    assert_eq!(app.input, "abc");
}

// ------------------------------------------------------------------ clear --

#[test]
fn clear_resets_the_derived_state_too() {
    let mut app = app_with_input("你好");
    app.pinyin_lines = vec!["nǐ hǎo".to_string()];
    app.hanzi_lines = vec!["你 好".to_string()];
    app.translation = "hello".to_string();
    app.error_message = Some("boom".to_string());

    app.clear();

    assert_eq!(app.input, "");
    assert_eq!(app.cursor_position, 0);
    assert!(app.pinyin_lines.is_empty());
    assert!(app.hanzi_lines.is_empty());
    assert_eq!(app.translation, "");
    assert_eq!(app.error_message, None);
    assert_eq!(app.last_input_time, None);
}

// ------------------------------------------------------ language detection --

#[test]
fn hanzi_input_is_detected_as_chinese() {
    let mut app = app_with_input("你好");

    app.detect_input_language();

    assert_eq!(app.input_language, InputLanguage::Chinese);
    assert!(app.is_input_chinese());
}

#[test]
fn ascii_input_is_detected_as_english() {
    let mut app = app_with_input("hello");

    app.detect_input_language();

    assert_eq!(app.input_language, InputLanguage::English);
}

#[test]
fn mixed_input_is_detected_as_chinese() {
    // Chinese wins, because the pinyin pass is what the user wants here.
    let mut app = app_with_input("hello 你好");

    app.detect_input_language();

    assert_eq!(app.input_language, InputLanguage::Chinese);
}

#[test]
fn input_without_letters_is_detected_as_other() {
    let mut app = app_with_input("12345 !?");

    app.detect_input_language();

    assert_eq!(app.input_language, InputLanguage::Other);
}

#[test]
fn empty_input_leaves_the_previous_detection_alone() {
    // detect_input_language returns early on empty input, so whatever was
    // detected last stays. Clearing the box does not reset the language.
    let mut app = app_with_input("hello");
    app.detect_input_language();
    assert_eq!(app.input_language, InputLanguage::English);

    app.set_input(String::new());
    app.detect_input_language();

    assert_eq!(app.input_language, InputLanguage::English);
}

#[test]
fn whitespace_only_input_leaves_the_previous_detection_alone() {
    let mut app = app_with_input("你好");
    app.detect_input_language();

    app.set_input("   ".to_string());
    app.detect_input_language();

    assert_eq!(app.input_language, InputLanguage::Chinese);
}

// -------------------------------------------------------------- sequences --

#[test]
fn a_realistic_editing_session_keeps_the_cursor_valid() {
    let mut app = TestApp::new();

    for c in "你好".chars() {
        app.insert_char(c);
        assert_cursor_is_valid(&app);
    }

    app.move_cursor_left();
    app.insert_char('a');
    assert_cursor_is_valid(&app);

    app.backspace();
    assert_cursor_is_valid(&app);

    app.move_cursor_to_start();
    app.insert_char('世');
    assert_cursor_is_valid(&app);

    app.delete_word_backwards();
    assert_cursor_is_valid(&app);

    app.move_cursor_to_end();
    app.delete_to_start();
    assert_cursor_is_valid(&app);

    assert_eq!(app.input, "");
}

#[test]
fn editing_records_when_the_user_last_typed() {
    // The OCR loop debounces on this, so an edit that fails to update it would
    // stall the pinyin refresh.
    let mut app = TestApp::new();
    assert_eq!(app.last_input_time, None);

    app.insert_char('a');

    assert!(app.last_input_time.is_some());
}
