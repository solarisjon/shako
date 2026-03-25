use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Default, Deserialize, Clone)]
pub struct JboshConfig {
    /// Legacy single-provider config. Used when no `active_provider` is set.
    #[serde(default)]
    pub llm: LlmConfig,
    /// Named provider configs. Example: `[providers.lm_studio]` or `[providers.work_proxy]`.
    #[serde(default)]
    pub providers: HashMap<String, LlmConfig>,
    /// Which named provider to use. If unset, falls back to `[llm]`.
    pub active_provider: Option<String>,
    #[serde(default)]
    pub behavior: BehaviorConfig,
    #[serde(default)]
    pub fish: FishConfig,
    #[serde(default)]
    pub aliases: HashMap<String, String>,
}

impl JboshConfig {
    /// Returns the active LLM config: the named provider if `active_provider` is set,
    /// otherwise the legacy `[llm]` block.
    pub fn active_llm(&self) -> &LlmConfig {
        if let Some(ref name) = self.active_provider {
            if let Some(provider) = self.providers.get(name) {
                return provider;
            }
            log::warn!("active_provider '{name}' not found in [providers], falling back to [llm]");
        }
        &self.llm
    }
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
    #[serde(default = "default_temperature")]
    pub temperature: f32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BehaviorConfig {
    #[serde(default = "default_true")]
    pub confirm_ai_commands: bool,
    #[serde(default = "default_true")]
    pub auto_correct_typos: bool,
    #[serde(default = "default_history_lines")]
    pub history_context_lines: usize,
    #[serde(default = "default_safety_mode")]
    pub safety_mode: String,
    #[serde(default = "default_edit_mode")]
    pub edit_mode: String,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct FishConfig {
    #[serde(default)]
    pub source_config: bool,
}

// Defaults

fn default_endpoint() -> String {
    "http://localhost:11434/v1/chat/completions".to_string()
}

fn default_model() -> String {
    "claude-haiku-4.5".to_string()
}

fn default_api_key_env() -> String {
    "LLMPROXY_KEY".to_string()
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

fn default_temperature() -> f32 {
    0.1
}

fn default_history_lines() -> usize {
    20
}

fn default_safety_mode() -> String {
    "warn".to_string()
}

fn default_edit_mode() -> String {
    "emacs".to_string()
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
            temperature: default_temperature(),
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
            edit_mode: "emacs".to_string(),
        }
    }
}

impl JboshConfig {
    /// Load config from ~/.config/shako/config.toml, falling back to defaults.
    /// Returns `(config, first_run)` where `first_run` is true if the wizard was invoked.
    pub fn load() -> Result<(Self, bool)> {
        let config_path = Self::config_path();

        if config_path.exists() {
            let contents = std::fs::read_to_string(&config_path)?;
            let config: JboshConfig = toml::from_str(&contents)?;
            log::debug!("loaded config from {}", config_path.display());
            let active = config.active_llm();
            log::debug!(
                "  active provider: {}",
                config.active_provider.as_deref().unwrap_or("llm")
            );
            log::debug!("  llm endpoint: {}", active.endpoint);
            log::debug!("  llm model: {}", active.model);
            Ok((config, false))
        } else {
            log::info!(
                "no config found at {}, running first-time setup",
                config_path.display()
            );
            let toml = crate::setup::run_wizard(&config_path)?;
            let config: JboshConfig = toml::from_str(&toml)?;
            Ok((config, true))
        }
    }

    /// Return the shako config directory (e.g. `~/.config/shako`).
    pub fn config_dir() -> PathBuf {
        std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .ok()
            .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
            .unwrap_or_else(|| PathBuf::from("."))
            .join("shako")
    }

    /// Remove shako config files so the next `load()` triggers the first-run wizard.
    /// Removes config.toml, config.shako, starship.toml, conf.d/, and functions/.
    pub fn reset() -> Result<()> {
        let dir = Self::config_dir();
        if !dir.exists() {
            return Ok(());
        }

        let removals = [
            ("config.toml", false),
            ("config.shako", false),
            ("starship.toml", false),
            ("conf.d", true),
            ("functions", true),
        ];

        for (name, is_dir) in &removals {
            let path = dir.join(name);
            if path.exists() {
                if *is_dir {
                    std::fs::remove_dir_all(&path)?;
                } else {
                    std::fs::remove_file(&path)?;
                }
                eprintln!("  removed {}", path.display());
            }
        }

        Ok(())
    }

    fn config_path() -> PathBuf {
        // Prefer XDG-style ~/.config on all platforms (matches README docs).
        // Fall back to dirs::config_dir() (~/Library/Application Support on macOS).
        let xdg_path = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .ok()
            .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
            .map(|d| d.join("shako").join("config.toml"));

        if let Some(ref p) = xdg_path {
            if p.exists() {
                return p.clone();
            }
        }

        // Fall back to platform default (~/Library/Application Support/shako/config.toml on macOS)
        let platform_path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("shako")
            .join("config.toml");

        if platform_path.exists() {
            return platform_path;
        }

        // Neither exists — return XDG path as the canonical default
        xdg_path.unwrap_or(platform_path)
    }
}
