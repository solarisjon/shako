use std::error::Error;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::LlmConfig;

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

/// Normalize an endpoint URL: ensure it has a scheme and the chat completions path.
pub fn normalize_endpoint(endpoint: &str) -> String {
    let mut url = endpoint.trim().to_string();

    // Add scheme if missing
    if !url.starts_with("http://") && !url.starts_with("https://") {
        url = format!("https://{url}");
    }

    // Add OpenAI-compatible path if the URL looks like a bare host (no path component)
    if let Ok(parsed) = url.parse::<reqwest::Url>() {
        let path = parsed.path();
        if path == "/" || path.is_empty() {
            url = format!("{}/v1/chat/completions", url.trim_end_matches('/'));
        }
    }

    url
}

/// Query the configured LLM endpoint with OpenAI-compatible API.
pub async fn query_llm(
    system_prompt: &str,
    user_input: &str,
    config: &LlmConfig,
) -> Result<String> {
    let api_key = std::env::var(&config.api_key_env).unwrap_or_default();
    let endpoint = normalize_endpoint(&config.endpoint);
    log::debug!("LLM request → {endpoint}");

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(!config.verify_ssl)
        .timeout(std::time::Duration::from_secs(config.timeout_secs))
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build HTTP client: {e}"))?;

    let request = ChatRequest {
        model: config.model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: system_prompt.to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: user_input.to_string(),
            },
        ],
        max_tokens: config.max_tokens,
        temperature: 0.1,
    };

    let mut req = client.post(&endpoint).json(&request);

    if !api_key.is_empty() {
        req = req.bearer_auth(&api_key);
    }

    let response = req.send().await.map_err(|e| {
        let mut msg = format!("{e}");
        let mut source = e.source();
        while let Some(cause) = source {
            msg.push_str(&format!("\n  caused by: {cause}"));
            source = cause.source();
        }
        anyhow::anyhow!("{msg}")
    })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("LLM API {endpoint} returned {status}: {body}");
    }

    let chat_response: ChatResponse = response.json().await?;

    chat_response
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .ok_or_else(|| anyhow::anyhow!("no response from LLM"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_bare_hostname() {
        assert_eq!(
            normalize_endpoint("llm-proxy.example.com"),
            "https://llm-proxy.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_normalize_with_scheme_no_path() {
        assert_eq!(
            normalize_endpoint("https://llm-proxy.example.com"),
            "https://llm-proxy.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_normalize_already_complete() {
        assert_eq!(
            normalize_endpoint("https://llm-proxy.example.com/v1/chat/completions"),
            "https://llm-proxy.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_normalize_http_localhost() {
        assert_eq!(
            normalize_endpoint("http://localhost:1234/v1/chat/completions"),
            "http://localhost:1234/v1/chat/completions"
        );
    }

    #[test]
    fn test_normalize_bare_host_trailing_slash() {
        assert_eq!(
            normalize_endpoint("https://llm-proxy.example.com/"),
            "https://llm-proxy.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_normalize_custom_path_preserved() {
        assert_eq!(
            normalize_endpoint("https://proxy.example.com/api/llm"),
            "https://proxy.example.com/api/llm"
        );
    }
}
