use std::io;

use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;

mod app;
mod config;
mod language;
mod ltengine;
mod ocr;
mod pinyin_conv;
mod single_instance;
mod transcription;
mod translation;
mod ui;

use app::{App, AppMode, InputMode};
use transcription::TranscriptionEvent;
use ui::{draw_ui, update_pinyin_display};

#[derive(Parser, Debug)]
#[command(name = "cihui-tui")]
#[command(about = "Chinese vocabulary learning tool with TUI")]
struct Cli {
    #[arg(short, long, help = "Toggle: close the running instance if it exists")]
    toggle: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    if cli.toggle {
        match single_instance::send_shutdown_signal() {
            Ok(()) => {
                println!("Sent shutdown signal to running instance.");
                return Ok(());
            }
            Err(single_instance::SingleInstanceError::ToggleFailed(ref e))
                if e.kind() == std::io::ErrorKind::ConnectionRefused =>
            {
                // No instance running, continue to start a new one
                println!("No instance running, starting new instance...");
            }
            Err(e) => {
                eprintln!("Failed to toggle: {}", e);
                std::process::exit(1);
            }
        }
    }

    let listener = match single_instance::claim_instance() {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let shutdown_rx = single_instance::start_shutdown_server(listener);

    // Dup stdout fd before redirecting, so the terminal backend can still write
    let tty_fd = unsafe { libc::dup(1) };

    // Redirect both stdout and stderr to /dev/null to suppress
    // ALSA/JACK/PipeWire/candle library noise that corrupts the TUI
    suppress_output();

    // Create a File from the dup'd fd for the terminal backend
    let tty_file = unsafe { std::os::unix::io::FromRawFd::from_raw_fd(tty_fd) };
    let mut tty_write: std::fs::File = tty_file;

    enable_raw_mode()?;
    execute!(tty_write, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(tty_write);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    let res = run_app(&mut terminal, &mut app, shutdown_rx).await;

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
    mut shutdown_rx: mpsc::Receiver<()>,
) -> io::Result<()> {
    let mut last_tick = std::time::Instant::now();
    let tick_rate = std::time::Duration::from_millis(250);
    let debounce_duration = std::time::Duration::from_millis(500);

    // LTEngine process manager — always uses port 5050 (LTEngine default)
    let mut ltengine = ltengine::LTEngine::new(
        5050,
        app.ltengine_model.clone(),
        app.ltengine_path.clone(),
    );

    // Transcription state managed outside App (non-Send types)
    let (transcription_tx, mut transcription_rx) = mpsc::channel::<TranscriptionEvent>(64);
    let (model_tx, mut model_rx) = mpsc::channel::<kalosm::sound::Whisper>(1);
    let mut whisper_model: Option<kalosm::sound::Whisper> = None;
    let mut transcription_handle: Option<tokio::task::JoinHandle<()>> = None;

    loop {
        terminal.draw(|f| draw_ui(f, app))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| std::time::Duration::from_secs(0));

        tokio::select! {
            _ = tokio::time::sleep(timeout) => {
                // Continue to tick processing
            }

            _ = shutdown_rx.recv() => {
                stop_transcription(&mut transcription_handle, app);
                ltengine.shutdown().await;
                return Ok(());
            }

            Some(model) = model_rx.recv() => {
                whisper_model = Some(model);
            }

            Some(event) = transcription_rx.recv() => {
                match event {
                    TranscriptionEvent::ModelLoading { progress, status } => {
                        app.transcription.model_loading = true;
                        app.transcription.model_progress = progress;
                        app.transcription.status = status;
                    }
                    TranscriptionEvent::ModelReady => {
                        app.transcription.model_loading = false;
                        app.transcription.model_ready = true;
                        app.transcription.status = "Model ready. Press Space to record.".into();
                    }
                    TranscriptionEvent::Segment(text) => {
                        if !app.transcription.transcript.is_empty()
                            && !app.transcription.transcript.ends_with(' ')
                            && !app.transcription.transcript.ends_with('\n')
                        {
                            app.transcription.transcript.push(' ');
                        }
                        app.transcription.transcript.push_str(text.trim());

                        // Process the full transcript for pinyin + translation
                        let full_text = app.transcription.transcript.clone();
                        let target_lang = app.target_language;
                        let service = app.translation_service;
                        let local_url = app.local_translate_url.clone();
                        process_transcription_text(app, &full_text, target_lang, service, &local_url, &mut ltengine).await;
                    }
                    TranscriptionEvent::VadActivity(active) => {
                        app.transcription.vad_active = active;
                    }
                    TranscriptionEvent::Error(e) => {
                        app.transcription.status = format!("Error: {}", e);
                        app.transcription.is_recording = false;
                    }
                }
            }

            event_result = async { crossterm::event::poll(timeout).map(|ready| if ready { event::read().ok() } else { None }) } => {
                if let Ok(Some(Event::Key(key))) = event_result {
                    // --- Transcription mode input ---
                    if app.mode == AppMode::Transcription {
                        if app.transcription.device_selector_open {
                            handle_device_selector_input(app, key);
                            continue;
                        }
                        if app.transcription.settings_open {
                            handle_transcription_settings_input(app, key);
                            // Clear cached model if settings changed it
                            if !app.transcription.model_ready {
                                whisper_model = None;
                                app.transcription.status = "Settings changed. Press Space to reload model and record.".into();
                            }
                            continue;
                        }

                        if key.code == KeyCode::Esc {
                            stop_transcription(&mut transcription_handle, app);
                            ltengine.shutdown().await;
                            return Ok(());
                        }

                        if key.modifiers.contains(KeyModifiers::CONTROL) {
                            match key.code {
                                KeyCode::Char('t') | KeyCode::Char('T') => {
                                    stop_transcription(&mut transcription_handle, app);
                                    app.toggle_mode();
                                    continue;
                                }
                                KeyCode::Char('d') | KeyCode::Char('D') => {
                                    app.toggle_transcription_device_selector();
                                    continue;
                                }
                                KeyCode::Char('s') | KeyCode::Char('S') => {
                                    app.toggle_transcription_settings();
                                    continue;
                                }
                                KeyCode::Char('x') | KeyCode::Char('X') => {
                                    app.transcription.transcript.clear();
                                    app.transcription.pinyin_lines.clear();
                                    app.transcription.hanzi_lines.clear();
                                    app.transcription.translation.clear();
                                    app.transcription.transcript_scroll = 0;
                                    continue;
                                }
                                KeyCode::Char('c') | KeyCode::Char('C') => {
                                    stop_transcription(&mut transcription_handle, app);
                                    ltengine.shutdown().await;
                                    return Ok(());
                                }
                                KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('K') => {
                                    app.transcription.transcript_scroll =
                                        app.transcription.transcript_scroll.saturating_sub(3);
                                    continue;
                                }
                                KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('J') => {
                                    app.transcription.transcript_scroll =
                                        app.transcription.transcript_scroll.saturating_add(3);
                                    continue;
                                }
                                _ => {}
                            }
                        }

                        match key.code {
                            KeyCode::Char(' ') => {
                                if app.transcription.is_recording {
                                    stop_transcription(&mut transcription_handle, app);
                                } else if app.transcription.model_ready {
                                    if let Some(ref model) = whisper_model {
                                        let handle = transcription::start_transcription(
                                            model.clone(),
                                            app.transcription.selected_device.clone(),
                                            transcription_tx.clone(),
                                        );
                                        transcription_handle = Some(handle);
                                        app.transcription.is_recording = true;
                                        app.transcription.status = "Recording...".into();
                                    }
                                } else if !app.transcription.model_loading {
                                    let tx = transcription_tx.clone();
                                    let mtx = model_tx.clone();
                                    let model_size = app.transcription.model_size;
                                    let language = app.transcription.language;
                                    app.transcription.model_loading = true;
                                    app.transcription.status = "Loading model...".into();
                                    tokio::spawn(async move {
                                        match transcription::load_model(model_size, language, tx.clone()).await {
                                            Ok(m) => { let _ = mtx.send(m).await; }
                                            Err(e) => {
                                                let _ = tx.send(TranscriptionEvent::Error(
                                                    format!("Model load failed: {}", e),
                                                )).await;
                                            }
                                        }
                                    });
                                }
                                continue;
                            }
                            _ => {}
                        }

                        continue;
                    }

                    // --- Normal mode input ---
                    if app.language_selector_open {
                        handle_language_selector_input(app, key).await;
                        continue;
                    }

                    if app.settings_open {
                        handle_settings_input(app, key).await;
                        continue;
                    }

                    if key.code == KeyCode::Esc {
                        ltengine.shutdown().await;
                        return Ok(());
                    }

                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        match key.code {
                            KeyCode::Char('t') | KeyCode::Char('T') => {
                                app.toggle_mode();
                                // Start loading model on first entry
                                if app.mode == AppMode::Transcription
                                    && whisper_model.is_none()
                                    && !app.transcription.model_loading
                                {
                                    let tx = transcription_tx.clone();
                                    let mtx = model_tx.clone();
                                    let model_size = app.transcription.model_size;
                                    let language = app.transcription.language;
                                    app.transcription.model_loading = true;
                                    app.transcription.status = "Loading model...".into();
                                    tokio::spawn(async move {
                                        match transcription::load_model(model_size, language, tx.clone()).await {
                                            Ok(m) => { let _ = mtx.send(m).await; }
                                            Err(e) => {
                                                let _ = tx.send(TranscriptionEvent::Error(
                                                    format!("Model load failed: {}", e),
                                                )).await;
                                            }
                                        }
                                    });
                                }
                                continue;
                            }
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

            // Check if LTEngine should be shut down due to idle
            ltengine.check_idle().await;

            if app.mode == AppMode::Normal {
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
}

fn stop_transcription(
    handle: &mut Option<tokio::task::JoinHandle<()>>,
    app: &mut App,
) {
    if let Some(h) = handle.take() {
        h.abort();
    }
    app.transcription.is_recording = false;
    app.transcription.vad_active = false;
    if app.transcription.model_ready {
        app.transcription.status = "Stopped. Press Space to record.".into();
    }
}

fn handle_device_selector_input(app: &mut App, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.transcription.device_selector_open = false;
        }
        KeyCode::Up => {
            if app.transcription.device_selector_scroll > 0 {
                app.transcription.device_selector_scroll -= 1;
            }
        }
        KeyCode::Down => {
            let max = app.transcription.available_devices.len().saturating_sub(1);
            if app.transcription.device_selector_scroll < max {
                app.transcription.device_selector_scroll += 1;
            }
        }
        KeyCode::Enter => {
            app.transcription_device_select();
        }
        _ => {}
    }
}

fn handle_transcription_settings_input(app: &mut App, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.transcription.settings_open = false;
        }
        KeyCode::Up => {
            app.transcription_settings_move_up();
        }
        KeyCode::Down => {
            app.transcription_settings_move_down();
        }
        KeyCode::Left => {
            app.transcription_settings_cycle_backward();
        }
        KeyCode::Right | KeyCode::Enter | KeyCode::Char(' ') => {
            app.transcription_settings_cycle_forward();
        }
        _ => {}
    }
}

async fn handle_settings_input(app: &mut App, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.toggle_settings();
        }
        KeyCode::Up => {
            app.settings_move_up();
        }
        KeyCode::Down => {
            app.settings_move_down();
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            app.settings_select();
        }
        KeyCode::Left => {
            if app.settings_selection == 1 {
                app.cycle_translation_service_backward();
            }
        }
        KeyCode::Right => {
            if app.settings_selection == 1 {
                app.cycle_translation_service_forward();
            }
        }
        _ => {}
    }
}

async fn handle_language_selector_input(app: &mut App, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.toggle_language_selector();
        }
        KeyCode::Up => {
            app.language_selector_move_up();
        }
        KeyCode::Down => {
            app.language_selector_move_down();
        }
        KeyCode::Enter => {
            app.language_selector_select();
        }
        KeyCode::Char(c) => {
            app.language_selector_search_add_char(c);
        }
        KeyCode::Backspace => {
            app.language_selector_search_backspace();
        }
        _ => {}
    }
}

async fn handle_clipboard_paste(app: &mut App) -> anyhow::Result<()> {
    use arboard::Clipboard;

    let mut clipboard = Clipboard::new()?;

    // Try to get image from clipboard first
    match clipboard.get_image() {
        Ok(image) => {
            // Raw image in clipboard - process it
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
            // Try external tools as fallback on Linux
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
            
            // No raw image, try text
            match clipboard.get_text() {
                Ok(text) => {
                    let trimmed = text.trim();

                    // Check if text contains image references
                    if let Some(image_source) = extract_image_source(trimmed) {
                        // It's an image reference - download/process it
                        let ocr_result =
                            process_image_from_source(image_source).await?;
                        app.input_mode = InputMode::Text;
                        app.set_input(ocr_result.text);
                        app.last_input_time = Some(std::time::Instant::now());
                    } else {
                        // It's plain text - use as-is
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
    
    // Try wl-paste first (Wayland)
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
    
    // Try xclip (X11) - target image/png
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
    
    // Try xclip with target image/jpeg
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
    
    // Try xclip with target image/bmp
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

/// Extract image source from various formats
fn extract_image_source(text: &str) -> Option<ImageSource> {
    let trimmed = text.trim();
    
    // Try HTML img tag: <img src="path/to/image.png">
    if let Some(src) = extract_html_img_src(trimmed) {
        return Some(ImageSource::PathOrUrl(src));
    }

    // Try Markdown image: ![alt](path/to/image.png)
    if let Some(src) = extract_markdown_image(trimmed) {
        return Some(ImageSource::PathOrUrl(src));
    }

    // Try file:// URLs first (copied files from file managers)
    if trimmed.starts_with("file://") && is_image_path(trimmed) {
        return Some(ImageSource::Path(trimmed.to_string()));
    }

    // Try URL ending with image extension
    if is_image_url(trimmed) {
        return Some(ImageSource::Url(trimmed.to_string()));
    }

    // Try local path that looks like an image
    if is_image_path(trimmed) {
        return Some(ImageSource::Path(trimmed.to_string()));
    }

    None
}

#[derive(Debug)]
enum ImageSource {
    Path(String),       // Local file path
    Url(String),        // HTTP(S) URL
    PathOrUrl(String),  // Could be either
}

fn extract_html_img_src(text: &str) -> Option<String> {
    // Handle simple img tag: <img src="path">
    // Also handle <img src="path" alt="...">
    let text_lower = text.to_lowercase();
    if let Some(img_idx) = text_lower.find("<img") {
        let after_img = &text[img_idx..];
        if let Some(src_start) = after_img.to_lowercase().find("src=\"") {
            let after_src = &after_img[src_start + 5..];
            if let Some(src_end) = after_src.find('"') {
                return Some(after_src[..src_end].to_string());
            }
        }
        if let Some(src_start) = after_img.to_lowercase().find("src='") {
            let after_src = &after_img[src_start + 5..];
            if let Some(src_end) = after_src.find('\'') {
                return Some(after_src[..src_end].to_string());
            }
        }
    }
    None
}

fn extract_markdown_image(text: &str) -> Option<String> {
    // Handle ![alt text](image.png)
    if text.starts_with("![") {
        if let Some(start) = text.find("](") {
            let after_paren = &text[start + 2..];
            if let Some(end) = after_paren.find(')') {
                return Some(after_paren[..end].to_string());
            }
        }
    }
    None
}

fn is_image_url(text: &str) -> bool {
    (text.starts_with("http://") 
        || text.starts_with("https://")
        || text.starts_with("file://"))
        && text.chars().any(|c| c == '.')
        && text.split('.').last().map_or(false, |ext| {
            matches!(
                ext.to_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tiff"
            )
        })
}

fn is_image_path(text: &str) -> bool {
    let trimmed = text.trim();
    
    // Must contain path separators or dots (for extensions)
    if !trimmed.chars().any(|c| c == '/' || c == '\\' || c == '.') {
        return false;
    }
    
    // Check for file:// prefix
    if trimmed.starts_with("file://") {
        return true;
    }
    
    // Check if it ends with a known image extension
    let has_image_ext = trimmed.split('.').last().map_or(false, |ext| {
        matches!(
            ext.to_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tiff"
        )
    });
    
    has_image_ext
}

async fn process_image_from_source(source: ImageSource) -> anyhow::Result<ocr::OcrResult> {
    match source {
        ImageSource::Path(path) | ImageSource::PathOrUrl(path) => {
            // Handle file:// URLs by stripping the prefix
            let clean_path = if path.starts_with("file://") {
                path.strip_prefix("file://").unwrap_or(&path).to_string()
            } else {
                path
            };
            
            // Try as local path first
            if std::path::Path::new(&clean_path).exists() {
                ocr::recognize_image_from_path(&clean_path).await
            } else if clean_path.starts_with("http://") || clean_path.starts_with("https://") {
                // It's a URL
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

async fn process_transcription_text(
    app: &mut App,
    text: &str,
    target_language: crate::language::Language,
    service: crate::config::TranslationService,
    local_url: &str,
    ltengine: &mut ltengine::LTEngine,
) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }

    // Detect if text is Chinese
    let has_chinese = text.chars().any(|c| {
        ('\u{4e00}'..='\u{9fff}').contains(&c)
            || ('\u{3400}'..='\u{4dbf}').contains(&c)
            || ('\u{f900}'..='\u{faff}').contains(&c)
    });

    if has_chinese {
        // Chinese input: show pinyin, translate to target language
        let pinyin_lines = pinyin_conv::convert_to_pinyin_lines(text);
        app.transcription.pinyin_lines.clear();
        app.transcription.hanzi_lines.clear();
        for line in pinyin_lines {
            app.transcription.pinyin_lines.push(line.pinyin);
            app.transcription.hanzi_lines.push(line.hanzi);
        }

        let target_code = target_language.google_code();
        match translation::translate("zh-CN", target_code, text, service, local_url, ltengine).await {
            Ok(t) => app.transcription.translation = t,
            Err(_) => {}
        }
    } else {
        // Non-Chinese input: translate to Chinese, show pinyin of Chinese
        let target_code = target_language.google_code();
        match translation::translate(target_code, "zh-CN", text, service, local_url, ltengine).await {
            Ok(chinese_text) => {
                let pinyin_lines = pinyin_conv::convert_to_pinyin_lines(&chinese_text);
                app.transcription.pinyin_lines.clear();
                app.transcription.hanzi_lines.clear();
                for line in pinyin_lines {
                    app.transcription.pinyin_lines.push(line.pinyin);
                    app.transcription.hanzi_lines.push(line.hanzi);
                }
                app.transcription.translation = chinese_text;
            }
            Err(_) => {}
        }
    }
}

async fn process_text_input(app: &mut App, ltengine: &mut ltengine::LTEngine) -> anyhow::Result<()> {
    let text = app.input.trim().to_string();
    if text.is_empty() {
        return Ok(());
    }

    app.detect_input_language();

    if app.is_input_chinese() {
        // Input is Chinese - translate to target language (default: English)
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
        // Input is NOT Chinese - translate to Chinese and show pinyin
        let target_lang_code = app.target_language.google_code();

        // First, translate from the detected language to Chinese
        match translation::translate(target_lang_code, "zh-CN", &text, app.translation_service, &app.local_translate_url, ltengine).await {
            Ok(chinese_text) => {
                // Show the Chinese text with pinyin
                let pinyin_lines = pinyin_conv::convert_to_pinyin_lines(&chinese_text);
                update_pinyin_display(app, pinyin_lines);

                // Show the Chinese translation
                app.translation = chinese_text;
            }
            Err(e) => {
                app.error_message = Some(format!("Translation failed: {}", e));
                // Show original text without pinyin since it's not Chinese
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
        // Empty input - try to get image from clipboard
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
                // Try to get text and check if it's an image reference
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
        // URL in input field
        let client = reqwest::Client::new();
        let response = client.get(input).send().await?;
        let bytes = response.bytes().await?;
        ocr::recognize_image(&bytes).await?
    } else if std::path::Path::new(input).exists() {
        // Local file path in input field
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

/// Redirect stdout and stderr to /dev/null to suppress ALSA/JACK/PipeWire/candle
/// library noise. The caller must `libc::dup(1)` *before* calling this so the
/// terminal backend can still write to the original tty fd.
fn suppress_output() {
    use std::fs::OpenOptions;
    use std::os::unix::io::AsRawFd;

    if let Ok(dev_null) = OpenOptions::new().write(true).open("/dev/null") {
        unsafe {
            libc::dup2(dev_null.as_raw_fd(), 1); // stdout
            libc::dup2(dev_null.as_raw_fd(), 2); // stderr
        }
    }
}
