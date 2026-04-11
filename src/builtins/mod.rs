//! Shell builtins — dispatcher and shared state.
//!
//! Each builtin is implemented in its own submodule; this file is the thin
//! dispatcher that routes `run_builtin` calls to the correct implementation.
//!
//! # Thread-safety of `env::set_var` / `env::remove_var`
//!
//! All builtins are called exclusively from the interactive REPL main thread,
//! never from within a `tokio::Runtime::block_on()` call or from any spawned
//! thread.  The tokio runtime used for AI features is idle (not executing tasks)
//! while the REPL loop is dispatching commands.  Therefore:
//!
//! - No concurrent readers of the process environment exist when these builtins run.
//! - All `unsafe { env::set_var(...) }` / `unsafe { env::remove_var(...) }` calls
//!   in the submodules are safe under those invariants.
//!
//! If the architecture ever changes to run builtins from async tasks, each call site
//! must be revisited.

mod alias;
mod dirs;
mod echo;
mod env;
mod history;
mod jobs;
mod nav;
mod query;
mod read;
mod set;
pub mod source;
pub mod state;
mod test;

pub use alias::{builtin_abbr, builtin_alias, builtin_unalias};
pub use dirs::{builtin_dirs, builtin_popd, builtin_pushd};
pub use echo::builtin_echo;
pub use env::{builtin_export, builtin_unset};
pub use history::builtin_history;
pub use nav::{builtin_cd, builtin_z, builtin_zi};
pub use query::{builtin_functions, builtin_type};
pub use read::builtin_read;
pub use source::{load_functions_dir, source_conf_d, source_fish_string};
pub use state::{ShellFunction, ShellState};
pub use test::builtin_test;

pub const BUILTINS: &[&str] = &[
    "cd",
    "exit",
    "export",
    "unset",
    "set",
    "source",
    "alias",
    "unalias",
    "abbr",
    "fish-import",
    "history",
    "type",
    "z",
    "zi",
    "jobs",
    "fg",
    "bg",
    "disown",
    "wait",
    "function",
    "functions",
    // Phase 2
    "echo",
    "read",
    "test",
    "[",
    "pwd",
    "pushd",
    "popd",
    "dirs",
    "true",
    "false",
    // Phase 3
    "return",
    "command",
    // Phase 4 (control flow)
    "break",
    "continue",
    "local",
    // Incident mode
    "incident",
];

// Thread-local signal for early return from user-defined functions.
// Set by `return [code]`; read and cleared by `take_function_return()`.
std::thread_local! {
    pub(crate) static FUNCTION_RETURN: std::cell::Cell<Option<i32>> = const { std::cell::Cell::new(None) };
}

/// Read and clear the FUNCTION_RETURN signal. Called by the control-flow
/// engine after each simple command to detect `return` inside control flow.
pub fn take_function_return() -> Option<i32> {
    FUNCTION_RETURN.with(|r| r.take())
}

/// Check if a token is a builtin command name.
pub fn is_builtin(token: &str) -> bool {
    BUILTINS.contains(&token)
}

/// Run a shell builtin command. Returns the exit code (0 = success).
pub fn run_builtin(input: &str, state: &mut ShellState) -> i32 {
    // Use parse_args so builtins get glob expansion, tilde expansion,
    // env var expansion, and quoting — same as regular commands.
    let parsed = crate::parser::parse_args(input);
    let parts: Vec<&str> = parsed.iter().map(|s| s.as_str()).collect();
    if parts.is_empty() {
        return 0;
    }

    match parts[0] {
        "cd" => builtin_cd(&parts[1..]),
        "z" => {
            builtin_z(&parts[1..]);
            0
        }
        "zi" => {
            builtin_zi();
            0
        }
        "exit" => std::process::exit(0),
        "export" => {
            builtin_export(&parts[1..]);
            0
        }
        "unset" => {
            builtin_unset(&parts[1..]);
            0
        }
        "set" => {
            set::builtin_set(&parts[1..]);
            0
        }
        "history" => {
            builtin_history(&parts[1..], state);
            0
        }
        "alias" => {
            builtin_alias(&parts[1..], state);
            0
        }
        "unalias" => {
            builtin_unalias(&parts[1..], state);
            0
        }
        "abbr" => {
            builtin_abbr(&parts[1..], state);
            0
        }
        "fish-import" => {
            #[cfg(feature = "fish-import")]
            crate::fish_import::run_import();
            #[cfg(not(feature = "fish-import"))]
            eprintln!("shako: fish-import: not compiled in (rebuild with --features fish-import)");
            0
        }
        "source" => {
            source::builtin_source(&parts[1..], state);
            0
        }
        "type" => builtin_type(&parts[1..], state),
        "jobs" => {
            jobs::builtin_jobs(state);
            0
        }
        "fg" => {
            jobs::builtin_fg(&parts[1..], state);
            0
        }
        "bg" => {
            jobs::builtin_bg(&parts[1..], state);
            0
        }
        "disown" => jobs::builtin_disown(&parts[1..], state),
        "wait" => jobs::builtin_wait(&parts[1..], state),
        "functions" => {
            builtin_functions(state);
            0
        }
        // Phase 2
        "echo" => builtin_echo(&parts[1..]),
        "read" => builtin_read(&parts[1..]),
        "test" => builtin_test(&parts[1..]),
        "[" => {
            let args: Vec<&str> = parts[1..].iter().copied().filter(|a| *a != "]").collect();
            builtin_test(&args)
        }
        "pwd" => {
            println!("{}", std::env::current_dir().unwrap_or_default().display());
            0
        }
        "pushd" => builtin_pushd(&parts[1..], state),
        "popd" => builtin_popd(&parts[1..], state),
        "dirs" => {
            builtin_dirs(state);
            0
        }
        "true" => 0,
        "false" => 1,
        "return" => {
            let code = parts
                .get(1)
                .and_then(|n| n.parse::<i32>().ok())
                .unwrap_or(0);
            FUNCTION_RETURN.with(|r| r.set(Some(code)));
            code
        }
        "break" | "continue" => {
            eprintln!("shako: {}: only meaningful inside a loop", parts[0]);
            1
        }
        "local" => {
            eprintln!("shako: local: only meaningful inside a function");
            1
        }
        "command" => {
            // Run a command bypassing aliases/functions (like fish's `command`).
            if parts.len() < 2 {
                eprintln!("shako: command: missing command name");
                return 1;
            }
            let cmd = parts[1..].join(" ");
            crate::executor::execute_command(&cmd)
                .and_then(|s| s.code())
                .unwrap_or(0)
        }
        "incident" => builtin_incident(&parts[1..], state),
        other => {
            eprintln!("shako: unknown builtin: {other}");
            1
        }
    }
}

/// Handle the `incident` builtin: start / status / end / report.
fn builtin_incident(args: &[&str], state: &mut ShellState) -> i32 {
    use crate::incident::{self, IncidentSession};

    let subcommand = args.first().copied().unwrap_or("status");

    match subcommand {
        "start" => {
            if state.incident_session.is_some() {
                let id = state.incident_session.as_ref().unwrap().id();
                eprintln!("shako: incident already active: {id}");
                eprintln!("  Use `incident end` or `incident report` first.");
                return 1;
            }
            let name = if args.len() > 1 {
                args[1..].join("-")
            } else {
                // Default name based on timestamp
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                format!("incident-{ts}")
            };
            let session = IncidentSession::new(&name);
            let id = session.id();
            eprintln!("\x1b[1;31m⚡ INCIDENT MODE ACTIVATED\x1b[0m");
            eprintln!("\x1b[90m  ID:   {id}\x1b[0m");
            eprintln!("\x1b[90m  All commands will be timestamped and recorded.\x1b[0m");
            eprintln!("\x1b[90m  Run `incident report` when done to generate a runbook.\x1b[0m");
            state.incident_session = Some(session);
            // Signal prompt to show [INC] indicator
            crate::shell::prompt::set_incident_active(true);
            0
        }
        "status" => match &state.incident_session {
            None => {
                eprintln!("shako: no active incident session");
                eprintln!("  Start one with: incident start <name>");
                1
            }
            Some(session) => {
                eprintln!("\x1b[1;31m⚡ INCIDENT ACTIVE\x1b[0m  {}", session.id());
                eprintln!(
                    "  Elapsed: {}  |  Steps recorded: {}",
                    session.elapsed_display(),
                    session.steps.len()
                );
                if !session.steps.is_empty() {
                    eprintln!("\n  Last 5 steps:");
                    let start = session.steps.len().saturating_sub(5);
                    for step in &session.steps[start..] {
                        let icon = if step.exit_code == 0 { "✓" } else { "✗" };
                        eprintln!(
                            "  \x1b[90m{icon} exit={:<3}  $ {}\x1b[0m",
                            step.exit_code, step.command
                        );
                    }
                }
                0
            }
        },
        "end" => {
            match state.incident_session.take() {
                None => {
                    eprintln!("shako: no active incident session");
                    1
                }
                Some(session) => {
                    eprintln!(
                        "\x1b[90mIncident {} ended after {} ({} steps recorded).\x1b[0m",
                        session.id(),
                        session.elapsed_display(),
                        session.steps.len()
                    );
                    eprintln!("\x1b[90m  Tip: use `incident report` next time to generate a runbook.\x1b[0m");
                    crate::shell::prompt::set_incident_active(false);
                    0
                }
            }
        }
        "report" => {
            // Pull the session out so we can pass it to the AI without borrowing `state`.
            match state.incident_session.take() {
                None => {
                    eprintln!("shako: no active incident session");
                    1
                }
                Some(session) => {
                    crate::shell::prompt::set_incident_active(false);
                    eprintln!(
                        "\x1b[1;31m⚡\x1b[0m Incident {} ended after {} ({} steps).",
                        session.id(),
                        session.elapsed_display(),
                        session.steps.len()
                    );

                    if session.steps.is_empty() {
                        eprintln!("  No steps were recorded — nothing to report.");
                        return 0;
                    }

                    let step_log = session.step_log();
                    let incident_name = session.name.clone();
                    let incident_id = session.id();

                    // Print the step log first so the user can review it while waiting.
                    eprintln!(
                        "\n\x1b[90m─── Step Journal ───────────────────────────────────\x1b[0m"
                    );
                    for line in step_log.lines() {
                        eprintln!("  \x1b[90m{line}\x1b[0m");
                    }
                    eprintln!(
                        "\x1b[90m────────────────────────────────────────────────────\x1b[0m\n"
                    );

                    // We need a config reference; access it via a thread-local trick or
                    // just emit the step log as a formatted markdown report without AI.
                    // For the AI call we need the runtime — store the step_log and
                    // generate a minimal markdown report synchronously, then offer the
                    // AI-enhanced version if AI is enabled (handled by the REPL caller).
                    // For now: emit a complete markdown report synchronously.
                    let md =
                        incident::build_markdown_report(&incident_id, &incident_name, &step_log);
                    eprintln!("\x1b[90mGenerated post-incident report:\x1b[0m");
                    println!("{md}");

                    // Attempt auto-save to configured runbook_dir if we can find it.
                    // We read it from the local .shako.toml `[incident] runbook_dir` key.
                    if let Some(save_path) = incident_save_path(&incident_id) {
                        match std::fs::write(&save_path, &md) {
                            Ok(_) => eprintln!("\x1b[90m  Saved: {}\x1b[0m", save_path.display()),
                            Err(e) => eprintln!("shako: could not save runbook: {e}"),
                        }
                    }

                    0
                }
            }
        }
        _ => {
            eprintln!("shako: incident: unknown subcommand: {subcommand}");
            eprintln!("  Usage: incident [start <name>|status|end|report]");
            1
        }
    }
}

/// Determine where to save the runbook markdown file.
/// Reads `.shako.toml` in the current directory for `[incident] runbook_dir`.
fn incident_save_path(incident_id: &str) -> Option<std::path::PathBuf> {
    use std::io::BufRead;

    // Try .shako.toml in the current directory.
    let toml_path = std::env::current_dir().ok()?.join(".shako.toml");
    if !toml_path.exists() {
        return None;
    }
    let file = std::fs::File::open(&toml_path).ok()?;
    let reader = std::io::BufReader::new(file);

    let mut in_incident_section = false;
    let mut runbook_dir: Option<String> = None;

    for line in reader.lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed == "[incident]" {
            in_incident_section = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_incident_section = false;
        }
        if in_incident_section {
            if let Some(val) = trimmed.strip_prefix("runbook_dir") {
                // e.g. `runbook_dir = "~/incidents"`
                let val = val.trim_start_matches(|c: char| c == ' ' || c == '=');
                let val = val.trim().trim_matches('"').trim_matches('\'');
                runbook_dir = Some(val.to_string());
            }
        }
    }

    let dir_str = runbook_dir?;
    let dir_str = if dir_str.starts_with('~') {
        let home = std::env::var("HOME").unwrap_or_default();
        dir_str.replacen('~', &home, 1)
    } else {
        dir_str
    };

    let dir = std::path::PathBuf::from(dir_str);
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(format!("{incident_id}.md")))
}

/// Try to parse and register a function definition.
/// Returns true if the input was a function definition.
/// Supports: `function name() { body }` and `function name { body }`
pub fn try_define_function(input: &str, state: &mut ShellState) -> bool {
    let trimmed = input.trim();

    let rest = match trimmed.strip_prefix("function ") {
        Some(r) => r.trim(),
        None => return false,
    };

    // Extract function name
    let name_end = rest
        .find(|c: char| c == '(' || c == '{' || c.is_whitespace())
        .unwrap_or(rest.len());
    let name = rest[..name_end].trim().to_string();
    if name.is_empty() {
        eprintln!("shako: function: missing name");
        return true;
    }

    let after_name = rest[name_end..].trim();

    // Strip optional "()"
    let after_parens = after_name
        .strip_prefix("()")
        .map(|s| s.trim())
        .unwrap_or(after_name);

    // Extract body between { }
    let body = if let Some(inner) = after_parens.strip_prefix('{') {
        if let Some(body) = inner.strip_suffix('}') {
            body.trim().to_string()
        } else {
            eprintln!("shako: function: missing closing '}}' for {name}");
            return true;
        }
    } else {
        eprintln!("shako: function: missing '{{' for {name}");
        return true;
    };

    state.functions.insert(name.clone(), ShellFunction { body });
    true
}

/// Run a shell function by parsing its body through the control-flow engine.
/// Returns the exit code: the argument to `return`, the last command's exit
/// code, or 0 if the body was empty.
pub fn run_function(func: &ShellFunction, args: &[&str]) -> i32 {
    use std::env;
    // Set positional parameters as env vars
    for (i, arg) in args.iter().enumerate() {
        unsafe { env::set_var(format!("{}", i + 1), arg) };
    }
    unsafe { env::set_var("@", args.join(" ")) };
    unsafe { env::set_var("#", args.len().to_string()) };

    let stmts = crate::control::parse_body(&func.body);
    let mut locals: Vec<(String, Option<String>)> = Vec::new();

    let code = match crate::control::exec_statements(&stmts, &mut locals) {
        crate::control::ExecSignal::Normal(c) => c,
        crate::control::ExecSignal::Return(c) => c,
        crate::control::ExecSignal::Break => {
            eprintln!("shako: break: only meaningful inside a loop");
            0
        }
        crate::control::ExecSignal::Continue => {
            eprintln!("shako: continue: only meaningful inside a loop");
            0
        }
    };

    // Restore local variables (innermost first)
    for (var, old_val) in locals.iter().rev() {
        match old_val {
            Some(v) => unsafe { env::set_var(var, v) },
            None => unsafe { env::remove_var(var) },
        }
    }

    // Clear any stale return signal
    FUNCTION_RETURN.with(|r| r.set(None));

    // Clean up positional parameters
    for i in 1..=args.len() {
        unsafe { env::remove_var(format!("{i}")) };
    }
    unsafe { env::remove_var("@") };
    unsafe { env::remove_var("#") };

    code
}

/// Dispatch a builtin that does not need `ShellState` (usable inside function
/// bodies and the control-flow engine where we don't have access to the REPL state).
/// Public so `control.rs` can call it for conditions and simple statements.
pub fn run_builtin_stateless(line: &str) -> i32 {
    let parsed = crate::parser::parse_args(line);
    let parts: Vec<&str> = parsed.iter().map(|s| s.as_str()).collect();
    let first = parts.first().copied().unwrap_or("");
    run_builtin_no_state(first, line)
}

fn run_builtin_no_state(first: &str, line: &str) -> i32 {
    let parsed = crate::parser::parse_args(line);
    let parts: Vec<&str> = parsed.iter().map(|s| s.as_str()).collect();
    match first {
        "echo" => builtin_echo(&parts[1..]),
        "read" => builtin_read(&parts[1..]),
        "test" => builtin_test(&parts[1..]),
        "[" => {
            let args: Vec<&str> = parts[1..].iter().copied().filter(|a| *a != "]").collect();
            builtin_test(&args)
        }
        "pwd" => {
            println!("{}", std::env::current_dir().unwrap_or_default().display());
            0
        }
        "true" => 0,
        "false" => 1,
        "return" => {
            let code = parts
                .get(1)
                .and_then(|n| n.parse::<i32>().ok())
                .unwrap_or(0);
            FUNCTION_RETURN.with(|r| r.set(Some(code)));
            code
        }
        "exit" => std::process::exit(0),
        "cd" => builtin_cd(&parts[1..]),
        "export" => {
            builtin_export(&parts[1..]);
            0
        }
        "unset" => {
            builtin_unset(&parts[1..]);
            0
        }
        "break" | "continue" => {
            eprintln!("shako: {first}: only meaningful inside a loop");
            1
        }
        "local" => {
            eprintln!("shako: local: only meaningful inside a function");
            1
        }
        other => {
            eprintln!("shako: {other}: builtin not available inside function body");
            127
        }
    }
}
