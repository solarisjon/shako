use termimad::crossterm::style::{Attribute, Color};
use termimad::MadSkin;

/// Render an AI explanation as styled markdown in the terminal.
///
/// Applies ANSI styles for headers, code blocks, bold/italic, and lists via
/// `termimad`. Falls back to plain text when `NO_COLOR` is set or the
/// terminal is identified as `TERM=dumb`.
pub fn render_markdown_explanation(text: &str) -> String {
    if is_plain_terminal() {
        return text.to_string();
    }
    let skin = explanation_skin();
    skin.term_text(text).to_string()
}

/// Build a `MadSkin` tuned for AI explanation output.
fn explanation_skin() -> MadSkin {
    let mut skin = MadSkin::default();

    // Headers: bold cyan, left-aligned
    for h in &mut skin.headers {
        h.set_fg(Color::Cyan);
        h.add_attr(Attribute::Bold);
    }
    skin.headers[0].align = termimad::minimad::Alignment::Left;

    // Inline code: yellow
    skin.inline_code.set_fg(Color::Yellow);

    // Code blocks: yellow text
    skin.code_block.set_fg(Color::Yellow);

    // Bold: bright white + bold attribute
    skin.bold.set_fg(Color::White);
    skin.bold.add_attr(Attribute::Bold);

    // Italic: dim
    skin.italic.add_attr(Attribute::Dim);

    skin
}

/// Returns true when the terminal environment calls for plain (no-colour) output.
fn is_plain_terminal() -> bool {
    // Respect NO_COLOR convention (https://no-color.org/)
    if std::env::var_os("NO_COLOR").is_some() {
        return true;
    }
    // Dumb terminal
    if std::env::var("TERM").map(|v| v == "dumb").unwrap_or(false) {
        return true;
    }
    false
}
