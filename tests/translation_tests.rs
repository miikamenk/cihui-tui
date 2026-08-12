//! Translation talks to four services with different response shapes and a
//! fallback chain between them. These tests are hermetic: the parsers run
//! against captured payloads, and the HTTP paths run against a local mock
//! server, so nothing here depends on a free API being up.

use cihui_tui::ltengine::LTEngine;
use cihui_tui::translation::{
    parse_google_response, parse_libretranslate_response, parse_mymemory_response,
    to_ltengine_code, translate_with_google, translate_with_libretranslate,
    translate_with_mymemory,
};
use pretty_assertions::assert_eq;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

// ---------------------------------------------------------------- parsers --

#[test]
fn google_response_yields_the_translation() {
    let translated = parse_google_response(&fixture("google_simple.json")).unwrap();

    assert_eq!(translated, "Hello world");
}

#[test]
fn google_chunks_are_concatenated() {
    // Long input comes back split across several chunks, and dropping any of
    // them would silently truncate the translation.
    let translated = parse_google_response(&fixture("google_multi_chunk.json")).unwrap();

    assert_eq!(
        translated, "Hello world. This is a second sentence.",
        "every chunk must appear, in order"
    );
}

#[test]
fn an_empty_google_result_is_an_error() {
    assert!(parse_google_response(&fixture("google_empty.json")).is_err());
}

#[test]
fn malformed_google_json_is_an_error() {
    assert!(parse_google_response("not json at all").is_err());
}

#[test]
fn an_unexpected_google_shape_is_an_error() {
    // An object rather than the expected array, e.g. an error envelope.
    assert!(parse_google_response(r#"{"error":"quota exceeded"}"#).is_err());
}

#[test]
fn mymemory_response_yields_the_translation() {
    let translated = parse_mymemory_response(&fixture("mymemory_simple.json")).unwrap();

    assert_eq!(translated, "Hello world");
}

#[test]
fn a_mymemory_response_without_the_envelope_is_an_error() {
    assert!(parse_mymemory_response(r#"{"translatedText":"Hello"}"#).is_err());
}

#[test]
fn libretranslate_response_yields_the_translation() {
    let translated = parse_libretranslate_response(&fixture("libretranslate_simple.json")).unwrap();

    assert_eq!(translated, "Hello world");
}

#[test]
fn a_libretranslate_error_body_is_an_error() {
    assert!(parse_libretranslate_response(r#"{"error":"unsupported"}"#).is_err());
}

// ------------------------------------------------------------ code mapping --

#[test]
fn chinese_codes_map_to_ltengine_script_codes() {
    assert_eq!(to_ltengine_code("zh-CN"), "zh-Hans");
    assert_eq!(to_ltengine_code("zh"), "zh-Hans");
    assert_eq!(to_ltengine_code("zh-TW"), "zh-Hant");
}

#[test]
fn a_few_other_codes_are_remapped() {
    assert_eq!(to_ltengine_code("no"), "nb");
    assert_eq!(to_ltengine_code("fil"), "tl");
}

#[test]
fn unknown_codes_pass_through_unchanged() {
    for code in ["en", "fi", "de", "ja", "pt-BR"] {
        assert_eq!(to_ltengine_code(code), code);
    }
}

// -------------------------------------------------------------- http paths --

#[tokio::test]
async fn google_translates_over_http() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/translate_a/single"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("google_simple.json")))
        .mount(&server)
        .await;

    let endpoint = format!("{}/translate_a/single", server.uri());
    let translated = translate_with_google(&endpoint, "你好世界", "zh-CN", "en")
        .await
        .unwrap();

    assert_eq!(translated, "Hello world");
}

#[tokio::test]
async fn a_google_server_error_is_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let endpoint = format!("{}/translate_a/single", server.uri());

    assert!(translate_with_google(&endpoint, "你好", "zh-CN", "en")
        .await
        .is_err());
}

#[tokio::test]
async fn a_google_rate_limit_is_an_error() {
    // Free endpoints answer 429 rather than failing to connect, and that has
    // to count as a failure so Auto mode moves on.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&server)
        .await;

    let endpoint = format!("{}/translate_a/single", server.uri());

    assert!(translate_with_google(&endpoint, "你好", "zh-CN", "en")
        .await
        .is_err());
}

#[tokio::test]
async fn mymemory_translates_over_http() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/get"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("mymemory_simple.json")))
        .mount(&server)
        .await;

    let endpoint = format!("{}/get", server.uri());
    let translated = translate_with_mymemory(&endpoint, "你好世界", "zh-CN", "en")
        .await
        .unwrap();

    assert_eq!(translated, "Hello world");
}

#[tokio::test]
async fn libretranslate_translates_over_http() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/translate"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(fixture("libretranslate_simple.json")),
        )
        .mount(&server)
        .await;

    let instance = format!("{}/translate", server.uri());
    let translated = translate_with_libretranslate(&[&instance], "你好世界", "zh-CN", "en")
        .await
        .unwrap();

    assert_eq!(translated, "Hello world");
}

#[tokio::test]
async fn libretranslate_falls_through_to_the_next_instance() {
    // The public instances go down regularly, which is why there is a list.
    let broken = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(502))
        .mount(&broken)
        .await;

    let working = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/translate"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(fixture("libretranslate_simple.json")),
        )
        .mount(&working)
        .await;

    let instances = [
        format!("{}/translate", broken.uri()),
        format!("{}/translate", working.uri()),
    ];
    let refs: Vec<&str> = instances.iter().map(String::as_str).collect();

    let translated = translate_with_libretranslate(&refs, "你好", "zh-CN", "en")
        .await
        .unwrap();

    assert_eq!(translated, "Hello world");
}

#[tokio::test]
async fn libretranslate_skips_an_instance_returning_junk() {
    // A 200 with an unparseable body must not end the search either.
    let junk = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>maintenance</html>"))
        .mount(&junk)
        .await;

    let working = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(fixture("libretranslate_simple.json")),
        )
        .mount(&working)
        .await;

    let instances = [
        format!("{}/translate", junk.uri()),
        format!("{}/translate", working.uri()),
    ];
    let refs: Vec<&str> = instances.iter().map(String::as_str).collect();

    assert_eq!(
        translate_with_libretranslate(&refs, "你好", "zh-CN", "en")
            .await
            .unwrap(),
        "Hello world"
    );
}

#[tokio::test]
async fn libretranslate_fails_when_every_instance_fails() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let instance = format!("{}/translate", server.uri());

    assert!(
        translate_with_libretranslate(&[&instance], "你好", "zh-CN", "en")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn an_empty_instance_list_fails_rather_than_hanging() {
    assert!(translate_with_libretranslate(&[], "你好", "zh-CN", "en")
        .await
        .is_err());
}

// ---------------------------------------------------------------- ltengine --

#[test]
fn ltengine_builds_its_url_from_the_port() {
    let engine = LTEngine::new(5050, "gemma3-4b".to_string(), "ltengine".to_string());

    assert_eq!(engine.base_url(), "http://localhost:5050");
}

#[test]
fn ltengine_honours_a_custom_port() {
    let engine = LTEngine::new(9999, "m".to_string(), "ltengine".to_string());

    assert_eq!(engine.base_url(), "http://localhost:9999");
}
