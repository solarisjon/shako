use reedline::{Completer, Span, Suggestion};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use crate::path_cache::PathCache;

const GIT_SUBCOMMANDS: &[&str] = &[
    "add",
    "bisect",
    "blame",
    "branch",
    "checkout",
    "cherry-pick",
    "clone",
    "commit",
    "config",
    "diff",
    "fetch",
    "init",
    "log",
    "merge",
    "mv",
    "pull",
    "push",
    "rebase",
    "reflog",
    "remote",
    "reset",
    "restore",
    "revert",
    "rm",
    "show",
    "stash",
    "status",
    "switch",
    "tag",
    "worktree",
];

const CARGO_SUBCOMMANDS: &[&str] = &[
    "bench", "build", "check", "clean", "clippy", "doc", "fetch", "fix", "fmt", "init", "install",
    "new", "publish", "run", "search", "test", "tree", "update", "vendor",
];

const DOCKER_SUBCOMMANDS: &[&str] = &[
    "build",
    "compose",
    "container",
    "cp",
    "create",
    "exec",
    "image",
    "images",
    "inspect",
    "kill",
    "logs",
    "network",
    "ps",
    "pull",
    "push",
    "rm",
    "rmi",
    "run",
    "start",
    "stop",
    "system",
    "volume",
];

const KUBECTL_SUBCOMMANDS: &[&str] = &[
    "apply",
    "attach",
    "create",
    "delete",
    "describe",
    "edit",
    "exec",
    "expose",
    "get",
    "label",
    "logs",
    "patch",
    "port-forward",
    "rollout",
    "run",
    "scale",
    "set",
    "top",
];

const MAKE_SUBCOMMANDS: &[&str] = &[];

pub struct JboshCompleter {
    cache: Arc<PathCache>,
}

impl JboshCompleter {
    pub fn new(cache: Arc<PathCache>) -> Self {
        Self { cache }
    }

    fn path_commands(&self) -> &[String] {
        &self.cache.commands
    }

    fn path_completions(
        &self,
        partial: &str,
        dirs_only: bool,
        start: usize,
        pos: usize,
    ) -> Vec<Suggestion> {
        // Split on the last `/` so that trailing-slash partials like `src/`
        // work correctly.  `PathBuf::parent` + `file_name` cannot handle them.
        let (dir, prefix, dir_prefix) = if let Some(slash) = partial.rfind('/') {
            let dir_str = &partial[..=slash]; // includes the trailing '/'
            let file_prefix = &partial[slash + 1..];
            // Expand a leading `~/` to the real home directory so that paths
            // like `~/.co` resolve correctly for fs::read_dir.
            let expanded_dir = if dir_str.starts_with("~/") {
                dirs::home_dir()
                    .map(|h| h.join(&dir_str[2..]))
                    .unwrap_or_else(|| PathBuf::from(dir_str))
            } else {
                PathBuf::from(dir_str)
            };
            (expanded_dir, file_prefix.to_string(), dir_str.to_string())
        } else {
            (PathBuf::from("."), partial.to_string(), String::new())
        };

        let mut completions = Vec::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                // Resolve whether this entry is a directory (follow symlinks).
                let is_dir = entry
                    .file_type()
                    .ok()
                    .map(|ft| {
                        if ft.is_dir() {
                            true
                        } else if ft.is_symlink() {
                            entry.path().metadata().ok().is_some_and(|m| m.is_dir())
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false);

                if dirs_only && !is_dir {
                    continue;
                }

                if let Ok(name) = entry.file_name().into_string() {
                    if name.starts_with(&prefix) {
                        let mut value = format!("{dir_prefix}{name}");
                        let append_whitespace = if is_dir {
                            value.push('/');
                            false
                        } else {
                            true
                        };
                        // Escape spaces in filenames with backslash
                        if value.contains(' ') {
                            value = value.replace(' ', "\\ ");
                        }
                        completions.push(Suggestion {
                            value,
                            display_override: None,
                            description: None,
                            style: None,
                            extra: None,
                            span: Span::new(start, pos),
                            append_whitespace,
                            match_indices: None,
                        });
                    }
                }
            }
        }

        completions.sort_by(|a, b| a.value.cmp(&b.value));
        completions
    }

    fn subcommand_completions(
        &self,
        subcommands: &[&str],
        partial: &str,
        start: usize,
        pos: usize,
    ) -> Vec<Suggestion> {
        subcommands
            .iter()
            .filter(|sc| sc.starts_with(partial))
            .map(|sc| Suggestion {
                value: sc.to_string(),
                display_override: None,
                description: None,
                style: None,
                extra: None,
                span: Span::new(start, pos),
                append_whitespace: true,
                match_indices: None,
            })
            .collect()
    }

    /// Run `git branch -a` and return branch names matching `partial`.
    fn git_branches(&self, partial: &str) -> Vec<String> {
        let output = std::process::Command::new("git")
            .args(["branch", "-a", "--format=%(refname:short)"])
            .output()
            .ok();
        let Some(output) = output else {
            return vec![];
        };
        if !output.status.success() {
            return vec![];
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut branches: Vec<String> = stdout
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|b| {
                !b.is_empty() && !b.ends_with("/HEAD") && b.starts_with(partial)
            })
            .collect();
        branches.sort();
        branches.dedup();
        branches
    }

    fn branch_suggestions(&self, partial: &str, start: usize, pos: usize) -> Vec<Suggestion> {
        self.git_branches(partial)
            .into_iter()
            .map(|b| Suggestion {
                value: b,
                display_override: None,
                description: None,
                style: None,
                extra: None,
                span: Span::new(start, pos),
                append_whitespace: true,
                match_indices: None,
            })
            .collect()
    }

    /// Read Makefile targets for `make` tab completion.
    fn makefile_targets(&self, partial: &str) -> Vec<String> {
        let makefile = if PathBuf::from("Makefile").exists() {
            "Makefile"
        } else if PathBuf::from("makefile").exists() {
            "makefile"
        } else if PathBuf::from("GNUmakefile").exists() {
            "GNUmakefile"
        } else {
            return vec![];
        };

        let mut targets = Vec::new();
        if let Ok(contents) = fs::read_to_string(makefile) {
            for line in contents.lines() {
                // Match lines like "target:" or "target: deps"
                // Skip lines starting with tab/space (recipe lines)
                if !line.starts_with('\t') && !line.starts_with(' ') && !line.starts_with('#') {
                    if let Some(target) = line.split(':').next() {
                        let target = target.trim();
                        // Skip variable assignments, .PHONY, etc.
                        if !target.is_empty()
                            && !target.contains('=')
                            && !target.contains('$')
                            && !target.starts_with('.')
                            && target.starts_with(partial)
                        {
                            targets.push(target.to_string());
                        }
                    }
                }
            }
        }
        targets.sort();
        targets.dedup();
        targets
    }
}

impl Completer for JboshCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        let line_to_cursor = &line[..pos];
        let parts: Vec<&str> = line_to_cursor.split_whitespace().collect();

        if parts.is_empty() {
            return vec![];
        }

        let completing_first_token = !line_to_cursor.ends_with(' ') && parts.len() == 1;
        let partial = if line_to_cursor.ends_with(' ') {
            ""
        } else {
            parts.last().copied().unwrap_or("")
        };
        let start = pos - partial.len();

        // First token: complete commands from PATH + builtins
        if completing_first_token {
            let mut commands: Vec<String> = self.path_commands().to_vec();
            for &b in crate::builtins::BUILTINS {
                commands.push(b.to_string());
            }
            commands.sort();
            commands.dedup();
            return commands
                .into_iter()
                .filter(|cmd| cmd.starts_with(partial))
                .map(|cmd| Suggestion {
                    value: cmd,
                    display_override: None,
                    description: None,
                    style: None,
                    extra: None,
                    span: Span::new(start, pos),
                    append_whitespace: true,
                    match_indices: None,
                })
                .collect();
        }

        let first_cmd = parts[0];

        // After `sudo`, complete like a first token
        if first_cmd == "sudo" && parts.len() == 2 && !line_to_cursor.ends_with(' ') {
            return self.path_commands()
                .iter()
                .filter(|cmd| cmd.starts_with(partial))
                .map(|cmd| Suggestion {
                    value: cmd.clone(),
                    display_override: None,
                    description: None,
                    style: None,
                    extra: None,
                    span: Span::new(start, pos),
                    append_whitespace: true,
                    match_indices: None,
                })
                .collect();
        }

        // Subcommand completions for known tools
        let is_second_token = (parts.len() == 2 && !line_to_cursor.ends_with(' '))
            || (parts.len() == 1 && line_to_cursor.ends_with(' '));

        if is_second_token {
            // `gco <branch>` — git checkout shortcut alias
            if matches!(first_cmd, "gco") {
                let branches = self.branch_suggestions(partial, start, pos);
                if !branches.is_empty() {
                    return branches;
                }
            }

            let subcommands = match first_cmd {
                "git" => Some(GIT_SUBCOMMANDS),
                "cargo" => Some(CARGO_SUBCOMMANDS),
                "docker" | "podman" => Some(DOCKER_SUBCOMMANDS),
                "kubectl" | "k" => Some(KUBECTL_SUBCOMMANDS),
                "make" | "gmake" => {
                    let targets = self.makefile_targets(partial);
                    if !targets.is_empty() {
                        return targets
                            .into_iter()
                            .map(|t| Suggestion {
                                value: t,
                                display_override: None,
                                description: None,
                                style: None,
                                extra: None,
                                span: Span::new(start, pos),
                                append_whitespace: true,
                                match_indices: None,
                            })
                            .collect();
                    }
                    Some(MAKE_SUBCOMMANDS)
                }
                _ => None,
            };

            if let Some(subs) = subcommands {
                return self.subcommand_completions(subs, partial, start, pos);
            }
        }

        // Third token: `git <subcmd> <branch>` — complete branch names
        let is_third_token = (parts.len() == 3 && !line_to_cursor.ends_with(' '))
            || (parts.len() == 2 && line_to_cursor.ends_with(' '));

        if is_third_token && first_cmd == "git" {
            let subcmd = parts.get(1).copied().unwrap_or("");
            const BRANCH_SUBCMDS: &[&str] =
                &["checkout", "switch", "merge", "rebase", "cherry-pick", "diff", "show", "reset"];
            if BRANCH_SUBCMDS.contains(&subcmd) {
                let branches = self.branch_suggestions(partial, start, pos);
                if !branches.is_empty() {
                    return branches;
                }
            }
        }

        // `cd` and `z` — directories only
        let dirs_only = matches!(first_cmd, "cd" | "z" | "pushd" | "mkdir" | "rmdir");

        self.path_completions(partial, dirs_only, start, pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reedline::Completer;

    fn test_completer() -> JboshCompleter {
        JboshCompleter::new(PathCache::new())
    }

    #[test]
    fn test_cd_with_trailing_space_returns_dirs() {
        let mut c = test_completer();
        let suggestions = c.complete("cd ", 3);
        assert!(!suggestions.is_empty(), "expected directory completions for 'cd '");
        for s in &suggestions {
            assert!(s.value.ends_with('/'), "dir completion '{}' should end with '/'", s.value);
            assert!(!s.append_whitespace, "dir completion should not append whitespace");
        }
        assert_eq!(suggestions[0].span.start, 3);
        assert_eq!(suggestions[0].span.end, 3);
    }

    #[test]
    fn test_path_partial_name_gets_slash() {
        let mut c = test_completer();
        // repo root contains src/ — "cat sr" should complete to "src/"
        let suggestions = c.complete("cat sr", 6);
        let src = suggestions.iter().find(|s| s.value == "src/");
        assert!(src.is_some(), "expected 'src/' in completions, got: {:?}", suggestions.iter().map(|s| &s.value).collect::<Vec<_>>());
        let src = src.unwrap();
        assert!(!src.append_whitespace);
        assert_eq!(src.span.start, 4);
        assert_eq!(src.span.end, 6);
    }

    #[test]
    fn test_path_trailing_slash_descends() {
        let mut c = test_completer();
        let suggestions = c.complete("cat src/", 8);
        assert!(!suggestions.is_empty(), "expected completions inside src/");
        for s in &suggestions {
            assert!(s.value.starts_with("src/"), "completion '{}' should start with 'src/'", s.value);
        }
        assert_eq!(suggestions[0].span.start, 4);
        assert_eq!(suggestions[0].span.end, 8);
    }

    #[test]
    fn test_first_token_completion() {
        let mut c = test_completer();
        let suggestions = c.complete("gi", 2);
        assert!(suggestions.iter().any(|s| s.value == "git"), "expected 'git' in command completions");
    }

    #[test]
    fn test_git_subcommand_completion() {
        let mut c = test_completer();
        let suggestions = c.complete("git stat", 8);
        assert!(suggestions.iter().any(|s| s.value == "status"), "expected 'status' in git subcommand completions");
    }

    #[test]
    fn test_tilde_path_completion() {
        let mut c = test_completer();
        // "ls ~/" — should return entries from the real home directory, not "NO RECORDS FOUND"
        if let Some(home) = dirs::home_dir() {
            if home.is_dir() {
                let suggestions = c.complete("ls ~/", 5);
                assert!(!suggestions.is_empty(), "tilde path '~/' should expand to home dir and return entries");
                // All values should be prefixed with "~/"
                for s in &suggestions {
                    assert!(s.value.starts_with("~/"), "completion '{}' should start with '~/'", s.value);
                }
            }
        }
    }

    #[test]
    fn test_git_checkout_branch_completion() {
        let mut c = test_completer();
        // In a git repo with at least a `main` branch this should return branch completions.
        let suggestions = c.complete("git checkout ", 13);
        // Only run the assertion when we are inside a git repo (git_branches returns results).
        if !suggestions.is_empty() {
            // Completions should not look like file paths (no trailing slash for branches).
            for s in &suggestions {
                assert!(!s.value.ends_with('/'), "branch completion '{}' should not end with '/'", s.value);
                assert!(s.append_whitespace, "branch completion should append whitespace");
            }
        }
    }

    #[test]
    fn test_git_checkout_branch_prefix_filter() {
        let mut c = test_completer();
        // Completing "git checkout ma" should only return branches starting with "ma".
        let suggestions = c.complete("git checkout ma", 15);
        for s in &suggestions {
            assert!(
                s.value.starts_with("ma"),
                "branch '{}' should start with 'ma'",
                s.value
            );
        }
    }

    #[test]
    fn test_git_switch_branch_completion() {
        let mut c = test_completer();
        let suggestions = c.complete("git switch ", 11);
        // If inside a git repo, branches should be offered (not file paths).
        for s in &suggestions {
            assert!(!s.value.ends_with('/'), "unexpected dir completion for git switch");
        }
    }

    #[test]
    fn test_gco_branch_completion() {
        let mut c = test_completer();
        let suggestions = c.complete("gco ", 4);
        // gco is an alias for git checkout — branch completions when in a git repo.
        for s in &suggestions {
            assert!(!s.value.ends_with('/'), "branch completion '{}' should not end with '/'", s.value);
        }
    }

    #[test]
    fn test_no_head_in_branch_completions() {
        let c = test_completer();
        let branches = c.git_branches("");
        for b in &branches {
            assert!(!b.ends_with("/HEAD"), "HEAD ref should be filtered: {b}");
        }
    }
}
