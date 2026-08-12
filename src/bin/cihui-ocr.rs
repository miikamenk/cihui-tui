use std::io;

use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use cihui_tui::app::{App, InputMode};
use cihui_tui::image_source::{extract_image_source, ImageSource};
use cihui_tui::keys::{handle_language_selector_input, handle_settings_input};
use cihui_tui::ltengine;
use cihui_tui::ocr;
use cihui_tui::pinyin_conv;
use cihui_tui::translation;
use cihui_tui::ui::{draw_ui, update_pinyin_display};

#[derive(Parser, Debug)]
#[command(name = "cihui-ocr")]
#[command(about = "Chinese vocabulary learning tool with OCR")]
struct Cli {}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _cli = Cli::parse();

    let tty_fd = unsafe { libc::dup(1) };
    suppress_output();

    let tty_file = unsafe { std::os::unix::io::FromRawFd::from_raw_fd(tty_fd) };
    let mut tty_write: std::fs::File = tty_file;

    enable_raw_mode()?;
    execute!(tty_write, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(tty_write);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    let res = run_app(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("Error: {:?}", err);
    }

    Ok(())
}

async fn run_app<W: io::Write>(
    terminal: &mut Terminal<CrosstermBackend<W>>,
    app: &mut App,
) -> io::Result<()> {
    let mut last_tick = std::time::Instant::now();
    let tick_rate = std::time::Duration::from_millis(250);
    let debounce_duration = std::time::Duration::from_millis(500);

    let mut ltengine = ltengine::LTEngine::new(
        5050,
        app.ltengine_model.clone(),
        app.ltengine_path.clone(),
    );

    loop {
        terminal.draw(|f| draw_ui(f, app))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| std::time::Duration::from_secs(0));

        tokio::select! {
            _ = tokio::time::sleep(timeout) => {}

            event_result = async { crossterm::event::poll(timeout).map(|ready| if ready { event::read().ok() } else { None }) } => {
                if let Ok(Some(Event::Key(key))) = event_result {
                    if app.language_selector_open {
                        handle_language_selector_input(app, key);
                        continue;
                    }

                    if app.settings_open {
                        handle_settings_input(app, key);
                        continue;
                    }

                    if key.code == KeyCode::Esc {
                        ltengine.shutdown().await;
                        return Ok(());
                    }

                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        match key.code {
                            KeyCode::Char('l') | KeyCode::Char('L') => {
                                app.toggle_language_selector();
                                continue;
                            }
                            KeyCode::Char('s') | KeyCode::Char('S') => {
                                app.toggle_settings();
                                continue;
                            }
                            KeyCode::Char('x') | KeyCode::Char('X') => {
                                app.clear();
                                continue;
                            }
                            KeyCode::Char('v') | KeyCode::Char('V') => {
                                if let Err(e) = handle_clipboard_paste(app).await {
                                    app.error_message = Some(format!("Paste error: {}", e));
                                }
                                continue;
                            }
                            KeyCode::Char('a') | KeyCode::Char('A') => {
                                app.select_all();
                                continue;
                            }
                            KeyCode::Char('w') | KeyCode::Char('W') => {
                                app.delete_word_backwards();
                                continue;
                            }
                            KeyCode::Char('u') | KeyCode::Char('U') => {
                                app.delete_to_start();
                                continue;
                            }
                            KeyCode::Char('e') | KeyCode::Char('E') => {
                                app.move_cursor_to_end();
                                continue;
                            }
                            KeyCode::Char('c') | KeyCode::Char('C') => {
                                ltengine.shutdown().await;
                                return Ok(());
                            }
                            KeyCode::Up => {
                                app.pinyin_scroll = app.pinyin_scroll.saturating_sub(3);
                                app.translation_scroll = app.translation_scroll.saturating_sub(3);
                                continue;
                            }
                            KeyCode::Down => {
                                app.pinyin_scroll = app.pinyin_scroll.saturating_add(3);
                                app.translation_scroll = app.translation_scroll.saturating_add(3);
                                continue;
                            }
                            KeyCode::Char('j') | KeyCode::Char('J') => {
                                app.pinyin_scroll = app.pinyin_scroll.saturating_add(3);
                                app.translation_scroll = app.translation_scroll.saturating_add(3);
                                continue;
                            }
                            KeyCode::Char('k') | KeyCode::Char('K') => {
                                app.pinyin_scroll = app.pinyin_scroll.saturating_sub(3);
                                app.translation_scroll = app.translation_scroll.saturating_sub(3);
                                continue;
                            }
                            KeyCode::Backspace => {
                                app.delete_word_backwards();
                                continue;
                            }
                            _ => {}
                        }
                    }

                    match key.code {
                        KeyCode::Char(c) => {
                            if app.input_mode == InputMode::ImagePath {
                                app.input_mode = InputMode::Text;
                            }
                            app.insert_char(c);
                        }
                        KeyCode::Enter => {
                            app.insert_char('\n');
                        }
                        KeyCode::Backspace => {
                            app.backspace();
                        }
                        KeyCode::Delete => {
                            if app.select_all {
                                app.select_all = false;
                                app.clear();
                                app.last_input_time = Some(std::time::Instant::now());
                            } else {
                                let byte_idx = app.cursor_byte_index();
                                if byte_idx < app.input.len() {
                                    let next_char = app.input[byte_idx..].chars().next();
                                    if let Some(ch) = next_char {
                                        app.input.drain(byte_idx..(byte_idx + ch.len_utf8()));
                                    }
                                    app.last_input_time = Some(std::time::Instant::now());
                                }
                            }
                        }
                        KeyCode::Left => {
                            app.move_cursor_left();
                        }
                        KeyCode::Right => {
                            app.move_cursor_right();
                        }
                        KeyCode::Up => {
                            app.move_cursor_up();
                        }
                        KeyCode::Down => {
                            app.move_cursor_down();
                        }
                        KeyCode::Home => {
                            app.move_cursor_to_start();
                        }
                        KeyCode::End => {
                            app.move_cursor_to_end();
                        }
                        _ => {}
                    }
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = std::time::Instant::now();
            ltengine.check_idle().await;

            if let Some(last_input) = app.last_input_time {
                if !app.processing && !app.input.trim().is_empty() {
                    let elapsed = last_input.elapsed();
                    if elapsed >= debounce_duration {
                        app.last_input_time = None;
                        if let Err(e) = process_input(app, &mut ltengine).await {
                            app.error_message = Some(format!("Processing error: {}", e));
                        }
                    }
                }
            }
        }
    }
}

async fn handle_clipboard_paste(app: &mut App) -> anyhow::Result<()> {
    use arboard::Clipboard;

    let mut clipboard = Clipboard::new()?;

    match clipboard.get_image() {
        Ok(image) => {
            let ocr_result = ocr::recognize_image_from_rgba(
                image.width as u32,
                image.height as u32,
                &image.bytes,
            )
            .await?;
            app.input_mode = InputMode::Text;
            app.set_input(ocr_result.text);
            app.last_input_time = Some(std::time::Instant::now());
            return Ok(());
        }
        Err(_) => {
            #[cfg(target_os = "linux")]
            {
                match try_external_clipboard_image().await {
                    Ok(Some(image_data)) => {
                        let ocr_result = ocr::recognize_image(&image_data).await?;
                        app.input_mode = InputMode::Text;
                        app.set_input(ocr_result.text);
                        app.last_input_time = Some(std::time::Instant::now());
                        return Ok(());
                    }
                    _ => {}
                }
            }
            
            match clipboard.get_text() {
                Ok(text) => {
                    let trimmed = text.trim();

                    if let Some(image_source) = extract_image_source(trimmed) {
                        let ocr_result = process_image_from_source(image_source).await?;
                        app.input_mode = InputMode::Text;
                        app.set_input(ocr_result.text);
                        app.last_input_time = Some(std::time::Instant::now());
                    } else {
                        let sanitized: String = trimmed
                            .chars()
                            .filter(|c| c.is_whitespace() || !c.is_control())
                            .collect();
                        app.set_input(sanitized.to_string());
                        app.input_mode = InputMode::Text;
                    }
                    Ok(())
                }
                Err(_) => Err(anyhow::anyhow!("No text or image in clipboard")),
            }
        }
    }
}

#[cfg(target_os = "linux")]
async fn try_external_clipboard_image() -> anyhow::Result<Option<Vec<u8>>> {
    use tokio::process::Command;
    
    let output = Command::new("wl-paste")
        .args(&["--type", "image/png"])
        .output()
        .await;
    
    match output {
        Ok(result) if result.status.success() && !result.stdout.is_empty() => {
            return Ok(Some(result.stdout));
        }
        _ => {}
    }
    
    let output = Command::new("xclip")
        .args(&["-selection", "clipboard", "-t", "image/png", "-o"])
        .output()
        .await;
    
    match output {
        Ok(result) if result.status.success() && !result.stdout.is_empty() => {
            return Ok(Some(result.stdout));
        }
        _ => {}
    }
    
    let output = Command::new("xclip")
        .args(&["-selection", "clipboard", "-t", "image/jpeg", "-o"])
        .output()
        .await;
    
    match output {
        Ok(result) if result.status.success() && !result.stdout.is_empty() => {
            return Ok(Some(result.stdout));
        }
        _ => {}
    }
    
    let output = Command::new("xclip")
        .args(&["-selection", "clipboard", "-t", "image/bmp", "-o"])
        .output()
        .await;
    
    match output {
        Ok(result) if result.status.success() && !result.stdout.is_empty() => {
            return Ok(Some(result.stdout));
        }
        _ => {}
    }
    
    Ok(None)
}

async fn process_image_from_source(source: ImageSource) -> anyhow::Result<ocr::OcrResult> {
    match source {
        ImageSource::Path(path) | ImageSource::PathOrUrl(path) => {
            let clean_path = if path.starts_with("file://") {
                path.strip_prefix("file://").unwrap_or(&path).to_string()
            } else {
                path
            };
            
            if std::path::Path::new(&clean_path).exists() {
                ocr::recognize_image_from_path(&clean_path).await
            } else if clean_path.starts_with("http://") || clean_path.starts_with("https://") {
                download_and_recognize(&clean_path).await
            } else {
                Err(anyhow::anyhow!("Cannot find image: {}", clean_path))
            }
        }
        ImageSource::Url(url) => download_and_recognize(&url).await,
    }
}

async fn download_and_recognize(url: &str) -> anyhow::Result<ocr::OcrResult> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let response = client.get(url).send().await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to download image: HTTP {}",
            response.status()
        ));
    }

    let bytes = response.bytes().await?;
    ocr::recognize_image(&bytes).await
}

async fn process_input(app: &mut App, ltengine: &mut ltengine::LTEngine) -> anyhow::Result<()> {
    app.processing = true;
    app.error_message = None;

    let result = match app.input_mode {
        InputMode::Text => process_text_input(app, ltengine).await,
        InputMode::ImagePath => process_image_input(app, ltengine).await,
    };

    app.processing = false;
    result
}

async fn process_text_input(app: &mut App, ltengine: &mut ltengine::LTEngine) -> anyhow::Result<()> {
    let text = app.input.trim().to_string();
    if text.is_empty() {
        return Ok(());
    }

    app.detect_input_language();

    if app.is_input_chinese() {
        let target_lang_code = app.target_language.google_code();
        let pinyin_lines = pinyin_conv::convert_to_pinyin_lines(&text);
        update_pinyin_display(app, pinyin_lines);

        match translation::translate("zh-CN", target_lang_code, &text, app.translation_service, &app.local_translate_url, ltengine).await {
            Ok(translation) => {
                app.translation = translation;
            }
            Err(e) => {
                app.error_message = Some(format!("Translation failed: {}", e));
                app.translation = format!("[Translation error] {}", text);
            }
        }
    } else {
        let target_lang_code = app.target_language.google_code();

        match translation::translate(target_lang_code, "zh-CN", &text, app.translation_service, &app.local_translate_url, ltengine).await {
            Ok(chinese_text) => {
                let pinyin_lines = pinyin_conv::convert_to_pinyin_lines(&chinese_text);
                update_pinyin_display(app, pinyin_lines);
                app.translation = chinese_text;
            }
            Err(e) => {
                app.error_message = Some(format!("Translation failed: {}", e));
                app.pinyin_lines.clear();
                app.hanzi_lines.clear();
                app.pinyin_lines.push(text.clone());
                app.hanzi_lines.push(text.clone());
                app.translation = format!("[Translation error] {}", text);
            }
        }
    }

    Ok(())
}

async fn process_image_input(app: &mut App, ltengine: &mut ltengine::LTEngine) -> anyhow::Result<()> {
    let input = app.input.trim();

    use arboard::Clipboard;
    let mut clipboard = Clipboard::new()?;

    let ocr_result = if input.is_empty() {
        match clipboard.get_image() {
            Ok(image) => {
                ocr::recognize_image_from_rgba(
                    image.width as u32,
                    image.height as u32,
                    &image.bytes,
                )
                .await?
            }
            Err(_) => {
                match clipboard.get_text() {
                    Ok(text) => {
                        if let Some(source) = extract_image_source(&text) {
                            process_image_from_source(source).await?
                        } else {
                            return Err(anyhow::anyhow!(
                                "No image in clipboard. Copy an image or an image URL first."
                            ));
                        }
                    }
                    Err(_) => {
                        return Err(anyhow::anyhow!(
                            "No image or text in clipboard. Copy an image first."
                        ));
                    }
                }
            }
        }
    } else if input.starts_with("http://") || input.starts_with("https://") {
        let client = reqwest::Client::new();
        let response = client.get(input).send().await?;
        let bytes = response.bytes().await?;
        ocr::recognize_image(&bytes).await?
    } else if std::path::Path::new(input).exists() {
        ocr::recognize_image_from_path(input).await?
    } else {
        return Err(anyhow::anyhow!(
            "Only clipboard images, URLs, and file paths are supported."
        ));
    };

    let text = ocr_result.text;

    if text.is_empty() {
        app.error_message = Some("No text found in image".to_string());
        return Ok(());
    }

    app.input = text.clone();

    process_text_input(app, ltengine).await
}

fn suppress_output() {
    use std::fs::OpenOptions;
    use std::os::unix::io::AsRawFd;

    if let Ok(dev_null) = OpenOptions::new().write(true).open("/dev/null") {
        unsafe {
            libc::dup2(dev_null.as_raw_fd(), 1);
            libc::dup2(dev_null.as_raw_fd(), 2);
        }
    }
}
