use nu_ansi_term::{Color, Style};
use reedline::{Highlighter, StyledText};
use which::which;

pub struct JboshHighlighter;

impl JboshHighlighter {
    pub fn new() -> Self {
        Self
    }
}

impl Highlighter for JboshHighlighter {
    fn highlight(&self, line: &str, _cursor: usize) -> StyledText {
        let mut styled = StyledText::new();

        if line.is_empty() {
            return styled;
        }

        let first_token = line.split_whitespace().next().unwrap_or("");
        let rest = if line.len() > first_token.len() {
            &line[first_token.len()..]
        } else {
            ""
        };

        if first_token.starts_with('?') || first_token == "ai:" {
            styled.push((
                Style::new().fg(Color::Purple).bold(),
                first_token.to_string(),
            ));
        } else if crate::builtins::is_builtin(first_token) {
            styled.push((Style::new().fg(Color::Cyan).bold(), first_token.to_string()));
        } else if which(first_token).is_ok() {
            styled.push((
                Style::new().fg(Color::Green).bold(),
                first_token.to_string(),
            ));
        } else if first_token.starts_with('/') || first_token.starts_with("./") {
            styled.push((Style::new().fg(Color::Yellow), first_token.to_string()));
        } else {
            styled.push((Style::new().fg(Color::Red), first_token.to_string()));
        }

        if !rest.is_empty() {
            styled.push((Style::new(), rest.to_string()));
        }

        styled
    }
}
