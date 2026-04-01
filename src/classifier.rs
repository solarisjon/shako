//! Input classification — the first stage of shako's dispatch pipeline.
//!
//! Every line of user input passes through [`Classifier::classify`], which
//! decides whether to execute it as a shell command, route it to the AI
//! engine, handle it as a builtin, suggest a typo correction, or present an
//! explain-mode response.  Nothing else in shako calls the executor or AI
//! layer directly without first going through this module.

use std::sync::Arc;

use strsim::damerau_levenshtein;
use which::which;

use crate::builtins::BUILTINS;
use crate::path_cache::PathCache;

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
    /// AI-powered semantic history search (prefixed with `??`).
    HistorySearch(String),
    /// Likely typo — suggested correction.
    Typo { suggestion: String },
    /// Command ending with `?` — explain what it does without executing.
    ExplainCommand(String),
    /// Slash command — internal shako meta-command (e.g. `/validate`, `/help`).
    SlashCommand { name: String, args: String },
    /// Empty input.
    Empty,
}

/// Classifies raw user input into the appropriate dispatch category.
///
/// The classifier is the first stage of the shako dispatch pipeline. It
/// examines each line of user input and decides whether to run it as a shell
/// command, route it to the AI engine, trigger a builtin, or suggest a typo
/// correction.
pub struct Classifier {
    cache: Arc<PathCache>,
}

impl Classifier {
    /// Create a new `Classifier` backed by the given [`PathCache`].
    pub fn new(cache: Arc<PathCache>) -> Self {
        Self { cache }
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

        // AI history search: `??query` or `?? query`
        // Must be checked before the single-`?` catch-all so that `??foo`
        // routes to HistorySearch rather than ForcedAI("?foo").
        if let Some(rest) = trimmed.strip_prefix("??") {
            return Classification::HistorySearch(rest.trim().to_string());
        }

        // Single `?` with no space = also forced AI for everything after
        if trimmed.starts_with('?') && trimmed.len() > 1 {
            return Classification::ForcedAI(trimmed[1..].trim().to_string());
        }

        // Trailing `?` on a command — explain it without executing.
        // e.g. `git rebase -i?` or `tar xzf?` or `chmod 755?`
        if trimmed.ends_with('?') && trimmed.len() > 1 {
            let cmd = trimmed.trim_end_matches('?').trim();
            if !cmd.is_empty() {
                return Classification::ExplainCommand(cmd.to_string());
            }
        }

        // Slash commands: `/word` where word is alphabetic (not a filesystem path).
        // e.g. `/validate`, `/help`, `/config` — but not `/usr/bin/ls`.
        if let Some(rest) = trimmed.strip_prefix('/') {
            let cmd_part = rest.split_whitespace().next().unwrap_or("");
            if !cmd_part.is_empty()
                && cmd_part
                    .chars()
                    .all(|c| c.is_ascii_alphabetic() || c == '-' || c == '_')
            {
                let args = rest[cmd_part.len()..].trim().to_string();
                return Classification::SlashCommand {
                    name: cmd_part.to_string(),
                    args,
                };
            }
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

        // Check if first token resolves to a binary in $PATH.
        // Even if it does, the rest of the input might be prose rather than
        // shell arguments (e.g. "find all the .md files in this directory").
        // Collect args once and reuse for both the PATH and typo checks below.
        let args_after_first: Vec<&str> = trimmed.split_whitespace().skip(1).collect();
        if which(first_token).is_ok() {
            if looks_like_natural_language(&args_after_first) {
                return Classification::NaturalLanguage(trimmed.to_string());
            }
            return Classification::Command(trimmed.to_string());
        }

        // Typo detection: only for short inputs that look like command attempts
        // (not full sentences). 3+ words is likely natural language, and we also
        // check if the args read as prose (e.g. "list all files" has "all" in NL_WORDS).
        let word_count = args_after_first.len() + 1;
        if word_count <= 2 || (word_count == 3 && !looks_like_natural_language(&args_after_first)) {
            if let Some(suggestion) = self.find_typo_match(first_token) {
                // first_token is a sub-slice of trimmed; use pointer arithmetic
                // for its byte-end so the slice is valid with multi-byte chars.
                let token_end =
                    first_token.as_ptr() as usize - trimmed.as_ptr() as usize + first_token.len();
                let rest = &trimmed[token_end..];
                let corrected = if !rest.is_empty() {
                    format!("{suggestion}{rest}")
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
            if dist > 0 && dist <= 2 && best.as_ref().is_none_or(|(_, d)| dist < *d) {
                best = Some((builtin.to_string(), dist));
            }
        }

        // Check PATH commands
        for cmd in &self.cache.commands {
            let dist = damerau_levenshtein(token, cmd);
            if dist > 0 && dist <= 2 && best.as_ref().is_none_or(|(_, d)| dist < *d) {
                best = Some((cmd.clone(), dist));
            }
        }

        best.map(|(cmd, _)| cmd)
    }
}

/// Returns true if the argument list looks like English prose rather than shell arguments.
///
/// Rules:
///   - If any arg is a common prose word → natural language (takes priority)
///   - Any arg starts with `-`           → flags present → real command
///   - Any arg is a clear path (absolute, relative, or multi-segment tilde) → real command
///   - Simple `~/dir` (one slash) is ambiguous and does NOT signal a real command,
///     so that "find me files in ~/Documents" routes to AI correctly.
fn looks_like_natural_language(args: &[&str]) -> bool {
    // Need at least two args for this heuristic to be reliable.
    if args.len() < 2 {
        return false;
    }

    // Common words that appear in English prose but never as shell arguments.
    const NL_WORDS: &[&str] = &[
        "the",
        "a",
        "an",
        "all",
        "every",
        "each",
        "any",
        "some",
        "in",
        "on",
        "at",
        "to",
        "for",
        "of",
        "by",
        "with",
        "from",
        "into",
        "this",
        "that",
        "these",
        "those",
        "my",
        "me",
        "i",
        "file",
        "files",
        "directory",
        "folder",
        "folders",
        "which",
        "what",
        "how",
        "where",
        "when",
        "are",
        "is",
        "was",
        "were",
        "be",
        "been",
        "have",
        "has",
        "over",
        "under",
        "above",
        "below",
        "than",
        "between",
        "larger",
        "smaller",
        "bigger",
        "size",
        "sized",
        "modified",
        "created",
        "changed",
        "named",
        "called",
        "today",
        "yesterday",
        "recent",
        "latest",
        "largest",
        "smallest",
        "biggest",
        "newest",
        "oldest",
    ];

    // NL words take priority: prose words in the args mean it's natural language
    // even if a path like ~/Documents is also present.
    if args
        .iter()
        .any(|a| NL_WORDS.iter().any(|w| a.eq_ignore_ascii_case(w)))
    {
        return true;
    }

    // Flags → real shell invocation.
    if args.iter().any(|a| a.starts_with('-')) {
        return false;
    }

    // Clear path indicators → real shell invocation.
    // Exclude simple ~/dir (one slash) which is ambiguous in prose queries.
    if args.iter().any(|a| {
        *a == ".."
            || a.starts_with('/')        // absolute path
            || a.starts_with("./")       // explicit relative path
            || a.starts_with("../")      // parent-relative path
            || (a.contains('/') && !a.starts_with("~/"))  // embedded slash (not tilde-home)
            || (a.starts_with("~/") && a[2..].contains('/')) // multi-segment tilde: ~/a/b
    }) {
        return false;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_classifier() -> Classifier {
        Classifier::new(PathCache::new())
    }

    #[test]
    fn test_empty_input() {
        let c = test_classifier();
        assert!(matches!(c.classify(""), Classification::Empty));
        assert!(matches!(c.classify("   "), Classification::Empty));
    }

    #[test]
    fn test_forced_ai() {
        let c = test_classifier();
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
    fn test_history_search() {
        let c = test_classifier();
        assert!(matches!(
            c.classify("?? find files by size"),
            Classification::HistorySearch(_)
        ));
        let Classification::HistorySearch(query) = c.classify("?? git commands") else {
            panic!("expected HistorySearch");
        };
        assert_eq!(query, "git commands");
    }

    #[test]
    fn test_history_search_no_space() {
        // `??query` (no space after ??) must route to HistorySearch, not ForcedAI.
        let c = test_classifier();
        let result = c.classify("??git log");
        assert!(
            matches!(result, Classification::HistorySearch(ref q) if q == "git log"),
            "expected HistorySearch('git log'), got {result:?}",
        );
    }

    #[test]
    fn test_builtins() {
        let c = test_classifier();
        assert!(matches!(c.classify("cd /tmp"), Classification::Builtin(_)));
        assert!(matches!(c.classify("exit"), Classification::Builtin(_)));
        assert!(matches!(
            c.classify("export FOO=bar"),
            Classification::Builtin(_)
        ));
    }

    #[test]
    fn test_known_commands() {
        let c = test_classifier();
        // ls should be in PATH on any system
        assert!(matches!(c.classify("ls -la"), Classification::Command(_)));
    }

    #[test]
    fn test_natural_language() {
        let c = test_classifier();
        assert!(matches!(
            c.classify("show me the largest files in this directory"),
            Classification::NaturalLanguage(_)
        ));
    }

    #[test]
    fn test_known_command_with_nl_args_routes_to_ai() {
        let c = test_classifier();
        // "find" is in PATH but the rest is prose — should go to AI.
        assert!(matches!(
            c.classify("find all the .md files in this directory"),
            Classification::NaturalLanguage(_)
        ));
        // "ls" with prose args.
        assert!(matches!(
            c.classify("ls all the files modified today"),
            Classification::NaturalLanguage(_)
        ));
        // Tilde path should NOT block NL detection when prose words are present.
        assert!(matches!(
            c.classify("find me files in ~/Documents that are over 41GB"),
            Classification::NaturalLanguage(_)
        ));
        // Size queries.
        assert!(matches!(
            c.classify("find files over 100mb in ~/Downloads"),
            Classification::NaturalLanguage(_)
        ));
    }

    #[test]
    fn test_known_command_with_real_args_stays_command() {
        let c = test_classifier();
        // Flags present → real command.
        assert!(matches!(
            c.classify("find . -name '*.md'"),
            Classification::Command(_)
        ));
        assert!(matches!(
            c.classify("ls -la /tmp"),
            Classification::Command(_)
        ));
        // Path arg → real command.
        assert!(matches!(
            c.classify("cat README.md"),
            Classification::Command(_)
        ));
    }

    #[test]
    fn test_typo_detection() {
        let c = test_classifier();
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
        let c = test_classifier();
        let result = c.classify("gti status");
        assert!(
            matches!(result, Classification::Typo { ref suggestion, .. } if suggestion == "git status"),
            "expected Typo with suggestion 'git status', got {:?}",
            result
        );
    }

    #[test]
    fn test_3word_nl_not_typo() {
        // "list all files" — "list" is 1 edit from "klist" but the args look like
        // natural language, so it must route to NaturalLanguage, not Typo.
        let c = test_classifier();
        let result = c.classify("list all files");
        assert!(
            matches!(result, Classification::NaturalLanguage(_)),
            "expected NaturalLanguage for 'list all files', got {:?}",
            result
        );
    }

    #[test]
    fn test_slash_command_basic() {
        let c = test_classifier();
        let result = c.classify("/help");
        assert!(
            matches!(result, Classification::SlashCommand { ref name, .. } if name == "help"),
            "expected SlashCommand 'help', got {:?}",
            result
        );
    }

    #[test]
    fn test_slash_command_with_args() {
        let c = test_classifier();
        let result = c.classify("/safety warn");
        match result {
            Classification::SlashCommand { name, args } => {
                assert_eq!(name, "safety");
                assert_eq!(args, "warn");
            }
            other => panic!("expected SlashCommand, got {:?}", other),
        }
    }

    #[test]
    fn test_slash_command_with_hyphen() {
        let c = test_classifier();
        let result = c.classify("/my-command");
        assert!(
            matches!(result, Classification::SlashCommand { ref name, .. } if name == "my-command"),
            "expected SlashCommand 'my-command', got {:?}",
            result
        );
    }

    #[test]
    fn test_absolute_path_not_slash_command() {
        let c = test_classifier();
        let result = c.classify("/usr/bin/ls");
        assert!(
            matches!(result, Classification::Command(_)),
            "expected Command for absolute path, got {:?}",
            result
        );
    }

    #[test]
    fn test_slash_root_not_slash_command() {
        let c = test_classifier();
        let result = c.classify("/bin/echo hello");
        assert!(
            matches!(result, Classification::Command(_)),
            "expected Command for /bin/echo, got {:?}",
            result
        );
    }

    // ── ExplainCommand edge cases ──────────────────────────────────────────────

    #[test]
    fn test_explain_command_trailing_question() {
        let c = test_classifier();
        let result = c.classify("grep?");
        assert!(
            matches!(result, Classification::ExplainCommand(ref s) if s == "grep"),
            "expected ExplainCommand('grep'), got {:?}",
            result
        );
    }

    #[test]
    fn test_explain_flags_trailing_question() {
        let c = test_classifier();
        let result = c.classify("grep -rn?");
        assert!(
            matches!(result, Classification::ExplainCommand(ref s) if s == "grep -rn"),
            "expected ExplainCommand('grep -rn'), got {:?}",
            result
        );
    }

    // ── AI vs shell boundary ───────────────────────────────────────────────────

    #[test]
    fn test_command_with_flag_stays_command() {
        let c = test_classifier();
        // A known command followed only by flags must stay Command, not NaturalLanguage.
        assert!(matches!(
            c.classify("echo -n hello"),
            Classification::Command(_) | Classification::Builtin(_)
        ));
    }

    #[test]
    fn test_ai_prefix_variants() {
        let c = test_classifier();
        // Both "?" and "ai:" prefixes must force AI.
        assert!(matches!(
            c.classify("ai: list running processes"),
            Classification::ForcedAI(_)
        ));
        assert!(matches!(
            c.classify("? show git log"),
            Classification::ForcedAI(_)
        ));
    }

    #[test]
    fn test_builtin_cd_always_builtin() {
        let c = test_classifier();
        // cd must never fall through to Command even if a 'cd' binary were on PATH.
        assert!(matches!(c.classify("cd ~"), Classification::Builtin(_)));
    }

    #[test]
    fn test_history_search_strips_prefix() {
        let c = test_classifier();
        // `?? docker commands` → query should be "docker commands" not "?? docker commands".
        let Classification::HistorySearch(query) = c.classify("?? docker commands") else {
            panic!("expected HistorySearch");
        };
        assert_eq!(query, "docker commands");
    }

    #[test]
    fn test_empty_whitespace_only_is_empty() {
        let c = test_classifier();
        assert!(matches!(c.classify("\t  \n"), Classification::Empty));
    }

    // ── AI vs shell boundary: additional edge cases ───────────────────────────

    #[test]
    fn test_explain_bare_command_name() {
        // `? grep` with a bare known command name should force AI (explain mode).
        let c = test_classifier();
        assert!(matches!(c.classify("? grep"), Classification::ForcedAI(_)));
    }

    #[test]
    fn test_command_with_only_path_arg_stays_command() {
        // Known command + a path argument (no prose) → Command.
        let c = test_classifier();
        assert!(matches!(
            c.classify("cat /etc/hosts"),
            Classification::Command(_)
        ));
    }

    #[test]
    fn test_mixed_flags_and_prose_routes_to_ai() {
        // A command with a mix of flags and prose words should go to AI.
        let c = test_classifier();
        assert!(matches!(
            c.classify("find all python files modified today"),
            Classification::NaturalLanguage(_)
        ));
    }

    #[test]
    fn test_numeric_args_do_not_trigger_nl() {
        // Numeric args are never prose — command should stay Command.
        let c = test_classifier();
        assert!(matches!(
            c.classify("sleep 5"),
            Classification::Command(_) | Classification::Builtin(_)
        ));
    }

    #[test]
    fn test_trailing_question_on_flags_is_explain() {
        // A flag string followed by `?` should produce ExplainCommand.
        let c = test_classifier();
        let result = c.classify("ls -la?");
        assert!(
            matches!(result, Classification::ExplainCommand(ref s) if s == "ls -la"),
            "expected ExplainCommand('ls -la'), got {result:?}"
        );
    }

    #[test]
    fn test_history_search_with_empty_query() {
        // `??` with nothing after it — either Empty or HistorySearch with empty query.
        let c = test_classifier();
        let result = c.classify("??");
        // Should not crash and must not misclassify as a normal command.
        assert!(!matches!(result, Classification::Command(_)));
    }

    #[test]
    fn test_double_question_prefix_with_whitespace_only() {
        // `??   ` (only spaces after ??) should not produce a non-empty query.
        let c = test_classifier();
        let result = c.classify("??   ");
        match result {
            Classification::HistorySearch(q) => {
                assert!(q.trim().is_empty(), "expected empty query, got {q:?}")
            }
            Classification::Empty => {}
            other => panic!("unexpected classification: {other:?}"),
        }
    }

    #[test]
    fn test_ai_prefix_colon_with_prose() {
        let c = test_classifier();
        // `ai:` prefix must force AI regardless of how command-like the rest looks.
        assert!(matches!(c.classify("ai: ls"), Classification::ForcedAI(_)));
    }

    #[test]
    fn test_nl_detection_single_prose_word_unknown_command() {
        // A single unknown word that is not close to any known command → NL.
        let c = test_classifier();
        let result = c.classify("xyzzy");
        // Should be NaturalLanguage (unknown) or Typo — never Command.
        assert!(!matches!(result, Classification::Command(_)));
    }
}
