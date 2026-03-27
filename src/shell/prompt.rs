use std::borrow::Cow;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, AtomicU64, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::Instant;

use reedline::{Prompt, PromptEditMode, PromptHistorySearch};

/// Global last exit code, updated after each command.
static LAST_STATUS: AtomicI32 = AtomicI32::new(0);
/// Global last command duration in milliseconds.
static LAST_DURATION_MS: AtomicU64 = AtomicU64::new(0);
/// Global background job count, updated after each reap.
static LAST_JOB_COUNT: AtomicUsize = AtomicUsize::new(0);

pub fn set_last_status(code: i32) {
    LAST_STATUS.store(code, Ordering::Relaxed);
}

pub fn last_status() -> i32 {
    LAST_STATUS.load(Ordering::Relaxed)
}

pub fn set_last_duration(duration: std::time::Duration) {
    LAST_DURATION_MS.store(duration.as_millis() as u64, Ordering::Relaxed);
}

pub fn set_job_count(n: usize) {
    LAST_JOB_COUNT.store(n, Ordering::Relaxed);
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

pub struct StarshipPrompt {
    /// Whether the starship binary was found at startup.
    starship_available: bool,
    /// Right prompt is rendered in a background thread kicked off during left render.
    right_handle: Mutex<Option<JoinHandle<String>>>,
}

impl StarshipPrompt {
    pub fn new() -> Self {
        let starship_available = which::which("starship").is_ok();

        if starship_available {
            // Generate a session key so stateful Starship modules work correctly.
            // Using PID + startup timestamp — no extra deps needed.
            let key = format!(
                "{:x}{:x}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            );
            // Safety: called once at startup before any threads are spawned by the shell.
            unsafe { std::env::set_var("STARSHIP_SESSION_KEY", key) };

            // Suppress Starship's own debug/trace output leaking into the terminal.
            unsafe { std::env::set_var("STARSHIP_LOG", "error") };
        }

        Self {
            starship_available,
            right_handle: Mutex::new(None),
        }
    }

    fn prompt_args() -> (i32, u64, String, usize) {
        let status = LAST_STATUS.load(Ordering::Relaxed);
        let duration = LAST_DURATION_MS.load(Ordering::Relaxed);
        let jobs = LAST_JOB_COUNT.load(Ordering::Relaxed);
        let width = crossterm::terminal::size()
            .map(|(w, _)| w.to_string())
            .unwrap_or_else(|_| "80".to_string());
        (status, duration, width, jobs)
    }

    fn run_starship(right: bool, status: i32, duration: u64, width: &str, jobs: usize) -> String {
        let mut cmd = Command::new("starship");
        cmd.arg("prompt");

        if right {
            cmd.arg("--right");
        }

        cmd.args(["--status", &status.to_string()]);
        cmd.args(["--cmd-duration", &duration.to_string()]);
        cmd.args(["--terminal-width", width]);
        cmd.args(["--jobs", &jobs.to_string()]);
        // Report emacs keymap — update this if vi mode is added later.
        cmd.args(["--keymap", "emacs"]);

        match cmd.output() {
            Ok(output) if output.status.success() || !output.stdout.is_empty() => {
                String::from_utf8_lossy(&output.stdout).to_string()
            }
            _ => {
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
        if !self.starship_available {
            return Cow::Borrowed("\x1b[32m❯\x1b[0m ");
        }

        let (status, duration, width, jobs) = Self::prompt_args();

        // Kick off the right prompt in a background thread so both renders
        // happen in parallel rather than sequentially.
        let width_clone = width.clone();
        let handle = std::thread::spawn(move || {
            Self::run_starship(true, status, duration, &width_clone, jobs)
        });
        // Recover from mutex poison: if the spawned thread panicked while holding
        // this lock, `into_inner()` gives back the guard so we can still write.
        *self.right_handle.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);

        Cow::Owned(Self::run_starship(false, status, duration, &width, jobs))
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        if !self.starship_available {
            return Cow::Borrowed("");
        }

        // Join the thread started during left render.
        // Recover from mutex poison: if the background thread panicked while holding
        // the lock we still get back the guard so we can take the handle (or None).
        let handle = self.right_handle.lock().unwrap_or_else(|e| e.into_inner()).take();
        match handle {
            Some(h) => Cow::Owned(h.join().unwrap_or_default()),
            // Fallback: render inline if left wasn't called first (shouldn't happen).
            None => {
                let (status, duration, width, jobs) = Self::prompt_args();
                Cow::Owned(Self::run_starship(true, status, duration, &width, jobs))
            }
        }
    }

    fn render_prompt_indicator(&self, _edit_mode: PromptEditMode) -> Cow<'_, str> {
        // Starship includes the indicator in its prompt output.
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
