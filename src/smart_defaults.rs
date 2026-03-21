use std::collections::HashMap;
use which::which;

/// Modern CLI tool mappings.
/// Each entry: (modern_tool, classic_tool, default_args_for_modern)
const TOOL_UPGRADES: &[(&str, &str, &str)] = &[
    ("eza", "ls", "--icons --group-directories-first"),
    ("bat", "cat", "--style=auto"),
    ("fd", "find", ""),
    ("rg", "grep", ""),
    ("dust", "du", ""),
    ("procs", "ps", ""),
    ("sd", "sed", ""),
    ("delta", "diff", ""),
    ("btop", "top", ""),
    ("bottom", "top", ""),
];

/// Compound aliases that use modern tools with specific flags.
const SMART_ALIASES: &[(&str, &str, &str)] = &[
    // eza-powered aliases
    ("eza", "ll", "eza -la --icons --group-directories-first"),
    ("eza", "la", "eza -a --icons --group-directories-first"),
    ("eza", "lt", "eza --tree --icons --level=2"),
    // bat-powered aliases
    ("bat", "preview", "bat --style=auto --color=always"),
    // fd-powered aliases
    ("fd", "ff", "fd --type f"),
    ("fd", "fdir", "fd --type d"),
];

/// Detect installed modern tools and return aliases to apply.
/// Skips aliases the user has already defined (user config wins).
pub fn detect_smart_defaults(existing_aliases: &HashMap<String, String>) -> HashMap<String, String> {
    let mut aliases = HashMap::new();

    // Direct tool upgrades: ls → eza, cat → bat, etc.
    for &(modern, classic, default_args) in TOOL_UPGRADES {
        if which(modern).is_ok() && !existing_aliases.contains_key(classic) {
            let value = if default_args.is_empty() {
                modern.to_string()
            } else {
                format!("{modern} {default_args}")
            };
            aliases.insert(classic.to_string(), value);
        }
    }

    // Smart compound aliases (ll, la, lt, etc.)
    for &(requires, name, value) in SMART_ALIASES {
        if which(requires).is_ok() && !existing_aliases.contains_key(name) {
            aliases.insert(name.to_string(), value.to_string());
        }
    }

    aliases
}

/// Check if zoxide is available.
pub fn has_zoxide() -> bool {
    which("zoxide").is_ok()
}

/// Check if fzf is available.
pub fn has_fzf() -> bool {
    which("fzf").is_ok()
}

/// Query zoxide for the best match for a path.
pub fn zoxide_query(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("zoxide")
        .arg("query")
        .args(args)
        .output()
        .ok()?;

    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(path);
        }
    }
    None
}

/// Tell zoxide to track a directory visit.
pub fn zoxide_add(path: &str) {
    let _ = std::process::Command::new("zoxide")
        .args(["add", path])
        .output();
}

/// Run fzf on the given input lines, return the selected line.
pub fn fzf_select(input: &str, prompt: &str) -> Option<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("fzf")
        .args([
            "--height=40%",
            "--reverse",
            "--border",
            &format!("--prompt={prompt} "),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .ok()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes()).ok();
    }

    let output = child.wait_with_output().ok()?;
    if output.status.success() {
        let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !selected.is_empty() {
            return Some(selected);
        }
    }
    None
}
