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
}

/// Modern tools the AI should prefer when available.
const TOOL_PREFERENCES: &[(&str, &str)] = &[
    ("fd", "use fd instead of find"),
    ("rg", "use rg (ripgrep) instead of grep"),
    ("eza", "use eza instead of ls"),
    ("bat", "use bat instead of cat"),
    ("dust", "use dust instead of du"),
    ("sd", "use sd instead of sed"),
    ("procs", "use procs instead of ps"),
    ("delta", "use delta instead of diff"),
];

/// Build context from the current environment.
pub fn build_context() -> Result<ShellContext> {
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

    Ok(ShellContext {
        os: env::consts::OS.to_string(),
        arch: env::consts::ARCH.to_string(),
        cwd,
        shell: "jbosh".to_string(),
        user,
        available_tools,
        dir_context,
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

    // Home directory (if not already cwd)
    let home = dirs::home_dir().unwrap_or_default();
    let cwd = env::current_dir().unwrap_or_default();
    if home != cwd {
        if let Ok(entries) = list_dir_names(&home) {
            if !entries.is_empty() {
                ctx.push_str("Home directory (~) contents: ");
                ctx.push_str(&entries.join(", "));
                ctx.push('\n');
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
