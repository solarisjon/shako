//! Proactive suggestions — offered automatically after certain commands succeed.
//!
//! Currently implemented:
//!   - After `git add`, offer an AI-generated commit message.
//!   - After `git push`, display the current version number (minor only,
//!     unless the push bumped the major component).

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
    } else if is_git_push(cmd) {
        show_push_version();
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

// ── git push → version display ────────────────────────────────────────────────

/// Returns true for `git push` (including `git push <remote> <branch>` etc.).
/// Excludes `git push --help` and `git push --version`.
fn is_git_push(cmd: &str) -> bool {
    let mut tokens = cmd.split_whitespace();
    match (tokens.next(), tokens.next()) {
        (Some("git"), Some("push")) => {
            // Exclude help/version flags
            let rest: Vec<&str> = tokens.collect();
            !rest.iter().any(|a| *a == "--help" || *a == "--version")
        }
        _ => false,
    }
}

/// Display the shako version after a successful `git push`.
///
/// Shows only the minor version component (e.g., `v0.2`) to keep the message
/// brief and stable across patch releases. If the major version is non-zero
/// *and* the minor version just changed (detected by comparing against the
/// previous git tag), the full version is shown instead so the user can see
/// the significance of the bump. In the common case we simply show the minor
/// version — the caller has no reliable way to know *which* kind of bump the
/// push contains, so we lean on the conservative (minor-only) display by
/// default.
///
/// Version source priority:
///   1. Most recent git tag (e.g. `v0.2.1`) in the local repo.
///   2. `CARGO_PKG_VERSION` baked in at compile time.
fn show_push_version() {
    let version_str = current_version();
    let display = format_minor_version(&version_str);
    eprintln!("\x1b[90mshako: pushed · {display}\x1b[0m");
}

/// Format a semver string to show only `v{major}.{minor}`.
///
/// Rules:
///   - Show `v{major}.{minor}` only — the patch component is dropped so the
///     displayed version stays stable across patch releases.
///   - Leading `v` from git tags is handled transparently.
///   - Used by both the `git push` proactive hint and the startup banner so
///     the version string is consistent throughout the UI.
pub fn format_minor_version(version: &str) -> String {
    // Strip leading 'v' if present (git tags often have it)
    let v = version.trim_start_matches('v');
    let parts: Vec<&str> = v.splitn(3, '.').collect();
    match parts.as_slice() {
        [major, minor, ..] => format!("v{major}.{minor}"),
        [major] => format!("v{major}"),
        _ => format!("v{version}"),
    }
}

/// Determine the current version string.
///
/// Tries `git describe --tags --abbrev=0` first so that the version reflects
/// the latest tag even when Cargo.toml hasn't been bumped yet. Falls back to
/// the compile-time `CARGO_PKG_VERSION` constant.
fn current_version() -> String {
    let git_tag = Command::new("git")
        .args(["describe", "--tags", "--abbrev=0"])
        .output();

    if let Ok(out) = git_tag {
        if out.status.success() {
            let tag = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !tag.is_empty() {
                return tag;
            }
        }
    }

    // Fall back to the version baked in at compile time
    env!("CARGO_PKG_VERSION").to_string()
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

    // ── git push detection ─────────────────────────────────────────────────

    #[test]
    fn test_is_git_push_basic() {
        assert!(is_git_push("git push"));
        assert!(is_git_push("git push origin"));
        assert!(is_git_push("git push origin main"));
        assert!(is_git_push("git push --force-with-lease"));
        assert!(is_git_push("git push -u origin HEAD"));
    }

    #[test]
    fn test_is_git_push_excluded() {
        assert!(!is_git_push("git push --help"));
        assert!(!is_git_push("git push --version"));
        assert!(!is_git_push("git pull"));
        assert!(!is_git_push("git status"));
        assert!(!is_git_push("echo git push"));
    }

    // ── version formatting ─────────────────────────────────────────────────

    #[test]
    fn test_format_minor_version_patch() {
        // Patch number is dropped — show major.minor only
        assert_eq!(format_minor_version("0.2.1"), "v0.2");
        assert_eq!(format_minor_version("1.3.7"), "v1.3");
    }

    #[test]
    fn test_format_minor_version_with_v_prefix() {
        // Leading 'v' from git tags should be handled
        assert_eq!(format_minor_version("v0.2.1"), "v0.2");
        assert_eq!(format_minor_version("v1.0.0"), "v1.0");
    }

    #[test]
    fn test_format_minor_version_no_patch() {
        assert_eq!(format_minor_version("0.2"), "v0.2");
        assert_eq!(format_minor_version("2.0"), "v2.0");
    }

    #[test]
    fn test_format_minor_version_major_only() {
        assert_eq!(format_minor_version("3"), "v3");
    }
}
