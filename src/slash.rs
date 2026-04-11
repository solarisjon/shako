use std::path::Path;

use crate::ai;
use crate::config::ShakoConfig;

pub const SLASH_COMMANDS: &[(&str, &str)] = &[
    ("help", "List available slash commands"),
    ("history", "Fuzzy-browse shell history and select a command"),
    ("validate", "Validate the AI endpoint connection"),
    ("config", "Show current configuration"),
    ("model", "Show or switch the active AI model/provider"),
    ("safety", "Show or change safety mode (warn/block/off)"),
    ("provider", "Show or switch the active LLM provider"),
    (
        "audit",
        "Verify audit log chain or search AI history (/audit verify|search <q>)",
    ),
];

/// The outcome of running a slash command.
///
/// Most commands return a simple exit `Code`. The `/history` command may
/// return a `Prefill` string — a history entry the user selected — that the
/// REPL loop should offer for confirmation/editing before execution.
pub enum SlashOutcome {
    Code(i32),
    Prefill(String),
}

/// Run a slash command and return its outcome.
///
/// `history_path` is passed through so that history-aware commands (e.g.
/// `/history`) can read the history file without duplicating path logic.
pub fn run(
    name: &str,
    args: &str,
    config: &mut ShakoConfig,
    rt: &tokio::runtime::Runtime,
    history_path: &Path,
) -> SlashOutcome {
    match name {
        "help" => SlashOutcome::Code(cmd_help()),
        "history" => cmd_history(args, history_path),
        "validate" => SlashOutcome::Code(cmd_validate(config, rt)),
        "config" => SlashOutcome::Code(cmd_config(config)),
        "model" => SlashOutcome::Code(cmd_model(args, config)),
        "safety" => SlashOutcome::Code(cmd_safety(args, config)),
        "provider" => SlashOutcome::Code(cmd_provider(args, config)),
        "audit" => SlashOutcome::Code(cmd_audit(args)),
        _ => {
            eprintln!("shako: unknown command /{name}");
            eprintln!("       run /help to see available commands");
            SlashOutcome::Code(1)
        }
    }
}

fn cmd_help() -> i32 {
    eprintln!("\x1b[1mshako slash commands\x1b[0m\n");
    for (name, desc) in SLASH_COMMANDS {
        eprintln!("  \x1b[36m/{name:<12}\x1b[0m {desc}");
    }
    eprintln!();
    0
}

// ─── /history ────────────────────────────────────────────────────────────────

/// Run the `/history` fuzzy browser.
///
/// Strategy (in order):
/// 1. If `fzf` is in PATH, pipe history through it and capture the selection.
/// 2. If `sk` (skim) is in PATH, use it instead.
/// 3. Fall back to a native crossterm TUI with arrow-key navigation.
///
/// Returns `SlashOutcome::Prefill(cmd)` when the user picks an entry, or
/// `SlashOutcome::Code(0)` when they cancel / no history exists.
fn cmd_history(_args: &str, history_path: &Path) -> SlashOutcome {
    // Read deduplicated history (most-recent at the end).
    let entries = read_history_entries(history_path);
    if entries.is_empty() {
        eprintln!("\x1b[33mshako: history is empty\x1b[0m");
        return SlashOutcome::Code(0);
    }

    // Try external fuzzy pickers first.
    if let Some(selected) = try_external_picker(
        &entries,
        "fzf",
        &[
            "--height=40%",
            "--reverse",
            "--prompt=history> ",
            "--info=inline",
        ],
    ) {
        return SlashOutcome::Prefill(selected);
    }
    if let Some(selected) = try_external_picker(
        &entries,
        "sk",
        &["--height=40%", "--reverse", "--prompt=history> "],
    ) {
        return SlashOutcome::Prefill(selected);
    }

    // Native crossterm fallback.
    match native_history_picker(&entries) {
        Some(selected) => SlashOutcome::Prefill(selected),
        None => SlashOutcome::Code(0),
    }
}

/// Read all history lines, deduplicated and most-recent-last.
fn read_history_entries(history_path: &Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(history_path) else {
        return Vec::new();
    };
    // Deduplicate while preserving order (last occurrence wins).
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for line in content.lines().rev() {
        let line = line.trim();
        if !line.is_empty() && seen.insert(line.to_string()) {
            out.push(line.to_string());
        }
    }
    out.reverse(); // oldest → newest
    out
}

/// Try running an external fuzzy picker, returning the selected line or None.
fn try_external_picker(entries: &[String], bin: &str, args: &[&str]) -> Option<String> {
    if which::which(bin).is_err() {
        return None;
    }

    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // fzf/sk need to talk to the real terminal for interactive display
        .stderr(
            std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/tty")
                .map(|f| Stdio::from(f))
                .unwrap_or(Stdio::inherit()),
        )
        .spawn()
        .ok()?;

    // Feed history lines newest-first so the most recent is at the top.
    if let Some(stdin) = child.stdin.take() {
        let mut stdin = stdin;
        for entry in entries.iter().rev() {
            let _ = writeln!(stdin, "{entry}");
        }
    }

    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None; // user pressed Esc
    }

    let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if selected.is_empty() {
        None
    } else {
        Some(selected)
    }
}

/// Native crossterm-based interactive history picker.
///
/// Renders a scrollable, filterable list using raw terminal mode.
/// Arrow keys navigate; typing filters; Enter selects; Esc cancels.
fn native_history_picker(entries: &[String]) -> Option<String> {
    use crossterm::{cursor, execute, terminal};
    use std::io;

    let Ok(_) = terminal::enable_raw_mode() else {
        eprintln!("shako: /history: cannot enable raw mode");
        return None;
    };

    let mut stdout = io::stderr();
    let _ = execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide);

    let result = run_native_picker(&mut stdout, entries);

    let _ = execute!(stdout, terminal::LeaveAlternateScreen, cursor::Show);
    let _ = terminal::disable_raw_mode();

    result
}

fn run_native_picker(out: &mut impl std::io::Write, entries: &[String]) -> Option<String> {
    use crossterm::{
        cursor,
        event::{self, Event, KeyCode, KeyModifiers},
        queue,
        style::{self, Color, Stylize},
        terminal::{self, ClearType},
    };

    let mut query = String::new();
    let mut selected_idx: usize = 0;
    let visible_rows: usize = 15;

    loop {
        // Filter entries (newest-first for display)
        let filtered: Vec<&str> = entries
            .iter()
            .rev()
            .filter(|e| {
                if query.is_empty() {
                    true
                } else {
                    e.to_lowercase().contains(&query.to_lowercase())
                }
            })
            .map(|s| s.as_str())
            .collect();

        let total = filtered.len();
        if selected_idx >= total && total > 0 {
            selected_idx = total - 1;
        }

        // ── render ─────────────────────────────────────────────────────────

        let (term_w, _) = terminal::size().unwrap_or((80, 24));
        let term_w = term_w as usize;

        // Clear and move to top
        let _ = queue!(out, cursor::MoveTo(0, 0), terminal::Clear(ClearType::All),);

        // Border/header
        let header = format!(" ╭─ history ─ {} matches / {} total ", total, entries.len());
        let header_rest = "─".repeat(term_w.saturating_sub(header.chars().count() + 1));
        let _ = queue!(
            out,
            style::PrintStyledContent(format!("{header}{header_rest}╮").with(style::Color::Cyan)),
            cursor::MoveToNextLine(1),
        );

        // Query line
        let prompt = format!("  │ > {query}");
        let pad = term_w.saturating_sub(prompt.chars().count() + 3);
        let _ = queue!(
            out,
            style::PrintStyledContent("  │ ".with(Color::Cyan)),
            style::PrintStyledContent("> ".with(Color::White)),
            style::PrintStyledContent(query.clone().bold()),
            style::Print(" ".repeat(pad)),
            style::PrintStyledContent("  │".with(Color::Cyan)),
            cursor::MoveToNextLine(1),
        );

        // Separator
        let sep = format!("  ├{}┤", "─".repeat(term_w.saturating_sub(4)));
        let _ = queue!(
            out,
            style::PrintStyledContent(sep.with(Color::Cyan)),
            cursor::MoveToNextLine(1),
        );

        // List entries (scroll window)
        let window_start = selected_idx
            .saturating_sub(visible_rows / 2)
            .min(total.saturating_sub(visible_rows));
        let window = &filtered[window_start..total.min(window_start + visible_rows)];

        for (i, entry) in window.iter().enumerate() {
            let abs_idx = window_start + i;
            let is_selected = abs_idx == selected_idx;
            let prefix = if is_selected {
                "  │ ▶ "
            } else {
                "  │   "
            };
            let avail = term_w.saturating_sub(prefix.chars().count() + 3);
            let entry_display: String = entry.chars().take(avail).collect();
            let pad = avail.saturating_sub(entry_display.chars().count());

            let _ = queue!(out, style::PrintStyledContent("  │".with(Color::Cyan)));
            if is_selected {
                let _ = queue!(
                    out,
                    style::PrintStyledContent(
                        format!(" ▶ {entry_display}{}", " ".repeat(pad))
                            .bold()
                            .with(Color::Cyan)
                    ),
                    style::PrintStyledContent("  │".with(Color::Cyan)),
                );
            } else {
                let _ = queue!(
                    out,
                    style::Print(format!("   {entry_display}{}", " ".repeat(pad))),
                    style::PrintStyledContent("  │".with(Color::Cyan)),
                );
            }
            let _ = queue!(out, cursor::MoveToNextLine(1));
        }

        // Pad remaining rows
        for _ in window.len()..visible_rows {
            let blank = format!("  │{}  │", " ".repeat(term_w.saturating_sub(4)));
            let _ = queue!(
                out,
                style::PrintStyledContent(blank.with(Color::Cyan)),
                cursor::MoveToNextLine(1),
            );
        }

        // Footer
        let footer = format!("  ╰{}╯", "─".repeat(term_w.saturating_sub(4)));
        let _ = queue!(
            out,
            style::PrintStyledContent(footer.with(Color::Cyan)),
            cursor::MoveToNextLine(1),
        );

        // Key hint
        let _ = queue!(
            out,
            style::PrintStyledContent(
                "  ↑↓ navigate  Enter select  Esc cancel  type to filter\n".with(Color::DarkGrey)
            ),
        );

        let _ = out.flush();

        // ── input ──────────────────────────────────────────────────────────

        match event::read().ok()? {
            Event::Key(key) => match (key.code, key.modifiers) {
                (KeyCode::Esc, _)
                | (KeyCode::Char('c'), KeyModifiers::CONTROL)
                | (KeyCode::Char('q'), KeyModifiers::NONE) => {
                    return None;
                }
                (KeyCode::Enter, _) => {
                    if total > 0 {
                        return Some(filtered[selected_idx].to_string());
                    }
                }
                (KeyCode::Up, _) | (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                    if selected_idx > 0 {
                        selected_idx -= 1;
                    }
                }
                (KeyCode::Down, _) | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                    if selected_idx + 1 < total {
                        selected_idx += 1;
                    }
                }
                (KeyCode::Backspace, _) => {
                    query.pop();
                    selected_idx = 0;
                }
                (KeyCode::Char(c), KeyModifiers::NONE)
                | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                    query.push(c);
                    selected_idx = 0;
                }
                _ => {}
            },
            _ => {}
        }
    }
}

// ─── /validate ───────────────────────────────────────────────────────────────

fn cmd_validate(config: &ShakoConfig, rt: &tokio::runtime::Runtime) -> i32 {
    let llm = config.active_llm();
    let provider_label = config.active_provider.as_deref().unwrap_or("llm (default)");

    eprintln!("\x1b[90mvalidating provider \x1b[1m{provider_label}\x1b[0m\x1b[90m...\x1b[0m");
    eprintln!("  endpoint:  {}", llm.endpoint);
    eprintln!("  model:     {}", llm.model);
    eprintln!(
        "  api key:   {} {}",
        llm.api_key_env,
        if std::env::var(&llm.api_key_env).is_ok() {
            "(set)"
        } else {
            "(not set)"
        }
    );

    let result = rt.block_on(ai::client::check_ai_session(
        llm,
        config.behavior.ai_enabled,
    ));

    match result {
        ai::client::AiCheckResult::Ready => {
            eprintln!("\x1b[32m  status:    ready\x1b[0m");
            0
        }
        ai::client::AiCheckResult::Disabled => {
            eprintln!("\x1b[33m  status:    AI disabled (ai_enabled = false)\x1b[0m");
            0
        }
        ai::client::AiCheckResult::NoApiKey(env_var) => {
            eprintln!("\x1b[31m  status:    no API key (set ${env_var})\x1b[0m");
            1
        }
        ai::client::AiCheckResult::AuthFailed(code) => {
            eprintln!("\x1b[31m  status:    auth failed (HTTP {code})\x1b[0m");
            1
        }
        ai::client::AiCheckResult::Unreachable(reason) => {
            eprintln!("\x1b[31m  status:    unreachable ({reason})\x1b[0m");
            1
        }
    }
}

fn cmd_config(config: &ShakoConfig) -> i32 {
    let llm = config.active_llm();
    let provider_label = config.active_provider.as_deref().unwrap_or("llm (default)");

    eprintln!("\x1b[1mshako configuration\x1b[0m\n");

    eprintln!("\x1b[36m[active provider: {provider_label}]\x1b[0m");
    eprintln!("  endpoint         = {}", llm.endpoint);
    eprintln!("  model            = {}", llm.model);
    eprintln!("  api_key_env      = {}", llm.api_key_env);
    eprintln!("  timeout_secs     = {}", llm.timeout_secs);
    eprintln!("  max_tokens       = {}", llm.max_tokens);
    eprintln!("  temperature      = {}", llm.temperature);
    eprintln!("  verify_ssl       = {}", llm.verify_ssl);

    if !config.providers.is_empty() {
        eprintln!("\n\x1b[36m[providers]\x1b[0m");
        for name in config.providers.keys() {
            let marker = if config.active_provider.as_deref() == Some(name) {
                " (active)"
            } else {
                ""
            };
            eprintln!("  {name}{marker}");
        }
    }

    eprintln!("\n\x1b[36m[behavior]\x1b[0m");
    eprintln!("  ai_enabled           = {}", config.behavior.ai_enabled);
    eprintln!(
        "  confirm_ai_commands  = {}",
        config.behavior.confirm_ai_commands
    );
    eprintln!(
        "  auto_correct_typos   = {}",
        config.behavior.auto_correct_typos
    );
    eprintln!("  safety_mode          = {}", config.behavior.safety_mode);
    eprintln!("  edit_mode            = {}", config.behavior.edit_mode);
    eprintln!(
        "  history_context      = {}",
        config.behavior.history_context_lines
    );

    if !config.aliases.is_empty() {
        eprintln!("\n\x1b[36m[aliases]\x1b[0m");
        for (k, v) in &config.aliases {
            eprintln!("  {k} = {v}");
        }
    }

    eprintln!();
    0
}

fn cmd_model(args: &str, config: &ShakoConfig) -> i32 {
    if args.is_empty() {
        let llm = config.active_llm();
        let provider_label = config.active_provider.as_deref().unwrap_or("llm (default)");
        eprintln!("\x1b[36m{provider_label}\x1b[0m: {}", llm.model);
        return 0;
    }
    eprintln!("shako: runtime model switching not yet supported");
    eprintln!("       edit ~/.config/shako/config.toml to change models");
    1
}

fn cmd_safety(args: &str, config: &mut ShakoConfig) -> i32 {
    if args.is_empty() {
        eprintln!(
            "safety_mode = \x1b[1m{}\x1b[0m",
            config.behavior.safety_mode
        );
        eprintln!("  warn  — show warning for dangerous commands");
        eprintln!("  block — block dangerous commands entirely");
        eprintln!("  off   — no safety checks");
        return 0;
    }

    match args {
        "warn" | "block" | "off" => {
            config.behavior.safety_mode = args.to_string();
            eprintln!("safety_mode = \x1b[1m{args}\x1b[0m (session only)");
            0
        }
        _ => {
            eprintln!("shako: invalid safety mode '{args}'");
            eprintln!("       valid modes: warn, block, off");
            1
        }
    }
}

fn cmd_provider(args: &str, config: &mut ShakoConfig) -> i32 {
    if args.is_empty() {
        let current = config
            .active_provider
            .as_deref()
            .unwrap_or("(default [llm])");
        eprintln!("active provider: \x1b[1m{current}\x1b[0m");
        if !config.providers.is_empty() {
            eprintln!("\navailable providers:");
            for (name, p) in &config.providers {
                let marker = if config.active_provider.as_deref() == Some(name.as_str()) {
                    " \x1b[32m(active)\x1b[0m"
                } else {
                    ""
                };
                eprintln!("  \x1b[36m{name}\x1b[0m — {}{marker}", p.model);
            }
        }
        return 0;
    }

    if config.providers.contains_key(args) {
        config.active_provider = Some(args.to_string());
        let model = &config.providers[args].model;
        eprintln!("switched to \x1b[1m{args}\x1b[0m ({model}) (session only)");
        0
    } else {
        eprintln!("shako: unknown provider '{args}'");
        if !config.providers.is_empty() {
            let names: Vec<&str> = config.providers.keys().map(|s| s.as_str()).collect();
            eprintln!("       available: {}", names.join(", "));
        }
        1
    }
}

// ─── /audit ───────────────────────────────────────────────────────────────────

/// Handle the `/audit` slash command.
///
/// Subcommands:
/// - `/audit verify`          — re-compute the hash chain and report integrity
/// - `/audit search <query>`  — search past AI queries by keyword
fn cmd_audit(args: &str) -> i32 {
    let args = args.trim();
    let (sub, rest) = match args.split_once(' ') {
        Some((s, r)) => (s.trim(), r.trim()),
        None => (args, ""),
    };

    match sub {
        "verify" | "" => {
            let path = crate::audit::audit_path();
            if !path.exists() {
                eprintln!("\x1b[90mshako: audit log is empty (no entries yet)\x1b[0m");
                return 0;
            }
            match crate::audit::verify_chain() {
                Ok(count) => {
                    eprintln!(
                        "\x1b[32mshako: audit log OK — {count} entr{} verified, chain intact\x1b[0m",
                        if count == 1 { "y" } else { "ies" }
                    );
                    0
                }
                Err(e) => {
                    eprintln!("\x1b[1;31mshako: audit log TAMPERED — {e}\x1b[0m");
                    1
                }
            }
        }
        "search" => {
            if rest.is_empty() {
                eprintln!("shako: /audit search requires a query");
                eprintln!("       usage: /audit search <keyword>");
                return 1;
            }
            let results = crate::audit::search_entries(rest, 20);
            if results.is_empty() {
                eprintln!("\x1b[90mshako: no audit entries matching '{rest}'\x1b[0m");
                return 0;
            }
            eprintln!(
                "\x1b[90mshako: found {} match{} for '{rest}':\x1b[0m",
                results.len(),
                if results.len() == 1 { "" } else { "es" }
            );
            for entry in &results {
                let kind = match entry.kind {
                    crate::audit::AuditKind::AiQuery => "ai",
                    crate::audit::AuditKind::DirectCommand => "cmd",
                    crate::audit::AuditKind::SafetyBlock => "block",
                    crate::audit::AuditKind::ExfilBlock => "exfil",
                };
                let display = if !entry.executed.is_empty() {
                    &entry.executed
                } else if !entry.generated.is_empty() {
                    &entry.generated
                } else {
                    &entry.nl_input
                };
                eprintln!(
                    "  \x1b[90m[{}]\x1b[0m \x1b[36m{kind:<5}\x1b[0m  {}",
                    &entry.ts[..10],
                    display
                );
                if !entry.nl_input.is_empty() && entry.nl_input != *display {
                    eprintln!("         \x1b[90mintent: {}\x1b[0m", entry.nl_input);
                }
            }
            0
        }
        _ => {
            eprintln!("shako: unknown /audit subcommand: '{sub}'");
            eprintln!("       usage: /audit verify | /audit search <query>");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Runtime::new()
            .expect("failed to create tokio runtime for slash command test")
    }

    fn dummy_history() -> PathBuf {
        PathBuf::from("/nonexistent/history.txt")
    }

    /// Helper: run a slash command and extract the exit code.
    /// `/history` is not tested here because it requires a TTY.
    fn run_code(name: &str, args: &str, config: &mut ShakoConfig) -> i32 {
        let rt = make_runtime();
        match run(name, args, config, &rt, &dummy_history()) {
            SlashOutcome::Code(c) => c,
            SlashOutcome::Prefill(_) => 0,
        }
    }

    #[test]
    fn test_slash_commands_list_not_empty() {
        assert!(!SLASH_COMMANDS.is_empty());
    }

    #[test]
    fn test_slash_commands_includes_history() {
        let names: Vec<&str> = SLASH_COMMANDS.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"history"), "/history must be listed");
    }

    #[test]
    fn test_unknown_command_returns_1() {
        let mut config = ShakoConfig::default();
        assert_eq!(run_code("nonexistent", "", &mut config), 1);
    }

    #[test]
    fn test_help_returns_0() {
        let mut config = ShakoConfig::default();
        assert_eq!(run_code("help", "", &mut config), 0);
    }

    #[test]
    fn test_config_returns_0() {
        let mut config = ShakoConfig::default();
        assert_eq!(run_code("config", "", &mut config), 0);
    }

    #[test]
    fn test_model_no_args_returns_0() {
        let mut config = ShakoConfig::default();
        assert_eq!(run_code("model", "", &mut config), 0);
    }

    #[test]
    fn test_safety_no_args_returns_0() {
        let mut config = ShakoConfig::default();
        assert_eq!(run_code("safety", "", &mut config), 0);
    }

    #[test]
    fn test_safety_set_valid_mode() {
        let mut config = ShakoConfig::default();
        assert_eq!(run_code("safety", "off", &mut config), 0);
        assert_eq!(config.behavior.safety_mode, "off");
    }

    #[test]
    fn test_safety_set_invalid_mode() {
        let mut config = ShakoConfig::default();
        assert_eq!(run_code("safety", "invalid", &mut config), 1);
    }

    #[test]
    fn test_provider_no_args_returns_0() {
        let mut config = ShakoConfig::default();
        assert_eq!(run_code("provider", "", &mut config), 0);
    }

    #[test]
    fn test_provider_switch_unknown() {
        let mut config = ShakoConfig::default();
        assert_eq!(run_code("provider", "nonexistent", &mut config), 1);
    }

    #[test]
    fn test_history_no_tty_returns_code() {
        // In a non-TTY test environment (no fzf/sk, stdin not a terminal),
        // /history should fall back gracefully and return a Code outcome.
        let rt = make_runtime();
        let result = run(
            "history",
            "",
            &mut ShakoConfig::default(),
            &rt,
            &dummy_history(),
        );
        // We only care that it doesn't panic; the exact code may vary.
        match result {
            SlashOutcome::Code(_) | SlashOutcome::Prefill(_) => {}
        }
    }
}
