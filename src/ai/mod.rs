pub mod capability_scope;
pub mod client;
pub mod confirm;
pub mod context;
pub mod exfil_guard;
pub mod prompt;
pub mod prompt_guard;
pub mod render;

use crate::config::ShakoConfig;
use crate::journal;
use anyhow::Result;
use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// Translate natural language to a shell command via LLM, confirm, and execute.
///
/// When `config.behavior.session_journal` is true, appends a journal record
/// to `~/.local/share/shako/journal.jsonl` for every confirmed execution.
pub async fn translate_and_execute(
    input: &str,
    config: &ShakoConfig,
    recent_history: Vec<String>,
    session_memory: &mut Vec<(String, String)>,
) -> Result<()> {
    if !config.behavior.ai_enabled {
        eprintln!("shako: ai is disabled (set ai_enabled = true in config to enable)");
        return Ok(());
    }
    let guard_cfg = prompt_guard::GuardConfig {
        enabled: config.security.prompt_injection_guard,
    };
    let mut ctx = context::build_context(recent_history, session_memory.clone(), Some(guard_cfg.clone()))?;

    // Sanitize `ai_system_prompt_extra` (from user config) before LLM injection.
    ctx.system_prompt_extra = config.behavior.ai_system_prompt_extra.as_deref().map(|raw| {
        let safe = prompt_guard::sanitize_or_warn(
            raw,
            "`[behavior].ai_system_prompt_extra` in config.toml",
            &guard_cfg,
        );
        if safe.is_empty() { None } else { Some(safe) }
    }).flatten();
    let system_prompt = prompt::system_prompt(&ctx);

    let mut current_input = input.to_string();

    'translate: loop {
        let response =
            client::query_llm(&system_prompt, &current_input, config.active_llm()).await?;
        let command = collapse_multiline(response.trim());

        if command == "SHAKO_CANNOT_TRANSLATE" || command.is_empty() {
            eprintln!("shako: couldn't translate that to a command");
            return Ok(());
        }

        // Safety check on AI-generated commands
        if config.behavior.safety_mode != "off" && crate::safety::is_dangerous(&command) {
            if config.behavior.safety_mode == "block" {
                eprintln!("shako: dangerous command blocked: {command}");
                if config.security.audit_log {
                    crate::audit::record_safety_block(&command, "dangerous_command_block");
                }
                return Ok(());
            }
            eprintln!("shako: dangerous command detected: {command}");
        }

        // Secret Canary: scan for credential exfiltration patterns.
        // This runs regardless of `confirm_ai_commands` — even in auto-execute
        // mode the user must see a warning for Critical risk.
        let exfil_risk = exfil_guard::scan(&command);
        if exfil_risk.is_risky() {
            exfil_guard::print_risk_warning(&exfil_risk);
            // Critical + safety_mode block → refuse to execute.
            if matches!(exfil_risk, exfil_guard::ExfilRisk::Critical { .. })
                && config.behavior.safety_mode == "block"
            {
                eprintln!("shako: command blocked by Secret Canary (credential exfiltration risk)");
                if config.security.audit_log {
                    crate::audit::record_exfil_block(&command, "critical_exfil_risk");
                }
                return Ok(());
            }
        }

        // Capability scope check: enforce per-project command allowlist.
        // Runs before the confirm prompt so out-of-scope commands are never
        // shown to the user — instead we ask the AI to regenerate.
        if let Some(scope) = capability_scope::CapabilityScope::load_from_project() {
            let verdict = scope.check(&command);
            if verdict.is_denied() {
                capability_scope::print_scope_denial(&verdict, &scope);
                eprintln!(
                    "\x1b[90mshako: asking AI to regenerate within project scope…\x1b[0m"
                );
                // Inject scope context into the next translation attempt.
                let scope_hint = build_scope_hint(&scope);
                current_input = format!("{current_input} ({scope_hint})");
                continue 'translate;
            }
        }

        let extra_warning = config.behavior.safety_mode != "off"
            && (crate::safety::needs_extra_confirmation(&command) || exfil_risk.is_risky());

        // Show the translated command and ask for confirmation
        if config.behavior.confirm_ai_commands {
            if extra_warning {
                eprintln!("shako: warning: this command modifies system state");
            }
            // Show numbered preview for multi-step commands
            confirm::print_multi_command_preview(&command);
                    loop {
                        match confirm::confirm_command(&command)? {
                            confirm::ConfirmAction::Execute => {
                                // Offer pre-execution snapshot for dangerous commands.
                                maybe_take_snapshot(&command, config);

                                let status = crate::executor::execute_command(&command);
                                let exit_code = status.and_then(|s| s.code()).unwrap_or(0);
                                push_memory(session_memory, input, &command);
                                // Journal the confirmed execution for session resumption.
                                if config.behavior.session_journal {
                                    journal::append_async(input, &command, exit_code);
                                }
                                // Audit log: record AI query + decision.
                                if config.security.audit_log {
                                    crate::audit::record_ai_query(
                                        input, &command, &command, "execute", exit_code,
                                    );
                                }
                                break 'translate;
                            }
                            confirm::ConfirmAction::Edit(edited) => {
                                crate::learned_prefs::record_edit(&command, &edited);
                                // Offer pre-execution snapshot for the edited command too.
                                maybe_take_snapshot(&edited, config);

                                let status = crate::executor::execute_command(&edited);
                                let exit_code = status.and_then(|s| s.code()).unwrap_or(0);
                                push_memory(session_memory, input, &edited);
                                // Journal the edited command too.
                                if config.behavior.session_journal {
                                    journal::append_async(input, &edited, exit_code);
                                }
                                // Audit log: record AI query + edit decision.
                                if config.security.audit_log {
                                    crate::audit::record_ai_query(
                                        input, &command, &edited, "edit", exit_code,
                                    );
                                }
                                break 'translate;
                            }
                    confirm::ConfirmAction::Cancel => {
                        // Audit log: record cancelled AI query.
                        if config.security.audit_log {
                            crate::audit::record_ai_query(
                                input, &command, "", "cancel", -1,
                            );
                        }
                        println!("cancelled");
                        break 'translate;
                    }
                    confirm::ConfirmAction::Why => {
                        match explain_command(&command, config, None).await {
                            Ok(explanation) => {
                                println!("\x1b[90m{explanation}\x1b[0m");
                            }
                            Err(e) => {
                                eprintln!("shako: couldn't explain: {e}");
                            }
                        }
                        // loop continues — re-shows the command and prompt
                    }
                    confirm::ConfirmAction::Refine => {
                        print!("\x1b[36mRefine:\x1b[0m ");
                        io::stdout().flush()?;
                        let mut clarification = String::new();
                        io::stdin().read_line(&mut clarification)?;
                        let clarification = clarification.trim();
                        if clarification.is_empty() {
                            // loop again without change
                            continue;
                        }
                        current_input = format!("{input} (clarification: {clarification})");
                        // Break inner confirm loop to re-translate
                        break;
                    }
                }
            }
        } else {
            let status = crate::executor::execute_command(&command);
            let exit_code = status.and_then(|s| s.code()).unwrap_or(0);
            push_memory(session_memory, input, &command);
            // Journal the auto-executed command.
            if config.behavior.session_journal {
                journal::append_async(input, &command, exit_code);
            }
            // Audit log: auto-executed (no confirmation required).
            if config.security.audit_log {
                crate::audit::record_ai_query(
                    input, &command, &command, "auto_execute", exit_code,
                );
            }
            break 'translate;
        }
    }

    Ok(())
}

/// Push a (user NL input, AI command) pair into session memory, capped at 5.
fn push_memory(memory: &mut Vec<(String, String)>, input: &str, command: &str) {
    memory.push((input.to_string(), command.to_string()));
    if memory.len() > 5 {
        memory.remove(0);
    }
}

/// Attempt to take a pre-execution snapshot when the command is snapshotable
/// and the user has snapshots enabled.  Prints a brief status line.
///
/// This is called immediately before executing a confirmed dangerous/risky command.
pub fn maybe_take_snapshot(command: &str, config: &ShakoConfig) {
    if !config.behavior.undo_snapshots {
        return;
    }
    if !crate::safety::is_snapshotable(command) {
        return;
    }
    let max_bytes = config.behavior.snapshot_max_bytes;
    match crate::undo::take_snapshot(command, max_bytes) {
        crate::undo::SnapshotResult::Taken(sha) => {
            let paths = crate::undo::extract_paths(command);
            let path_display = paths.join(", ");
            eprintln!(
                "\x1b[90mshako: snapshotting {} first (sha: {})\x1b[0m",
                path_display, sha
            );
        }
        crate::undo::SnapshotResult::GitTracked => {
            eprintln!("\x1b[90mshako: paths are git-tracked, skipping snapshot\x1b[0m");
        }
        crate::undo::SnapshotResult::TooLarge(size) => {
            eprintln!(
                "\x1b[90mshako: skipping snapshot ({:.1} MB > limit)\x1b[0m",
                size as f64 / 1_048_576.0
            );
        }
        crate::undo::SnapshotResult::NoPaths => {} // nothing to snapshot
        crate::undo::SnapshotResult::Error(e) => {
            eprintln!("\x1b[33mshako: snapshot failed: {e}\x1b[0m");
        }
    }
    // Run GC opportunistically (cheap: only opens the graph file).
    crate::undo::gc_old_snapshots(config.behavior.snapshot_gc_days);
}

/// Handle a natural-language undo/restore request.
///
/// Resolves the user's intent to the most relevant snapshot entry and offers
/// a styled confirmation panel before restoring.
///
/// Returns `Ok(true)` if a restore was performed, `Ok(false)` if cancelled or
/// no snapshot matched.
pub fn handle_undo_request(query: &str, config: &ShakoConfig) -> Result<bool> {
    if !config.behavior.undo_snapshots {
        eprintln!("shako: undo snapshots are disabled (undo_snapshots = false in config)");
        return Ok(false);
    }

    // Extract a keyword hint from the query for smarter matching.
    let keyword = extract_undo_keyword(query);

    let entry = if !keyword.is_empty() {
        crate::undo::find_snapshot_matching(&keyword)
            .or_else(|| crate::undo::find_latest_snapshot())
    } else {
        crate::undo::find_latest_snapshot()
    };

    let entry = match entry {
        Some(e) => e,
        None => {
            eprintln!("shako: no snapshots available to restore");
            return Ok(false);
        }
    };

    // ── Styled confirmation panel ────────────────────────────────────────────
    use std::io::Write;
    const GRAD: &[u8] = &[30, 31, 32, 37, 38, 44, 45];
    let mid_color = GRAD[GRAD.len() / 2];
    let border = |c: char| format!("\x1b[38;5;{mid_color}m{c}\x1b[0m");
    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);
    let grad_line = |width: usize| -> String {
        (0..width)
            .map(|i| {
                let idx = if width <= 1 { 0 } else { i * (GRAD.len() - 1) / (width - 1) };
                format!("\x1b[38;5;{}m─\x1b[0m", GRAD[idx])
            })
            .collect()
    };
    let visible_len = |s: &str| -> usize {
        let mut len = 0;
        let mut in_esc = false;
        for c in s.chars() {
            if c == '\x1b' { in_esc = true; }
            else if in_esc { if c.is_ascii_alphabetic() { in_esc = false; } }
            else { len += 1; }
        }
        len
    };

    let snapshot_display = crate::undo::format_snapshot(&entry);
    let prompt_styled = format!(
        "\x1b[1;36mrestore\x1b[0m \x1b[90m{snapshot_display}\x1b[0m? \x1b[90m[y/N]\x1b[0m"
    );
    let opts_styled = "\x1b[90m[y]es  [n]o (default: abort)\x1b[0m";

    let pv = visible_len(&prompt_styled);
    let ov = visible_len(opts_styled);
    let content_width = pv.max(ov).max(36);
    let inner_width = (content_width + 4).min(term_width.saturating_sub(2));
    let content_inner = inner_width.saturating_sub(4);

    let box_line = |content: &str| -> String {
        let vl = visible_len(content);
        let pad = content_inner.saturating_sub(vl);
        format!(" {}  {}{}  {}", border('│'), content, " ".repeat(pad), border('│'))
    };

    let label = format!("\x1b[38;5;{mid_color}m undo \x1b[0m");
    let label_vis = 7usize;
    let rest_width = inner_width.saturating_sub(label_vis + 2);

    eprintln!(
        " {tl}{sep}{label}{rest}{tr}",
        tl = border('╭'),
        sep = grad_line(2),
        rest = grad_line(rest_width),
        tr = border('╮'),
    );
    eprintln!("{}", box_line(&prompt_styled));
    eprintln!(
        " {bl}{sep}{br}",
        bl = border('├'),
        sep = grad_line(inner_width),
        br = border('┤'),
    );

    let opts_pad = content_inner.saturating_sub(ov);
    eprint!(
        " {}  {}{}  {} ",
        border('│'),
        opts_styled,
        " ".repeat(opts_pad),
        border('│'),
    );
    io::stdout().flush()?;

    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    let answer = answer.trim().to_lowercase();

    eprintln!(
        " {bl}{bot}{br}",
        bl = border('╰'),
        bot = grad_line(inner_width),
        br = border('╯'),
    );

    if answer != "y" && answer != "yes" {
        return Ok(false);
    }

    // Perform the restore.
    match crate::undo::restore_snapshot(&entry.sha) {
        Ok(()) => {
            eprintln!("\x1b[32mrestored.\x1b[0m");
            Ok(true)
        }
        Err(e) => {
            eprintln!("shako: restore failed: {e}");
            Ok(false)
        }
    }
}

/// Extract a meaningful keyword from an undo query.
///
/// For "undo that rm", returns "rm". For "restore what I deleted", returns "".
fn extract_undo_keyword(query: &str) -> String {
    const SKIP: &[&str] = &[
        "undo", "that", "the", "last", "restore", "what", "i", "deleted", "removed",
        "go", "back", "revert", "un-delete", "undelete", "bring", "it", "roll", "command",
    ];
    let lower = query.to_ascii_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    let meaningful: Vec<&str> = words
        .iter()
        .copied()
        .filter(|w| !SKIP.contains(w))
        .collect();
    meaningful.first().copied().unwrap_or("").to_string()
}

/// Structured result from AI error diagnosis.
///
/// Separates the human-readable explanation from the executable fix command
/// so callers can present them independently and pipe the fix through the
/// standard `confirm::confirm_command` flow.
#[derive(Debug, Clone)]
pub struct DiagnosisResult {
    /// One-line human-readable explanation of why the command failed.
    pub explanation: String,
    /// Suggested corrective shell command(s), if the AI could determine one.
    /// `None` when the AI returns `SHAKO_NO_FIX` or cannot suggest a fix.
    pub suggested_command: Option<String>,
}

impl DiagnosisResult {
    /// Parse a raw LLM response string into a `DiagnosisResult`.
    ///
    /// Expected format (from `error_recovery_prompt`):
    /// ```text
    /// CAUSE: One-line explanation
    /// FIX: corrective-command
    /// ```
    /// Additional lines after `FIX:` are treated as continuation lines
    /// and joined with `&&` for multi-step fixes.
    pub fn parse(raw: &str) -> Self {
        let mut explanation = String::new();
        let mut fix_lines: Vec<String> = Vec::new();
        let mut in_fix = false;

        for line in raw.lines() {
            let line = line.trim();
            if let Some(cause) = line.strip_prefix("CAUSE:") {
                explanation = cause.trim().to_string();
                in_fix = false;
            } else if let Some(fix) = line.strip_prefix("FIX:") {
                let fix = fix.trim();
                if !fix.is_empty() && fix != "SHAKO_NO_FIX" {
                    fix_lines.push(fix.to_string());
                }
                in_fix = true;
            } else if in_fix && !line.is_empty() && line != "SHAKO_NO_FIX" {
                fix_lines.push(line.to_string());
            }
        }

        let suggested_command = if fix_lines.is_empty() {
            None
        } else {
            Some(fix_lines.join(" && "))
        };

        DiagnosisResult {
            explanation,
            suggested_command,
        }
    }
}

/// Ask the AI to diagnose a failed command and suggest a fix.
///
/// Returns a [`DiagnosisResult`] containing a human-readable explanation and
/// an optional suggested corrective command.  Callers should pass
/// `suggested_command` through the `confirm::confirm_command` loop so the
/// user can choose to execute, edit, or cancel the fix.
pub async fn diagnose_error(
    command: &str,
    exit_code: i32,
    stderr_hint: &str,
    config: &ShakoConfig,
    recent_history: Vec<String>,
) -> Result<DiagnosisResult> {
    let guard_cfg = prompt_guard::GuardConfig {
        enabled: config.security.prompt_injection_guard,
    };
    let ctx = context::build_context(recent_history, vec![], Some(guard_cfg))?;
    let system_prompt = prompt::error_recovery_prompt(&ctx);
    let user_msg = if stderr_hint.is_empty() {
        format!("Command: {command}\nExit code: {exit_code}")
    } else {
        format!("Command: {command}\nExit code: {exit_code}\nStderr:\n{stderr_hint}")
    };

    let raw = client::query_llm(&system_prompt, &user_msg, config.active_llm()).await?;
    Ok(DiagnosisResult::parse(&raw))
}

/// Generate a git commit message for the currently staged changes.
///
/// `stat` is the output of `git diff --staged --stat`.
/// `diff` is the output of `git diff --staged` (may be truncated).
pub async fn suggest_commit(stat: &str, diff: &str, config: &ShakoConfig) -> Result<String> {
    let system_prompt = prompt::commit_message_prompt();
    let user_msg = format!("Staged changes summary:\n{stat}\nFull diff:\n{diff}");
    let raw = client::query_llm(&system_prompt, &user_msg, config.active_llm()).await?;
    // Strip any wrapping quotes the model might add
    Ok(raw.trim().trim_matches('"').trim_matches('\'').to_string())
}

/// Collapse a multi-line AI response into a single executable command.
///
/// The AI should return one line but sometimes returns alternatives or extra
/// prose. Strategy:
/// - Drop blank lines and lines that look like markdown/numbered lists
/// - If only one non-trivial line remains, use it
/// - If multiple lines remain, join them with " && " so the user can see
///   them all in the confirm prompt and edit before running
fn collapse_multiline(raw: &str) -> String {
    let lines: Vec<&str> = raw
        .lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && !l.starts_with('#')
                && !l.starts_with("```")
                // Skip numbered/bulleted list items ("1. cmd", "- cmd", "* cmd")
                && !l.starts_with("- ")
                && !l.starts_with("* ")
                && !matches!(l.chars().next(), Some(c) if c.is_ascii_digit())
        })
        .collect();

    match lines.len() {
        0 => String::new(),
        1 => lines[0].to_string(),
        _ => {
            // Warn the user so they know the model returned multiple candidates
            eprintln!(
                "shako: ai returned {} lines — showing first as best guess",
                lines.len()
            );
            lines[0].to_string()
        }
    }
}

/// Build a scope constraint hint string to append to the user input when
/// the AI-generated command violates the project's capability scope.
///
/// The hint instructs the model to regenerate using only the declared commands.
fn build_scope_hint(scope: &capability_scope::CapabilityScope) -> String {
    let mut parts: Vec<String> = Vec::new();

    if !scope.allow_commands.is_empty() {
        parts.push(format!(
            "only use commands from this allowlist: {}",
            scope.allow_commands.join(", ")
        ));
    }
    if !scope.deny_commands.is_empty() {
        parts.push(format!(
            "do NOT use: {}",
            scope.deny_commands.join(", ")
        ));
    }
    if !scope.allow_sudo {
        parts.push("do NOT use sudo".to_string());
    }
    if !scope.allow_network {
        parts.push("do NOT use any network commands (curl, wget, etc.)".to_string());
    }

    if parts.is_empty() {
        "stay within the project capability scope".to_string()
    } else {
        format!("project capability constraints: {}", parts.join("; "))
    }
}

/// Explain what a command does without executing it.
///
/// Collects the full LLM response silently, then renders it as styled markdown.
/// If a spinner_flag is provided, the spinner is cleared when the first token arrives.
pub async fn explain_command(
    command: &str,
    config: &ShakoConfig,
    spinner_flag: Option<Arc<AtomicBool>>,
) -> Result<String> {
    let guard_cfg = prompt_guard::GuardConfig {
        enabled: config.security.prompt_injection_guard,
    };
    let ctx = context::build_context(vec![], vec![], Some(guard_cfg))?;
    let system_prompt = prompt::explain_prompt(&ctx);

    let raw = if let Some(flag) = spinner_flag {
        client::query_llm_with_spinner(&system_prompt, command, config.active_llm(), flag).await?
    } else {
        client::query_llm(&system_prompt, command, config.active_llm()).await?
    };

    Ok(render::render_markdown_explanation(&raw))
}

/// Generate an AI-powered post-mortem runbook from an incident step log.
///
/// `incident_name` is the human label; `step_log` is the output of
/// `IncidentSession::step_log()`.
pub async fn generate_incident_runbook(
    incident_name: &str,
    step_log: &str,
    config: &ShakoConfig,
) -> Result<String> {
    let system_prompt = prompt::incident_runbook_prompt();
    let user_msg = format!(
        "Incident: {incident_name}\n\nCommand Journal:\n{step_log}"
    );
    let raw = client::query_llm(&system_prompt, &user_msg, config.active_llm()).await?;
    Ok(raw.trim().to_string())
}

/// Synthesise an AI-powered session resumption brief for a returning user.
///
/// `summary` is the [`journal::SessionSummary`] for the directory they just
/// `cd`d into.  Returns a 2-4 line plain-text brief suitable for display
/// in a shako proactive panel.
pub async fn synthesize_session_brief(
    summary: &journal::SessionSummary,
    config: &ShakoConfig,
) -> Result<String> {
    let system = prompt::session_resumption_prompt();

    // Format the journal entries as a compact text block for the LLM.
    let mut journal_text = format!(
        "Days since last session: {}\nLast known branch: {}\n\nRecent command journal (oldest first):\n",
        summary.days_ago, summary.branch
    );
    for entry in &summary.entries {
        let status = if entry.exit_code == 0 { "ok" } else { &format!("exit={}", entry.exit_code) };
        journal_text.push_str(&format!(
            "  intent=\"{}\"  cmd=\"{}\"  {}\n",
            entry.intent, entry.cmd, status
        ));
    }

    let raw = client::query_llm(system, &journal_text, config.active_llm()).await?;
    Ok(raw.trim().to_string())
}

/// Search shell history using AI semantic matching.
pub async fn search_history(
    query: &str,
    history: &[String],
    config: &ShakoConfig,
) -> Result<String> {
    if history.is_empty() {
        return Ok("No history available.".to_string());
    }
    let history_text = history
        .iter()
        .enumerate()
        .map(|(i, cmd)| format!("{}: {}", i + 1, cmd))
        .collect::<Vec<_>>()
        .join("\n");

    let system = "You are a shell history search assistant. Given a list of shell commands and a search query, find the most relevant commands. Return just the matching commands, one per line, most relevant first. If nothing matches well, say so briefly.";
    let user_msg =
        format!("Search query: {query}\n\nShell history (most recent last):\n{history_text}");

    client::query_llm(system, &user_msg, config.active_llm()).await
}

#[cfg(test)]
mod tests {
    use super::DiagnosisResult;

    #[test]
    fn test_parse_cause_and_fix() {
        let raw = "CAUSE: missing cargo dependency\nFIX: cargo add serde";
        let r = DiagnosisResult::parse(raw);
        assert_eq!(r.explanation, "missing cargo dependency");
        assert_eq!(r.suggested_command, Some("cargo add serde".to_string()));
    }

    #[test]
    fn test_parse_no_fix_sentinel() {
        let raw = "CAUSE: cannot determine issue\nFIX: SHAKO_NO_FIX";
        let r = DiagnosisResult::parse(raw);
        assert_eq!(r.explanation, "cannot determine issue");
        assert!(r.suggested_command.is_none());
    }

    #[test]
    fn test_parse_no_fix_line() {
        let raw = "CAUSE: unknown error";
        let r = DiagnosisResult::parse(raw);
        assert_eq!(r.explanation, "unknown error");
        assert!(r.suggested_command.is_none());
    }

    #[test]
    fn test_parse_multiline_fix_joined_with_and() {
        let raw = "CAUSE: build failed\nFIX: cargo clean\ncargo build";
        let r = DiagnosisResult::parse(raw);
        assert_eq!(r.explanation, "build failed");
        assert_eq!(
            r.suggested_command,
            Some("cargo clean && cargo build".to_string())
        );
    }

    #[test]
    fn test_parse_empty_response() {
        let r = DiagnosisResult::parse("");
        assert!(r.explanation.is_empty());
        assert!(r.suggested_command.is_none());
    }

    #[test]
    fn test_parse_fix_with_leading_whitespace() {
        let raw = "CAUSE: port in use\nFIX:   lsof -ti:8080 | xargs kill";
        let r = DiagnosisResult::parse(raw);
        assert_eq!(r.suggested_command, Some("lsof -ti:8080 | xargs kill".to_string()));
    }
}
