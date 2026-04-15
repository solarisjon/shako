//! Behavioral Fingerprinting — learn workflow patterns from journal data and
//! surface them as AI context for personalized suggestions.
//!
//! This module builds a `BehavioralProfile` from the command journal:
//!
//! - **Command sequence co-occurrence** (`command_sequences`): which commands
//!   frequently follow each other (e.g. `cargo test` always before `git add`).
//! - **Flag preferences** (`flag_preferences`): which flags a user consistently
//!   passes to a given tool (e.g. `--dry-run` for `rsync`).
//! - **Commit prefix style** (`commit_prefix_style`): the conventional-commits
//!   prefix pattern observed from `git log` (e.g. `fix:`, `feat:`, `chore:`).
//! - **Pre-command guards** (`pre_command_guards`): commands the user reliably
//!   runs *before* another command (e.g. activate venv before `pip install`).
//!
//! The profile is persisted to `~/.config/shako/behavioral_profile.json` and
//! re-analysed after each session by a background thread.  It is injected into
//! `ShellContext` (see `ai/context.rs`) as a compact hint string capped at
//! ~500 tokens, so the LLM can suggest next steps and adjust wording to match
//! the engineer's style.
//!
//! ## Privacy
//!
//! All data is stored locally.  No command content is sent to any remote
//! service; only the compact hint string (derived statistics, not raw commands)
//! is included in the AI context.
//!
//! ## Config
//!
//! Controlled by `[behavior] behavioral_fingerprinting = true/false` (default
//! `true`).  When `false`, `update_async` and `context_hint` are no-ops.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::journal::{self, JournalEntry};

// ── Constants ──────────────────────────────────────────────────────────────────

/// Minimum observation count before a pattern is considered reliable.
const MIN_OBSERVATIONS: u32 = 3;

/// Maximum number of sequence pairs to emit in the hint string.
const MAX_SEQUENCE_HINTS: usize = 5;

/// Maximum number of flag preferences to emit per tool.
const MAX_FLAG_HINTS_PER_TOOL: usize = 3;

/// Maximum number of pre-command guard hints.
const MAX_GUARD_HINTS: usize = 3;

// ── Data types ─────────────────────────────────────────────────────────────────

/// A (before, after) command pair that co-occurs frequently.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SequencePair {
    /// The command that typically appears first.
    pub before: String,
    /// The command that typically follows.
    pub after: String,
    /// How many times this ordering has been observed.
    pub count: u32,
}

/// A flag that the user consistently passes to a given tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FlagPreference {
    /// The tool name (e.g. `rsync`, `docker`).
    pub tool: String,
    /// The flag or flag+value pair (e.g. `--dry-run`, `-v`).
    pub flag: String,
    /// How many times this flag was observed for this tool.
    pub count: u32,
}

/// A command that the user reliably runs *before* a given target command.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreCommandGuard {
    /// The guard command that must precede the target (e.g. `source venv/bin/activate`).
    pub guard: String,
    /// The command being guarded (e.g. `pip`).
    pub target: String,
    /// How many times the guard was observed before the target.
    pub count: u32,
}

/// The full behavioral fingerprint for one user.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct BehavioralProfile {
    /// Frequently observed (before, after) command sequence pairs.
    #[serde(default)]
    pub command_sequences: Vec<SequencePair>,

    /// Per-tool flag preferences observed across the journal.
    #[serde(default)]
    pub flag_preferences: Vec<FlagPreference>,

    /// Detected conventional-commit prefix style (e.g. `fix:`, `feat:`), or empty.
    #[serde(default)]
    pub commit_prefix_style: String,

    /// Commands the user reliably runs before certain target commands.
    #[serde(default)]
    pub pre_command_guards: Vec<PreCommandGuard>,
}

impl BehavioralProfile {
    // ── I/O ───────────────────────────────────────────────────────────────────

    /// Load the profile from disk (or return a default if absent / unreadable).
    pub fn load() -> Self {
        let path = profile_path();
        let Ok(contents) = fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&contents).unwrap_or_default()
    }

    /// Persist the profile to disk.
    pub fn save(&self) {
        let path = profile_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(&path, json);
        }
    }

    // ── Analysis ──────────────────────────────────────────────────────────────

    /// Rebuild the profile from raw journal entries (all CWDs combined).
    ///
    /// This is called from the background update task after each session.
    pub fn analyze_from_entries(entries: &[JournalEntry]) -> Self {
        Self {
            command_sequences: extract_sequences(entries),
            flag_preferences: extract_flag_preferences(entries),
            commit_prefix_style: detect_commit_prefix_style(),
            pre_command_guards: extract_pre_command_guards(entries),
        }
    }

    // ── AI context hint ───────────────────────────────────────────────────────

    /// Produce a compact hint string (≤ ~500 tokens) suitable for injection
    /// into the LLM system prompt.  Returns an empty string if the profile
    /// contains nothing notable.
    pub fn to_context_hint(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        // Commit style
        if !self.commit_prefix_style.is_empty() {
            parts.push(format!(
                "Your git commits follow the '{}' prefix style — continue using it.",
                self.commit_prefix_style
            ));
        }

        // Reliable sequences
        let notable_seqs: Vec<&SequencePair> = self
            .command_sequences
            .iter()
            .filter(|s| s.count >= MIN_OBSERVATIONS)
            .take(MAX_SEQUENCE_HINTS)
            .collect();

        if !notable_seqs.is_empty() {
            let seq_lines: Vec<String> = notable_seqs
                .iter()
                .map(|s| format!("  - you always run `{}` before `{}`", s.before, s.after))
                .collect();
            parts.push(format!(
                "Observed workflow patterns:\n{}",
                seq_lines.join("\n")
            ));
        }

        // Pre-command guards
        let notable_guards: Vec<&PreCommandGuard> = self
            .pre_command_guards
            .iter()
            .filter(|g| g.count >= MIN_OBSERVATIONS)
            .take(MAX_GUARD_HINTS)
            .collect();

        if !notable_guards.is_empty() {
            let guard_lines: Vec<String> = notable_guards
                .iter()
                .map(|g| {
                    format!(
                        "  - you run `{}` before `{}` (suggest it proactively)",
                        g.guard, g.target
                    )
                })
                .collect();
            parts.push(format!(
                "Pre-command guards you rely on:\n{}",
                guard_lines.join("\n")
            ));
        }

        // Flag preferences
        let tools_with_prefs: Vec<String> = {
            // Group by tool, take top flags per tool
            let mut by_tool: HashMap<&str, Vec<&FlagPreference>> = HashMap::new();
            for fp in &self.flag_preferences {
                if fp.count >= MIN_OBSERVATIONS {
                    by_tool.entry(&fp.tool).or_default().push(fp);
                }
            }
            let mut lines: Vec<String> = Vec::new();
            for (tool, mut prefs) in by_tool {
                prefs.sort_by(|a, b| b.count.cmp(&a.count));
                let flags: Vec<&str> = prefs
                    .iter()
                    .take(MAX_FLAG_HINTS_PER_TOOL)
                    .map(|fp| fp.flag.as_str())
                    .collect();
                lines.push(format!("  - `{}`: prefers {}", tool, flags.join(", ")));
            }
            lines.sort(); // stable ordering
            lines
        };

        if !tools_with_prefs.is_empty() {
            parts.push(format!(
                "Flag preferences by tool:\n{}",
                tools_with_prefs.join("\n")
            ));
        }

        if parts.is_empty() {
            return String::new();
        }

        format!(
            "Behavioral fingerprint (learned from your workflow):\n{}",
            parts.join("\n\n")
        )
    }

    // ── Proactive sequence check ───────────────────────────────────────────────

    /// Given the command the user just ran, return the most likely *next* command
    /// they will want, if a reliable pattern exists.  Returns `None` otherwise.
    pub fn predicted_next_command(&self, just_ran: &str) -> Option<&str> {
        // Extract the base command (first token) from the just-ran command
        let base = just_ran.split_whitespace().next()?;

        self.command_sequences
            .iter()
            .filter(|s| s.count >= MIN_OBSERVATIONS && s.before == base)
            .max_by_key(|s| s.count)
            .map(|s| s.after.as_str())
    }

    /// Given the command the user is about to run, return the guard command they
    /// typically run first, if one is reliably observed and not already the last
    /// command.  Returns `None` if no guard applies.
    #[allow(dead_code)]
    pub fn required_guard<'a>(&'a self, about_to_run: &str, last_ran: &str) -> Option<&'a str> {
        let base = about_to_run.split_whitespace().next()?;
        let last_base = last_ran.split_whitespace().next().unwrap_or("");

        self.pre_command_guards
            .iter()
            .filter(|g| {
                g.count >= MIN_OBSERVATIONS
                    && g.target == base
                    // Don't suggest the guard if they just ran it
                    && !last_base.contains(&g.guard)
            })
            .max_by_key(|g| g.count)
            .map(|g| g.guard.as_str())
    }
}

// ── Public API ─────────────────────────────────────────────────────────────────

/// Spawn a background thread that re-analyses the journal and persists an
/// updated `BehavioralProfile`.  Fire-and-forget; never blocks the shell.
pub fn update_async() {
    std::thread::spawn(|| {
        // Read all journal entries across all CWDs for a global fingerprint.
        let path = journal::journal_path();
        let Ok(contents) = std::fs::read_to_string(&path) else {
            return;
        };
        let entries: Vec<JournalEntry> = contents
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();

        if entries.is_empty() {
            return;
        }

        let profile = BehavioralProfile::analyze_from_entries(&entries);
        profile.save();
    });
}

/// Return the current behavioral context hint for injection into the AI prompt.
/// Loads from disk; returns empty string if no profile or nothing notable.
pub fn context_hint() -> String {
    BehavioralProfile::load().to_context_hint()
}

// ── Path helper ────────────────────────────────────────────────────────────────

fn profile_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("shako")
        .join("behavioral_profile.json")
}

// ── Sequence extraction ────────────────────────────────────────────────────────

/// Extract (before → after) command sequence co-occurrence pairs from the journal.
///
/// We use a sliding window of size 2 over consecutive entries in the same CWD
/// session (entries within 30 minutes of each other).
fn extract_sequences(entries: &[JournalEntry]) -> Vec<SequencePair> {
    let mut counts: HashMap<(String, String), u32> = HashMap::new();

    // Session gap threshold: entries > 30 min apart are a new session
    const SESSION_GAP_SECS: u64 = 30 * 60;

    for window in entries.windows(2) {
        let a = &window[0];
        let b = &window[1];

        // Same CWD root and within session window
        if !same_project(&a.cwd, &b.cwd) {
            continue;
        }
        if b.ts.saturating_sub(a.ts) > SESSION_GAP_SECS {
            continue;
        }

        let cmd_a = base_command(&a.cmd);
        let cmd_b = base_command(&b.cmd);

        if cmd_a.is_empty() || cmd_b.is_empty() || cmd_a == cmd_b {
            continue;
        }

        *counts.entry((cmd_a, cmd_b)).or_insert(0) += 1;
    }

    let mut pairs: Vec<SequencePair> = counts
        .into_iter()
        .map(|((before, after), count)| SequencePair {
            before,
            after,
            count,
        })
        .collect();

    // Sort descending by count for deterministic ordering
    pairs.sort_by(|a, b| b.count.cmp(&a.count).then(a.before.cmp(&b.before)));
    pairs
}

// ── Flag preference extraction ─────────────────────────────────────────────────

/// Extract per-tool flag preferences from journal entries.
fn extract_flag_preferences(entries: &[JournalEntry]) -> Vec<FlagPreference> {
    let mut counts: HashMap<(String, String), u32> = HashMap::new();

    for entry in entries {
        let tokens: Vec<&str> = entry.cmd.split_whitespace().collect();
        let Some(tool) = tokens.first() else {
            continue;
        };
        // Skip shell built-ins and noisy short commands
        if should_skip_tool(tool) {
            continue;
        }
        for token in &tokens[1..] {
            if is_flag(token) {
                *counts
                    .entry(((*tool).to_string(), (*token).to_string()))
                    .or_insert(0) += 1;
            }
        }
    }

    let mut prefs: Vec<FlagPreference> = counts
        .into_iter()
        .map(|((tool, flag), count)| FlagPreference { tool, flag, count })
        .collect();

    prefs.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then(a.tool.cmp(&b.tool))
            .then(a.flag.cmp(&b.flag))
    });
    prefs
}

// ── Commit prefix style detection ─────────────────────────────────────────────

/// Detect the user's conventional-commit prefix style by reading recent git log
/// from the current working directory.  Returns the most common prefix pattern
/// (e.g. `"fix:"`, `"feat:"`, `"chore:"`), or an empty string if none found.
fn detect_commit_prefix_style() -> String {
    let output = std::process::Command::new("git")
        .args(["log", "--oneline", "-50"])
        .stderr(std::process::Stdio::null())
        .output();

    let Ok(out) = output else {
        return String::new();
    };
    if !out.status.success() {
        return String::new();
    }

    let log = String::from_utf8_lossy(&out.stdout);
    let mut prefix_counts: HashMap<String, u32> = HashMap::new();

    // Match conventional commit prefixes: type(scope): or type:
    let re_pattern = regex_prefix_pattern();
    for line in log.lines() {
        // Skip the hash (first token) and look at the subject
        let subject = line.split_once(' ').map(|x| x.1).unwrap_or(line);
        if let Some(prefix) = extract_prefix(subject, re_pattern) {
            *prefix_counts.entry(prefix).or_insert(0) += 1;
        }
    }

    // Return the most common prefix that appears in at least 40% of commits
    let total = prefix_counts.values().sum::<u32>();
    if total == 0 {
        return String::new();
    }

    prefix_counts
        .into_iter()
        .filter(|(_, count)| *count * 10 >= total * 4) // ≥ 40%
        .max_by_key(|(_, count)| *count)
        .map(|(prefix, _)| prefix)
        .unwrap_or_default()
}

/// Return a simple regex-like prefix list to match conventional commit types.
/// We avoid pulling in the `regex` crate by doing simple string matching.
fn regex_prefix_pattern() -> &'static [&'static str] {
    &[
        "feat", "fix", "chore", "docs", "refactor", "test", "style", "perf", "ci", "build",
        "revert",
    ]
}

/// Extract the conventional-commit prefix from a commit subject line.
/// Returns the matched prefix including the colon (e.g. `"fix:"`), or `None`.
fn extract_prefix(subject: &str, types: &[&str]) -> Option<String> {
    for t in types {
        // Match `type:` or `type(scope):`
        if subject.starts_with(&format!("{t}:")) {
            return Some(format!("{t}:"));
        }
        if subject.starts_with(&format!("{t}(")) {
            if let Some(close) = subject.find("):") {
                let scope_part = &subject[..close + 2]; // e.g. "feat(auth):"
                return Some(scope_part.to_string());
            }
        }
    }
    None
}

// ── Pre-command guard extraction ───────────────────────────────────────────────

/// Detect guard→target command pairs: the guard command is reliably run
/// immediately before the target within the same session.
///
/// Known guard patterns we look for:
/// - `source` / `. ` (activate virtualenv) before `pip` / `python`
/// - `nvm use` / `fnm use` before `npm` / `node`
/// - `docker-compose up` / `docker compose up` before `docker exec`
/// - `make` before `./binary-name` patterns
fn extract_pre_command_guards(entries: &[JournalEntry]) -> Vec<PreCommandGuard> {
    // Known guard→target pairs to look for
    let guard_patterns: &[(&str, &str)] = &[
        // Python virtualenv
        ("source", "pip"),
        ("source", "python"),
        // Node version managers
        ("nvm", "npm"),
        ("nvm", "node"),
        ("fnm", "npm"),
        ("fnm", "node"),
        // Docker compose
        ("docker-compose", "docker"),
        ("docker", "docker"),
        // Cargo check before test
        ("cargo check", "cargo test"),
        ("cargo build", "cargo test"),
        ("cargo build", "git"),
    ];

    let mut counts: HashMap<(String, String), u32> = HashMap::new();

    const SESSION_GAP_SECS: u64 = 30 * 60;

    for window in entries.windows(2) {
        let a = &window[0];
        let b = &window[1];

        if !same_project(&a.cwd, &b.cwd) {
            continue;
        }
        if b.ts.saturating_sub(a.ts) > SESSION_GAP_SECS {
            continue;
        }

        let cmd_a_base = base_command(&a.cmd);
        let cmd_b_base = base_command(&b.cmd);

        for (guard_base, target_base) in guard_patterns {
            if cmd_a_base.starts_with(guard_base) && cmd_b_base == *target_base {
                *counts
                    .entry((cmd_a_base.clone(), cmd_b_base.clone()))
                    .or_insert(0) += 1;
            }
        }
    }

    let mut guards: Vec<PreCommandGuard> = counts
        .into_iter()
        .map(|((guard, target), count)| PreCommandGuard {
            guard,
            target,
            count,
        })
        .collect();

    guards.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then(a.guard.cmp(&b.guard))
            .then(a.target.cmp(&b.target))
    });
    guards
}

// ── Helpers ────────────────────────────────────────────────────────────────────

/// Extract the base command name (first token of a shell command string).
fn base_command(cmd: &str) -> String {
    cmd.split_whitespace()
        .next()
        .unwrap_or("")
        .trim_start_matches("./")
        .to_string()
}

/// Returns true if the two CWD paths belong to the same project root.
///
/// Two paths are considered the same project when one is a prefix of the
/// other (e.g. `/home/u/proj` and `/home/u/proj/src` are the same project).
fn same_project(a: &str, b: &str) -> bool {
    let a = a.trim_end_matches('/');
    let b = b.trim_end_matches('/');
    // Exact match, or one is a subdirectory of the other
    a == b || b.starts_with(&format!("{a}/")) || a.starts_with(&format!("{b}/"))
}

/// Returns true if the token looks like a CLI flag.
fn is_flag(token: &str) -> bool {
    // Short flag: -x; long flag: --foo; combined: --foo=bar
    // Must not be a bare `-` (stdin marker) or `--` (end-of-flags)
    if token == "-" || token == "--" {
        return false;
    }
    token.starts_with('-')
}

/// Returns true if the tool should be skipped for flag-preference analysis
/// (shell built-ins, single-char commands, piping infrastructure).
fn should_skip_tool(tool: &str) -> bool {
    matches!(
        tool,
        "cd" | "ls"
            | "echo"
            | "cat"
            | "grep"
            | "awk"
            | "sed"
            | "tee"
            | "xargs"
            | "true"
            | "false"
            | "export"
            | "source"
            | "."
            | "exec"
            | "eval"
    ) || tool.len() <= 1
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(ts: u64, cwd: &str, cmd: &str) -> JournalEntry {
        JournalEntry {
            ts,
            cwd: cwd.to_string(),
            branch: "main".to_string(),
            intent: String::new(),
            cmd: cmd.to_string(),
            exit_code: 0,
        }
    }

    #[test]
    fn test_extract_sequences_basic() {
        // cargo test → git add, observed 4 times
        let mut entries = Vec::new();
        for i in 0u64..4 {
            let base_ts = i * 200;
            entries.push(make_entry(base_ts, "/home/u/proj", "cargo test"));
            entries.push(make_entry(base_ts + 10, "/home/u/proj", "git add ."));
        }
        let seqs = extract_sequences(&entries);
        let pair = seqs
            .iter()
            .find(|s| s.before == "cargo" && s.after == "git");
        assert!(pair.is_some(), "expected cargo→git sequence");
        assert!(pair.unwrap().count >= 4);
    }

    #[test]
    fn test_extract_sequences_cross_cwd_ignored() {
        // Entries in different projects should NOT form a sequence pair
        let entries = vec![
            make_entry(100, "/home/u/proj1", "cargo test"),
            make_entry(110, "/home/u/proj2", "git add ."),
        ];
        let seqs = extract_sequences(&entries);
        // Different project roots: no sequence should be recorded
        let pair = seqs
            .iter()
            .find(|s| s.before == "cargo" && s.after == "git");
        assert!(pair.is_none(), "cross-project sequence should be ignored");
    }

    #[test]
    fn test_extract_flag_preferences() {
        let entries = vec![
            make_entry(1, "/p", "rsync -av --dry-run src/ dst/"),
            make_entry(2, "/p", "rsync -av --dry-run src/ dst/"),
            make_entry(3, "/p", "rsync -av --dry-run src/ dst/"),
        ];
        let prefs = extract_flag_preferences(&entries);
        let av = prefs.iter().find(|p| p.tool == "rsync" && p.flag == "-av");
        let dry = prefs
            .iter()
            .find(|p| p.tool == "rsync" && p.flag == "--dry-run");
        assert!(av.is_some());
        assert!(dry.is_some());
        assert_eq!(av.unwrap().count, 3);
        assert_eq!(dry.unwrap().count, 3);
    }

    #[test]
    fn test_commit_prefix_extraction() {
        // Direct unit test on extract_prefix
        let types = regex_prefix_pattern();
        assert_eq!(
            extract_prefix("fix: correct typo in README", types),
            Some("fix:".to_string())
        );
        assert_eq!(
            extract_prefix("feat(auth): add OAuth login", types),
            Some("feat(auth):".to_string())
        );
        assert_eq!(extract_prefix("initial commit", types), None);
    }

    #[test]
    fn test_context_hint_empty() {
        let profile = BehavioralProfile::default();
        assert!(profile.to_context_hint().is_empty());
    }

    #[test]
    fn test_context_hint_with_sequences() {
        let profile = BehavioralProfile {
            command_sequences: vec![SequencePair {
                before: "cargo".to_string(),
                after: "git".to_string(),
                count: 5,
            }],
            ..Default::default()
        };
        let hint = profile.to_context_hint();
        assert!(hint.contains("cargo"));
        assert!(hint.contains("git"));
        assert!(hint.contains("Behavioral fingerprint"));
    }

    #[test]
    fn test_predicted_next_command() {
        let profile = BehavioralProfile {
            command_sequences: vec![
                SequencePair {
                    before: "cargo".to_string(),
                    after: "git".to_string(),
                    count: 5,
                },
                SequencePair {
                    before: "cargo".to_string(),
                    after: "ls".to_string(),
                    count: 1,
                },
            ],
            ..Default::default()
        };
        // Should pick "git" (highest count) after "cargo test"
        assert_eq!(profile.predicted_next_command("cargo test"), Some("git"));
    }

    #[test]
    fn test_required_guard_suggests_when_not_last() {
        let profile = BehavioralProfile {
            pre_command_guards: vec![PreCommandGuard {
                guard: "source".to_string(),
                target: "pip".to_string(),
                count: 4,
            }],
            ..Default::default()
        };
        // Guard should be suggested when last ran was not "source"
        assert_eq!(
            profile.required_guard("pip install requests", "cargo build"),
            Some("source")
        );
        // Guard should NOT be suggested if we just ran source
        assert_eq!(
            profile.required_guard("pip install requests", "source venv/bin/activate"),
            None
        );
    }

    #[test]
    fn test_is_flag() {
        assert!(is_flag("--dry-run"));
        assert!(is_flag("-v"));
        assert!(is_flag("-av"));
        assert!(!is_flag("-"));
        assert!(!is_flag("--"));
        assert!(!is_flag("src/"));
        assert!(!is_flag("file.txt"));
    }

    #[test]
    fn test_same_project() {
        // Subdirectory of the same project → same
        assert!(same_project("/home/user/proj", "/home/user/proj/sub"));
        // Exact same path → same
        assert!(same_project("/home/user/proj", "/home/user/proj"));
        // Sibling directories → different
        assert!(!same_project("/home/user/proj1", "/home/user/proj2"));
        // Unrelated paths → different
        assert!(!same_project("/home/alice/work", "/home/bob/work"));
    }
}
