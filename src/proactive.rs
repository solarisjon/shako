//! Proactive suggestions — offered automatically after certain commands succeed.
//!
//! Currently implemented:
//!   - After `git add`, offer an AI-generated commit message.

use std::io::{self, Write};
use std::process::Command;

use crate::ai;
use crate::config::ShakoConfig;
use crate::executor;

// ── Public entry point ────────────────────────────────────────────────────────

/// Called after every successful foreground command. Checks whether a
/// proactive suggestion is appropriate and, if so, offers it to the user.
pub fn check(cmd: &str, config: &ShakoConfig, rt: &tokio::runtime::Runtime) {
    if is_git_add(cmd) {
        offer_commit_suggestion(config, rt);
    } else if let Some(suggestion) = check_passive(cmd) {
        eprintln!("\x1b[90mshako: {suggestion}\x1b[0m");
    }
}

/// Check for lightweight passive suggestions that don't require user interaction.
/// Returns a formatted suggestion string or `None`.
fn check_passive(cmd: &str) -> Option<String> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let first = *parts.first()?;
    let second = parts.get(1).copied().unwrap_or("");

    // After `git clone <url>`, suggest `cd <repo-name>`
    if first == "git" && second == "clone" {
        let repo_name = extract_repo_name(parts.last()?)?;
        return Some(format!("tip: cd {repo_name}"));
    }

    // After successful `cd`, check for a Makefile
    if first == "cd" {
        let cwd = std::env::current_dir().ok()?;
        if cwd.join("Makefile").exists() {
            let targets = read_make_targets(&cwd.join("Makefile"));
            if !targets.is_empty() {
                let shown = targets[..targets.len().min(3)].join(", ");
                return Some(format!("make targets available: {shown}"));
            }
        }
    }

    None
}

// ── git add → commit message ──────────────────────────────────────────────────

/// Returns true for `git add <anything>` that actually stages files.
/// Excludes `git add --help`, `git add --version`, and bare `git add`.
fn is_git_add(cmd: &str) -> bool {
    let mut tokens = cmd.split_whitespace();
    match (tokens.next(), tokens.next(), tokens.next()) {
        (Some("git"), Some("add"), Some(arg)) => {
            !arg.starts_with("--help") && !arg.starts_with("--version")
        }
        _ => false,
    }
}

struct StagedInfo {
    /// `git diff --staged --stat` output (human-readable summary).
    stat: String,
    /// `git diff --staged` output, capped at 4 KB.
    diff: String,
    /// Number of changed files reported in the stat.
    file_count: usize,
}

/// Run `git diff --staged` and collect info about what's staged.
/// Returns `None` when not in a git repo, nothing is staged, or git fails.
fn get_staged_info() -> Option<StagedInfo> {
    let stat_out = Command::new("git")
        .args(["diff", "--staged", "--stat"])
        .output()
        .ok()?;

    if !stat_out.status.success() {
        return None;
    }

    let stat = String::from_utf8_lossy(&stat_out.stdout).to_string();
    if stat.trim().is_empty() {
        return None; // nothing staged
    }

    // Count lines that describe a changed file (contain "|" or "Bin")
    let file_count = stat
        .lines()
        .filter(|l| l.contains('|') || l.contains("Bin "))
        .count();

    // Get the actual diff for richer AI context, capped to keep prompt size sane
    let diff_out = Command::new("git")
        .args(["diff", "--staged"])
        .output()
        .ok()?;

    let full_diff = String::from_utf8_lossy(&diff_out.stdout).to_string();
    const DIFF_CAP: usize = 4_000;
    let diff = if full_diff.len() > DIFF_CAP {
        format!(
            "{}\n[...diff truncated at {DIFF_CAP} bytes...]",
            &full_diff[..DIFF_CAP]
        )
    } else {
        full_diff
    };

    Some(StagedInfo {
        stat,
        diff,
        file_count,
    })
}

/// Offer an AI-generated commit message to the user.
fn offer_commit_suggestion(config: &ShakoConfig, rt: &tokio::runtime::Runtime) {
    let Some(staged) = get_staged_info() else {
        return;
    };

    let file_word = if staged.file_count == 1 {
        "file"
    } else {
        "files"
    };
    print!(
        "\x1b[90mshako: {} {} staged — suggest a commit message? [y/N] \x1b[0m",
        staged.file_count, file_word
    );
    io::stdout().flush().ok();

    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).is_err() {
        return;
    }

    if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
        return;
    }

    // Show a spinner while the AI thinks
    print!("\x1b[90mthinking...\x1b[0m");
    io::stdout().flush().ok();

    let result = rt.block_on(ai::suggest_commit(&staged.stat, &staged.diff, config));

    // Clear the spinner line
    print!("\r\x1b[K");
    io::stdout().flush().ok();

    match result {
        Ok(message) => {
            let commit_cmd = format!("git commit -m {}", shell_quote(&message));
            loop {
                match ai::confirm::confirm_command(&commit_cmd) {
                    Ok(ai::confirm::ConfirmAction::Execute) => {
                        executor::execute_command(&commit_cmd);
                        break;
                    }
                    Ok(ai::confirm::ConfirmAction::Edit(edited)) => {
                        executor::execute_command(&edited);
                        break;
                    }
                    Ok(ai::confirm::ConfirmAction::Cancel) => break,
                    Ok(ai::confirm::ConfirmAction::Why) => {
                        // Show what's staged as the "why"
                        eprintln!("\x1b[90m{}\x1b[0m", staged.stat.trim());
                        // loop continues — re-shows the command and prompt
                    }
                    Ok(ai::confirm::ConfirmAction::Refine) => {
                        // Refine not meaningful in commit context; loop continues
                    }
                    Err(_) => break,
                }
            }
        }
        Err(e) => eprintln!("shako: couldn't suggest commit message: {e}"),
    }
}

/// Quote a string safely for use as a shell argument.
fn shell_quote(s: &str) -> String {
    if !s.contains('"') {
        format!("\"{s}\"")
    } else if !s.contains('\'') {
        format!("'{s}'")
    } else {
        // Escape double quotes inside double-quoted string
        format!("\"{}\"", s.replace('"', "\\\""))
    }
}

/// Extract the likely repository directory name from a git clone URL.
/// `git clone https://github.com/owner/repo.git` → `"repo"`
fn extract_repo_name(url: &str) -> Option<String> {
    // Strip trailing slashes
    let url = url.trim_end_matches('/');
    // Take the last path segment
    let segment = url.split('/').next_back()?;
    // Strip .git suffix
    let name = segment.strip_suffix(".git").unwrap_or(segment);
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Parse a Makefile and return the list of public targets (no leading dot/underscore,
/// no special make variables).  Returns at most the first `limit` targets.
fn read_make_targets(makefile: &std::path::Path) -> Vec<String> {
    let Ok(contents) = std::fs::read_to_string(makefile) else {
        return vec![];
    };
    let mut targets = Vec::new();
    for line in contents.lines() {
        // A target line starts with an identifier followed by `:` but not `:=`
        if let Some(target) = line.split(':').next() {
            let target = target.trim();
            if target.is_empty()
                || target.starts_with('.')
                || target.starts_with('_')
                || target.starts_with('#')
                || target.contains('$')
                || target.contains(' ')
                || target.contains('\t')
            {
                continue;
            }
            // Must have at least one colon after the target name (not `:=`)
            let after = &line[target.len()..];
            if after.starts_with(':') && !after.starts_with(":=") {
                targets.push(target.to_string());
            }
        }
    }
    targets
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_git_add_basic() {
        assert!(is_git_add("git add ."));
        assert!(is_git_add("git add -A"));
        assert!(is_git_add("git add src/main.rs"));
        assert!(is_git_add("git add -p"));
        assert!(is_git_add("git add --patch"));
    }

    #[test]
    fn test_is_git_add_excluded() {
        assert!(!is_git_add("git add")); // no target
        assert!(!is_git_add("git add --help"));
        assert!(!is_git_add("git add --version"));
        assert!(!is_git_add("git commit -m test"));
        assert!(!is_git_add("git status"));
        assert!(!is_git_add("echo hello"));
    }

    #[test]
    fn test_shell_quote_no_special() {
        assert_eq!(shell_quote("feat: add login"), r#""feat: add login""#);
    }

    #[test]
    fn test_shell_quote_has_double_quotes() {
        // falls back to single quotes when message contains "
        assert_eq!(
            shell_quote(r#"fix: handle "empty" input"#),
            r#"'fix: handle "empty" input'"#
        );
    }

    #[test]
    fn test_shell_quote_has_both_quotes() {
        let msg = r#"it's a "fix""#;
        let quoted = shell_quote(msg);
        // should use escaped double quotes
        assert!(quoted.starts_with('"'));
        assert!(quoted.contains(r#"\""#));
    }

    #[test]
    fn test_extract_repo_name_https() {
        assert_eq!(
            extract_repo_name("https://github.com/owner/myrepo.git"),
            Some("myrepo".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_no_git_suffix() {
        assert_eq!(
            extract_repo_name("https://github.com/owner/myrepo"),
            Some("myrepo".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_ssh() {
        assert_eq!(
            extract_repo_name("git@github.com:owner/myrepo.git"),
            Some("myrepo".to_string())
        );
    }

    #[test]
    fn test_check_passive_git_clone() {
        let suggestion = check_passive("git clone https://github.com/owner/shako.git");
        assert_eq!(suggestion, Some("tip: cd shako".to_string()));
    }

    #[test]
    fn test_check_passive_non_matching() {
        // `git status` should return None (no passive suggestion)
        assert!(check_passive("git status").is_none());
        assert!(check_passive("ls -la").is_none());
    }
}
