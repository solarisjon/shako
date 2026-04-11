//! Watch-and-learn: track when the user edits AI-suggested commands and extract
//! tool substitutions to improve future AI translations.
//!
//! When the user edits a suggestion like `grep foo src/` → `rg foo src/`, we
//! note that they prefer `rg` over `grep`. That preference is persisted to
//! `~/.config/shako/learned_prefs.toml` and injected into subsequent AI prompts.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use which::which;

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct Substitution {
    /// The token the AI originally used (e.g. "grep")
    pub from: String,
    /// The token the user replaced it with (e.g. "rg")
    pub to: String,
    /// How many times the user has made this substitution
    pub uses: u32,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LearnedPrefs {
    #[serde(default)]
    pub substitutions: Vec<Substitution>,
}

impl LearnedPrefs {
    pub fn load() -> Self {
        let path = prefs_path();
        let Ok(contents) = fs::read_to_string(&path) else {
            return Self::default();
        };
        toml::from_str(&contents).unwrap_or_default()
    }

    pub fn save(&self) {
        let path = prefs_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let header = "# shako learned preferences — auto-generated, safe to edit or delete\n\n";
        if let Ok(body) = toml::to_string_pretty(self) {
            let _ = fs::write(&path, format!("{header}{body}"));
        }
    }

    /// Record one (from → to) substitution, incrementing an existing entry or inserting a new one.
    ///
    /// **Security**: the `to` token is validated to be an actual binary on PATH before
    /// it is persisted.  This prevents session-history poisoning by crafted edit sequences
    /// where an attacker tricks the learned-prefs file into injecting a malicious tool name
    /// (e.g. a script named after a real tool placed in `/tmp`).
    ///
    /// If `to` is not found on PATH the substitution is silently ignored.
    pub fn record(&mut self, from: &str, to: &str) {
        // Only persist if the target binary actually exists on PATH.
        if which(to).is_err() {
            log::debug!(
                "learned_prefs: ignoring substitution '{from}' → '{to}': \
                 '{to}' not found on PATH"
            );
            return;
        }

        if let Some(existing) = self
            .substitutions
            .iter_mut()
            .find(|s| s.from == from && s.to == to)
        {
            existing.uses += 1;
        } else {
            self.substitutions.push(Substitution {
                from: from.to_string(),
                to: to.to_string(),
                uses: 1,
            });
        }
    }

    /// Format all substitutions as a hint string for the AI system prompt.
    pub fn to_context_hint(&self) -> String {
        if self.substitutions.is_empty() {
            return String::new();
        }

        // Only emit substitutions the user has confirmed at least once
        let hints: Vec<String> = self
            .substitutions
            .iter()
            .filter(|s| s.uses >= 1)
            .map(|s| format!("  - prefer `{}` over `{}`", s.to, s.from))
            .collect();

        if hints.is_empty() {
            return String::new();
        }

        format!(
            "User preferences learned from past edits:\n{}",
            hints.join("\n")
        )
    }
}

// ── Public helpers ─────────────────────────────────────────────────────────────

/// Called whenever the user edits an AI-suggested command before running it.
/// Extracts tool substitutions and persists them.
pub fn record_edit(original: &str, edited: &str) {
    let subs = extract_substitutions(original, edited);
    if subs.is_empty() {
        return;
    }
    let mut prefs = LearnedPrefs::load();
    for (from, to) in subs {
        prefs.record(&from, &to);
    }
    prefs.save();
}

/// Return a formatted hint string to inject into the AI system prompt.
pub fn context_hint() -> String {
    LearnedPrefs::load().to_context_hint()
}

// ── Internals ─────────────────────────────────────────────────────────────────

fn prefs_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("shako")
        .join("learned_prefs.toml")
}

/// Compare tokens positionally. Where both positions contain tool-like tokens
/// that differ, record (original_token → edited_token).
fn extract_substitutions(original: &str, edited: &str) -> Vec<(String, String)> {
    let orig_tokens: Vec<&str> = original.split_whitespace().collect();
    let edit_tokens: Vec<&str> = edited.split_whitespace().collect();

    let mut subs = Vec::new();

    // Walk the shorter of the two token lists
    let len = orig_tokens.len().min(edit_tokens.len());
    for i in 0..len {
        let from = orig_tokens[i];
        let to = edit_tokens[i];
        if from != to && is_tool_name(from) && is_tool_name(to) {
            subs.push((from.to_string(), to.to_string()));
        }
    }

    subs
}

/// A "tool name" is a bare identifier — no flags, paths, special chars, or env vars.
fn is_tool_name(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    !token.starts_with('-')
        && !token.starts_with('/')
        && !token.starts_with('.')
        && !token.starts_with('~')
        && !token.starts_with('$')
        && !token.contains('=')
        && !token.contains('/')
        && !token.contains('\'')
        && !token.contains('"')
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_tool_substitution() {
        let subs = extract_substitutions("grep foo src/", "rg foo src/");
        assert_eq!(subs, vec![("grep".to_string(), "rg".to_string())]);
    }

    #[test]
    fn test_no_substitution_same_command() {
        let subs = extract_substitutions("ls -la", "ls -la");
        assert!(subs.is_empty());
    }

    #[test]
    fn test_no_substitution_flag_change() {
        // Changing a flag should not be recorded as a tool substitution
        let subs = extract_substitutions("ls -l", "ls -la");
        assert!(subs.is_empty());
    }

    #[test]
    fn test_no_substitution_path_arg() {
        // Paths are not tool names
        let subs = extract_substitutions("cat /etc/hosts", "bat /etc/hosts");
        assert_eq!(subs, vec![("cat".to_string(), "bat".to_string())]);
    }

    #[test]
    fn test_is_tool_name_basic() {
        assert!(is_tool_name("grep"));
        assert!(is_tool_name("rg"));
        assert!(is_tool_name("fd"));
        assert!(!is_tool_name("-l"));
        assert!(!is_tool_name("/usr/bin/grep"));
        assert!(!is_tool_name("./script.sh"));
        assert!(!is_tool_name("$HOME"));
        assert!(!is_tool_name("KEY=value"));
    }

    #[test]
    fn test_record_increments_existing() {
        let mut prefs = LearnedPrefs::default();
        prefs.record("grep", "rg");
        prefs.record("grep", "rg");
        assert_eq!(prefs.substitutions.len(), 1);
        assert_eq!(prefs.substitutions[0].uses, 2);
    }

    #[test]
    fn test_context_hint_empty() {
        let prefs = LearnedPrefs::default();
        assert!(prefs.to_context_hint().is_empty());
    }

    #[test]
    fn test_context_hint_with_subs() {
        let mut prefs = LearnedPrefs::default();
        prefs.record("grep", "rg");
        let hint = prefs.to_context_hint();
        assert!(hint.contains("rg"));
        assert!(hint.contains("grep"));
    }
}
