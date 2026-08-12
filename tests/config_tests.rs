//! Config is what survives between runs, so its failure modes are all
//! user-visible: a service that cannot be cycled to, an old config file that
//! stops loading, or a parse error that silently resets every setting.

mod common;

use cihui_tui::config::{Config, TranslationService};
use cihui_tui::language::Language;
use common::TestApp;
use pretty_assertions::assert_eq;

// ---------------------------------------------------------------- cycling --

#[test]
fn next_reaches_every_service_and_wraps() {
    let all = TranslationService::all();
    let mut service = all[0];
    let mut visited = vec![service];

    for _ in 1..all.len() {
        service = service.next();
        visited.push(service);
    }

    assert_eq!(
        visited,
        all.to_vec(),
        "cycling forward should visit every service in order"
    );
    assert_eq!(
        service.next(),
        all[0],
        "the last service should wrap around to the first"
    );
}

#[test]
fn prev_wraps_from_the_first_to_the_last() {
    let all = TranslationService::all();

    assert_eq!(all[0].prev(), all[all.len() - 1]);
}

#[test]
fn next_and_prev_are_inverses() {
    for service in TranslationService::all() {
        assert_eq!(
            service.next().prev(),
            *service,
            "next then prev for {service:?}"
        );
        assert_eq!(
            service.prev().next(),
            *service,
            "prev then next for {service:?}"
        );
    }
}

#[test]
fn every_service_has_a_distinct_name() {
    let mut names: Vec<_> = TranslationService::all().iter().map(|s| s.name()).collect();
    let count = names.len();
    names.sort_unstable();
    names.dedup();

    assert_eq!(names.len(), count, "service display names must be unique");
}

#[test]
fn default_service_is_auto() {
    assert_eq!(TranslationService::default(), TranslationService::Auto);
}

// ------------------------------------------------------------ persistence --

#[test]
fn missing_file_yields_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config::load_from(&dir.path().join("does-not-exist.json")).unwrap();

    assert_eq!(config.target_language, Language::English);
    assert_eq!(config.translation_service, TranslationService::Auto);
}

#[test]
fn save_then_load_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");

    let mut original = Config::default();
    original.target_language = Language::Finnish;
    original.translation_service = TranslationService::MyMemory;
    original.ltengine_model = "custom-model".to_string();
    original.save_to(&path).unwrap();

    let loaded = Config::load_from(&path).unwrap();

    assert_eq!(loaded.target_language, Language::Finnish);
    assert_eq!(loaded.translation_service, TranslationService::MyMemory);
    assert_eq!(loaded.ltengine_model, "custom-model");
}

#[test]
fn save_creates_missing_parent_directories() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("deeper").join("config.json");

    Config::default().save_to(&path).unwrap();

    assert!(path.exists(), "save_to should create the directory tree");
}

#[test]
fn malformed_json_is_an_error_rather_than_a_silent_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(&path, "{ this is not json").unwrap();

    assert!(
        Config::load_from(&path).is_err(),
        "a corrupt config should surface as an error"
    );
}

#[test]
fn optional_fields_fall_back_to_defaults() {
    // Only target_language is required, so a minimal file written by an older
    // version must still load.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(&path, r#"{"target_language":"German"}"#).unwrap();

    let config = Config::load_from(&path).unwrap();

    assert_eq!(config.target_language, Language::German);
    assert_eq!(config.translation_service, TranslationService::Auto);
    assert_eq!(config.local_translate_url, "http://localhost:5050");
    assert_eq!(config.ltengine_model, "gemma3-4b");
    assert_eq!(config.ltengine_path, "ltengine");
}

#[test]
fn target_language_is_required() {
    // Documents a sharp edge: target_language has no serde default, so a file
    // missing it fails to parse. App::new turns that error into
    // unwrap_or_default(), silently discarding every other saved setting.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(&path, r#"{"translation_service":"Google"}"#).unwrap();

    assert!(
        Config::load_from(&path).is_err(),
        "a config without target_language currently fails to parse"
    );
}

#[test]
fn the_old_local_model_service_name_still_loads() {
    // LTEngine used to be called LocalModel. Anyone who selected it before the
    // rename has that string on disk, and a serde alias keeps it working.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(
        &path,
        r#"{"target_language":"English","translation_service":"LocalModel"}"#,
    )
    .unwrap();

    let config = Config::load_from(&path).unwrap();

    assert_eq!(config.translation_service, TranslationService::LTEngine);
}

#[test]
fn unknown_service_names_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(
        &path,
        r#"{"target_language":"English","translation_service":"DeepL"}"#,
    )
    .unwrap();

    assert!(Config::load_from(&path).is_err());
}

// --------------------------------------------------------- app write-back --

#[test]
fn changing_a_setting_persists_it() {
    let mut test_app = TestApp::new();
    let path = test_app.config_path();

    test_app.cycle_translation_service_forward();
    let expected = test_app.translation_service;

    let reloaded = Config::load_from(&path).unwrap();
    assert_eq!(
        reloaded.translation_service, expected,
        "cycling the service should write it straight to disk"
    );
}

#[test]
fn selecting_a_language_persists_it() {
    let mut test_app = TestApp::new();
    let path = test_app.config_path();

    test_app.toggle_language_selector();
    test_app.language_selector_search_add_char('f');
    test_app.language_selector_search_add_char('i');
    test_app.language_selector_search_add_char('n');
    let selected = test_app.filtered_languages[test_app.language_selector_scroll];
    test_app.language_selector_select();

    let reloaded = Config::load_from(&path).unwrap();
    assert_eq!(reloaded.target_language, selected);
    assert_eq!(test_app.target_language, selected);
}

#[test]
fn app_settings_survive_a_save_and_reload() {
    let mut test_app = TestApp::new();
    let path = test_app.config_path();

    test_app.cycle_translation_service_forward();
    test_app.cycle_translation_service_forward();

    let reloaded = Config::load_from(&path).unwrap();
    let restored = cihui_tui::app::App::with_config(reloaded);

    assert_eq!(restored.translation_service, test_app.translation_service);
    assert_eq!(restored.target_language, test_app.target_language);
}
