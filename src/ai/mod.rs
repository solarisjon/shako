pub mod client;
pub mod confirm;
pub mod context;
pub mod prompt;

use crate::config::JboshConfig;
use anyhow::Result;

/// Translate natural language to a shell command via LLM, confirm, and execute.
pub async fn translate_and_execute(input: &str, config: &JboshConfig) -> Result<()> {
    let ctx = context::build_context()?;
    let system_prompt = prompt::system_prompt(&ctx);

    let response = client::query_llm(&system_prompt, input, &config.active_llm()).await?;

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
        match confirm::confirm_command(command)? {
            confirm::ConfirmAction::Execute => {
                crate::executor::execute_command(command);
            }
            confirm::ConfirmAction::Edit(edited) => {
                crate::executor::execute_command(&edited);
            }
            confirm::ConfirmAction::Cancel => {
                println!("cancelled");
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
    config: &JboshConfig,
) -> Result<String> {
    let ctx = context::build_context()?;
    let system_prompt = prompt::error_recovery_prompt(&ctx);
    let user_msg = format!(
        "Command: {command}\nExit code: {exit_code}\n{stderr_hint}"
    );

    client::query_llm(&system_prompt, &user_msg, &config.active_llm()).await
}
