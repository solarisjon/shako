use std::borrow::Cow;
use std::process::Command;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::time::Instant;

use reedline::{Prompt, PromptEditMode, PromptHistorySearch};

/// Global last exit code, updated after each command.
static LAST_STATUS: AtomicI32 = AtomicI32::new(0);
/// Global last command duration in milliseconds.
static LAST_DURATION_MS: AtomicU64 = AtomicU64::new(0);

pub fn set_last_status(code: i32) {
    LAST_STATUS.store(code, Ordering::Relaxed);
}

pub fn last_status() -> i32 {
    LAST_STATUS.load(Ordering::Relaxed)
}

pub fn set_last_duration(duration: std::time::Duration) {
    LAST_DURATION_MS.store(duration.as_millis() as u64, Ordering::Relaxed);
}

/// A timer for tracking command duration.
pub struct CommandTimer {
    start: Instant,
}

impl CommandTimer {
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub fn stop(self) {
        set_last_duration(self.start.elapsed());
    }
}

pub struct StarshipPrompt;

impl StarshipPrompt {
    pub fn new() -> Self {
        Self
    }

    fn call_starship(&self, right: bool) -> String {
        let status = last_status();
        let duration = LAST_DURATION_MS.load(Ordering::Relaxed);

        let width = crossterm::terminal::size()
            .map(|(w, _)| w.to_string())
            .unwrap_or_else(|_| "80".to_string());

        let mut cmd = Command::new("starship");
        cmd.arg("prompt");

        if right {
            cmd.arg("--right");
        }

        cmd.args(["--status", &status.to_string()]);
        cmd.args(["--cmd-duration", &duration.to_string()]);
        cmd.args(["--terminal-width", &width]);

        match cmd.output() {
            Ok(output) => String::from_utf8_lossy(&output.stdout).to_string(),
            Err(_) => {
                if right {
                    String::new()
                } else {
                    "\x1b[32m❯\x1b[0m ".to_string()
                }
            }
        }
    }
}

impl Prompt for StarshipPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        Cow::Owned(self.call_starship(false))
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Owned(self.call_starship(true))
    }

    fn render_prompt_indicator(&self, _edit_mode: PromptEditMode) -> Cow<'_, str> {
        // Starship includes the indicator in its prompt output
        Cow::Borrowed("")
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Borrowed("... ")
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: PromptHistorySearch,
    ) -> Cow<'_, str> {
        Cow::Owned(format!("(search: {}) ", history_search.term))
    }
}
