use std::sync::Arc;

use nu_ansi_term::{Color, Style};
use reedline::{Highlighter, StyledText};

use crate::path_cache::PathCache;

pub struct ShakoHighlighter {
    cache: Arc<PathCache>,
}

impl ShakoHighlighter {
    pub fn new(cache: Arc<PathCache>) -> Self {
        Self { cache }
    }

    fn is_known_command(&self, token: &str) -> bool {
        self.cache.command_set.contains(token)
    }
}

/// Style used for each syntactic role.
fn style_command_valid() -> Style {
    Style::new().fg(Color::Green).bold()
}
fn style_builtin() -> Style {
    Style::new().fg(Color::Cyan).bold()
}
fn style_ai_prefix() -> Style {
    Style::new().fg(Color::Purple).bold()
}
fn style_path_command() -> Style {
    Style::new().fg(Color::Yellow)
}
fn style_unknown_command() -> Style {
    Style::new().fg(Color::Red)
}
fn style_flag() -> Style {
    Style::new().fg(Color::Blue)
}
fn style_pipe_redirect() -> Style {
    Style::new().fg(Color::Cyan)
}
fn style_string() -> Style {
    Style::new().fg(Color::Yellow)
}
fn style_variable() -> Style {
    Style::new().fg(Color::Green)
}
fn style_comment() -> Style {
    Style::new().fg(Color::DarkGray).italic()
}
fn style_default() -> Style {
    Style::new()
}

impl Highlighter for ShakoHighlighter {
    fn highlight(&self, line: &str, _cursor: usize) -> StyledText {
        let mut styled = StyledText::new();

        if line.is_empty() {
            return styled;
        }

        let tokens = tokenize_for_highlighting(line);
        let mut is_first_command = true;
        let mut after_pipe = false;

        for token in &tokens {
            match token {
                HlToken::Whitespace(s) => {
                    styled.push((style_default(), s.clone()));
                }
                HlToken::Comment(s) => {
                    styled.push((style_comment(), s.clone()));
                }
                HlToken::Pipe(s) | HlToken::Redirect(s) | HlToken::Chain(s) => {
                    styled.push((style_pipe_redirect(), s.clone()));
                    if matches!(token, HlToken::Pipe(_) | HlToken::Chain(_)) {
                        after_pipe = true;
                    }
                }
                HlToken::String(s) => {
                    styled.push((style_string(), s.clone()));
                }
                HlToken::Variable(s) => {
                    styled.push((style_variable(), s.clone()));
                }
                HlToken::Flag(s) => {
                    styled.push((style_flag(), s.clone()));
                }
                HlToken::Word(s) => {
                    if is_first_command || after_pipe {
                        let style = if s.starts_with('?') || s == "ai:" {
                            style_ai_prefix()
                        } else if crate::builtins::is_builtin(s) {
                            style_builtin()
                        } else if self.is_known_command(s) {
                            style_command_valid()
                        } else if s.starts_with('/') || s.starts_with("./") {
                            style_path_command()
                        } else {
                            style_unknown_command()
                        };
                        styled.push((style, s.clone()));
                        is_first_command = false;
                        after_pipe = false;
                    } else {
                        styled.push((style_default(), s.clone()));
                    }
                }
            }
        }

        styled
    }
}

/// Token types for syntax highlighting (not execution — just visual).
#[derive(Debug)]
enum HlToken {
    Word(String),
    Flag(String),
    String(String),
    Variable(String),
    Pipe(String),
    Redirect(String),
    Chain(String),
    Comment(String),
    Whitespace(String),
}

/// Tokenize a line for highlighting purposes.
/// Preserves all characters (including whitespace) so the highlighted
/// output matches the input exactly.
fn tokenize_for_highlighting(line: &str) -> Vec<HlToken> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];

        // Whitespace
        if c.is_whitespace() {
            let start = i;
            while i < len && chars[i].is_whitespace() {
                i += 1;
            }
            tokens.push(HlToken::Whitespace(chars[start..i].iter().collect()));
            continue;
        }

        // Comment (# at start of word position)
        if c == '#' {
            tokens.push(HlToken::Comment(chars[i..].iter().collect()));
            break;
        }

        // Single-quoted string
        if c == '\'' {
            let start = i;
            i += 1;
            while i < len && chars[i] != '\'' {
                i += 1;
            }
            if i < len {
                i += 1; // consume closing quote
            }
            tokens.push(HlToken::String(chars[start..i].iter().collect()));
            continue;
        }

        // Double-quoted string
        if c == '"' {
            let start = i;
            i += 1;
            while i < len && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < len {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if i < len {
                i += 1; // consume closing quote
            }
            tokens.push(HlToken::String(chars[start..i].iter().collect()));
            continue;
        }

        // Variable: $VAR, ${VAR}, $?
        if c == '$' && i + 1 < len {
            let start = i;
            i += 1;
            if i < len && chars[i] == '{' {
                while i < len && chars[i] != '}' {
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
            } else if i < len && chars[i] == '(' {
                // $(cmd) — treat as string
                let mut depth = 1;
                i += 1;
                while i < len && depth > 0 {
                    if chars[i] == '(' {
                        depth += 1;
                    } else if chars[i] == ')' {
                        depth -= 1;
                    }
                    i += 1;
                }
            } else {
                while i < len && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '?')
                {
                    i += 1;
                }
            }
            tokens.push(HlToken::Variable(chars[start..i].iter().collect()));
            continue;
        }

        // Pipe and operators
        if c == '|' {
            if i + 1 < len && chars[i + 1] == '|' {
                tokens.push(HlToken::Chain("||".to_string()));
                i += 2;
            } else {
                tokens.push(HlToken::Pipe("|".to_string()));
                i += 1;
            }
            continue;
        }

        // Chain operators
        if c == '&' && i + 1 < len && chars[i + 1] == '&' {
            tokens.push(HlToken::Chain("&&".to_string()));
            i += 2;
            continue;
        }

        if c == ';' {
            tokens.push(HlToken::Chain(";".to_string()));
            i += 1;
            continue;
        }

        // Redirects: >, >>, <, 2>, 2>>, 2>&1
        if c == '>' {
            if i + 1 < len && chars[i + 1] == '>' {
                tokens.push(HlToken::Redirect(">>".to_string()));
                i += 2;
            } else {
                tokens.push(HlToken::Redirect(">".to_string()));
                i += 1;
            }
            continue;
        }

        if c == '<' {
            tokens.push(HlToken::Redirect("<".to_string()));
            i += 1;
            continue;
        }

        if c == '2' && i + 1 < len && chars[i + 1] == '>' {
            if i + 2 < len && chars[i + 2] == '>' {
                tokens.push(HlToken::Redirect("2>>".to_string()));
                i += 3;
            } else if i + 2 < len && chars[i + 2] == '&' && i + 3 < len && chars[i + 3] == '1' {
                tokens.push(HlToken::Redirect("2>&1".to_string()));
                i += 4;
            } else {
                tokens.push(HlToken::Redirect("2>".to_string()));
                i += 2;
            }
            continue;
        }

        // Flags: -x, --flag
        if c == '-' && (i == 0 || chars[i - 1].is_whitespace()) {
            let start = i;
            i += 1;
            while i < len && !chars[i].is_whitespace() && chars[i] != '|' && chars[i] != '>'
                && chars[i] != '<' && chars[i] != ';' && chars[i] != '&'
            {
                i += 1;
            }
            tokens.push(HlToken::Flag(chars[start..i].iter().collect()));
            continue;
        }

        // Regular word
        let start = i;
        while i < len
            && !chars[i].is_whitespace()
            && chars[i] != '|'
            && chars[i] != '>'
            && chars[i] != '<'
            && chars[i] != ';'
            && chars[i] != '\''
            && chars[i] != '"'
            && chars[i] != '$'
        {
            // Don't break on & unless it's && or 2>&1
            if chars[i] == '&' && i + 1 < len && chars[i + 1] == '&' {
                break;
            }
            i += 1;
        }
        if i > start {
            tokens.push(HlToken::Word(chars[start..i].iter().collect()));
        }
    }

    tokens
}
