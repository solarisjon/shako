use reedline::{Hinter, History};

pub struct JboshHinter {
    current_hint: String,
}

impl JboshHinter {
    pub fn new() -> Self {
        Self {
            current_hint: String::new(),
        }
    }
}

impl Hinter for JboshHinter {
    fn handle(
        &mut self,
        line: &str,
        _pos: usize,
        history: &dyn History,
        _use_ansi_coloring: bool,
        _cwd: &str,
    ) -> String {
        self.current_hint = String::new();

        if line.is_empty() {
            return String::new();
        }

        if let Ok(results) = history.search(reedline::SearchQuery::last_with_prefix(
            line.to_string(),
            None,
        )) {
            if let Some(entry) = results.first() {
                if entry.command_line.len() > line.len() {
                    let hint = &entry.command_line[line.len()..];
                    self.current_hint = hint.to_string();
                    return format!("\x1b[90m{hint}\x1b[0m");
                }
            }
        }

        String::new()
    }

    fn complete_hint(&self) -> String {
        self.current_hint.clone()
    }

    fn next_hint_token(&self) -> String {
        let trimmed = self.current_hint.trim_start();
        if let Some(pos) = trimmed.find(char::is_whitespace) {
            trimmed[..pos].to_string()
        } else {
            trimmed.to_string()
        }
    }
}
