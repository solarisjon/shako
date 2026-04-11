pub mod client;
pub mod confirm;
pub mod context;
pub mod prompt;
pub mod prompt_guard;
pub mod render;

use crate::config::ShakoConfig;
use anyhow::Result;
use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// Translate natural language to a shell command via LLM, confirm, and execute.
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
                return Ok(());
            }
            eprintln!("shako: dangerous command detected: {command}");
        }

        let extra_warning = config.behavior.safety_mode != "off"
            && crate::safety::needs_extra_confirmation(&command);

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
                        crate::executor::execute_command(&command);
                        push_memory(session_memory, input, &command);
                        break 'translate;
                    }
                    confirm::ConfirmAction::Edit(edited) => {
                        crate::learned_prefs::record_edit(&command, &edited);
                        crate::executor::execute_command(&edited);
                        push_memory(session_memory, input, &edited);
                        break 'translate;
                    }
                    confirm::ConfirmAction::Cancel => {
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
            crate::executor::execute_command(&command);
            push_memory(session_memory, input, &command);
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

/// Ask the AI to diagnose a failed command and suggest a fix.
pub async fn diagnose_error(
    command: &str,
    exit_code: i32,
    stderr_hint: &str,
    config: &ShakoConfig,
    recent_history: Vec<String>,
) -> Result<String> {
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

    client::query_llm(&system_prompt, &user_msg, config.active_llm()).await
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
