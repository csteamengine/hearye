use anyhow::{anyhow, Result};
use serde_json::json;

const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
const DEFAULT_MODEL: &str = "claude-haiku-4-5-20251001";
const SYSTEM_PROMPT: &str = "You clean up dictated speech. Fix punctuation, capitalization, and obvious filler words (um, uh, like). Preserve the speaker's wording and intent — do NOT rephrase, summarize, translate, or add content. Return only the cleaned text, nothing else.";

pub async fn cleanup(api_key: &str, model: Option<&str>, text: &str) -> Result<String> {
    if api_key.is_empty() {
        return Err(anyhow!("Anthropic API key not set"));
    }
    let model = model.unwrap_or(DEFAULT_MODEL);
    let body = json!({
        "model": model,
        "max_tokens": 1024,
        "system": [{
            "type": "text",
            "text": SYSTEM_PROMPT,
            "cache_control": { "type": "ephemeral" }
        }],
        "messages": [{
            "role": "user",
            "content": text
        }]
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let resp = client
        .post(ANTHROPIC_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let err = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Anthropic error {status}: {err}"));
    }
    let v: serde_json::Value = resp.json().await?;
    let text = v["content"][0]["text"]
        .as_str()
        .ok_or_else(|| anyhow!("unexpected Anthropic response: {v}"))?
        .trim()
        .to_string();
    Ok(text)
}
