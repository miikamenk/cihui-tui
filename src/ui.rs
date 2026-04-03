use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, InputMode, UiLanguage};
use crate::pinyin_conv::PinyinLine;

pub fn draw_ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),      // Header with status
            Constraint::Percentage(15), // Input area (15% of screen)
            Constraint::Percentage(37), // Pinyin (37% of remaining space)
            Constraint::Percentage(43), // Translation (43% of remaining space)
            Constraint::Length(2),      // Help
        ])
        .split(f.area());

    // Header
    draw_header(f, app, chunks[0]);

    // Input
    draw_input(f, app, chunks[1]);

    // Pinyin display
    draw_pinyin(f, app, chunks[2]);

    // Translation
    draw_translation(f, app, chunks[3]);

    // Help/Status bar
    draw_help(f, app, chunks[4]);

    // Language selector overlay (if open)
    if app.language_selector_open {
        draw_language_selector(f, app);
    }

    // Settings overlay (if open)
    if app.settings_open {
        draw_settings(f, app);
    }
}

fn draw_header(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let title = app.get_title();
    let mode_text = match app.input_mode {
        InputMode::Text => match app.ui_language {
            UiLanguage::English => "[TEXT MODE]",
            UiLanguage::Chinese => "[文本模式]",
        },
        InputMode::ImagePath => match app.ui_language {
            UiLanguage::English => "[IMAGE MODE]",
            UiLanguage::Chinese => "[图片模式]",
        },
    };

    let lang_text = match app.ui_language {
        UiLanguage::English => "[EN]",
        UiLanguage::Chinese => "[中文]",
    };

    // Show current target language
    let target_lang_text = format!("[Target: {}]", app.target_language.name());

    let header_spans = vec![
        Span::styled(
            title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(mode_text, Style::default().fg(Color::Yellow)),
        Span::raw("  "),
        Span::styled(lang_text, Style::default().fg(Color::Green)),
        Span::raw("  "),
        Span::styled(target_lang_text, Style::default().fg(Color::Magenta)),
    ];

    let header = Paragraph::new(Line::from(header_spans))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::White)),
        );

    f.render_widget(header, area);
}

fn draw_input(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let label = app.get_input_label();
    let input_style = if app.processing {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let block = Block::default()
        .title(label)
        .borders(Borders::ALL)
        .border_style(input_style);

    let text = if app.processing {
        format!("{} [Processing...]", app.input)
    } else {
        app.input.clone()
    };

    let text_style = if app.select_all {
        Style::default().bg(Color::DarkGray).fg(Color::White)
    } else {
        Style::default()
    };

    let styled_lines: Vec<Line> = text
        .split('\n')
        .map(|line| Line::from(Span::styled(line.to_string(), text_style)))
        .collect();
    let input = Paragraph::new(Text::from(styled_lines))
        .block(block)
        .wrap(Wrap { trim: false });

    f.render_widget(input, area);

    // Set cursor position for multi-line input with wrapping
    if !app.processing && !app.settings_open && !app.language_selector_open {
        let inner_width = (area.width as usize).saturating_sub(2); // subtract borders
        let chars: Vec<char> = app.input.chars().collect();
        let mut visual_row: u16 = 0;
        let mut visual_col: usize = 0;

        for i in 0..app.cursor_position.min(chars.len()) {
            if chars[i] == '\n' {
                visual_row += 1;
                visual_col = 0;
            } else {
                let w = if is_wide_char(chars[i]) { 2 } else { 1 };
                if inner_width > 0 && visual_col + w > inner_width {
                    visual_row += 1;
                    visual_col = w;
                } else {
                    visual_col += w;
                }
            }
        }

        let cursor_x = area.x + 1 + visual_col as u16;
        let cursor_y = area.y + 1 + visual_row;
        if cursor_x < area.x + area.width && cursor_y < area.y + area.height {
            f.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

fn draw_pinyin(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let label = app.get_pinyin_label();

    let block = Block::default()
        .title(label)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let mut lines: Vec<Line> = Vec::new();

    if app.pinyin_lines.is_empty() && app.hanzi_lines.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            match app.ui_language {
                UiLanguage::English => "Pinyin will appear above Chinese characters",
                UiLanguage::Chinese => "拼音将显示在汉字上方",
            },
            Style::default().fg(Color::Gray),
        )]));
    } else {
        // Display pinyin lines
        for (pinyin, hanzi) in app.pinyin_lines.iter().zip(app.hanzi_lines.iter()) {
            // Pinyin line
            lines.push(Line::from(vec![Span::styled(
                pinyin.clone(),
                Style::default()
                    .fg(Color::LightBlue)
                    .add_modifier(Modifier::BOLD),
            )]));
            // Hanzi line
            lines.push(Line::from(vec![Span::styled(
                hanzi.clone(),
                Style::default().fg(Color::White),
            )]));
        }
    }

    let pinyin_widget = Paragraph::new(Text::from(lines))
        .block(block)
        .wrap(Wrap { trim: true })
        .scroll((app.pinyin_scroll, 0));

    f.render_widget(pinyin_widget, area);
}

fn draw_translation(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let label = app.get_translation_label();

    let block = Block::default()
        .title(label)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));

    let content = if app.translation.is_empty() {
        match app.ui_language {
            UiLanguage::English => "Translation will appear here...",
            UiLanguage::Chinese => "翻译将显示在这里...",
        }
        .to_string()
    } else {
        app.translation.clone()
    };

    let translation_widget = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: true })
        .scroll((app.translation_scroll, 0));

    f.render_widget(translation_widget, area);
}

fn draw_help(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let help_text = if app.language_selector_open {
        match app.ui_language {
            UiLanguage::English => {
                "↑↓ Navigate | Enter Select | Type to Search | Esc Close | Ctrl+X Clear search"
            }
            UiLanguage::Chinese => "↑↓ 导航 | Enter 选择 | 输入搜索 | Esc 关闭 | Ctrl+X 清除搜索",
        }
        .to_string()
    } else if app.settings_open {
        match app.ui_language {
            UiLanguage::English => "Settings: ↑↓ Navigate | Enter Toggle | Esc Close",
            UiLanguage::Chinese => "设置: ↑↓ 导航 | Enter 切换 | Esc 关闭",
        }
        .to_string()
    } else if let Some(ref error) = app.error_message {
        format!("Error: {} | {}", error, app.get_help_text())
    } else {
        app.get_help_text().to_string()
    };

    let style = if app.error_message.is_some() && !app.settings_open && !app.language_selector_open
    {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::Gray)
    };

    let help = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .style(style);

    f.render_widget(help, area);
}

fn draw_language_selector(f: &mut Frame, app: &App) {
    // Calculate popup area - takes up 70% of screen
    let popup_area = centered_rect(70, 80, f.area());

    // Clear background
    f.render_widget(Clear, popup_area);

    // Title
    let title = match app.ui_language {
        UiLanguage::English => "Select Target Language",
        UiLanguage::Chinese => "选择目标语言",
    };

    // Build the content
    let mut lines: Vec<Line> = Vec::new();

    // Show search query
    let search_label = match app.ui_language {
        UiLanguage::English => "Search:",
        UiLanguage::Chinese => "搜索:",
    };

    let search_style = if app.language_selector_search.is_empty() {
        Style::default().fg(Color::Gray)
    } else {
        Style::default().fg(Color::White)
    };

    lines.push(Line::from(vec![
        Span::styled(search_label, Style::default().fg(Color::Cyan)),
        Span::raw(" "),
        Span::styled(
            if app.language_selector_search.is_empty() {
                "(type to filter...)"
            } else {
                &app.language_selector_search
            },
            search_style,
        ),
    ]));

    // Separator
    lines.push(Line::from(vec![Span::styled(
        "─".repeat(popup_area.width as usize - 2),
        Style::default().fg(Color::Gray),
    )]));

    // Calculate visible items
    let visible_count = (popup_area.height as usize).saturating_sub(6); // Account for borders, title, search bar, separator

    // Determine scroll window
    let total_items = app.filtered_languages.len();
    let scroll = app.language_selector_scroll;

    // Show languages
    let start_idx = scroll.saturating_sub(visible_count / 2);
    let end_idx = (start_idx + visible_count).min(total_items);

    for (idx, &language) in app
        .filtered_languages
        .iter()
        .enumerate()
        .skip(start_idx)
        .take(end_idx - start_idx)
    {
        let is_selected = idx == scroll;
        let is_current = language == app.target_language;

        let arrow = if is_selected { "> " } else { "  " };
        let check = if is_current { "✓ " } else { "  " };

        let lang_name = language.name();
        let lang_code = language.google_code();
        let display = format!(
            "{}{} ({}) {}",
            check,
            lang_name,
            lang_code,
            if is_current { "[CURRENT]" } else { "" }
        );

        let style = if is_selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else if is_current {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::White)
        };

        lines.push(Line::from(vec![
            Span::styled(arrow, style),
            Span::styled(display, style),
        ]));
    }

    // Show scroll indicator if needed
    if total_items > visible_count {
        let scroll_info = format!("{} / {}", scroll + 1, total_items);
        lines.push(Line::from(vec![])); // Empty line
        lines.push(Line::from(vec![Span::styled(
            scroll_info,
            Style::default().fg(Color::Gray),
        )]));
    }

    // Show empty message if no results
    if app.filtered_languages.is_empty() {
        let no_results = match app.ui_language {
            UiLanguage::English => "No languages match your search",
            UiLanguage::Chinese => "没有匹配的语言",
        };
        lines.push(Line::from(vec![Span::styled(
            no_results,
            Style::default().fg(Color::Red),
        )]));
    }

    let selector_widget = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .alignment(Alignment::Left);

    f.render_widget(selector_widget, popup_area);
}

fn draw_settings(f: &mut Frame, app: &App) {
    // Calculate centered popup area
    let popup_area = centered_rect(60, 40, f.area());

    // Clear background
    f.render_widget(Clear, popup_area);

    // Settings title
    let title = match app.ui_language {
        UiLanguage::English => "Settings",
        UiLanguage::Chinese => "设置",
    };

    // Create settings content
    let mut lines: Vec<Line> = Vec::new();

    // UI Language setting
    let lang_label = match app.ui_language {
        UiLanguage::English => "UI Language",
        UiLanguage::Chinese => "界面语言",
    };
    let lang_value = match app.ui_language {
        UiLanguage::English => "English",
        UiLanguage::Chinese => "中文",
    };

    let is_selected = app.settings_selection == 0;
    let style = if is_selected {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let arrow = if is_selected { "> " } else { "  " };

    lines.push(Line::from(vec![
        Span::styled(arrow, style),
        Span::styled(format!("{}: {}", lang_label, lang_value), style),
    ]));

    // Add empty line
    lines.push(Line::from(vec![]));

    // Instructions
    let instructions = match app.ui_language {
        UiLanguage::English => "Press Enter to toggle",
        UiLanguage::Chinese => "按 Enter 键切换",
    };
    lines.push(Line::from(vec![Span::styled(
        instructions,
        Style::default().fg(Color::Gray),
    )]));

    let settings_block = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .alignment(Alignment::Left);

    f.render_widget(settings_block, popup_area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

pub fn update_pinyin_display(app: &mut App, pinyin_lines: Vec<PinyinLine>) {
    app.pinyin_lines.clear();
    app.hanzi_lines.clear();

    for line in pinyin_lines {
        app.pinyin_lines.push(line.pinyin);
        app.hanzi_lines.push(line.hanzi);
    }
}

fn is_wide_char(c: char) -> bool {
    // CJK characters are typically 2 columns wide
    ('\u{4e00}'..='\u{9fff}').contains(&c)
        || ('\u{3400}'..='\u{4dbf}').contains(&c)
        || ('\u{f900}'..='\u{faff}').contains(&c)
        || ('\u{20000}'..='\u{2a6df}').contains(&c)
        || ('\u{2a700}'..='\u{2b73f}').contains(&c)
        || ('\u{2b740}'..='\u{2b81f}').contains(&c)
        || c == '\u{3000}'  // Full-width space
        || ('\u{ff01}'..='\u{ff5e}').contains(&c)  // Full-width ASCII
        || ('\u{ff5f}'..='\u{ff60}').contains(&c)  // Full-width brackets
        || ('\u{ffe0}'..='\u{ffe6}').contains(&c) // Full-width symbols
}
