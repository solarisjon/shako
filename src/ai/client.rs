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

/// Query the configured LLM endpoint with OpenAI-compatible API.
pub async fn query_llm(
    system_prompt: &str,
    user_input: &str,
    config: &LlmConfig,
) -> Result<String> {
    let api_key = std::env::var(&config.api_key_env).unwrap_or_default();

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(!config.verify_ssl)
        .timeout(std::time::Duration::from_secs(config.timeout_secs))
        .build()?;

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

    let mut req = client.post(&config.endpoint).json(&request);

    if !api_key.is_empty() {
        req = req.bearer_auth(&api_key);
    }

    let response = req.send().await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("LLM API returned {status}: {body}");
    }

    let chat_response: ChatResponse = response.json().await?;

    chat_response
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .ok_or_else(|| anyhow::anyhow!("no response from LLM"))
}
