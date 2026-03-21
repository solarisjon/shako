use anyhow::Result;
use std::env;
use which::which;

/// Shell context sent to the LLM for better command translation.
pub struct ShellContext {
    pub os: String,
    pub arch: String,
    pub cwd: String,
    pub shell: String,
    pub user: String,
    pub available_tools: Vec<(&'static str, &'static str)>,
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

    Ok(ShellContext {
        os: env::consts::OS.to_string(),
        arch: env::consts::ARCH.to_string(),
        cwd,
        shell: "jbosh".to_string(),
        user,
        available_tools,
    })
}
