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
        TranslationService::Google => {
            translate_with_google(text, source, target)
                .await
                .or_else(|_| Ok(format!("[Google Translate unavailable] {}", text)))
        }
        TranslationService::MyMemory => {
            translate_with_mymemory(text, source, target)
                .await
                .or_else(|_| Ok(format!("[MyMemory unavailable] {}", text)))
        }
        TranslationService::LibreTranslate => {
            translate_with_libretranslate(text, source, target)
                .await
                .or_else(|_| Ok(format!("[LibreTranslate unavailable] {}", text)))
        }
        TranslationService::LTEngine => {
            translate_with_ltengine(text, source, target, ltengine)
                .await
                .or_else(|e| Ok(format!("[LTEngine: {}] {}", e, text)))
        }
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
    if let Ok(translation) = translate_with_google(text, source, target).await {
        return Ok(translation);
    }
    if let Ok(translation) = translate_with_mymemory(text, source, target).await {
        return Ok(translation);
    }
    if let Ok(translation) = translate_with_libretranslate(text, source, target).await {
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
pub async fn translate_chinese_to_english(text: &str, ltengine: &mut LTEngine) -> anyhow::Result<String> {
    translate("zh-CN", "en", text, TranslationService::Auto, "http://localhost:5050", ltengine).await
}

/// Legacy function for English to Chinese translation
#[allow(dead_code)]
pub async fn translate_english_to_chinese(text: &str, ltengine: &mut LTEngine) -> anyhow::Result<String> {
    translate("en", "zh-CN", text, TranslationService::Auto, "http://localhost:5050", ltengine).await
}

async fn translate_with_google(text: &str, source: &str, target: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/120.0.0.0 Safari/537.36",
        )
        .build()?;

    let url = format!(
        "https://translate.google.com/translate_a/single?client=gtx&sl={}&tl={}&dt=t&q={}",
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
    let json: serde_json::Value = serde_json::from_str(&body)?;

    let translated = json[0]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Unexpected response structure"))?
        .iter()
        .filter_map(|chunk| chunk[0].as_str())
        .collect::<String>();

    if translated.is_empty() {
        eprintln!("Google Translate response: {}", body);
        return Err(anyhow::anyhow!("Empty translation result"));
    }

    Ok(translated)
}

async fn translate_with_mymemory(text: &str, source: &str, target: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let url = format!(
        "https://api.mymemory.translated.net/get?q={}&langpair={}|{}",
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

    // Try to parse JSON response
    match serde_json::from_str::<MyMemoryResponse>(&body) {
        Ok(data) => Ok(data.response_data.translated_text),
        Err(e) => {
            // Log the actual response for debugging
            eprintln!("MyMemory response: {}", body);
            Err(anyhow::anyhow!("Failed to parse MyMemory response: {}", e))
        }
    }
}

async fn translate_with_libretranslate(
    text: &str,
    source: &str,
    target: &str,
) -> anyhow::Result<String> {
    let client = reqwest::Client::new();

    // Try multiple LibreTranslate instances
    let urls = [
        "https://libretranslate.de/translate",
        "https://translate.argosopentech.com/translate",
    ];

    for url in &urls {
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
                match serde_json::from_str::<LibreTranslateResponse>(&body) {
                    Ok(data) => return Ok(data.translated_text),
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
fn to_ltengine_code(code: &str) -> &str {
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
    match serde_json::from_str::<LibreTranslateResponse>(&body) {
        Ok(data) => Ok(data.translated_text),
        Err(e) => {
            eprintln!("LTEngine response: {}", body);
            Err(anyhow::anyhow!(
                "Failed to parse LTEngine response: {}",
                e
            ))
        }
    }
}

// Alternative: Simple word-by-word dictionary for common words
#[allow(dead_code)]
pub fn simple_fallback_translation(text: &str) -> String {
    format!("[Chinese text: {}]", text)
}
