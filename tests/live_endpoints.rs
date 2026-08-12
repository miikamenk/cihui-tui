//! Tests that call the real translation services.
//!
//! Every test here is `#[ignore]`d, so `cargo nextest run` never touches the
//! network and CI never fails because a free API rate-limited us. Run them
//! deliberately when you want to know whether the services still work:
//!
//! ```text
//! cargo nextest run --run-ignored all -E 'binary(live_endpoints)'
//! ```
//!
//! They assert only that a plausible translation came back, not what it says:
//! these are machine translations of free services and the wording changes.

use cihui_tui::translation::{
    translate_with_google, translate_with_libretranslate, translate_with_mymemory, GOOGLE_ENDPOINT,
    LIBRETRANSLATE_INSTANCES, MYMEMORY_ENDPOINT,
};

/// A translation that actually came from the service.
fn assert_is_a_translation(translated: &str, source_text: &str) {
    assert!(
        !translated.trim().is_empty(),
        "the service returned an empty translation"
    );
    assert!(
        !translated.contains("unavailable"),
        "got a placeholder rather than a translation: {translated:?}"
    );
    assert_ne!(
        translated, source_text,
        "the text came back untranslated, which usually means the language \
         pair was rejected"
    );
}

#[tokio::test]
#[ignore = "calls translate.google.com"]
async fn google_translates_chinese_to_english() {
    let translated = translate_with_google(GOOGLE_ENDPOINT, "你好世界", "zh-CN", "en")
        .await
        .expect("Google Translate should answer");

    assert_is_a_translation(&translated, "你好世界");
}

#[tokio::test]
#[ignore = "calls translate.google.com"]
async fn google_translates_english_to_chinese() {
    let translated = translate_with_google(GOOGLE_ENDPOINT, "hello world", "en", "zh-CN")
        .await
        .expect("Google Translate should answer");

    assert_is_a_translation(&translated, "hello world");
    assert!(
        translated
            .chars()
            .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
        "expected hanzi in the result, got {translated:?}"
    );
}

#[tokio::test]
#[ignore = "calls api.mymemory.translated.net"]
async fn mymemory_translates_chinese_to_english() {
    let translated = translate_with_mymemory(MYMEMORY_ENDPOINT, "你好", "zh-CN", "en")
        .await
        .expect("MyMemory should answer");

    assert_is_a_translation(&translated, "你好");
}

/// Known to fail as of 2026-08-12: both configured instances are gone.
///
/// `libretranslate.de` now redirects to `de.libretranslate.com`, which answers
/// `{"error": "...um einen API-Schlüssel zu erhalten"}` - it requires an API
/// key the app has no way to supply - and the redirect itself turns the POST
/// into a GET, so the app receives an HTML page and fails to parse it.
/// `translate.argosopentech.com` no longer resolves at all.
///
/// The failure is therefore about the services, not this crate: the
/// LibreTranslate step of the fallback chain is currently dead weight, and
/// Auto mode just spends two timeouts before moving on to LTEngine.
#[tokio::test]
#[ignore = "calls the public LibreTranslate instances, which are currently unreachable"]
async fn libretranslate_translates_chinese_to_english() {
    let translated = translate_with_libretranslate(&LIBRETRANSLATE_INSTANCES, "你好", "zh", "en")
        .await
        .expect("at least one LibreTranslate instance should answer");

    assert_is_a_translation(&translated, "你好");
}

#[tokio::test]
#[ignore = "calls translate.google.com"]
async fn google_handles_a_longer_passage() {
    // Long input comes back in several chunks, and the parser has to join
    // them. This is the case a single-chunk fixture cannot prove.
    let text = "我们今天去北京大学学习中文，然后一起去餐厅吃饭。\
                明天早上我要坐火车去上海，在那里我会见到我的朋友。";

    let translated = translate_with_google(GOOGLE_ENDPOINT, text, "zh-CN", "en")
        .await
        .expect("Google Translate should answer");

    assert_is_a_translation(&translated, text);
    assert!(
        translated.len() > 40,
        "a long passage should not come back as a fragment: {translated:?}"
    );
}
