//! Pinyin conversion is the heart of the app: it lays a row of pinyin above a
//! row of hanzi, and the two rows have to line up in a fixed-width terminal.
//! These tests exercise the public API and the alignment property it promises.

use cihui_tui::pinyin_conv::{convert_to_pinyin_lines, PinyinLine};
use pretty_assertions::assert_eq;

/// Terminal columns a string occupies, counting CJK as double-width.
///
/// This mirrors what a terminal does when it draws the two rows on top of each
/// other, which is the thing alignment actually depends on.
fn display_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

fn char_width(c: char) -> usize {
    let wide = ('\u{1100}'..='\u{115f}').contains(&c)
        || ('\u{2e80}'..='\u{303e}').contains(&c)
        || ('\u{3041}'..='\u{33ff}').contains(&c)
        || ('\u{3400}'..='\u{4dbf}').contains(&c)
        || ('\u{4e00}'..='\u{9fff}').contains(&c)
        || ('\u{a000}'..='\u{a4cf}').contains(&c)
        || ('\u{f900}'..='\u{faff}').contains(&c)
        || ('\u{fe30}'..='\u{fe6f}').contains(&c)
        || ('\u{ff00}'..='\u{ff60}').contains(&c)
        || ('\u{ffe0}'..='\u{ffe6}').contains(&c);

    if wide {
        2
    } else {
        1
    }
}

/// Column at which each whitespace-separated group starts.
///
/// Tokens never contain whitespace, so runs of spaces are exactly the column
/// separators and padding.
fn column_offsets(row: &str) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut column = 0;
    let mut in_token = false;

    for c in row.chars() {
        if c == ' ' {
            in_token = false;
        } else {
            if !in_token {
                offsets.push(column);
                in_token = true;
            }
        }
        column += char_width(c);
    }

    offsets
}

/// Assert the two rows line up when stacked in a fixed-width terminal.
///
/// Comparing total widths would be wrong: both rows are right-trimmed, so the
/// final column's padding is dropped and the widths legitimately differ. What
/// has to hold is that every column begins at the same offset in both rows.
fn assert_rows_align(line: &PinyinLine, context: &str) {
    let pinyin_columns = column_offsets(&line.pinyin);
    let hanzi_columns = column_offsets(&line.hanzi);

    assert_eq!(
        pinyin_columns.len(),
        hanzi_columns.len(),
        "different column counts for {context}\n\
         pinyin: {:?}\n\
         hanzi:  {:?}",
        line.pinyin,
        line.hanzi,
    );

    assert_eq!(
        pinyin_columns, hanzi_columns,
        "columns start at different offsets for {context}\n\
         pinyin: {:?}\n\
         hanzi:  {:?}",
        line.pinyin, line.hanzi,
    );
}

// ----------------------------------------------------------- basic output --

#[test]
fn converts_hanzi_to_toned_pinyin() {
    let lines = convert_to_pinyin_lines("你好世界");

    assert_eq!(lines.len(), 1);
    // Compare column contents rather than the padded strings, so the test
    // describes the reading rather than the exact spacing.
    assert_eq!(
        lines[0].pinyin.split_whitespace().collect::<Vec<_>>(),
        vec!["nǐ", "hǎo", "shì", "jiè"]
    );
    assert_eq!(
        lines[0].hanzi.split_whitespace().collect::<Vec<_>>(),
        vec!["你", "好", "世", "界"]
    );
}

#[test]
fn passes_english_through_unchanged() {
    let lines = convert_to_pinyin_lines("hello world");

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].pinyin, "hello world");
    assert_eq!(lines[0].hanzi, "hello world");
}

#[test]
fn handles_mixed_scripts_in_one_line() {
    let lines = convert_to_pinyin_lines("认推给word别");

    assert_eq!(lines.len(), 1);
    for expected in ["rèn", "tuī", "gěi", "word", "bié"] {
        assert!(
            lines[0].pinyin.contains(expected),
            "expected {expected:?} in {:?}",
            lines[0].pinyin
        );
    }
}

#[test]
fn empty_input_produces_no_lines() {
    assert!(convert_to_pinyin_lines("").is_empty());
}

#[test]
fn whitespace_only_input_produces_no_lines() {
    assert!(convert_to_pinyin_lines("   \t  ").is_empty());
}

// -------------------------------------------------------------- multiline --

#[test]
fn each_input_line_becomes_its_own_output_line() {
    let lines = convert_to_pinyin_lines("你好\n世界");

    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[0].hanzi.split_whitespace().collect::<Vec<_>>(),
        vec!["你", "好"]
    );
    assert_eq!(
        lines[1].hanzi.split_whitespace().collect::<Vec<_>>(),
        vec!["世", "界"]
    );
}

#[test]
fn blank_input_lines_are_dropped() {
    // A blank line tokenizes to nothing, so it contributes no output line.
    let lines = convert_to_pinyin_lines("你好\n\n世界");

    assert_eq!(lines.len(), 2);
}

// ------------------------------------------------------------------ wrap --

#[test]
fn long_input_wraps_onto_several_lines() {
    // Each hanzi costs at least three columns, so 80 of them cannot fit in the
    // 80-column budget.
    let lines = convert_to_pinyin_lines(&"中".repeat(80));

    assert!(
        lines.len() > 1,
        "expected wrapping, got {} line(s)",
        lines.len()
    );
}

#[test]
fn wrapped_lines_stay_within_the_width_budget() {
    let lines = convert_to_pinyin_lines(&"中文测试".repeat(30));

    for (i, line) in lines.iter().enumerate() {
        assert!(
            display_width(&line.hanzi) <= 80,
            "line {i} is {} columns wide: {:?}",
            display_width(&line.hanzi),
            line.hanzi
        );
    }
}

#[test]
fn a_single_oversized_token_is_not_dropped() {
    let long_word = "a".repeat(200);
    let lines = convert_to_pinyin_lines(&long_word);

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].hanzi, long_word);
}

// ------------------------------------------------------------- alignment --

#[test]
fn rows_align_for_plain_hanzi() {
    for line in &convert_to_pinyin_lines("你好世界") {
        assert_rows_align(line, "你好世界");
    }
}

#[test]
fn rows_align_for_long_pinyin_readings() {
    // "zhuāng" is far wider than the two columns its hanzi occupies.
    for line in &convert_to_pinyin_lines("装置") {
        assert_rows_align(line, "装置");
    }
}

#[test]
fn rows_align_for_mixed_scripts() {
    for line in &convert_to_pinyin_lines("认推给word别") {
        assert_rows_align(line, "认推给word别");
    }
}

#[test]
fn rows_align_with_punctuation() {
    // Punctuation is attached to the preceding hanzi rather than getting its
    // own column, so the column width has to account for it.
    let input = "你好，世界！";

    for line in &convert_to_pinyin_lines(input) {
        assert_rows_align(line, input);
    }
}

#[test]
fn rows_align_across_a_realistic_sentence() {
    let input = "我们今天去北京大学学习中文，然后吃饭。";

    for line in &convert_to_pinyin_lines(input) {
        assert_rows_align(line, input);
    }
}

#[test]
fn rows_align_for_every_wrapped_line() {
    let input = "中文测试".repeat(30);

    for line in &convert_to_pinyin_lines(&input) {
        assert_rows_align(line, "wrapped input");
    }
}

// ----------------------------------------------------------- misc detail --

#[test]
fn rows_are_right_trimmed() {
    for line in &convert_to_pinyin_lines("你好，") {
        assert_eq!(line.pinyin, line.pinyin.trim_end());
        assert_eq!(line.hanzi, line.hanzi.trim_end());
    }
}

#[test]
fn punctuation_stays_attached_to_its_character() {
    let lines = convert_to_pinyin_lines("你好，世界");

    assert_eq!(lines.len(), 1);
    assert!(
        lines[0].hanzi.contains("好，"),
        "punctuation should follow the character it belongs to, got {:?}",
        lines[0].hanzi
    );
}
