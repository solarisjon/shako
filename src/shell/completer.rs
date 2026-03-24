use reedline::{Completer, Span, Suggestion};
use std::env;
use std::fs;
use std::path::PathBuf;

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

pub struct JboshCompleter;

impl JboshCompleter {
    pub fn new() -> Self {
        Self
    }

    fn path_commands(&self) -> Vec<String> {
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
            (PathBuf::from(dir_str), file_prefix.to_string(), dir_str.to_string())
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
                            entry.path().metadata().ok().map_or(false, |m| m.is_dir())
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
                        // Directories get a trailing `/` so the next Tab press
                        // descends into the directory instead of appending a space.
                        let mut value = format!("{dir_prefix}{name}");
                        let append_whitespace;
                        if is_dir {
                            value.push('/');
                            append_whitespace = false;
                        } else {
                            append_whitespace = true;
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
            let mut commands = self.path_commands();
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
            let commands = self.path_commands();
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
                _ => None,
            };

            if let Some(subs) = subcommands {
                return self.subcommand_completions(subs, partial, start, pos);
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

    #[test]
    fn test_cd_with_trailing_space_returns_dirs() {
        let mut c = JboshCompleter::new();
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
        let mut c = JboshCompleter::new();
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
        let mut c = JboshCompleter::new();
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
        let mut c = JboshCompleter::new();
        let suggestions = c.complete("gi", 2);
        assert!(suggestions.iter().any(|s| s.value == "git"), "expected 'git' in command completions");
    }

    #[test]
    fn test_git_subcommand_completion() {
        let mut c = JboshCompleter::new();
        let suggestions = c.complete("git stat", 8);
        assert!(suggestions.iter().any(|s| s.value == "status"), "expected 'status' in git subcommand completions");
    }
}
