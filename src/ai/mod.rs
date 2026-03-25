pub mod client;
pub mod confirm;
pub mod context;
pub mod prompt;

use crate::config::ShakoConfig;
use anyhow::Result;

/// Translate natural language to a shell command via LLM, confirm, and execute.
pub async fn translate_and_execute(
    input: &str,
    config: &ShakoConfig,
    recent_history: Vec<String>,
) -> Result<()> {
    let ctx = context::build_context(recent_history)?;
    let system_prompt = prompt::system_prompt(&ctx);

    let response = client::query_llm(&system_prompt, input, config.active_llm()).await?;

    let command = response.trim();

    if command == "SHAKO_CANNOT_TRANSLATE" || command.is_empty() {
        eprintln!("shako: couldn't translate that to a command");
        return Ok(());
    }

    // Safety check on AI-generated commands
    if config.behavior.safety_mode != "off" && crate::safety::is_dangerous(command) {
        if config.behavior.safety_mode == "block" {
            eprintln!("\x1b[31;1m⚠ dangerous command blocked:\x1b[0m {command}");
            return Ok(());
        }
        eprintln!("\x1b[31;1m⚠ dangerous command detected:\x1b[0m {command}");
    }

    let extra_warning =
        config.behavior.safety_mode != "off" && crate::safety::needs_extra_confirmation(command);

    // Show the translated command and ask for confirmation
    if config.behavior.confirm_ai_commands {
        if extra_warning {
            eprintln!("\x1b[33;1m⚠ this command modifies system state\x1b[0m");
        }
        loop {
            match confirm::confirm_command(command)? {
                confirm::ConfirmAction::Execute => {
                    crate::executor::execute_command(command);
                    break;
                }
                confirm::ConfirmAction::Edit(edited) => {
                    crate::executor::execute_command(&edited);
                    break;
                }
                confirm::ConfirmAction::Cancel => {
                    println!("cancelled");
                    break;
                }
                confirm::ConfirmAction::Why => {
                    match explain_command(command, config).await {
                        Ok(explanation) => {
                            println!("\x1b[90m{explanation}\x1b[0m");
                        }
                        Err(e) => {
                            eprintln!("shako: couldn't explain: {e}");
                        }
                    }
                    // loop continues — re-shows the command and prompt
                }
            }
        }
    } else {
        crate::executor::execute_command(command);
    }

    Ok(())
}

/// Ask the AI to diagnose a failed command and suggest a fix.
pub async fn diagnose_error(
    command: &str,
    exit_code: i32,
    stderr_hint: &str,
    config: &ShakoConfig,
    recent_history: Vec<String>,
) -> Result<String> {
    let ctx = context::build_context(recent_history)?;
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

/// Explain what a command does without executing it.
pub async fn explain_command(
    command: &str,
    config: &ShakoConfig,
) -> Result<String> {
    let ctx = context::build_context(vec![])?;
    let system_prompt = prompt::explain_prompt(&ctx);

    client::query_llm(&system_prompt, command, config.active_llm()).await
}
