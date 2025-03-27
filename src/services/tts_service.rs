// src/services/tts_service.rs
use reqwest::Client;
use serde::Serialize;

#[derive(Serialize)]
struct TtsRequest {
    model: String,
    input: String,
    voice: String,
}

pub async fn call_openai_tts(
    api_key: &str,
    input_text: &str,
    voice: &str,
) -> Result<Vec<u8>, String> {
    let client = Client::new();

    let body = TtsRequest {
        model: "tts-1".to_string(),
        input: input_text.to_string(),
        voice: voice.to_string(),
    };

    let resp = client
        .post("https://api.openai.com/v1/audio/speech")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Request error: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("TTS request failed: {status} - {text}"));
    }

    resp.bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| format!("Unable to read TTS response bytes: {e}"))
}
