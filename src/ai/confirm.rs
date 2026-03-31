use anyhow::Result;
use std::io::{self, Write};

pub enum ConfirmAction {
    Execute,
    Edit(String),
    Cancel,
    Why,
    Refine,
}

// Gradient colors matching the startup banner: teal → cyan
const GRAD: &[u8] = &[30, 31, 32, 37, 38, 44, 45];

/// Render a single gradient character from the banner palette.
fn grad_char(c: char, idx: usize, total: usize) -> String {
    let color_idx = if total <= 1 {
        0
    } else {
        idx * (GRAD.len() - 1) / (total - 1)
    };
    format!("\x1b[38;5;{}m{c}\x1b[0m", GRAD[color_idx])
}

/// Render a horizontal gradient line of `width` copies of `ch`.
fn grad_line(ch: char, width: usize) -> String {
    (0..width).map(|i| grad_char(ch, i, width)).collect()
}

/// Border character in the mid-gradient color.
fn border(c: char) -> String {
    format!("\x1b[38;5;{}m{c}\x1b[0m", GRAD[GRAD.len() / 2])
}

/// Measure visible character width of a string (strips ANSI escapes).
fn visible_len(s: &str) -> usize {
    let mut len = 0;
    let mut in_esc = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_esc = true;
        } else if in_esc {
            if c.is_ascii_alphabetic() {
                in_esc = false;
            }
        } else {
            len += 1;
        }
    }
    len
}

/// Render a content line inside the box, padded to `inner_width - 4` chars.
fn box_line(content: &str, content_width: usize) -> String {
    let vl = visible_len(content);
    let pad = content_width.saturating_sub(vl);
    format!(" {b}  {content}{}  {b}", " ".repeat(pad), b = border('│'))
}

/// Show the AI-translated command in a branded confirmation panel.
pub fn confirm_command(command: &str) -> Result<ConfirmAction> {
    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);

    // Lines to display inside the box
    let cmd_styled = format!("\x1b[1;36m{command}\x1b[0m");
    let hint_styled = "\x1b[90m[Y]es  [n]o  [e]dit  [w]hy  [r]efine\x1b[0m";

    let cmd_vis = visible_len(&cmd_styled);
    let hint_vis = visible_len(hint_styled);

    let content_width = cmd_vis.max(hint_vis).max(32);
    let inner_width = (content_width + 4).min(term_width.saturating_sub(2));
    let content_width = inner_width.saturating_sub(4);

    let top_bar = grad_line('─', inner_width);
    let bot_bar = grad_line('─', inner_width);

    // Header label
    let label = format!("\x1b[38;5;{}m shako \x1b[0m", GRAD[GRAD.len() / 2]);

    eprintln!(
        " {tl}{label}{rest}{tr}",
        tl = border('╭'),
        label = label,
        rest = grad_line('─', inner_width.saturating_sub(8)),
        tr = border('╮'),
    );
    eprintln!("{}", box_line(&cmd_styled, content_width));
    eprintln!(
        " {bl}{sep}{br}",
        bl = border('├'),
        sep = grad_line('─', inner_width),
        br = border('┤'),
    );
    eprintln!("{}", box_line(hint_styled, content_width));
    eprintln!(
        " {bl}{bot}{br}",
        bl = border('╰'),
        bot = bot_bar,
        br = border('╯'),
    );
    // Drop the top_bar binding since we only use it for inner width calc
    let _ = top_bar;

    print!(" {} ", border('❯'));
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim().to_lowercase();

    match input.as_str() {
        "" | "y" | "yes" => Ok(ConfirmAction::Execute),
        "n" | "no" => Ok(ConfirmAction::Cancel),
        "e" | "edit" => {
            print!(" {} ", border('❯'));
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
        steps = steps.into_iter().flat_map(|s| s.split(sep)).collect();
    }
    let steps: Vec<&str> = steps
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if steps.len() < 2 {
        return false;
    }

    eprintln!("\x1b[90m shako translated your request to:\x1b[0m");
    for (i, step) in steps.iter().enumerate() {
        eprintln!(
            "   {} \x1b[1m{step}\x1b[0m",
            format!("\x1b[38;5;{}m{}.\x1b[0m", GRAD[GRAD.len() / 2], i + 1)
        );
    }
    eprintln!("\x1b[90m Run all {} steps?\x1b[0m", steps.len());
    true
}
