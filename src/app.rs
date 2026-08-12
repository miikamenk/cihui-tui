use crate::config::{Config, TranslationService};
use crate::language::Language;
#[cfg(feature = "transcription")]
use crate::transcription::{AudioDevice, TranscriptionLanguage, WhisperModelSize};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AppMode {
    /// Text input mode: type or paste text, get pinyin and a translation.
    /// Always available; the `ocr` feature additionally allows image input.
    Normal,
    #[cfg(feature = "transcription")]
    Transcription,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum InputMode {
    Text,
    ImagePath,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum UiLanguage {
    English,
    Chinese,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum InputLanguage {
    English,
    Chinese,
    Other,
}

#[cfg(feature = "transcription")]
#[derive(Debug)]
pub struct TranscriptionState {
    pub is_recording: bool,
    pub language: TranscriptionLanguage,
    pub transcript: String,
    pub pinyin_lines: Vec<String>,
    pub hanzi_lines: Vec<String>,
    pub translation: String,
    pub vad_active: bool,
    pub selected_device: Option<String>,
    pub available_devices: Vec<AudioDevice>,
    pub device_selector_open: bool,
    pub device_selector_scroll: usize,
    pub settings_open: bool,
    pub settings_selection: usize,
    pub status: String,
    pub model_size: WhisperModelSize,
    pub model_ready: bool,
    pub model_loading: bool,
    pub model_progress: f32,
    pub transcript_scroll: u16,
}

#[cfg(feature = "transcription")]
impl TranscriptionState {
    pub fn new(config: &Config) -> Self {
        Self {
            is_recording: false,
            language: config.transcription_language,
            transcript: String::new(),
            pinyin_lines: Vec::new(),
            hanzi_lines: Vec::new(),
            translation: String::new(),
            vad_active: false,
            selected_device: config.transcription_device.clone(),
            available_devices: Vec::new(),
            device_selector_open: false,
            device_selector_scroll: 0,
            settings_open: false,
            settings_selection: 0,
            status: "Press Space to start recording".into(),
            model_size: config.whisper_model,
            model_ready: false,
            model_loading: false,
            model_progress: 0.0,
            transcript_scroll: 0,
        }
    }
}

#[derive(Debug)]
pub struct App {
    pub mode: AppMode,
    pub input: String,
    pub pinyin_lines: Vec<String>,
    pub hanzi_lines: Vec<String>,
    pub translation: String,
    pub input_mode: InputMode,
    pub ui_language: UiLanguage,
    pub input_language: InputLanguage,
    pub target_language: Language,
    pub translation_service: TranslationService,
    pub local_translate_url: String,
    pub ltengine_model: String,
    pub ltengine_path: String,
    pub cursor_position: usize, // Character index (not byte index)
    pub error_message: Option<String>,
    pub processing: bool,
    pub settings_open: bool,
    pub settings_selection: usize, // 0 = UI Language, 1 = other settings
    pub language_selector_open: bool,
    pub language_selector_search: String,
    pub language_selector_scroll: usize,
    pub filtered_languages: Vec<Language>,
    pub last_input_time: Option<std::time::Instant>, // Track when user last typed
    pub pinyin_scroll: u16,                          // Scroll offset for pinyin display
    pub translation_scroll: u16,                     // Scroll offset for translation display
    pub select_all: bool, // Whether all text is selected (next char replaces)
    #[cfg(feature = "transcription")]
    pub transcription: TranscriptionState,
    /// Where settings are persisted. `None` means the default user config
    /// location; tests point this at a temporary directory.
    pub config_path: Option<PathBuf>,
}

impl App {
    /// Build an app from the user's saved configuration.
    pub fn new() -> Self {
        Self::with_config(Config::load().unwrap_or_default())
    }

    /// Build an app from an explicit configuration, without touching the
    /// filesystem. Settings changes are persisted to the default user config
    /// location unless `config_path` is set afterwards.
    pub fn with_config(config: Config) -> Self {
        Self {
            // A transcription-only build has no text-input entry point, so it
            // starts (and stays) in transcription mode.
            #[cfg(all(not(feature = "ocr"), feature = "transcription"))]
            mode: AppMode::Transcription,
            #[cfg(not(all(not(feature = "ocr"), feature = "transcription")))]
            mode: AppMode::Normal,
            input: String::new(),
            pinyin_lines: Vec::new(),
            hanzi_lines: Vec::new(),
            translation: String::new(),
            input_mode: InputMode::Text,
            ui_language: UiLanguage::English,
            input_language: InputLanguage::Chinese,
            target_language: config.target_language,
            translation_service: config.translation_service,
            local_translate_url: config.local_translate_url.clone(),
            ltengine_model: config.ltengine_model.clone(),
            ltengine_path: config.ltengine_path.clone(),
            cursor_position: 0,
            error_message: None,
            processing: false,
            settings_open: false,
            settings_selection: 0,
            language_selector_open: false,
            language_selector_search: String::new(),
            language_selector_scroll: 0,
            filtered_languages: Language::all_for_picker(),
            last_input_time: None,
            pinyin_scroll: 0,
            translation_scroll: 0,
            select_all: false,
            #[cfg(feature = "transcription")]
            transcription: TranscriptionState::new(&config),
            config_path: None,
        }
    }

    /// Persist settings to `path` instead of the default user config location.
    pub fn with_config_path(mut self, path: PathBuf) -> Self {
        self.config_path = Some(path);
        self
    }

    pub fn get_title(&self) -> &'static str {
        match self.ui_language {
            UiLanguage::English => "Cihui - Chinese Vocabulary Tool",
            UiLanguage::Chinese => "词汇 - 中文词汇工具",
        }
    }

    pub fn get_input_label(&self) -> &'static str {
        match self.input_mode {
            InputMode::Text => match self.ui_language {
                UiLanguage::English => "Input",
                UiLanguage::Chinese => "输入",
            },
            InputMode::ImagePath => match self.ui_language {
                UiLanguage::English => "Image path or Ctrl+V for clipboard",
                UiLanguage::Chinese => "图片路径或 Ctrl+V 粘贴剪贴板",
            },
        }
    }

    pub fn get_pinyin_label(&self) -> &'static str {
        match self.ui_language {
            UiLanguage::English => "Pinyin",
            UiLanguage::Chinese => "拼音",
        }
    }

    pub fn get_translation_label(&self) -> &'static str {
        match self.ui_language {
            UiLanguage::English => "Translation",
            UiLanguage::Chinese => "翻译",
        }
    }

    pub fn get_help_text(&self) -> &'static str {
        match self.mode {
            #[cfg(feature = "transcription")]
            AppMode::Transcription => {
                // Only an ocr build can switch back to text mode.
                #[cfg(feature = "ocr")]
                {
                    match self.ui_language {
                        UiLanguage::English => {
                            "Space: Record | Ctrl+D: Device | Ctrl+S: Settings | Ctrl+X: Clear | Ctrl+T: Back | Esc: Quit"
                        }
                        UiLanguage::Chinese => {
                            "空格: 录音 | Ctrl+D: 设备 | Ctrl+S: 设置 | Ctrl+X: 清空 | Ctrl+T: 返回 | Esc: 退出"
                        }
                    }
                }
                #[cfg(not(feature = "ocr"))]
                {
                    match self.ui_language {
                        UiLanguage::English => {
                            "Space: Record | Ctrl+D: Device | Ctrl+S: Settings | Ctrl+X: Clear | Esc: Quit"
                        }
                        UiLanguage::Chinese => {
                            "空格: 录音 | Ctrl+D: 设备 | Ctrl+S: 设置 | Ctrl+X: 清空 | Esc: 退出"
                        }
                    }
                }
            }
            AppMode::Normal => {
                #[cfg(all(feature = "ocr", feature = "transcription"))]
                {
                    match self.ui_language {
                        UiLanguage::English => {
                            "Ctrl+T: Transcribe | Ctrl+L: Language | Ctrl+V: Paste | Ctrl+A: Select All | Ctrl+X: Clear | Ctrl+S: Settings | Esc: Quit"
                        }
                        UiLanguage::Chinese => "Ctrl+T: 转录 | Ctrl+L: 语言 | Ctrl+V: 粘贴 | Ctrl+A: 全选 | Ctrl+X: 清空 | Ctrl+S: 设置 | Esc: 退出",
                    }
                }
                #[cfg(all(feature = "ocr", not(feature = "transcription")))]
                {
                    match self.ui_language {
                        UiLanguage::English => {
                            "Ctrl+L: Language | Ctrl+V: Paste | Ctrl+A: Select All | Ctrl+X: Clear | Ctrl+S: Settings | Esc: Quit"
                        }
                        UiLanguage::Chinese => "Ctrl+L: 语言 | Ctrl+V: 粘贴 | Ctrl+A: 全选 | Ctrl+X: 清空 | Ctrl+S: 设置 | Esc: 退出",
                    }
                }
                // No clipboard-image input without the ocr feature.
                #[cfg(not(feature = "ocr"))]
                {
                    match self.ui_language {
                        UiLanguage::English => {
                            "Ctrl+L: Language | Ctrl+A: Select All | Ctrl+X: Clear | Ctrl+S: Settings | Esc: Quit"
                        }
                        UiLanguage::Chinese => {
                            "Ctrl+L: 语言 | Ctrl+A: 全选 | Ctrl+X: 清空 | Ctrl+S: 设置 | Esc: 退出"
                        }
                    }
                }
            }
        }
    }

    #[cfg(all(feature = "ocr", feature = "transcription"))]
    pub fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            AppMode::Normal => {
                self.transcription.available_devices = crate::transcription::list_input_devices();
                if self.transcription.selected_device.is_none() {
                    self.transcription.selected_device =
                        crate::transcription::default_device_name();
                }
                AppMode::Transcription
            }
            AppMode::Transcription => AppMode::Normal,
        };
    }

    #[cfg(all(not(feature = "ocr"), feature = "transcription"))]
    pub fn toggle_mode(&mut self) {
        // No-op: transcription-only build has no mode to toggle to
    }

    pub fn toggle_ui_language(&mut self) {
        self.ui_language = match self.ui_language {
            UiLanguage::English => UiLanguage::Chinese,
            UiLanguage::Chinese => UiLanguage::English,
        };
    }

    pub fn toggle_settings(&mut self) {
        self.settings_open = !self.settings_open;
        if self.settings_open {
            self.settings_selection = 0;
        }
    }

    pub fn toggle_language_selector(&mut self) {
        self.language_selector_open = !self.language_selector_open;
        if self.language_selector_open {
            self.language_selector_search.clear();
            self.language_selector_scroll = 0;
            self.filtered_languages = Language::all_for_picker();
        }
    }

    pub fn settings_move_up(&mut self) {
        if self.settings_selection > 0 {
            self.settings_selection -= 1;
        }
    }

    pub fn settings_move_down(&mut self) {
        if self.settings_selection < 1 {
            self.settings_selection += 1;
        }
    }

    pub fn settings_select(&mut self) {
        match self.settings_selection {
            0 => self.toggle_ui_language(),
            1 => self.cycle_translation_service_forward(),
            _ => {}
        }
    }

    pub fn cycle_translation_service_forward(&mut self) {
        self.translation_service = self.translation_service.next();
        self.save_config();
    }

    pub fn cycle_translation_service_backward(&mut self) {
        self.translation_service = self.translation_service.prev();
        self.save_config();
    }

    /// Snapshot the app's current settings as a `Config`.
    pub fn to_config(&self) -> Config {
        Config {
            target_language: self.target_language,
            translation_service: self.translation_service,
            local_translate_url: self.local_translate_url.clone(),
            ltengine_model: self.ltengine_model.clone(),
            ltengine_path: self.ltengine_path.clone(),
            #[cfg(feature = "transcription")]
            transcription_language: self.transcription.language,
            #[cfg(feature = "transcription")]
            transcription_device: self.transcription.selected_device.clone(),
            #[cfg(feature = "transcription")]
            whisper_model: self.transcription.model_size,
        }
    }

    fn save_config(&self) {
        let config = self.to_config();
        let _ = match &self.config_path {
            Some(path) => config.save_to(path),
            None => config.save(),
        };
    }

    #[cfg(feature = "transcription")]
    pub fn transcription_settings_move_up(&mut self) {
        if self.transcription.settings_selection > 0 {
            self.transcription.settings_selection -= 1;
        }
    }

    #[cfg(feature = "transcription")]
    pub fn transcription_settings_move_down(&mut self) {
        if self.transcription.settings_selection < 1 {
            self.transcription.settings_selection += 1;
        }
    }

    #[cfg(feature = "transcription")]
    pub fn transcription_settings_cycle_forward(&mut self) {
        match self.transcription.settings_selection {
            0 => {
                self.transcription.language = self.transcription.language.next();
                self.transcription.model_ready = false; // language baked into model
            }
            1 => {
                self.transcription.model_size = self.transcription.model_size.next();
                self.transcription.model_ready = false;
            }
            _ => {}
        }
        self.save_config();
    }

    #[cfg(feature = "transcription")]
    pub fn transcription_settings_cycle_backward(&mut self) {
        match self.transcription.settings_selection {
            0 => {
                self.transcription.language = self.transcription.language.prev();
                self.transcription.model_ready = false;
            }
            1 => {
                self.transcription.model_size = self.transcription.model_size.prev();
                self.transcription.model_ready = false;
            }
            _ => {}
        }
        self.save_config();
    }

    #[cfg(feature = "transcription")]
    pub fn toggle_transcription_device_selector(&mut self) {
        self.transcription.device_selector_open = !self.transcription.device_selector_open;
        if self.transcription.device_selector_open {
            self.transcription.device_selector_scroll = 0;
            self.transcription.available_devices = crate::transcription::list_input_devices();
        }
    }

    #[cfg(feature = "transcription")]
    pub fn transcription_device_select(&mut self) {
        if let Some(device) = self
            .transcription
            .available_devices
            .get(self.transcription.device_selector_scroll)
        {
            self.transcription.selected_device = Some(device.name.clone());
            self.save_config();
        }
        self.transcription.device_selector_open = false;
    }

    #[cfg(feature = "transcription")]
    pub fn toggle_transcription_settings(&mut self) {
        self.transcription.settings_open = !self.transcription.settings_open;
        if self.transcription.settings_open {
            self.transcription.settings_selection = 0;
        }
    }

    pub fn language_selector_move_up(&mut self) {
        if self.language_selector_scroll > 0 {
            self.language_selector_scroll -= 1;
        }
    }

    pub fn language_selector_move_down(&mut self) {
        if self.language_selector_scroll + 1 < self.filtered_languages.len() {
            self.language_selector_scroll += 1;
        }
    }

    pub fn language_selector_select(&mut self) {
        if let Some(&language) = self.filtered_languages.get(self.language_selector_scroll) {
            self.target_language = language;
            self.save_config();
        }
        self.toggle_language_selector();
    }

    pub fn language_selector_search_add_char(&mut self, c: char) {
        self.language_selector_search.push(c);
        self.filter_languages();
        self.language_selector_scroll = 0;
    }

    pub fn language_selector_search_backspace(&mut self) {
        self.language_selector_search.pop();
        self.filter_languages();
        self.language_selector_scroll = 0;
    }

    fn filter_languages(&mut self) {
        let search = self.language_selector_search.to_lowercase();
        self.filtered_languages = Language::all_for_picker()
            .into_iter()
            .filter(|lang| lang.name().to_lowercase().contains(&search))
            .collect();
    }

    pub fn language_selector_clear_search(&mut self) {
        self.language_selector_search.clear();
        self.filtered_languages = Language::all_for_picker();
        self.language_selector_scroll = 0;
    }

    pub fn toggle_input_mode(&mut self) {
        self.input_mode = match self.input_mode {
            InputMode::Text => InputMode::ImagePath,
            InputMode::ImagePath => InputMode::Text,
        };
        self.clear();
    }

    pub fn clear(&mut self) {
        self.input.clear();
        self.pinyin_lines.clear();
        self.hanzi_lines.clear();
        self.translation.clear();
        self.cursor_position = 0;
        self.error_message = None;
        self.last_input_time = None;
    }

    /// Detect the input language based on content
    pub fn detect_input_language(&mut self) {
        let text = self.input.trim();
        if text.is_empty() {
            return;
        }

        let has_chinese = text.chars().any(|c| {
            ('\u{4e00}'..='\u{9fff}').contains(&c)
                || ('\u{3400}'..='\u{4dbf}').contains(&c)
                || ('\u{f900}'..='\u{faff}').contains(&c)
        });

        let has_english = text.chars().any(|c| c.is_ascii_alphabetic());

        self.input_language = if has_chinese {
            InputLanguage::Chinese
        } else if has_english {
            InputLanguage::English
        } else {
            InputLanguage::Other
        };
    }

    pub fn is_input_chinese(&self) -> bool {
        matches!(self.input_language, InputLanguage::Chinese)
    }

    /// Get byte index from character index
    fn char_to_byte_index(&self, char_idx: usize) -> usize {
        self.input
            .chars()
            .take(char_idx)
            .map(|c| c.len_utf8())
            .sum()
    }

    /// Get character index from byte index
    #[allow(dead_code)]
    fn byte_to_char_index(&self, byte_idx: usize) -> usize {
        let mut current_byte = 0;
        let mut char_idx = 0;
        for c in self.input.chars() {
            if current_byte >= byte_idx {
                break;
            }
            current_byte += c.len_utf8();
            char_idx += 1;
        }
        char_idx
    }

    pub fn insert_char(&mut self, c: char) {
        if self.select_all {
            self.input.clear();
            self.cursor_position = 0;
            self.select_all = false;
        }
        let byte_idx = self.char_to_byte_index(self.cursor_position);
        if byte_idx <= self.input.len() {
            self.input.insert(byte_idx, c);
            self.cursor_position += 1;
            self.last_input_time = Some(std::time::Instant::now());
        }
    }

    #[allow(dead_code)]
    pub fn insert_text(&mut self, text: &str) {
        let byte_idx = self.char_to_byte_index(self.cursor_position);
        if byte_idx <= self.input.len() {
            self.input.insert_str(byte_idx, text);
            self.cursor_position += text.chars().count();
        }
    }

    pub fn delete_char(&mut self) {
        if self.cursor_position > 0 {
            let current_byte = self.char_to_byte_index(self.cursor_position);
            let prev_byte = self.char_to_byte_index(self.cursor_position - 1);
            self.input.drain(prev_byte..current_byte);
            self.cursor_position -= 1;
            self.last_input_time = Some(std::time::Instant::now());
        }
    }

    pub fn backspace(&mut self) {
        if self.select_all {
            self.select_all = false;
            self.clear();
            self.last_input_time = Some(std::time::Instant::now());
            return;
        }
        self.delete_char();
    }

    pub fn move_cursor_left(&mut self) {
        self.select_all = false;
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    pub fn move_cursor_right(&mut self) {
        self.select_all = false;
        let char_count = self.input.chars().count();
        if self.cursor_position < char_count {
            self.cursor_position += 1;
        }
    }

    pub fn move_cursor_up(&mut self) {
        self.select_all = false;
        let chars: Vec<char> = self.input.chars().collect();
        // Find start and column of current line
        let mut line_start = 0;
        let mut prev_line_start = None;
        let mut pos = 0;
        while pos < self.cursor_position {
            if chars[pos] == '\n' {
                prev_line_start = Some(line_start);
                line_start = pos + 1;
            }
            pos += 1;
        }
        let col = self.cursor_position - line_start;
        if let Some(prev_start) = prev_line_start {
            let prev_line_len = line_start - 1 - prev_start; // exclude the \n
            self.cursor_position = prev_start + col.min(prev_line_len);
        }
    }

    pub fn move_cursor_down(&mut self) {
        self.select_all = false;
        let chars: Vec<char> = self.input.chars().collect();
        // Find start of current line and column
        let mut line_start = 0;
        for i in 0..self.cursor_position {
            if chars[i] == '\n' {
                line_start = i + 1;
            }
        }
        let col = self.cursor_position - line_start;
        // Find next line
        let mut next_line_start = None;
        for i in self.cursor_position..chars.len() {
            if chars[i] == '\n' {
                next_line_start = Some(i + 1);
                break;
            }
        }
        if let Some(nls) = next_line_start {
            // Find length of next line
            let mut next_line_len = 0;
            for i in nls..chars.len() {
                if chars[i] == '\n' {
                    break;
                }
                next_line_len += 1;
            }
            self.cursor_position = nls + col.min(next_line_len);
        }
    }

    pub fn move_cursor_to_start(&mut self) {
        self.select_all = false;
        self.cursor_position = 0;
    }

    pub fn move_cursor_to_end(&mut self) {
        self.select_all = false;
        self.cursor_position = self.input.chars().count();
    }

    pub fn select_all(&mut self) {
        self.select_all = true;
        self.cursor_position = self.input.chars().count();
    }

    pub fn delete_word_backwards(&mut self) {
        if self.select_all {
            self.clear();
            return;
        }
        if self.cursor_position == 0 {
            return;
        }
        let chars: Vec<char> = self.input.chars().collect();
        let mut new_pos = self.cursor_position;
        // Skip whitespace
        while new_pos > 0 && chars[new_pos - 1].is_whitespace() {
            new_pos -= 1;
        }
        // Skip word characters
        while new_pos > 0 && !chars[new_pos - 1].is_whitespace() {
            new_pos -= 1;
        }
        let start_byte = self.char_to_byte_index(new_pos);
        let end_byte = self.char_to_byte_index(self.cursor_position);
        self.input.drain(start_byte..end_byte);
        self.cursor_position = new_pos;
        self.last_input_time = Some(std::time::Instant::now());
    }

    pub fn delete_to_start(&mut self) {
        if self.select_all {
            self.clear();
            return;
        }
        if self.cursor_position == 0 {
            return;
        }
        let byte_idx = self.char_to_byte_index(self.cursor_position);
        self.input.drain(..byte_idx);
        self.cursor_position = 0;
        self.last_input_time = Some(std::time::Instant::now());
    }

    pub fn set_input(&mut self, text: String) {
        self.select_all = false;
        self.input = text;
        self.cursor_position = self.input.chars().count();
        self.last_input_time = Some(std::time::Instant::now());
    }

    #[allow(dead_code)]
    pub fn append_input(&mut self, text: &str) {
        self.input.push_str(text);
        self.cursor_position = self.input.chars().count();
    }

    /// Get the byte index for the cursor position (for UI rendering)
    pub fn cursor_byte_index(&self) -> usize {
        self.char_to_byte_index(self.cursor_position)
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
