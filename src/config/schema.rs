use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct JboshConfig {
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub behavior: BehaviorConfig,
    #[serde(default)]
    pub aliases: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LlmConfig {
    #[serde(default = "default_endpoint")]
    pub endpoint: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_true")]
    pub verify_ssl: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BehaviorConfig {
    #[serde(default = "default_true")]
    pub confirm_ai_commands: bool,
    #[serde(default = "default_true")]
    pub auto_correct_typos: bool,
    #[serde(default = "default_history_lines")]
    #[allow(dead_code)] // reserved for AI context window
    pub history_context_lines: usize,
    #[serde(default = "default_safety_mode")]
    pub safety_mode: String,
}

// Defaults

fn default_endpoint() -> String {
    "http://localhost:11434/v1/chat/completions".to_string()
}

fn default_model() -> String {
    "claude-haiku-4.5".to_string()
}

fn default_api_key_env() -> String {
    "JBOSH_LLM_KEY".to_string()
}

fn default_timeout() -> u64 {
    30
}

fn default_max_tokens() -> u32 {
    512
}

fn default_true() -> bool {
    true
}

fn default_history_lines() -> usize {
    20
}

fn default_safety_mode() -> String {
    "warn".to_string()
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            endpoint: default_endpoint(),
            model: default_model(),
            api_key_env: default_api_key_env(),
            timeout_secs: default_timeout(),
            max_tokens: default_max_tokens(),
            verify_ssl: true,
        }
    }
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            confirm_ai_commands: true,
            auto_correct_typos: true,
            history_context_lines: 20,
            safety_mode: "warn".to_string(),
        }
    }
}

impl JboshConfig {
    /// Load config from ~/.config/jbosh/config.toml, falling back to defaults.
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path();

        if config_path.exists() {
            let contents = std::fs::read_to_string(&config_path)?;
            let config: JboshConfig = toml::from_str(&contents)?;
            log::debug!("loaded config from {}", config_path.display());
            log::debug!("  llm endpoint: {}", config.llm.endpoint);
            log::debug!("  llm model: {}", config.llm.model);
            Ok(config)
        } else {
            log::info!("no config found at {}, using defaults", config_path.display());
            Ok(Self {
                llm: LlmConfig::default(),
                behavior: BehaviorConfig::default(),
                aliases: HashMap::new(),
            })
        }
    }

    fn config_path() -> PathBuf {
        // Prefer XDG-style ~/.config on all platforms (matches README docs).
        // Fall back to dirs::config_dir() (~/Library/Application Support on macOS).
        let xdg_path = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .ok()
            .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
            .map(|d| d.join("jbosh").join("config.toml"));

        if let Some(ref p) = xdg_path {
            if p.exists() {
                return p.clone();
            }
        }

        // Fall back to platform default (~/Library/Application Support/jbosh/config.toml on macOS)
        let platform_path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("jbosh")
            .join("config.toml");

        if platform_path.exists() {
            return platform_path;
        }

        // Neither exists — return XDG path as the canonical default
        xdg_path.unwrap_or(platform_path)
    }
}
