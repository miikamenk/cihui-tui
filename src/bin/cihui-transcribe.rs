use std::io;

use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;

use cihui_tui::app::App;
use cihui_tui::config::TranslationService;
use cihui_tui::ltengine;
use cihui_tui::pinyin_conv;
use cihui_tui::single_instance;
use cihui_tui::transcription::{self, TranscriptionEvent};
use cihui_tui::translation;
use cihui_tui::ui::draw_ui;

#[derive(Parser, Debug)]
#[command(name = "cihui-transcribe")]
#[command(about = "Chinese vocabulary learning tool with transcription")]
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

    let tty_fd = unsafe { libc::dup(1) };
    suppress_output();

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

    let mut ltengine = ltengine::LTEngine::new(
        5050,
        app.ltengine_model.clone(),
        app.ltengine_path.clone(),
    );

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
            _ = tokio::time::sleep(timeout) => {}

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
                    if app.transcription.device_selector_open {
                        handle_device_selector_input(app, key);
                        continue;
                    }
                    if app.transcription.settings_open {
                        handle_transcription_settings_input(app, key);
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
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = std::time::Instant::now();
            ltengine.check_idle().await;
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

async fn process_transcription_text(
    app: &mut App,
    text: &str,
    target_language: cihui_tui::language::Language,
    service: TranslationService,
    local_url: &str,
    ltengine: &mut ltengine::LTEngine,
) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }

    let has_chinese = text.chars().any(|c| {
        ('\u{4e00}'..='\u{9fff}').contains(&c)
            || ('\u{3400}'..='\u{4dbf}').contains(&c)
            || ('\u{f900}'..='\u{faff}').contains(&c)
    });

    if has_chinese {
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
