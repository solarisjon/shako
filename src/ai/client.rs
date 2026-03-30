use std::error::Error;
use std::io::Write;

use anyhow::Result;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::config::LlmConfig;

// ── OpenAI-compatible types ──────────────────────────────────────────────────

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

// ── Anthropic native API types ───────────────────────────────────────────────

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    temperature: f32,
    system: String,
    messages: Vec<ChatMessage>,
    stream: bool,
}

#[derive(Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
}

/// SSE event from Anthropic's streaming API.
/// Only `content_block_delta` events carry text; others are safely ignored.
#[derive(Deserialize)]
struct AnthropicStreamDelta {
    #[serde(rename = "type")]
    delta_type: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    delta: Option<AnthropicStreamDelta>,
}

/// Normalize an Anthropic endpoint URL: ensure it has a scheme and the `/v1/messages` path.
fn normalize_anthropic_endpoint(endpoint: &str) -> String {
    let mut url = endpoint.trim().to_string();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        url = format!("https://{url}");
    }
    if let Ok(parsed) = url.parse::<reqwest::Url>() {
        let path = parsed.path();
        if path == "/" || path.is_empty() {
            url = format!("{}/v1/messages", url.trim_end_matches('/'));
        }
    }
    url
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
    query_llm_inner(system_prompt, user_input, config, None).await
}

pub async fn query_llm_with_spinner(
    system_prompt: &str,
    user_input: &str,
    config: &LlmConfig,
    spinner_flag: Arc<AtomicBool>,
) -> Result<String> {
    query_llm_inner(system_prompt, user_input, config, Some(spinner_flag)).await
}

async fn query_llm_inner(
    system_prompt: &str,
    user_input: &str,
    config: &LlmConfig,
    spinner_flag: Option<Arc<AtomicBool>>,
) -> Result<String> {
    let is_anthropic = config.provider_type.as_deref() == Some("anthropic");

    if is_anthropic {
        query_anthropic(system_prompt, user_input, config, spinner_flag).await
    } else {
        query_openai_compat(system_prompt, user_input, config, spinner_flag).await
    }
}

/// Query using the Anthropic native Messages API.
async fn query_anthropic(
    system_prompt: &str,
    user_input: &str,
    config: &LlmConfig,
    spinner_flag: Option<Arc<AtomicBool>>,
) -> Result<String> {
    let api_key = std::env::var(&config.api_key_env).unwrap_or_default();
    let endpoint = normalize_anthropic_endpoint(&config.endpoint);
    log::debug!("Anthropic request → {endpoint}");

    if api_key.is_empty() {
        return Err(anyhow::anyhow!(
            "Anthropic API key not set — export {}=sk-ant-...",
            config.api_key_env
        ));
    }

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(!config.verify_ssl)
        .timeout(std::time::Duration::from_secs(config.timeout_secs))
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build HTTP client: {e}"))?;

    let request = AnthropicRequest {
        model: config.model.clone(),
        max_tokens: config.max_tokens,
        temperature: config.temperature,
        system: system_prompt.to_string(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: user_input.to_string(),
        }],
        stream: true,
    };

    let mut last_err = None;

    for attempt in 0..2 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            log::debug!("Anthropic retry attempt {attempt}");
        }

        let req = client
            .post(&endpoint)
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&request);

        match req.send().await {
            Ok(response) => {
                if !response.status().is_success() {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    return Err(anyhow::anyhow!(
                        "Anthropic API returned {status}: {body}"
                    ));
                }

                let mut stream = response.bytes_stream();
                let mut full_text = String::new();
                let mut raw_bytes = Vec::new();
                let mut buf = String::new();
                let mut done = false;

                while let Some(chunk) = stream.next().await {
                    let chunk = chunk?;
                    raw_bytes.extend_from_slice(&chunk);
                    buf.push_str(&String::from_utf8_lossy(&chunk));

                    while let Some(nl) = buf.find('\n') {
                        let line = buf[..nl].trim().to_string();
                        buf = buf[nl + 1..].to_string();

                        if line.starts_with("data: ") {
                            let data = line.strip_prefix("data: ").unwrap_or("");
                            if let Ok(event) = serde_json::from_str::<AnthropicStreamEvent>(data) {
                                if event.event_type == "message_stop" {
                                    done = true;
                                    break;
                                }
                                if event.event_type == "content_block_delta" {
                                    if let Some(delta) = event.delta {
                                        if delta.delta_type == "text_delta" {
                                            if let Some(text) = delta.text {
                                                if let Some(ref flag) = spinner_flag {
                                                    if flag.load(Ordering::Relaxed) {
                                                        flag.store(false, Ordering::Relaxed);
                                                        std::thread::sleep(
                                                            std::time::Duration::from_millis(100),
                                                        );
                                                        eprint!("\r\x1b[K");
                                                        let _ = std::io::stderr().flush();
                                                    }
                                                }
                                                print!("{text}");
                                                let _ = std::io::stdout().flush();
                                                full_text.push_str(&text);
                                            }
                                        }
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
                    println!();
                    return Ok(full_text.trim().to_string());
                }

                // Fallback: parse as non-streaming Anthropic JSON
                log::debug!("Anthropic stream yielded no content, trying non-streaming parse");
                let body = String::from_utf8_lossy(&raw_bytes);
                let resp: AnthropicResponse = serde_json::from_str(&body).map_err(|e| {
                    anyhow::anyhow!("failed to parse Anthropic response: {e}\nbody: {body}")
                })?;
                return resp
                    .content
                    .iter()
                    .filter(|c| c.content_type == "text")
                    .find_map(|c| c.text.as_deref().map(|t| t.trim().to_string()))
                    .ok_or_else(|| anyhow::anyhow!("no text content in Anthropic response"));
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
        "could not reach Anthropic at {endpoint}: {}",
        last_err.unwrap_or_default()
    ))
}

/// Query using the OpenAI-compatible chat completions API.
async fn query_openai_compat(
    system_prompt: &str,
    user_input: &str,
    config: &LlmConfig,
    spinner_flag: Option<Arc<AtomicBool>>,
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
                    return Err(anyhow::anyhow!(
                        "LLM API {endpoint} returned {status}: {body}"
                    ));
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
                                        if let Some(ref flag) = spinner_flag {
                                            if flag.load(Ordering::Relaxed) {
                                                flag.store(false, Ordering::Relaxed);
                                                std::thread::sleep(
                                                    std::time::Duration::from_millis(100),
                                                );
                                                eprint!("\r\x1b[K");
                                                let _ = std::io::stderr().flush();
                                            }
                                        }
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
                let chat_response: ChatResponse = serde_json::from_str(&body).map_err(|e| {
                    anyhow::anyhow!("failed to parse LLM response: {e}\nbody: {body}")
                })?;
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

/// Result of the startup AI session probe.
pub enum AiCheckResult {
    Ready,
    Disabled,
    NoApiKey(String),
    AuthFailed(u16),
    Unreachable(String),
}

/// Probe the configured LLM endpoint at startup and return its status.
/// Hits `GET /v1/models` with a 3-second timeout — fast enough not to stall startup.
pub async fn check_ai_session(
    config: &crate::config::LlmConfig,
    ai_enabled: bool,
) -> AiCheckResult {
    if !ai_enabled {
        return AiCheckResult::Disabled;
    }

    let is_anthropic = config.provider_type.as_deref() == Some("anthropic");
    let api_key = std::env::var(&config.api_key_env).unwrap_or_default();

    // Normalize endpoint according to provider type to derive the probe origin.
    let endpoint = if is_anthropic {
        normalize_anthropic_endpoint(&config.endpoint)
    } else {
        normalize_endpoint(&config.endpoint)
    };

    // Derive the /v1/models URL from the same origin as the chat endpoint.
    let models_url = match endpoint.parse::<reqwest::Url>() {
        Ok(parsed) => {
            let origin = match parsed.port() {
                Some(port) => format!(
                    "{}://{}:{}",
                    parsed.scheme(),
                    parsed.host_str().unwrap_or("localhost"),
                    port
                ),
                None => format!(
                    "{}://{}",
                    parsed.scheme(),
                    parsed.host_str().unwrap_or("localhost")
                ),
            };
            format!("{origin}/v1/models")
        }
        Err(_) => return AiCheckResult::Unreachable("invalid endpoint URL".to_string()),
    };

    // Anthropic always requires an API key; local OpenAI-compat servers don't.
    let is_local = !is_anthropic
        && (models_url.contains("localhost") || models_url.contains("127.0.0.1"));
    if api_key.is_empty() && !is_local {
        return AiCheckResult::NoApiKey(config.api_key_env.clone());
    }

    let client = match reqwest::Client::builder()
        .danger_accept_invalid_certs(!config.verify_ssl)
        .timeout(std::time::Duration::from_secs(3))
        .build()
    {
        Ok(c) => c,
        Err(e) => return AiCheckResult::Unreachable(e.to_string()),
    };

    let mut req = client.get(&models_url);
    if !api_key.is_empty() {
        if is_anthropic {
            req = req
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01");
        } else {
            req = req.bearer_auth(&api_key);
        }
    }

    match req.send().await {
        Ok(resp) => match resp.status().as_u16() {
            200..=299 => AiCheckResult::Ready,
            401 | 403 => AiCheckResult::AuthFailed(resp.status().as_u16()),
            code => AiCheckResult::Unreachable(format!("HTTP {code}")),
        },
        Err(e) => {
            let reason = if e.is_connect() || e.is_timeout() {
                "endpoint unreachable".to_string()
            } else {
                e.to_string()
            };
            AiCheckResult::Unreachable(reason)
        }
    }
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
