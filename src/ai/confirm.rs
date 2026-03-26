use anyhow::Result;
use std::io::{self, Write};

pub enum ConfirmAction {
    Execute,
    Edit(String),
    Cancel,
    Why,
    Refine,
}

/// Show the AI-translated command and ask user to confirm, edit, or cancel.
pub fn confirm_command(command: &str) -> Result<ConfirmAction> {
    // Show the translated command with color
    println!("\x1b[36m❯\x1b[0m \x1b[1m{command}\x1b[0m");
    print!("\x1b[90m[Y]es / [n]o / [e]dit / [w]hy / [r]efine:\x1b[0m ");
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
        "w" | "why" => Ok(ConfirmAction::Why),
        "r" | "refine" => Ok(ConfirmAction::Refine),
        _ => Ok(ConfirmAction::Cancel),
    }
}

/// Print a numbered multi-step preview if the command has 2+ steps.
/// Returns true if the preview was printed (multi-step), false otherwise.
pub fn print_multi_command_preview(command: &str) -> bool {
    // Split on common chain operators and newlines (simple, not quote-aware)
    let mut steps: Vec<&str> = vec![command];
    for sep in [" && ", " || ", " ; ", "\n"] {
        steps = steps
            .into_iter()
            .flat_map(|s| s.split(sep))
            .collect();
    }
    let steps: Vec<&str> = steps.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();

    if steps.len() < 2 {
        return false;
    }

    println!("\x1b[90mshako translated your request to:\x1b[0m");
    for (i, step) in steps.iter().enumerate() {
        println!("  \x1b[36m{}.\x1b[0m {step}", i + 1);
    }
    println!("\x1b[90mRun all {} steps?\x1b[0m", steps.len());
    true
}
