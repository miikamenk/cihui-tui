use pinyin::ToPinyin;

pub struct PinyinLine {
    pub pinyin: String,
    pub hanzi: String,
}

#[derive(Debug, Clone)]
enum Token {
    Chinese { hanzi: String, pinyin: String },
    English(String),
}

pub fn convert_to_pinyin_lines(text: &str) -> Vec<PinyinLine> {
    let mut all_lines: Vec<PinyinLine> = Vec::new();
    for input_line in text.split('\n') {
        all_lines.extend(convert_single_line(input_line));
    }
    all_lines
}

fn convert_single_line(text: &str) -> Vec<PinyinLine> {
    let max_line_width = 80; // Maximum display width per line
    let tokens = tokenize(text);
    let mut lines: Vec<PinyinLine> = Vec::new();

    // Process tokens into lines
    let mut current_line_tokens: Vec<Token> = Vec::new();
    let mut current_line_width = 0;

    for token in tokens {
        let token_width = get_token_display_width(&token);

        // Check if adding this token would exceed line width
        if current_line_width + token_width > max_line_width && !current_line_tokens.is_empty() {
            // Build the line from current tokens
            lines.push(build_line(&current_line_tokens));
            current_line_tokens.clear();
            current_line_width = 0;
        }

        current_line_tokens.push(token);
        current_line_width += token_width;
    }

    // Don't forget the last line
    if !current_line_tokens.is_empty() {
        lines.push(build_line(&current_line_tokens));
    }

    lines
}

fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens: Vec<Token> = Vec::new();
    let mut english_buffer = String::new();
    let mut last_was_chinese = false;

    for c in text.chars() {
        if c.is_whitespace() {
            // Flush English buffer if any
            if !english_buffer.is_empty() {
                tokens.push(Token::English(english_buffer.clone()));
                english_buffer.clear();
                last_was_chinese = false;
            }
            continue;
        }

        if is_chinese(c) {
            // Flush English buffer if any
            if !english_buffer.is_empty() {
                tokens.push(Token::English(english_buffer.clone()));
                english_buffer.clear();
            }

            // Get pinyin for Chinese character
            let pinyin_str = get_pinyin_for_char(c).unwrap_or_default();
            tokens.push(Token::Chinese {
                hanzi: c.to_string(),
                pinyin: pinyin_str,
            });
            last_was_chinese = true;
        } else if is_punctuation(c) && last_was_chinese {
            // Attach punctuation to previous Chinese character
            if let Some(last) = tokens.last_mut() {
                if let Token::Chinese { hanzi, pinyin: _ } = last {
                    // Append punctuation to the hanzi string
                    hanzi.push(c);
                }
            }
        } else {
            // Non-Chinese, non-punctuation character (part of English word)
            english_buffer.push(c);
            last_was_chinese = false;
        }
    }

    // Flush remaining English buffer
    if !english_buffer.is_empty() {
        tokens.push(Token::English(english_buffer));
    }

    tokens
}

fn build_line(tokens: &[Token]) -> PinyinLine {
    let mut pinyin_parts: Vec<String> = Vec::new();
    let mut hanzi_parts: Vec<String> = Vec::new();
    let mut hanzi_display_widths: Vec<usize> = Vec::new();
    let mut column_widths: Vec<usize> = Vec::new();

    // First pass: calculate column widths
    for token in tokens {
        let (pinyin_str, hanzi_str, display_width) = match token {
            Token::Chinese { hanzi, pinyin } => {
                // Calculate display width: Chinese chars take 2 columns, punctuation takes 1
                let chinese_count = hanzi.chars().filter(|c| is_chinese(*c)).count();
                let punct_count = hanzi.chars().filter(|c| is_punctuation(*c)).count();
                let display_width = chinese_count * 2 + punct_count * 2; // Chinese = 2 cols, punct = 1 col
                (pinyin.clone(), hanzi.clone(), display_width)
            }
            Token::English(word) => {
                let display_width = word.chars().count(); // ASCII chars take 1 column each
                (word.clone(), word.clone(), display_width)
            }
        };

        // Column width is max of pinyin length and hanzi display width
        let pinyin_len = pinyin_str.chars().count();
        let width = pinyin_len.max(display_width).max(1);

        column_widths.push(width);
        hanzi_display_widths.push(display_width);
        pinyin_parts.push(pinyin_str);
        hanzi_parts.push(hanzi_str);
    }

    // Second pass: build aligned strings
    let mut aligned_pinyin = String::new();
    let mut aligned_hanzi = String::new();
    let total_tokens = pinyin_parts.len();

    for (i, (pinyin, hanzi)) in pinyin_parts.iter().zip(hanzi_parts.iter()).enumerate() {
        let width = column_widths[i];
        let hanzi_display = hanzi_display_widths[i];
        let is_last = i == total_tokens - 1;

        // Pad pinyin (based on character count)
        aligned_pinyin.push_str(pinyin);
        let pinyin_len = pinyin.chars().count();
        for _ in pinyin_len..width {
            aligned_pinyin.push(' ');
        }
        // Only add separator if not the last token
        if !is_last {
            aligned_pinyin.push(' ');
        }

        // Pad hanzi (based on display width)
        aligned_hanzi.push_str(hanzi);
        // Add extra spaces if hanzi display width is less than column width
        // For Chinese chars (width 2), we need to account for their visual width
        let visual_padding = if hanzi_display < width {
            width - hanzi_display
        } else {
            0
        };
        for _ in 0..visual_padding {
            aligned_hanzi.push(' ');
        }
        // Only add separator if not the last token
        if !is_last {
            aligned_hanzi.push(' ');
        }
    }

    PinyinLine {
        pinyin: aligned_pinyin.trim_end().to_string(),
        hanzi: aligned_hanzi.trim_end().to_string(),
    }
}

fn get_token_display_width(token: &Token) -> usize {
    match token {
        Token::Chinese { hanzi, pinyin } => {
            // Width is max of pinyin length and hanzi display width
            // Plus 1 for the space separator between columns
            let pinyin_len = pinyin.chars().count();
            // Calculate display width: Chinese chars take 2 columns, punctuation takes 1
            let chinese_count = hanzi.chars().filter(|c| is_chinese(*c)).count();
            let punct_count = hanzi.chars().filter(|c| is_punctuation(*c)).count();
            let hanzi_display = chinese_count * 2 + punct_count;
            pinyin_len.max(hanzi_display) + 1
        }
        Token::English(word) => {
            // English word width is its character count
            // Plus 1 for the space separator between columns
            word.chars().count() + 1
        }
    }
}

fn is_chinese(c: char) -> bool {
    // CJK Unified Ideographs
    ('\u{4e00}'..='\u{9fff}').contains(&c)
        || ('\u{3400}'..='\u{4dbf}').contains(&c)
        || ('\u{f900}'..='\u{faff}').contains(&c)
}

fn is_punctuation(c: char) -> bool {
    // Common Chinese and English punctuation
    matches!(
        c,
        '，' | '。'
            | '！'
            | '？'
            | '、'
            | '：'
            | '；'
            | '\"'
            | '\''
            | '（'
            | '）'
            | '【'
            | '】'
            | '《'
            | '》'
            | '…'
            | '—'
            | '-'
            | ','
            | '.'
            | '!'
            | '?'
            | ':'
            | ';'
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
    )
}

fn get_pinyin_for_char(c: char) -> Option<String> {
    let mut result = None;
    if let Some(pinyin) = c.to_pinyin() {
        result = Some(pinyin.with_tone().to_string());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_conversion() {
        let result = convert_to_pinyin_lines("你好世界");
        assert!(!result.is_empty());
        let line = &result[0];
        assert!(line.pinyin.contains("nǐ"));
        assert!(line.pinyin.contains("hǎo"));
        assert!(line.hanzi.contains("你"));
        assert!(line.hanzi.contains("好"));
    }

    #[test]
    fn test_english_word() {
        let result = convert_to_pinyin_lines("this");
        assert!(!result.is_empty());
        let line = &result[0];
        assert_eq!(line.pinyin, "this");
        assert_eq!(line.hanzi, "this");
    }

    #[test]
    fn test_mixed_content() {
        let result = convert_to_pinyin_lines("认推给word别");
        assert!(!result.is_empty());
        let line = &result[0];
        // Pinyin should be: "rèn tuī gěi word bié"
        // Hanzi should be:   "认 推 给 word 别"
        assert!(line.pinyin.contains("rèn"));
        assert!(line.pinyin.contains("tuī"));
        assert!(line.pinyin.contains("gěi"));
        assert!(line.pinyin.contains("word"));
        assert!(line.pinyin.contains("bié"));
    }

    #[test]
    fn test_is_chinese() {
        assert!(is_chinese('中'));
        assert!(!is_chinese('a'));
        assert!(!is_chinese('1'));
    }
}
