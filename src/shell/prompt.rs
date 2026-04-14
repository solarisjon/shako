use std::borrow::Cow;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Instant;

use reedline::{Prompt, PromptEditMode, PromptHistorySearch};

/// Global last exit code, updated after each command.
static LAST_STATUS: AtomicI32 = AtomicI32::new(0);
/// Global last command duration in milliseconds.
static LAST_DURATION_MS: AtomicU64 = AtomicU64::new(0);
/// Global background job count, updated after each reap.
static LAST_JOB_COUNT: AtomicUsize = AtomicUsize::new(0);
/// Whether AI session context is currently active (affects fallback prompt color).
static AI_CONTEXT_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Whether the current environment context is production (affects fallback prompt color).
static PRODUCTION_CONTEXT_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Whether an incident session is currently active (adds [INC] to prompt).
static INCIDENT_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Signal that the AI session context is active or inactive.
/// Used by the fallback prompt to switch to a yellow indicator.
#[allow(dead_code)]
pub fn set_ai_context_active(active: bool) {
    AI_CONTEXT_ACTIVE.store(active, Ordering::Relaxed);
}

/// Signal that the current shell context is (or isn't) a production environment.
///
/// When true the fallback prompt indicator turns amber/red to give a persistent
/// visual cue that destructive commands will affect production systems.
pub fn set_production_context_active(active: bool) {
    PRODUCTION_CONTEXT_ACTIVE.store(active, Ordering::Relaxed);
}

/// Return whether the prompt currently signals a production context.
#[allow(dead_code)]
pub fn is_production_context_active() -> bool {
    PRODUCTION_CONTEXT_ACTIVE.load(Ordering::Relaxed)
}

/// Signal that an incident session is active (or has ended).
/// When true the fallback prompt prepends a red `[INC]` indicator.
pub fn set_incident_active(active: bool) {
    INCIDENT_ACTIVE.store(active, Ordering::Relaxed);
}

/// Return whether an incident session is currently active.
#[allow(dead_code)]
pub fn is_incident_active() -> bool {
    INCIDENT_ACTIVE.load(Ordering::Relaxed)
}

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
                    fallback_prompt_left(status)
                }
            }
        }
    }
}

/// Mode-aware fallback left prompt used when Starship is unavailable.
///
/// Priority (highest wins):
/// - Last exit non-zero      → red ❯
/// - Production context      → amber ❯ (persistent warning: you are in prod)
/// - AI context active       → yellow ❯ (signals the shell has session memory)
/// - Normal                  → teal ❯ (matches brand gradient)
///
/// When an incident session is active a dim red `[INC]` prefix is prepended
/// regardless of the other priority tiers.
fn fallback_prompt_left(last_status: i32) -> String {
    let inc_prefix = if INCIDENT_ACTIVE.load(Ordering::Relaxed) {
        "\x1b[90m[INC]\x1b[0m "
    } else {
        ""
    };

    let chevron = if last_status != 0 {
        // Red — command failed
        "\x1b[31m❯\x1b[0m "
    } else if PRODUCTION_CONTEXT_ACTIVE.load(Ordering::Relaxed) {
        // Amber — currently in a production context; keep the engineer aware
        "\x1b[38;5;214m❯\x1b[0m "
    } else if AI_CONTEXT_ACTIVE.load(Ordering::Relaxed) {
        // Yellow — AI session memory is active
        "\x1b[33m❯\x1b[0m "
    } else {
        // Teal — normal, matches brand
        "\x1b[38;5;38m❯\x1b[0m "
    };

    format!("{inc_prefix}{chevron}")
}

impl Prompt for StarshipPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        if !self.starship_available {
            let status = LAST_STATUS.load(Ordering::Relaxed);
            return Cow::Owned(fallback_prompt_left(status));
        }

        let (status, duration, width, jobs) = Self::prompt_args();

        // Kick off the right prompt in a background thread so both renders
        // happen in parallel rather than sequentially.
        let width_clone = width.clone();
        let handle = std::thread::spawn(move || {
            Self::run_starship(true, status, duration, &width_clone, jobs)
        });
        *self.right_handle.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);

        Cow::Owned(Self::run_starship(false, status, duration, &width, jobs))
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        if !self.starship_available {
            return Cow::Borrowed("");
        }

        // Join the thread started during left render.
        let handle = self
            .right_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
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
        // Teal continuation glyph — visually distinct from the main ❯ prompt
        Cow::Borrowed("\x1b[38;5;30m·\x1b[0m ")
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: PromptHistorySearch,
    ) -> Cow<'_, str> {
        Cow::Owned(format!("(search: {}) ", history_search.term))
    }
}
