use anyhow::Result;
use std::io::{self, Write};

pub enum ConfirmAction {
    Execute,
    Edit(String),
    Cancel,
}

/// Show the AI-translated command and ask user to confirm, edit, or cancel.
pub fn confirm_command(command: &str) -> Result<ConfirmAction> {
    // Show the translated command with color
    println!("\x1b[36m❯\x1b[0m \x1b[1m{command}\x1b[0m");
    print!("\x1b[90m[Y]es / [n]o / [e]dit:\x1b[0m ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim().to_lowercase();

    match input.as_str() {
        "" | "y" | "yes" => Ok(ConfirmAction::Execute),
        "n" | "no" => Ok(ConfirmAction::Cancel),
        "e" | "edit" => {
            print!("\x1b[36m❯\x1b[0m ");
            io::stdout().flush()?;
            let mut edited = String::new();
            io::stdin().read_line(&mut edited)?;
            let edited = edited.trim().to_string();
            if edited.is_empty() {
                Ok(ConfirmAction::Cancel)
            } else {
                Ok(ConfirmAction::Edit(edited))
            }
        }
        _ => Ok(ConfirmAction::Cancel),
    }
}
