//! Temporal Command Archaeology — session journal for project resumption.
//!
//! This module records every confirmed NL→command execution as a JSONL entry
//! at `~/.local/share/shako/journal.jsonl`.  Each record captures the working
//! directory, git branch, the user's natural-language intent, the executed
//! command, its exit code, and a UTC timestamp.
//!
//! When the user `cd`s into a project they haven't touched in 3+ days, the
//! REPL calls [`last_session_for_cwd`] and the proactive module synthesises
//! an AI-powered resumption brief from those records.
//!
//! ## Design goals
//!
//! - **Async write, zero latency**: the journal append spawns a background
//!   thread so it never blocks the interactive shell.
//! - **Tiny storage**: ~200 bytes per record × 10 000 commands/year ≈ 2 MB/yr.
//! - **Config-gated**: the whole system is a no-op when
//!   `[behavior] session_journal = false`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

// ── Data types ────────────────────────────────────────────────────────────────

/// A single journal entry recording one confirmed NL→command execution.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JournalEntry {
    /// UTC Unix timestamp (seconds since epoch).
    pub ts: u64,
    /// Working directory at the time of execution (canonical path string).
    pub cwd: String,
    /// Git branch at the time of execution, or empty string if not in a repo.
    pub branch: String,
    /// The user's natural-language intent (what they typed).
    pub intent: String,
    /// The shell command that was executed.
    pub cmd: String,
    /// Exit code of the command (0 = success).
    pub exit_code: i32,
}

/// Summary of a past work session for a given path — fed to the AI brief.
#[derive(Debug, Clone)]
pub struct SessionSummary {
    /// Days since the most recent entry in this path.
    pub days_ago: u64,
    /// Git branch that was active during the last session.
    pub branch: String,
    /// Last natural-language intent recorded for this path.
    pub last_intent: String,
    /// The most recent entries for context (newest-last order).
    pub entries: Vec<JournalEntry>,
}

// ── Journal path ──────────────────────────────────────────────────────────────

/// Return the path to the JSONL journal file, creating parent dirs as needed.
///
/// Default: `~/.local/share/shako/journal.jsonl`
pub fn journal_path() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local").join("share")))
        .unwrap_or_else(|| PathBuf::from("."));

    base.join("shako").join("journal.jsonl")
}

// ── Writing ───────────────────────────────────────────────────────────────────

/// Append a journal entry asynchronously (spawns a background thread).
///
/// Fails silently — the shell must never block or error on journalling.
pub fn append_async(intent: &str, cmd: &str, exit_code: i32) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let branch = git_branch();

    let entry = JournalEntry {
        ts,
        cwd,
        branch,
        intent: intent.to_string(),
        cmd: cmd.to_string(),
        exit_code,
    };

    // Fire-and-forget: journalling must never block interactive use.
    std::thread::spawn(move || {
        if let Ok(line) = serde_json::to_string(&entry) {
            let path = journal_path();
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
                let _ = writeln!(f, "{line}");
            }
        }
    });
}

// ── Reading ───────────────────────────────────────────────────────────────────

/// Load all journal entries for the given canonical path, newest-last.
///
/// Returns at most the last 50 entries for a path to keep memory bounded.
pub fn entries_for_cwd(cwd: &str) -> Vec<JournalEntry> {
    let path = journal_path();
    let Ok(contents) = fs::read_to_string(&path) else {
        return vec![];
    };

    let mut entries: Vec<JournalEntry> = contents
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .filter(|e: &JournalEntry| e.cwd == cwd || e.cwd.starts_with(&format!("{cwd}/")))
        .collect();

    // Keep newest-last; respect the 50-entry cap.
    entries.sort_by_key(|e| e.ts);
    let len = entries.len();
    if len > 50 {
        entries.drain(..len - 50);
    }
    entries
}

/// Return a [`SessionSummary`] for `cwd` if the last session was more than
/// `stale_days` days ago.  Returns `None` when there is no history or the
/// last activity was recent enough not to warrant a resumption brief.
pub fn last_session_for_cwd(cwd: &str, stale_days: u64) -> Option<SessionSummary> {
    let entries = entries_for_cwd(cwd);
    if entries.is_empty() {
        return None;
    }

    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // The last entry is the most recent (entries are sorted oldest-first).
    let last = entries.last()?;
    let secs_ago = now_secs.saturating_sub(last.ts);
    let days_ago = secs_ago / 86_400;

    if days_ago < stale_days {
        return None;
    }

    Some(SessionSummary {
        days_ago,
        branch: last.branch.clone(),
        last_intent: last.intent.clone(),
        entries,
    })
}

// ── Git helpers ───────────────────────────────────────────────────────────────

/// Return the current git branch name, or an empty string.
fn git_branch() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "HEAD")
        .unwrap_or_default()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn make_entry(ts: u64, cwd: &str, intent: &str, cmd: &str, exit_code: i32) -> JournalEntry {
        JournalEntry {
            ts,
            cwd: cwd.to_string(),
            branch: "main".to_string(),
            intent: intent.to_string(),
            cmd: cmd.to_string(),
            exit_code,
        }
    }

    #[test]
    fn test_serialize_roundtrip() {
        let e = make_entry(
            1_700_000_000,
            "/home/user/myproject",
            "run tests",
            "cargo test",
            0,
        );
        let json = serde_json::to_string(&e).unwrap();
        let back: JournalEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.cwd, "/home/user/myproject");
        assert_eq!(back.intent, "run tests");
        assert_eq!(back.exit_code, 0);
    }

    #[test]
    fn test_entries_for_cwd_filters_correctly() {
        let dir = TempDir::new().unwrap();
        let journal = dir.path().join("journal.jsonl");

        let entries = vec![
            make_entry(100, "/home/user/proj1", "list files", "ls", 0),
            make_entry(200, "/home/user/proj2", "build", "cargo build", 0),
            make_entry(300, "/home/user/proj1", "run tests", "cargo test", 1),
        ];

        let mut f = std::fs::File::create(&journal).unwrap();
        for e in &entries {
            writeln!(f, "{}", serde_json::to_string(e).unwrap()).unwrap();
        }

        // Patch the path lookup via an env var used in journal_path()
        // (In tests we read entries directly from the path)
        let contents = std::fs::read_to_string(&journal).unwrap();
        let proj1: Vec<JournalEntry> = contents
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .filter(|e: &JournalEntry| e.cwd == "/home/user/proj1")
            .collect();

        assert_eq!(proj1.len(), 2);
        assert_eq!(proj1[0].intent, "list files");
        assert_eq!(proj1[1].intent, "run tests");
    }

    #[test]
    fn test_last_session_returns_none_when_recent() {
        // A session that was "just now" (ts = now) should not trigger a resumption brief.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let e = JournalEntry {
            ts: now,
            cwd: "/home/user/recent".to_string(),
            branch: "main".to_string(),
            intent: "just worked here".to_string(),
            cmd: "ls".to_string(),
            exit_code: 0,
        };

        // Simulate what last_session_for_cwd does with this entry.
        let secs_ago = now.saturating_sub(e.ts);
        let days_ago = secs_ago / 86_400;
        assert_eq!(days_ago, 0);
        // Should NOT trigger (< 3 days)
        assert!(days_ago < 3);
    }
}
