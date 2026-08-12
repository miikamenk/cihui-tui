use crate::config::TranslationService;
use crate::ltengine::LTEngine;
use serde::Deserialize;

#[derive(Deserialize)]
struct LibreTranslateResponse {
    #[serde(rename = "translatedText")]
    translated_text: String,
}

#[derive(Deserialize)]
struct MyMemoryResponse {
    #[serde(rename = "responseData")]
    response_data: ResponseData,
}

#[derive(Deserialize)]
struct ResponseData {
    #[serde(rename = "translatedText")]
    translated_text: String,
}

/// Google's undocumented single-translation endpoint.
pub const GOOGLE_ENDPOINT: &str = "https://translate.google.com/translate_a/single";

/// MyMemory's public translation endpoint.
pub const MYMEMORY_ENDPOINT: &str = "https://api.mymemory.translated.net/get";

/// Public LibreTranslate instances, tried in order.
pub const LIBRETRANSLATE_INSTANCES: [&str; 2] = [
    "https://libretranslate.de/translate",
    "https://translate.argosopentech.com/translate",
];

/// Extract the translation from Google's nested-array response.
///
/// The body is a JSON array whose first element is a list of chunks, each of
/// which has the translated text at index 0. Long input is split across
/// several chunks, so they have to be concatenated.
pub fn parse_google_response(body: &str) -> anyhow::Result<String> {
    let json: serde_json::Value = serde_json::from_str(body)?;

    let translated = json[0]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Unexpected response structure"))?
        .iter()
        .filter_map(|chunk| chunk[0].as_str())
        .collect::<String>();

    if translated.is_empty() {
        return Err(anyhow::anyhow!("Empty translation result"));
    }

    Ok(translated)
}

/// Extract the translation from MyMemory's response envelope.
pub fn parse_mymemory_response(body: &str) -> anyhow::Result<String> {
    serde_json::from_str::<MyMemoryResponse>(body)
        .map(|data| data.response_data.translated_text)
        .map_err(|e| anyhow::anyhow!("Failed to parse MyMemory response: {}", e))
}

/// Extract the translation from a LibreTranslate-shaped response.
///
/// LTEngine speaks the same protocol, so this covers both.
pub fn parse_libretranslate_response(body: &str) -> anyhow::Result<String> {
    serde_json::from_str::<LibreTranslateResponse>(body)
        .map(|data| data.translated_text)
        .map_err(|e| anyhow::anyhow!("Failed to parse LibreTranslate response: {}", e))
}

/// Translate text from source language to target language using the specified service
pub async fn translate(
    source: &str,
    target: &str,
    text: &str,
    service: TranslationService,
    local_url: &str,
    ltengine: &mut LTEngine,
) -> anyhow::Result<String> {
    if text.is_empty() {
        return Ok(String::new());
    }

    match service {
        TranslationService::Auto => translate_auto(source, target, text, local_url, ltengine).await,
        TranslationService::Google => translate_with_google(GOOGLE_ENDPOINT, text, source, target)
            .await
            .or_else(|_| Ok(format!("[Google Translate unavailable] {}", text))),
        TranslationService::MyMemory => {
            translate_with_mymemory(MYMEMORY_ENDPOINT, text, source, target)
                .await
                .or_else(|_| Ok(format!("[MyMemory unavailable] {}", text)))
        }
        TranslationService::LibreTranslate => {
            translate_with_libretranslate(&LIBRETRANSLATE_INSTANCES, text, source, target)
                .await
                .or_else(|_| Ok(format!("[LibreTranslate unavailable] {}", text)))
        }
        TranslationService::LTEngine => translate_with_ltengine(text, source, target, ltengine)
            .await
            .or_else(|e| Ok(format!("[LTEngine: {}] {}", e, text))),
    }
}

/// Auto mode: try all services in order, with LTEngine last
async fn translate_auto(
    source: &str,
    target: &str,
    text: &str,
    _local_url: &str,
    ltengine: &mut LTEngine,
) -> anyhow::Result<String> {
    if let Ok(translation) = translate_with_google(GOOGLE_ENDPOINT, text, source, target).await {
        return Ok(translation);
    }
    if let Ok(translation) = translate_with_mymemory(MYMEMORY_ENDPOINT, text, source, target).await
    {
        return Ok(translation);
    }
    if let Ok(translation) =
        translate_with_libretranslate(&LIBRETRANSLATE_INSTANCES, text, source, target).await
    {
        return Ok(translation);
    }
    if let Ok(translation) = translate_with_ltengine(text, source, target, ltengine).await {
        return Ok(translation);
    }

    Ok(format!(
        "[Translation unavailable - {} to {}] {}",
        source, target, text
    ))
}

/// Legacy function for Chinese to English translation
#[allow(dead_code)]
pub async fn translate_chinese_to_english(
    text: &str,
    ltengine: &mut LTEngine,
) -> anyhow::Result<String> {
    translate(
        "zh-CN",
        "en",
        text,
        TranslationService::Auto,
        "http://localhost:5050",
        ltengine,
    )
    .await
}

/// Legacy function for English to Chinese translation
#[allow(dead_code)]
pub async fn translate_english_to_chinese(
    text: &str,
    ltengine: &mut LTEngine,
) -> anyhow::Result<String> {
    translate(
        "en",
        "zh-CN",
        text,
        TranslationService::Auto,
        "http://localhost:5050",
        ltengine,
    )
    .await
}

/// Translate through Google's endpoint.
///
/// `endpoint` is a parameter rather than a constant so tests can point it at a
/// local server; production callers pass [`GOOGLE_ENDPOINT`].
pub async fn translate_with_google(
    endpoint: &str,
    text: &str,
    source: &str,
    target: &str,
) -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/120.0.0.0 Safari/537.36",
        )
        .build()?;

    let url = format!(
        "{}?client=gtx&sl={}&tl={}&dt=t&q={}",
        endpoint,
        source,
        target,
        urlencoding::encode(text)
    );

    let response = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Google Translate returned error: {}",
            response.status()
        ));
    }

    let body = response.text().await?;
    parse_google_response(&body).inspect_err(|_| {
        eprintln!("Google Translate response: {}", body);
    })
}

/// Translate through MyMemory's endpoint.
pub async fn translate_with_mymemory(
    endpoint: &str,
    text: &str,
    source: &str,
    target: &str,
) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}?q={}&langpair={}|{}",
        endpoint,
        urlencoding::encode(text),
        source,
        target
    );

    let response = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "MyMemory API returned error: {}",
            response.status()
        ));
    }

    let body = response.text().await?;
    parse_mymemory_response(&body).inspect_err(|_| {
        eprintln!("MyMemory response: {}", body);
    })
}

/// Translate through the first LibreTranslate instance that answers.
pub async fn translate_with_libretranslate(
    instances: &[&str],
    text: &str,
    source: &str,
    target: &str,
) -> anyhow::Result<String> {
    let client = reqwest::Client::new();

    for url in instances {
        let request = serde_json::json!({
            "q": text,
            "source": source,
            "target": target,
            "format": "text"
        });

        let response = client
            .post(*url)
            .header("Content-Type", "application/json")
            .json(&request)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await;

        if let Ok(response) = response {
            if response.status().is_success() {
                let body = response.text().await?;
                match parse_libretranslate_response(&body) {
                    Ok(translated) => return Ok(translated),
                    Err(e) => {
                        eprintln!("LibreTranslate response: {}", body);
                        eprintln!("Parse error: {}", e);
                    }
                }
            }
        }
    }

    Err(anyhow::anyhow!("All LibreTranslate servers failed"))
}

/// Map Google Translate language codes to LTEngine-compatible codes.
pub fn to_ltengine_code(code: &str) -> &str {
    match code {
        "zh-CN" | "zh" => "zh-Hans",
        "zh-TW" => "zh-Hant",
        "no" => "nb",
        "fil" => "tl",
        other => other,
    }
}

/// Translate using a local LTEngine instance, starting it if needed.
async fn translate_with_ltengine(
    text: &str,
    source: &str,
    target: &str,
    ltengine: &mut LTEngine,
) -> anyhow::Result<String> {
    ltengine.ensure_running().await?;

    let source = to_ltengine_code(source);
    let target = to_ltengine_code(target);

    let client = reqwest::Client::new();
    let url = format!("{}/translate", ltengine.base_url());

    let request = serde_json::json!({
        "q": text,
        "source": source,
        "target": target,
        "format": "text"
    });

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "LTEngine returned error: {}",
            response.status()
        ));
    }

    ltengine.touch();

    let body = response.text().await?;
    parse_libretranslate_response(&body).inspect_err(|_| {
        eprintln!("LTEngine response: {}", body);
    })
}

// Alternative: Simple word-by-word dictionary for common words
#[allow(dead_code)]
pub fn simple_fallback_translation(text: &str) -> String {
    format!("[Chinese text: {}]", text)
}
