use anyhow::Result;
use std::env;
use std::fs;
use std::path::PathBuf;
use which::which;

/// Shell context sent to the LLM for better command translation.
pub struct ShellContext {
    pub os: String,
    pub arch: String,
    pub cwd: String,
    pub shell: String,
    pub user: String,
    pub available_tools: Vec<(&'static str, &'static str)>,
    pub dir_context: String,
    pub recent_history: Vec<String>,
    pub git_context: String,
    pub project_context: String,
    /// Learned user preferences injected from ~/.config/shako/learned_prefs.toml
    pub user_preferences: String,
}

/// Modern tools the AI should prefer when available, with concrete syntax guidance.
const TOOL_PREFERENCES: &[(&str, &str)] = &[
    (
        "fd",
        "use fd instead of find. \
         Syntax: `fd PATTERN` (name/regex search), `fd -e EXTENSION` for files by extension \
         (e.g. `fd -e md` finds all .md files — do NOT use `fd .md` or `fd -t f .md`), \
         `fd -t f` files only, `fd -t d` dirs only, `fd -H` include hidden, \
         `fd --size +100m` to find files larger than 100 MB (supports k/m/g suffixes). \
         Always search from `.` (current dir) unless a different path is given.",
    ),
    (
        "rg",
        "use rg (ripgrep) instead of grep. \
         Syntax: `rg PATTERN`, `rg -l PATTERN` (filenames only), \
         `rg -t FILETYPE PATTERN` (e.g. `rg -t rust TODO`), \
         `rg -i PATTERN` (case-insensitive). Respects .gitignore by default.",
    ),
    (
        "eza",
        "use eza instead of ls. \
         Syntax: `eza` (basic), `eza -la` (long + hidden), `eza --tree` (tree view), \
         `eza --tree --level=N` (limit depth).",
    ),
    (
        "bat",
        "use bat instead of cat. \
         Syntax: `bat FILE` (with syntax highlighting), `bat -n FILE` (line numbers only).",
    ),
    (
        "dust",
        "use dust instead of du for DISK USAGE SUMMARIES only. \
         Syntax: `dust` (current dir), `dust PATH`, `dust -n N` (top N entries). \
         IMPORTANT: dust has no --size flag and cannot filter by size threshold.",
    ),
    (
        "sd",
        "use sd instead of sed for simple substitutions. \
         Syntax: `sd 'FIND' 'REPLACE' FILE` (in-place) or pipe: `echo foo | sd foo bar`.",
    ),
    (
        "procs",
        "use procs instead of ps. \
         Syntax: `procs` (all processes), `procs KEYWORD` (filter by name).",
    ),
    (
        "delta",
        "use delta instead of diff. \
         Syntax: `delta FILE1 FILE2` or pipe: `diff FILE1 FILE2 | delta`.",
    ),
];

/// Build context from the current environment.
pub fn build_context(recent_history: Vec<String>) -> Result<ShellContext> {
    let cwd = env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let user = env::var("USER")
        .or_else(|_| env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string());

    let available_tools: Vec<(&str, &str)> = TOOL_PREFERENCES
        .iter()
        .filter(|(tool, _)| which(tool).is_ok())
        .copied()
        .collect();

    let dir_context = build_dir_context();
    let git_context = build_git_context();
    let project_context = read_project_context();
    let user_preferences = crate::learned_prefs::context_hint();

    Ok(ShellContext {
        os: env::consts::OS.to_string(),
        arch: env::consts::ARCH.to_string(),
        cwd,
        shell: "shako".to_string(),
        user,
        available_tools,
        dir_context,
        recent_history,
        git_context,
        project_context,
        user_preferences,
    })
}

/// Build directory context: list contents of cwd and ~/
/// so the AI knows actual file/directory names.
fn build_dir_context() -> String {
    let mut ctx = String::new();

    // Current directory
    if let Ok(entries) = list_dir_names(".") {
        if !entries.is_empty() {
            ctx.push_str("Current directory contents: ");
            ctx.push_str(&entries.join(", "));
            ctx.push('\n');
        }
    }

    // Home directory and its subdirectories (one level deep)
    let home = dirs::home_dir().unwrap_or_default();
    let cwd = env::current_dir().unwrap_or_default();
    if home != cwd {
        if let Ok(entries) = list_dir_names(&home) {
            if !entries.is_empty() {
                ctx.push_str("Home directory (~) contents: ");
                ctx.push_str(&entries.join(", "));
                ctx.push('\n');
            }

            // List contents of home subdirectories (skip huge ones)
            let mut total_entries = entries.len();
            for entry in &entries {
                if total_entries > 200 {
                    break;
                }
                if let Some(dir_name) = entry.strip_suffix('/') {
                    let subdir = home.join(dir_name);
                    if let Ok(sub_entries) = list_dir_names(&subdir) {
                        if !sub_entries.is_empty() && sub_entries.len() <= 40 {
                            ctx.push_str(&format!("~/{dir_name}/ contents: "));
                            ctx.push_str(&sub_entries.join(", "));
                            ctx.push('\n');
                            total_entries += sub_entries.len();
                        }
                    }
                }
            }
        }
    }

    ctx
}

/// List directory entry names (files and dirs), capped to avoid huge prompts.
fn list_dir_names(path: impl Into<PathBuf>) -> Result<Vec<String>> {
    let path = path.into();
    let mut names = Vec::new();

    let entries = fs::read_dir(&path)?;
    for entry in entries.flatten() {
        if let Ok(name) = entry.file_name().into_string() {
            if name.starts_with('.') {
                continue;
            }
            let suffix = if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                "/"
            } else {
                ""
            };
            names.push(format!("{name}{suffix}"));
        }
    }

    names.sort();
    // Cap at 50 entries to avoid bloating the prompt
    names.truncate(50);
    Ok(names)
}

/// Build git context: branch, dirty/clean, recent log.
fn build_git_context() -> String {
    use std::process::Command;

    let branch = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    let branch = match branch {
        Some(b) => b,
        None => return String::new(),
    };

    let mut ctx = format!("Git branch: {branch}");

    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    if let Some(ref s) = status {
        if s.is_empty() {
            ctx.push_str(" (clean)");
        } else {
            let changed = s.lines().count();
            ctx.push_str(&format!(" ({changed} changed files)"));
        }
    }

    let log = Command::new("git")
        .args(["log", "--oneline", "-5"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    if let Some(ref log) = log {
        if !log.is_empty() {
            ctx.push_str(&format!("\nRecent commits:\n{log}"));
        }
    }

    ctx
}

/// Read per-project AI context from `.shako.toml` in the current directory.
fn read_project_context() -> String {
    let path = std::path::Path::new(".shako.toml");
    if !path.exists() {
        return String::new();
    }

    let contents = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    #[derive(serde::Deserialize)]
    struct ProjectConfig {
        #[serde(default)]
        ai: ProjectAiConfig,
    }

    #[derive(serde::Deserialize, Default)]
    struct ProjectAiConfig {
        #[serde(default)]
        context: String,
    }

    match toml::from_str::<ProjectConfig>(&contents) {
        Ok(config) if !config.ai.context.is_empty() => config.ai.context,
        _ => String::new(),
    }
}
