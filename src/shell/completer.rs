use reedline::{Completer, Span, Suggestion};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::path_cache::PathCache;

const GIT_COMMIT_FLAGS: &[&str] = &[
    "--message", "--amend", "--all", "--no-edit", "--allow-empty",
    "--author", "--date", "--signoff", "--verbose", "--dry-run",
];
const GIT_LOG_FLAGS: &[&str] = &[
    "--oneline", "--graph", "--all", "--author", "--since", "--until",
    "--stat", "--patch", "--follow", "--decorate", "--format",
];
const GIT_PUSH_FLAGS: &[&str] = &[
    "--force", "--force-with-lease", "--set-upstream", "--tags",
    "--dry-run", "--verbose", "--all",
];
const GIT_PULL_FLAGS: &[&str] = &[
    "--rebase", "--no-rebase", "--ff-only", "--all", "--tags", "--verbose",
];
const GIT_DIFF_FLAGS: &[&str] = &[
    "--stat", "--name-only", "--cached", "--staged", "--word-diff",
    "--color-words", "--unified",
];
const CARGO_BUILD_FLAGS: &[&str] = &[
    "--release", "--features", "--all-features", "--no-default-features",
    "--target", "--manifest-path", "--verbose", "--quiet",
];
const CARGO_TEST_FLAGS: &[&str] = &[
    "--release", "--features", "--all-features", "--no-default-features",
    "--verbose", "--quiet", "--no-run", "--test", "--lib", "--bin",
];
const CARGO_RUN_FLAGS: &[&str] = &[
    "--release", "--features", "--all-features", "--bin",
    "--example", "--manifest-path", "--verbose",
];

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

const NPM_SUBCOMMANDS: &[&str] = &[
    "audit", "cache", "ci", "dedupe", "exec", "fund", "help", "init", "install",
    "link", "list", "login", "logout", "outdated", "pack", "ping", "prefix",
    "publish", "rebuild", "restart", "root", "run", "set", "start", "stop",
    "test", "token", "uninstall", "unpublish", "update", "version", "view", "whoami",
];

const PNPM_SUBCOMMANDS: &[&str] = &[
    "add", "audit", "ci", "dedupe", "exec", "import", "init", "install", "licenses",
    "link", "list", "outdated", "pack", "patch", "publish", "rebuild", "remove",
    "run", "start", "store", "test", "unlink", "update", "why",
];

const YARN_SUBCOMMANDS: &[&str] = &[
    "add", "audit", "autoclean", "bin", "cache", "check", "config", "create",
    "exec", "global", "help", "import", "info", "init", "install", "licenses",
    "link", "list", "login", "logout", "outdated", "owner", "pack", "policies",
    "publish", "remove", "run", "tag", "team", "test", "unlink", "upgrade",
    "upgrade-interactive", "version", "versions", "why", "workspace", "workspaces",
];

const BUN_SUBCOMMANDS: &[&str] = &[
    "add", "build", "create", "dev", "exec", "init", "install", "link", "outdated",
    "patch", "pm", "publish", "remove", "run", "test", "unlink", "update", "upgrade", "x",
];

const BREW_SUBCOMMANDS: &[&str] = &[
    "analytics", "audit", "autoremove", "bundle", "cask", "cleanup", "commands",
    "completions", "config", "deps", "desc", "developer", "doctor", "edit",
    "fetch", "formulae", "gist-logs", "help", "home", "info", "install", "leaves",
    "link", "list", "log", "missing", "options", "outdated", "pin", "postinstall",
    "readall", "reinstall", "search", "services", "shellenv", "style", "tap",
    "tap-info", "uninstall", "unlink", "unpin", "untap", "update", "upgrade",
    "uses", "vendor-gems",
];

const GO_SUBCOMMANDS: &[&str] = &[
    "build", "clean", "doc", "env", "fix", "fmt", "generate", "get", "help",
    "install", "list", "mod", "run", "telemetry", "test", "tool", "vet", "version", "work",
];

const RUSTUP_SUBCOMMANDS: &[&str] = &[
    "check", "component", "completions", "default", "doc", "help", "man",
    "override", "run", "self", "set", "show", "target", "toolchain", "uninstall",
    "update", "which",
];

const HELM_SUBCOMMANDS: &[&str] = &[
    "completion", "create", "dependency", "diff", "env", "get", "help", "history",
    "install", "lint", "list", "package", "plugin", "pull", "push", "registry",
    "repo", "rollback", "search", "show", "status", "template", "test",
    "uninstall", "upgrade", "verify", "version",
];

const TERRAFORM_SUBCOMMANDS: &[&str] = &[
    "apply", "console", "destroy", "fmt", "force-unlock", "get", "graph", "import",
    "init", "login", "logout", "metadata", "modules", "output", "plan", "providers",
    "refresh", "show", "state", "taint", "test", "untaint", "validate", "version",
    "workspace",
];

const MAKE_SUBCOMMANDS: &[&str] = &[];

/// Commands where the next argument is a git branch/ref name.
const GIT_BRANCH_CMDS: &[&str] = &[
    "checkout", "switch", "merge", "rebase", "diff", "log",
    "cherry-pick", "push", "pull", "branch",
];

pub struct ShakoCompleter {
    cache: Arc<PathCache>,
    /// Alias and function names shared from the REPL loop for first-token completion.
    extra_completions: Arc<RwLock<Vec<String>>>,
}

impl ShakoCompleter {
    pub fn new(cache: Arc<PathCache>, extra_completions: Arc<RwLock<Vec<String>>>) -> Self {
        Self { cache, extra_completions }
    }

    /// Run `git branch` and return matching branch names as completions.
    fn git_branches(&self, partial: &str, start: usize, pos: usize) -> Vec<Suggestion> {
        let output = std::process::Command::new("git")
            .args(["branch", "-a", "--format=%(refname:short)"])
            .output()
            .ok();
        let Some(out) = output else { return vec![] };
        let stdout = String::from_utf8_lossy(&out.stdout);
        // Filter before allocating: only call trim()/to_string() on lines that match.
        let mut branches: Vec<String> = stdout
            .lines()
            .filter_map(|l| {
                let t = l.trim();
                if !t.is_empty() && t.starts_with(partial) {
                    Some(t.to_string())
                } else {
                    None
                }
            })
            .collect();
        branches.sort();
        branches.dedup();
        branches
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

    /// Parse `~/.ssh/config` and return matching `Host` entries.
    fn ssh_hosts(&self, partial: &str, start: usize, pos: usize) -> Vec<Suggestion> {
        let config_path = dirs::home_dir()
            .map(|h| h.join(".ssh/config"))
            .filter(|p| p.exists());
        let Some(path) = config_path else { return vec![] };
        let Ok(contents) = fs::read_to_string(&path) else { return vec![] };
        // Filter fully before calling to_string() — only allocate for matching hosts.
        let mut hosts: Vec<String> = contents
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                let mut words = line.split_whitespace();
                let key = words.next()?;
                if !key.eq_ignore_ascii_case("Host") {
                    return None;
                }
                let host = words.next()?;
                // Skip wildcard patterns
                if host.contains('*') || host.contains('?') {
                    return None;
                }
                if host.starts_with(partial) {
                    Some(host.to_string())
                } else {
                    None
                }
            })
            .collect();
        hosts.sort();
        hosts.dedup();
        hosts
            .into_iter()
            .map(|h| Suggestion {
                value: h,
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

    /// Read justfile targets for `just` tab completion.
    fn justfile_targets(&self, partial: &str) -> Vec<String> {
        let justfile = if PathBuf::from("justfile").exists() {
            "justfile"
        } else if PathBuf::from("Justfile").exists() {
            "Justfile"
        } else {
            return vec![];
        };

        let mut targets = Vec::new();
        if let Ok(contents) = fs::read_to_string(justfile) {
            for line in contents.lines() {
                // Match recipe definitions: `recipe-name:` or `recipe-name arg:`
                if let Some(name) = line.split(':').next() {
                    let name = name.trim();
                    // Only simple identifiers (no comments, must start with alphanum/underscore)
                    if !name.is_empty()
                        && !name.starts_with('#')
                        && !name.starts_with('@')
                        && name
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_alphanumeric() || c == '_')
                        && name
                            .split_whitespace()
                            .next()
                            .is_some_and(|first| first.starts_with(partial))
                    {
                        if let Some(recipe) = name.split_whitespace().next() {
                            targets.push(recipe.to_string());
                        }
                    }
                }
            }
        }
        targets.sort();
        targets.dedup();
        targets
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

impl Completer for ShakoCompleter {
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

        // First token: complete commands from PATH + builtins + aliases + functions.
        // We collect &str references from all sources, sort/dedup them, then
        // allocate Strings only for the filtered suggestions — avoiding a full
        // clone of the path commands Vec<String> on every Tab press.
        if completing_first_token {
            let path_cmds = self.path_commands();
            let extra_guard = self.extra_completions.read().ok();
            let extra_slice: &[String] = extra_guard.as_deref().map_or(&[], |v| v);

            let mut all_refs: Vec<&str> = Vec::with_capacity(
                path_cmds.len() + crate::builtins::BUILTINS.len() + extra_slice.len(),
            );
            all_refs.extend(path_cmds.iter().map(String::as_str));
            all_refs.extend(crate::builtins::BUILTINS.iter().copied());
            all_refs.extend(extra_slice.iter().map(String::as_str));
            all_refs.sort_unstable();
            all_refs.dedup();
            return all_refs
                .into_iter()
                .filter(|cmd| cmd.starts_with(partial))
                .map(|cmd| Suggestion {
                    value: cmd.to_string(),
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
                "just" => {
                    let targets = self.justfile_targets(partial);
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
                    Some(&[] as &[&str])
                }
                "npm" | "npx" => Some(NPM_SUBCOMMANDS),
                "pnpm" => Some(PNPM_SUBCOMMANDS),
                "yarn" => Some(YARN_SUBCOMMANDS),
                "bun" | "bunx" => Some(BUN_SUBCOMMANDS),
                "brew" => Some(BREW_SUBCOMMANDS),
                "go" => Some(GO_SUBCOMMANDS),
                "rustup" => Some(RUSTUP_SUBCOMMANDS),
                "helm" => Some(HELM_SUBCOMMANDS),
                "terraform" | "tf" => Some(TERRAFORM_SUBCOMMANDS),
                "ssh" | "scp" | "sftp" | "rsync" => {
                    let hosts = self.ssh_hosts(partial, start, pos);
                    if !hosts.is_empty() {
                        return hosts;
                    }
                    None
                }
                _ => None,
            };

            if let Some(subs) = subcommands {
                return self.subcommand_completions(subs, partial, start, pos);
            }
        }

        // Flag completion: triggered when partial starts with '-'
        if partial.starts_with('-') && parts.len() >= 3 {
            let subcmd = parts[1];
            let flags: &[&str] = match (first_cmd, subcmd) {
                ("git", "commit") => GIT_COMMIT_FLAGS,
                ("git", "log") => GIT_LOG_FLAGS,
                ("git", "push") => GIT_PUSH_FLAGS,
                ("git", "pull") => GIT_PULL_FLAGS,
                ("git", "diff") => GIT_DIFF_FLAGS,
                ("cargo", "build") => CARGO_BUILD_FLAGS,
                ("cargo", "test") => CARGO_TEST_FLAGS,
                ("cargo", "run") => CARGO_RUN_FLAGS,
                _ => &[],
            };
            let completions: Vec<Suggestion> = flags
                .iter()
                .filter(|f| f.starts_with(partial))
                .map(|f| Suggestion {
                    value: f.to_string(),
                    span: Span::new(start, pos),
                    append_whitespace: true,
                    ..Default::default()
                })
                .collect();
            if !completions.is_empty() {
                return completions;
            }
        }

        // Git branch completion: `git checkout <branch>`, `git merge <branch>`, etc.
        if first_cmd == "git" {
            let subcmd = if parts.len() >= 2 { parts[1] } else { "" };
            let is_branch_cmd = GIT_BRANCH_CMDS.contains(&subcmd);
            let past_subcmd = (parts.len() >= 3)
                || (parts.len() == 2 && line_to_cursor.ends_with(' '));
            if is_branch_cmd && past_subcmd {
                let branches = self.git_branches(partial, start, pos);
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

    fn test_completer() -> ShakoCompleter {
        ShakoCompleter::new(
            PathCache::new(),
            std::sync::Arc::new(std::sync::RwLock::new(vec![])),
        )
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
    fn test_ls_r_completes_readme() {
        // Regression: "ls R<TAB>" must return path completions, not subcommand completions.
        // README.md and ROADMAP.md live in the repo root, so this test must run from there.
        let mut c = test_completer();
        let suggestions = c.complete("ls R", 4);
        let values: Vec<&str> = suggestions.iter().map(|s| s.value.as_str()).collect();
        assert!(
            values.iter().any(|v| v.starts_with("R")),
            "expected completions starting with 'R' for 'ls R', got: {:?}",
            values
        );
        // Span must cover the partial token (start=3, end=4)
        for s in &suggestions {
            assert_eq!(s.span.start, 3, "span.start should be 3 (position of 'R')");
            assert_eq!(s.span.end, 4, "span.end should be 4 (cursor)");
        }
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
}
