//! Key handling for the modal popups.
//!
//! These are pure state transitions over [`App`]: they take a key event, apply
//! it, and return. Nothing here does I/O, so the popups can be driven from
//! tests without a terminal.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::App;

/// Handle a key while the settings popup is open.
pub fn handle_settings_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.toggle_settings(),
        KeyCode::Up => app.settings_move_up(),
        KeyCode::Down => app.settings_move_down(),
        KeyCode::Enter | KeyCode::Char(' ') => app.settings_select(),
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

/// Handle a key while the language selector is open.
pub fn handle_language_selector_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.toggle_language_selector(),
        KeyCode::Up => app.language_selector_move_up(),
        KeyCode::Down => app.language_selector_move_down(),
        KeyCode::Enter => app.language_selector_select(),
        KeyCode::Char(c) => app.language_selector_search_add_char(c),
        KeyCode::Backspace => app.language_selector_search_backspace(),
        _ => {}
    }
}

/// Handle a key while the audio device selector is open.
///
/// Returns whether the selected device changed, which tells the caller to
/// restart an in-progress recording.
#[cfg(feature = "transcription")]
pub fn handle_device_selector_input(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc => {
            app.transcription.device_selector_open = false;
            false
        }
        KeyCode::Up => {
            if app.transcription.device_selector_scroll > 0 {
                app.transcription.device_selector_scroll -= 1;
            }
            false
        }
        KeyCode::Down => {
            let max = app.transcription.available_devices.len().saturating_sub(1);
            if app.transcription.device_selector_scroll < max {
                app.transcription.device_selector_scroll += 1;
            }
            false
        }
        KeyCode::Enter => {
            let old_device = app.transcription.selected_device.clone();
            app.transcription_device_select();
            old_device != app.transcription.selected_device
        }
        _ => false,
    }
}

/// Handle a key while the transcription settings popup is open.
#[cfg(feature = "transcription")]
pub fn handle_transcription_settings_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.transcription.settings_open = false,
        KeyCode::Up => app.transcription_settings_move_up(),
        KeyCode::Down => app.transcription_settings_move_down(),
        KeyCode::Left => app.transcription_settings_cycle_backward(),
        KeyCode::Right | KeyCode::Enter | KeyCode::Char(' ') => {
            app.transcription_settings_cycle_forward()
        }
        _ => {}
    }
}
