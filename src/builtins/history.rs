//! History builtin: `history`.

use super::state::ShellState;

/// `history` — display recent history entries.
///   history [N]   show last N entries (default 25)
pub fn builtin_history(args: &[&str], state: &ShellState) {
    let limit: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(25);

    if !state.history_path.exists() {
        eprintln!("shako: history: no history file");
        return;
    }

    match std::fs::read_to_string(&state.history_path) {
        Ok(contents) => {
            let lines: Vec<&str> = contents.lines().collect();
            let start = lines.len().saturating_sub(limit);
            for (i, line) in lines[start..].iter().enumerate() {
                println!("{:>5}  {}", start + i + 1, line);
            }
        }
        Err(e) => eprintln!("shako: history: {e}"),
    }
}
