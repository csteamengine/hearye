use anyhow::{anyhow, Result};
use reqwest::multipart::{Form, Part};

const GROQ_URL: &str = "https://api.groq.com/openai/v1/audio/transcriptions";
const DEFAULT_MODEL: &str = "whisper-large-v3-turbo";

pub async fn transcribe(api_key: &str, model: Option<&str>, wav: Vec<u8>) -> Result<String> {
    if api_key.is_empty() {
        return Err(anyhow!("Groq API key not set"));
    }
    let model = model.unwrap_or(DEFAULT_MODEL).to_string();
    let part = Part::bytes(wav)
        .file_name("audio.wav")
        .mime_str("audio/wav")?;
    let form = Form::new()
        .part("file", part)
        .text("model", model)
        .text("response_format", "text")
        .text("temperature", "0");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let resp = client
        .post(GROQ_URL)
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Groq error {status}: {body}"));
    }
    Ok(resp.text().await?.trim().to_string())
}
