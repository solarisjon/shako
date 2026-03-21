use std::env;
use std::fs;

use strsim::damerau_levenshtein;
use which::which;

use crate::builtins::BUILTINS;

/// How user input was classified.
#[derive(Debug)]
pub enum Classification {
    /// Resolves to an executable in $PATH — run directly.
    Command(String),
    /// Shell builtin (cd, exit, export, etc.).
    Builtin(String),
    /// Natural language — route to AI.
    NaturalLanguage(String),
    /// Explicit AI request (prefixed with `?` or `ai:`).
    ForcedAI(String),
    /// Likely typo — suggested correction.
    Typo { suggestion: String },
    /// Empty input.
    Empty,
}

pub struct Classifier {
    path_commands: Vec<String>,
}

impl Classifier {
    pub fn new() -> Self {
        Self {
            path_commands: collect_path_commands(),
        }
    }

    /// Classify user input into command, builtin, typo, or natural language.
    ///
    /// Strategy:
    /// 1. Strip and check for empty input
    /// 2. Check for forced-AI sigils (`?` or `ai:` prefix)
    /// 3. Extract first token
    /// 4. Check if first token is a builtin
    /// 5. Check if first token resolves to a binary in $PATH
    /// 6. Check if first token is a typo (close to a known command)
    /// 7. Otherwise → natural language (AI)
    pub fn classify(&self, input: &str) -> Classification {
        let trimmed = input.trim();

        if trimmed.is_empty() {
            return Classification::Empty;
        }

        // Forced AI mode: `? query` or `ai: query`
        if let Some(rest) = trimmed.strip_prefix("? ") {
            return Classification::ForcedAI(rest.to_string());
        }
        if let Some(rest) = trimmed.strip_prefix("ai:") {
            return Classification::ForcedAI(rest.trim().to_string());
        }

        // Single `?` with no space = also forced AI for everything after
        if trimmed.starts_with('?') && trimmed.len() > 1 {
            return Classification::ForcedAI(trimmed[1..].trim().to_string());
        }

        // Extract first token
        let first_token = trimmed.split_whitespace().next().unwrap_or("");

        // Check builtins
        if BUILTINS.contains(&first_token) {
            return Classification::Builtin(trimmed.to_string());
        }

        // Check if first token starts with `.` or `/` (explicit path)
        if first_token.starts_with('/') || first_token.starts_with("./") {
            return Classification::Command(trimmed.to_string());
        }

        // Check if first token resolves to a binary in $PATH
        if which(first_token).is_ok() {
            return Classification::Command(trimmed.to_string());
        }

        // Typo detection: only for short inputs that look like command attempts
        // (not full sentences). 3+ words is likely natural language.
        let word_count = trimmed.split_whitespace().count();
        if word_count <= 3 {
            if let Some(suggestion) = self.find_typo_match(first_token) {
                let corrected = if trimmed.len() > first_token.len() {
                    format!("{}{}", suggestion, &trimmed[first_token.len()..])
                } else {
                    suggestion.clone()
                };
                return Classification::Typo {
                    suggestion: corrected,
                };
            }
        }

        // Fallback: natural language → AI
        Classification::NaturalLanguage(trimmed.to_string())
    }

    /// Find the closest matching command within edit distance 2.
    fn find_typo_match(&self, token: &str) -> Option<String> {
        // Only try typo correction on short-ish tokens that look like commands
        if token.len() < 2 || token.len() > 20 || token.contains(' ') {
            return None;
        }

        let mut best: Option<(String, usize)> = None;

        // Check builtins
        for &builtin in BUILTINS {
            let dist = damerau_levenshtein(token, builtin);
            if dist > 0 && dist <= 2 {
                if best.as_ref().is_none_or(|(_, d)| dist < *d) {
                    best = Some((builtin.to_string(), dist));
                }
            }
        }

        // Check PATH commands
        for cmd in &self.path_commands {
            let dist = damerau_levenshtein(token, cmd);
            if dist > 0 && dist <= 2 {
                if best.as_ref().is_none_or(|(_, d)| dist < *d) {
                    best = Some((cmd.clone(), dist));
                }
            }
        }

        best.map(|(cmd, _)| cmd)
    }
}

/// Collect all executable names from $PATH (cached at startup).
fn collect_path_commands() -> Vec<String> {
    let path_var = env::var("PATH").unwrap_or_default();
    let mut commands = Vec::new();

    for dir in path_var.split(':') {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Ok(name) = entry.file_name().into_string() {
                    commands.push(name);
                }
            }
        }
    }

    commands.sort();
    commands.dedup();
    commands
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_input() {
        let c = Classifier::new();
        assert!(matches!(c.classify(""), Classification::Empty));
        assert!(matches!(c.classify("   "), Classification::Empty));
    }

    #[test]
    fn test_forced_ai() {
        let c = Classifier::new();
        assert!(matches!(
            c.classify("? list all files"),
            Classification::ForcedAI(_)
        ));
        assert!(matches!(
            c.classify("ai: what does ls do"),
            Classification::ForcedAI(_)
        ));
    }

    #[test]
    fn test_builtins() {
        let c = Classifier::new();
        assert!(matches!(c.classify("cd /tmp"), Classification::Builtin(_)));
        assert!(matches!(c.classify("exit"), Classification::Builtin(_)));
        assert!(matches!(
            c.classify("export FOO=bar"),
            Classification::Builtin(_)
        ));
    }

    #[test]
    fn test_known_commands() {
        let c = Classifier::new();
        // ls should be in PATH on any system
        assert!(matches!(c.classify("ls -la"), Classification::Command(_)));
    }

    #[test]
    fn test_natural_language() {
        let c = Classifier::new();
        // Multi-word phrases that don't look like commands
        assert!(matches!(
            c.classify("show me the largest files in this directory"),
            Classification::NaturalLanguage(_)
        ));
    }

    #[test]
    fn test_typo_detection() {
        let c = Classifier::new();
        // "gti" is 1 transposition from "git"
        let result = c.classify("gti");
        assert!(
            matches!(result, Classification::Typo { ref suggestion, .. } if suggestion == "git"),
            "expected Typo with suggestion 'git', got {:?}",
            result
        );
    }

    #[test]
    fn test_typo_preserves_args() {
        let c = Classifier::new();
        let result = c.classify("gti status");
        assert!(
            matches!(result, Classification::Typo { ref suggestion, .. } if suggestion == "git status"),
            "expected Typo with suggestion 'git status', got {:?}",
            result
        );
    }
}
