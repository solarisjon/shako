use std::error::Error;
use std::io::Write;

use anyhow::Result;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use crate::config::LlmConfig;

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
    temperature: f32,
    stream: bool,
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

// Streaming response types (SSE)
#[derive(Deserialize)]
struct StreamDelta {
    content: Option<String>,
}

#[derive(Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(Deserialize)]
struct StreamResponse {
    choices: Vec<StreamChoice>,
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
/// Streams tokens to stdout as they arrive. Falls back to non-streaming
/// parse if the server ignores `stream: true` and returns plain JSON.
/// Retries once on transient network errors with a short delay.
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
        temperature: config.temperature,
        stream: true,
    };

    let mut last_err = None;

    for attempt in 0..2 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            log::debug!("LLM retry attempt {attempt}");
        }

        let mut req = client.post(&endpoint).json(&request);
        if !api_key.is_empty() {
            req = req.bearer_auth(&api_key);
        }

        match req.send().await {
            Ok(response) => {
                if !response.status().is_success() {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    return Err(anyhow::anyhow!("LLM API {endpoint} returned {status}: {body}"));
                }

                // Collect raw bytes while streaming SSE tokens to stdout
                let mut stream = response.bytes_stream();
                let mut full_text = String::new();
                let mut raw_bytes = Vec::new();
                let mut buf = String::new();
                let mut done = false;

                while let Some(chunk) = stream.next().await {
                    let chunk = chunk?;
                    raw_bytes.extend_from_slice(&chunk);
                    buf.push_str(&String::from_utf8_lossy(&chunk));

                    // Process complete lines
                    while let Some(nl) = buf.find('\n') {
                        let line = buf[..nl].trim().to_string();
                        buf = buf[nl + 1..].to_string();

                        if line.starts_with("data: ") {
                            let data = line.strip_prefix("data: ").unwrap_or("");
                            if data == "[DONE]" {
                                done = true;
                                break;
                            }
                            if let Ok(chunk_resp) = serde_json::from_str::<StreamResponse>(data) {
                                if let Some(choice) = chunk_resp.choices.first() {
                                    if let Some(content) = &choice.delta.content {
                                        print!("{content}");
                                        let _ = std::io::stdout().flush();
                                        full_text.push_str(content);
                                    }
                                }
                            }
                        }
                    }

                    if done {
                        break;
                    }
                }

                if !full_text.is_empty() {
                    // Streaming worked — emit trailing newline for the confirm prompt
                    println!();
                    return Ok(full_text.trim().to_string());
                }

                // Fallback: server returned plain JSON instead of SSE
                log::debug!("stream yielded no content, falling back to non-streaming parse");
                let body = String::from_utf8_lossy(&raw_bytes);
                let chat_response: ChatResponse = serde_json::from_str(&body)
                    .map_err(|e| anyhow::anyhow!("failed to parse LLM response: {e}\nbody: {body}"))?;
                return chat_response
                    .choices
                    .first()
                    .map(|c| c.message.content.trim().to_string())
                    .ok_or_else(|| anyhow::anyhow!("no response from LLM"));
            }
            Err(e) => {
                let mut msg = format!("{e}");
                let mut source = e.source();
                while let Some(cause) = source {
                    msg.push_str(&format!("\n  caused by: {cause}"));
                    source = cause.source();
                }
                last_err = Some(msg);
            }
        }
    }

    Err(anyhow::anyhow!(
        "could not reach LLM at {endpoint}: {}",
        last_err.unwrap_or_default()
    ))
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
