//! Speech-to-text via OpenRouter. Expects JSON with base64 audio (not multipart like OpenAI).

use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Result};
use base64::Engine;

pub fn transcribe(
    wav: &Path,
    api_key: &str,
    model: &str,
    language: &str,
    endpoint: &str,
    timeout_secs: u64,
) -> Result<String> {
    let bytes = std::fs::read(wav)?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);

    let mut body = serde_json::json!({
        "model": model,
        "input_audio": { "data": b64, "format": "wav" },
    });
    if !language.is_empty() {
        body["language"] = serde_json::Value::String(language.to_string());
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()?;

    let resp = client
        .post(endpoint)
        .bearer_auth(api_key)
        .json(&body)
        .send()?;

    let status = resp.status();
    let text_body = resp.text()?;
    if !status.is_success() {
        return Err(anyhow!("STT HTTP {status}: {text_body}"));
    }

    let v: serde_json::Value = serde_json::from_str(&text_body)?;
    let text = v
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    Ok(text)
}
