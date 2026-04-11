use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

/// Top-level shako configuration, loaded from `~/.config/shako/config.toml`.
#[derive(Debug, Default, Deserialize, Clone)]
pub struct ShakoConfig {
    /// Legacy single-provider config. Used when no `active_provider` is set.
    #[serde(default)]
    pub llm: LlmConfig,
    /// Named provider configs. Example: `[providers.lm_studio]` or `[providers.work_proxy]`.
    #[serde(default)]
    pub providers: HashMap<String, LlmConfig>,
    /// Which named provider to use. If unset, falls back to `[llm]`.
    pub active_provider: Option<String>,
    /// Shell behavior settings (confirmation, safety mode, history, etc.).
    #[serde(default)]
    pub behavior: BehaviorConfig,
    /// Security settings (prompt injection guard, etc.).
    #[serde(default)]
    pub security: SecurityConfig,
    /// Fish shell import settings.
    #[serde(default)]
    pub fish: FishConfig,
    /// User-defined aliases (e.g. `ll = "ls -la"`).
    #[serde(default)]
    pub aliases: HashMap<String, String>,
    /// Abbreviations: short strings expanded to longer commands on space.
    /// Example: `gc = "git commit"` so typing `gc ` expands to `git commit `.
    #[serde(default)]
    pub abbreviations: HashMap<String, String>,
    /// Startup environment variables set before any commands run.
    /// Example: `EDITOR = "vim"`.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

impl ShakoConfig {
    /// Returns the active LLM config: the named provider if `active_provider` is set,
    /// otherwise the legacy `[llm]` block.
    pub fn active_llm(&self) -> &LlmConfig {
        if let Some(ref name) = self.active_provider {
            if let Some(provider) = self.providers.get(name) {
                return provider;
            }
            // User-visible warning is already shown by the startup banner in main.rs;
            // silently fall through here to avoid duplicate noise on every active_llm() call.
        }
        &self.llm
    }
}

/// Configuration for a single LLM provider endpoint.
///
/// Used both as the legacy `[llm]` block and as named entries under
/// `[providers.<name>]`.
#[derive(Debug, Deserialize, Clone)]
pub struct LlmConfig {
    /// HTTP endpoint URL for the chat-completions API.
    #[serde(default = "default_endpoint")]
    pub endpoint: String,
    /// Model identifier sent in the API request body.
    #[serde(default = "default_model")]
    pub model: String,
    /// Name of the environment variable that holds the API key.
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,
    /// Request timeout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Maximum tokens the model may generate per response.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Whether to verify TLS certificates. Set to `false` for local proxies.
    #[serde(default = "default_true")]
    pub verify_ssl: bool,
    /// Sampling temperature (0.0–1.0). Lower values → more deterministic output.
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    /// API format: `"anthropic"` for Anthropic's native API, anything else (or unset)
    /// uses the OpenAI-compatible format.
    #[serde(default)]
    pub provider_type: Option<String>,
}

/// Behavior configuration for the shako REPL.
#[derive(Debug, Deserialize, Clone)]
pub struct BehaviorConfig {
    /// If `true`, show a `[Y/n/e]` confirmation prompt before running AI-generated commands.
    #[serde(default = "default_true")]
    pub confirm_ai_commands: bool,
    /// If `true`, automatically suggest corrections for likely typos.
    #[serde(default = "default_true")]
    pub auto_correct_typos: bool,
    /// If `false`, AI translation is disabled — unknown input is treated as a command error.
    #[serde(default = "default_true")]
    pub ai_enabled: bool,
    /// Number of recent history lines to include as context for AI requests.
    #[serde(default = "default_history_lines")]
    pub history_context_lines: usize,
    /// Safety mode: `"on"` (default), `"warn"`, or `"off"`.
    #[serde(default = "default_safety_mode")]
    pub safety_mode: String,
    /// Line-editing mode: `"emacs"` (default) or `"vi"`.
    #[serde(default = "default_edit_mode")]
    pub edit_mode: String,
    /// Maximum number of history entries to keep.  Defaults to 10,000.
    #[serde(default = "default_history_size")]
    pub history_size: usize,
    /// Deduplicate consecutive identical commands in history.  Defaults to true.
    ///
    /// When enabled, the `read_recent_history` function filters out consecutive
    /// duplicate entries before presenting them to the AI context.
    /// (Reedline's raw history file may still contain duplicates; dedup is
    /// applied on read, not on write.)
    #[serde(default = "default_true")]
    pub history_dedup: bool,
    /// Extra text appended to the AI system prompt.
    /// Useful for project-specific instructions not covered by .shako.toml.
    #[serde(default)]
    pub ai_system_prompt_extra: Option<String>,
    /// Context names (or substrings) that are considered "production".
    ///
    /// Used by the environment drift detector to decide when a context switch
    /// warrants a warning before destructive commands.
    ///
    /// Example in `.shako.toml`:
    /// ```toml
    /// [behavior]
    /// production_contexts = ["prod", "production", "arn:aws:eks:us-east-1:123456789"]
    /// ```
    ///
    /// If empty (the default), built-in heuristics apply: any context whose
    /// name contains `prod`, `production`, `live`, or `prd` is treated as
    /// production.
    #[serde(default)]
    pub production_contexts: Vec<String>,
    /// How long (in seconds) after a context switch shako continues to warn
    /// about destructive commands in the new context.  Defaults to 300 (5 min).
    #[serde(default = "default_warn_window_secs")]
    pub context_warn_window_secs: u64,
    /// Enable the Temporal Command Archaeology session journal.
    ///
    /// When `true` (default), every confirmed NL→command execution is appended
    /// to `~/.local/share/shako/journal.jsonl`.  When the user `cd`s into a
    /// project they haven't touched in `session_stale_days` days, shako offers
    /// an AI-powered resumption brief summarising what they were doing last time.
    ///
    /// Set to `false` to disable all journalling and resumption prompts.
    #[serde(default = "default_true")]
    pub session_journal: bool,
    /// Number of days of inactivity before a project is considered "stale"
    /// and a resumption brief is offered on `cd`.  Defaults to 3.
    #[serde(default = "default_session_stale_days")]
    pub session_stale_days: u64,

    // ── Danger Replay / Undo Graph ────────────────────────────────────────────
    /// Enable automatic filesystem snapshots before dangerous commands.
    ///
    /// When `true` (default), shako offers to snapshot affected paths before
    /// executing commands that match the safety layer's dangerous patterns.
    /// Snapshots are stored in `~/.local/share/shako/snapshots/` and can be
    /// restored via natural language ("undo that rm", "restore what I deleted").
    ///
    /// Set to `false` to disable all snapshotting.
    #[serde(default = "default_true")]
    pub undo_snapshots: bool,

    /// Maximum size (in bytes) of a snapshot target before we skip snapshotting.
    ///
    /// Defaults to 52_428_800 (50 MB).  Set to 0 to use the default.
    #[serde(default = "default_snapshot_max_bytes")]
    pub snapshot_max_bytes: u64,

    /// How many days to keep old snapshots before garbage-collecting them.
    ///
    /// Defaults to 7 days.
    #[serde(default = "default_snapshot_gc_days")]
    pub snapshot_gc_days: u64,
}

/// Security configuration for shako.
///
/// Example in `~/.config/shako/config.toml`:
/// ```toml
/// [security]
/// prompt_injection_guard = true   # default: true
/// ```
#[derive(Debug, Deserialize, Clone)]
pub struct SecurityConfig {
    /// Enable the prompt injection firewall for all user-controlled LLM context
    /// (`.shako.toml` `[ai].context`, `learned_prefs.toml`, `ai_system_prompt_extra`).
    ///
    /// When `true` (default), any field containing known injection patterns is stripped
    /// and a warning is printed.  All clean fields are wrapped in structural delimiters
    /// so the LLM treats them as data, not instructions.
    ///
    /// **Strongly recommended to leave enabled.**  Set to `false` only if you control
    /// every file that shako reads and trust its contents absolutely.
    #[serde(default = "default_true")]
    pub prompt_injection_guard: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            prompt_injection_guard: true,
        }
    }
}

/// Configuration for fish shell interoperability.
#[derive(Debug, Default, Deserialize, Clone)]
pub struct FishConfig {
    /// If `true`, source `~/.config/fish/config.fish` on startup after import.
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

fn default_history_size() -> usize {
    10_000
}

fn default_safety_mode() -> String {
    "warn".to_string()
}

fn default_edit_mode() -> String {
    "emacs".to_string()
}

fn default_warn_window_secs() -> u64 {
    300
}

fn default_session_stale_days() -> u64 {
    3
}

fn default_snapshot_max_bytes() -> u64 {
    50 * 1024 * 1024 // 50 MB
}

fn default_snapshot_gc_days() -> u64 {
    7
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
            provider_type: None,
        }
    }
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            confirm_ai_commands: true,
            auto_correct_typos: true,
            ai_enabled: true,
            history_context_lines: 20,
            safety_mode: "warn".to_string(),
            edit_mode: "emacs".to_string(),
            history_size: 10_000,
            history_dedup: true,
            ai_system_prompt_extra: None,
            production_contexts: Vec::new(),
            context_warn_window_secs: 300,
            session_journal: true,
            session_stale_days: 3,
            undo_snapshots: true,
            snapshot_max_bytes: default_snapshot_max_bytes(),
            snapshot_gc_days: default_snapshot_gc_days(),
        }
    }
}

impl ShakoConfig {
    /// Load config from ~/.config/shako/config.toml, falling back to defaults.
    /// Returns `(config, first_run)` where `first_run` is true if the wizard was invoked.
    pub fn load() -> Result<(Self, bool)> {
        let config_path = Self::config_path();

        if config_path.exists() {
            let contents = std::fs::read_to_string(&config_path)?;
            let config: ShakoConfig = toml::from_str(&contents)?;
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
            let config: ShakoConfig = toml::from_str(&toml)?;
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
                eprintln!("shako: removed {}", path.display());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ai_enabled_defaults_to_true() {
        let config = BehaviorConfig::default();
        assert!(config.ai_enabled, "ai_enabled should default to true");
    }

    #[test]
    fn test_ai_enabled_can_be_disabled_via_toml() {
        let toml_str = "[behavior]\nai_enabled = false\n";
        let shako_config: ShakoConfig = toml::from_str(toml_str).expect("parse failed");
        assert!(!shako_config.behavior.ai_enabled);
    }
}
