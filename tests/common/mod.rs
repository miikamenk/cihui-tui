//! Shared fixtures for the integration tests.
//!
//! Not every test binary uses every helper here, and Rust warns per binary
//! about the ones it does not touch.
#![allow(dead_code)]

use cihui_tui::app::App;
use cihui_tui::config::Config;
use tempfile::TempDir;

/// An app wired to a throwaway config file.
///
/// The [`TempDir`] must stay alive for as long as the app is used: dropping it
/// deletes the directory, and any settings change would then fail to save.
pub struct TestApp {
    pub app: App,
    _dir: TempDir,
}

impl TestApp {
    /// Build an app with default settings that persists to a temporary
    /// directory instead of the real user config.
    pub fn new() -> Self {
        Self::with_config(Config::default())
    }

    /// Build an app from an explicit config, persisting to a temporary
    /// directory.
    pub fn with_config(config: Config) -> Self {
        let dir = tempfile::tempdir().expect("create temp dir");
        let app = App::with_config(config).with_config_path(dir.path().join("config.json"));
        Self { app, _dir: dir }
    }

    /// Path the app writes its settings to.
    pub fn config_path(&self) -> std::path::PathBuf {
        self.app
            .config_path
            .clone()
            .expect("test app always has a config path")
    }
}

impl Default for TestApp {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for TestApp {
    type Target = App;

    fn deref(&self) -> &App {
        &self.app
    }
}

impl std::ops::DerefMut for TestApp {
    fn deref_mut(&mut self) -> &mut App {
        &mut self.app
    }
}
