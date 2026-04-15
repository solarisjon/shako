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
    ("duf", "df", ""),
    ("doggo", "dig", ""),
    ("xh", "curl", ""),
    ("tokei", "cloc", ""),
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
    // rg-powered aliases
    ("rg", "rgf", "rg -l"),
    // git shortcuts
    ("git", "ga",   "git add"),
    ("git", "gaa",  "git add -A"),
    ("git", "gs",   "git status"),
    ("git", "gb",   "git branch"),
    ("git", "gl",   "git log --oneline -20"),
    ("git", "gd",   "git diff"),
    ("git", "gf",   "git fetch"),
    ("git", "gp",   "git push"),
    ("git", "gpl",  "git pull"),
    ("git", "gco",  "git checkout"),
    ("git", "gcm",  "git commit -m"),
    ("git", "grb",  "git rebase"),
    ("git", "gst",  "git stash"),
    ("git", "gstp", "git stash pop"),
    // docker shortcuts
    ("docker", "dps",  "docker ps"),
    ("docker", "dex",  "docker exec -it"),
    ("docker", "dlog", "docker logs -f"),
    ("docker", "dst",  "docker stop"),
    ("docker", "drm",  "docker rm"),
    ("docker", "drmi", "docker rmi"),
    ("docker", "dimg", "docker images"),
    ("docker", "db",   "docker build"),
    // podman shortcuts
    ("podman", "pps",  "podman ps"),
    ("podman", "pex",  "podman exec -it"),
    ("podman", "plog", "podman logs -f"),
    ("podman", "ppod", "podman pod ps"),
    ("podman", "pimg", "podman images"),
    ("podman", "pb",   "podman build"),
    ("podman", "pst",  "podman stop"),
    ("podman", "prm",  "podman rm"),
    ("podman", "prmi", "podman rmi"),
    ("podman", "pnet", "podman network ls"),
    ("podman", "pvol", "podman volume ls"),
    // kubectl shortcuts
    ("kubectl", "k",   "kubectl"),
    ("kubectl", "kgp", "kubectl get pods"),
    ("kubectl", "kgs", "kubectl get services"),
    ("kubectl", "kgn", "kubectl get nodes"),
    ("kubectl", "kl",  "kubectl logs -f"),
    ("kubectl", "kex", "kubectl exec -it"),
    ("kubectl", "kaf", "kubectl apply -f"),
    ("kubectl", "kdf", "kubectl delete -f"),
    ("kubectl", "kdp", "kubectl describe pod"),
    // terraform shortcuts
    ("terraform", "tfi", "terraform init"),
    ("terraform", "tfp", "terraform plan"),
    ("terraform", "tfa", "terraform apply"),
    ("terraform", "tfd", "terraform destroy"),
    // cargo shortcuts
    ("cargo", "cb",  "cargo build"),
    ("cargo", "cr",  "cargo run"),
    ("cargo", "ct",  "cargo test"),
    ("cargo", "cc",  "cargo check"),
    ("cargo", "ccl", "cargo clippy"),
    // npm shortcuts
    ("npm", "ni", "npm install"),
    ("npm", "nr", "npm run"),
    ("npm", "nt", "npm test"),
    ("npm", "ns", "npm start"),
];

/// Return all smart aliases, optionally filtered by tool name.
///
/// Each entry is `(requires, alias, expansion, is_active)` where `is_active`
/// is true when the required binary is found on `$PATH`.
pub fn list_shortcuts(filter: &str) -> Vec<(&'static str, &'static str, &'static str, bool)> {
    let filter = filter.trim().to_lowercase();
    SMART_ALIASES
        .iter()
        .filter(|(requires, alias, _)| {
            if filter.is_empty() {
                true
            } else {
                requires.to_lowercase().contains(&filter)
                    || alias.to_lowercase().contains(&filter)
            }
        })
        .map(|(requires, alias, expansion)| {
            let active = which(requires).is_ok();
            (*requires, *alias, *expansion, active)
        })
        .collect()
}

/// Detect installed modern tools and return aliases to apply.
/// Skips aliases the user has already defined (user config wins).
pub fn detect_smart_defaults(
    existing_aliases: &HashMap<String, String>,
) -> HashMap<String, String> {
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
