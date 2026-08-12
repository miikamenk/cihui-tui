//! `Language` is three hand-maintained parallel match tables over ~130
//! variants: `all()`, `name()` and `google_code()`. Nothing forces them to
//! agree, so adding a variant and forgetting one table is silent. These tests
//! check the tables against each other rather than spot-checking individual
//! languages.

use std::collections::HashMap;

use cihui_tui::language::Language;
use pretty_assertions::assert_eq;

#[test]
fn all_contains_no_duplicates() {
    let all = Language::all();
    let mut seen = Vec::new();
    let mut duplicates = Vec::new();

    for language in &all {
        if seen.contains(language) {
            duplicates.push(language.name());
        } else {
            seen.push(*language);
        }
    }

    assert!(
        duplicates.is_empty(),
        "Language::all() lists these more than once: {:?}",
        duplicates
    );
}

#[test]
fn every_language_has_a_name() {
    let unnamed: Vec<_> = Language::all()
        .into_iter()
        .filter(|l| l.name().trim().is_empty())
        .collect();

    assert!(
        unnamed.is_empty(),
        "these variants have an empty name(): {:?}",
        unnamed
    );
}

#[test]
fn every_language_has_a_google_code() {
    let uncoded: Vec<_> = Language::all()
        .into_iter()
        .filter(|l| l.google_code().trim().is_empty())
        .collect();

    assert!(
        uncoded.is_empty(),
        "these variants have an empty google_code(): {:?}",
        uncoded
    );
}

#[test]
fn names_are_unique() {
    let mut by_name: HashMap<&str, Vec<Language>> = HashMap::new();
    for language in Language::all() {
        by_name.entry(language.name()).or_default().push(language);
    }

    let collisions: Vec<_> = by_name.iter().filter(|(_, v)| v.len() > 1).collect();

    assert!(
        collisions.is_empty(),
        "several variants share a display name, so the picker cannot \
         distinguish them: {:?}",
        collisions
    );
}

#[test]
fn google_codes_are_unique() {
    let mut by_code: HashMap<&str, Vec<Language>> = HashMap::new();
    for language in Language::all() {
        by_code
            .entry(language.google_code())
            .or_default()
            .push(language);
    }

    let collisions: Vec<_> = by_code.iter().filter(|(_, v)| v.len() > 1).collect();

    assert!(
        collisions.is_empty(),
        "several variants map to the same translation API code, so one of \
         them cannot be requested: {:?}",
        collisions
    );
}

#[test]
fn exactly_two_languages_are_chinese() {
    let chinese: Vec<_> = Language::all()
        .into_iter()
        .filter(|l| l.is_chinese())
        .map(|l| l.name())
        .collect();

    assert_eq!(
        chinese.len(),
        2,
        "expected simplified and traditional Chinese, got {:?}",
        chinese
    );
}

#[test]
fn picker_excludes_chinese() {
    // Chinese is the source language, so offering it as a translation target
    // would ask the app to translate Chinese into Chinese.
    let picker = Language::all_for_picker();

    assert!(
        !picker.iter().any(|l| l.is_chinese()),
        "the target-language picker must not offer Chinese"
    );
    assert_eq!(
        picker.len(),
        Language::all().len() - 2,
        "the picker should drop exactly the two Chinese variants"
    );
}

#[test]
fn picker_preserves_order_of_all() {
    let picker = Language::all_for_picker();
    let expected: Vec<_> = Language::all()
        .into_iter()
        .filter(|l| !l.is_chinese())
        .collect();

    assert_eq!(picker, expected);
}

#[test]
fn default_is_english() {
    assert_eq!(Language::default(), Language::English);
    assert_eq!(Language::English.google_code(), "en");
}

#[test]
fn every_language_survives_a_serde_round_trip() {
    // Target language is persisted in config.json, so a variant that fails to
    // round-trip would break config loading for anyone who selected it.
    for language in Language::all() {
        let json = serde_json::to_string(&language)
            .unwrap_or_else(|e| panic!("failed to serialize {:?}: {}", language, e));
        let restored: Language = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("failed to deserialize {} for {:?}: {}", json, language, e));

        assert_eq!(restored, language, "round trip changed the value");
    }
}

#[test]
fn google_codes_look_like_language_tags() {
    // Codes go straight into a query string, so they must be well-formed
    // BCP 47 tags: a lowercase primary subtag, optionally followed by a
    // region subtag such as the "CN" in "zh-CN".
    for language in Language::all() {
        let code = language.google_code();

        assert!(
            !code.contains(char::is_whitespace),
            "{:?} has whitespace in its code {:?}",
            language,
            code
        );

        let mut subtags = code.split('-');

        let primary = subtags.next().unwrap_or_default();
        assert!(
            (2..=3).contains(&primary.len()) && primary.chars().all(|c| c.is_ascii_lowercase()),
            "{:?} has a malformed primary subtag in {:?}",
            language,
            code
        );

        for subtag in subtags {
            assert!(
                !subtag.is_empty() && subtag.chars().all(|c| c.is_ascii_alphanumeric()),
                "{:?} has a malformed subtag in {:?}",
                language,
                code
            );
        }
    }
}
