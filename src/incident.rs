//! Incident Mode — structured runbook execution with timestamped command journal.
//!
//! When incident mode is active every command executed by the shell is recorded
//! as a timestamped `IncidentStep`.  At the end of the incident, `incident report`
//! calls the AI to synthesise a post-mortem timeline and structured markdown runbook.
//!
//! ## Lifecycle
//!
//! ```text
//! shako incident start <name>   → activates incident mode, opens session
//! shako incident status         → prints current session summary
//! shako incident end            → closes session (keeps log; no report)
//! shako incident report         → closes session AND generates AI runbook
//! ```
//!
//! ## Storage
//!
//! Sessions are kept in memory during the shell session.  `incident report`
//! can auto-save the markdown to the directory declared in `.shako.toml`
//! under `[incident] runbook_dir = "~/incidents"`.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ── Data types ────────────────────────────────────────────────────────────────

/// A single command executed during an active incident session.
#[derive(Debug, Clone)]
pub struct IncidentStep {
    /// Wall-clock timestamp (seconds since UNIX epoch) when the command started.
    pub timestamp: u64,
    /// The full command string that was run.
    pub command: String,
    /// Exit code returned by the command (0 = success).
    pub exit_code: i32,
    /// Tail of stderr output (up to 20 lines), used by the AI for context.
    pub stderr_summary: String,
    /// How long the command took.
    pub duration: Duration,
}

/// Active incident session state.
pub struct IncidentSession {
    /// Short identifier chosen by the user (e.g. `payment-svc-latency`).
    pub name: String,
    /// Wall-clock start time (seconds since UNIX epoch).
    pub start_timestamp: u64,
    /// Monotonic timer used to measure step durations.
    pub start_instant: Instant,
    /// Ordered list of commands run since the incident was started.
    pub steps: Vec<IncidentStep>,
}

impl IncidentSession {
    /// Create a new session with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        let start_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            name: name.into(),
            start_timestamp,
            start_instant: Instant::now(),
            steps: Vec::new(),
        }
    }

    /// Identifier string used in filenames and log headers.
    pub fn id(&self) -> String {
        // e.g. INC-2026-04-11-payment-svc-latency
        let dt = format_unix_date(self.start_timestamp);
        format!("INC-{}-{}", dt, slug(&self.name))
    }

    /// Record a completed command step.
    pub fn record(
        &mut self,
        command: impl Into<String>,
        exit_code: i32,
        stderr_summary: impl Into<String>,
        duration: Duration,
    ) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.steps.push(IncidentStep {
            timestamp,
            command: command.into(),
            exit_code,
            stderr_summary: stderr_summary.into(),
            duration,
        });
    }

    /// Build a compact text representation of the full step log suitable for
    /// passing to the AI runbook prompt.
    pub fn step_log(&self) -> String {
        let mut buf = String::new();
        let origin = self.start_timestamp;
        for (i, step) in self.steps.iter().enumerate() {
            let offset_secs = step.timestamp.saturating_sub(origin);
            let mins = offset_secs / 60;
            let secs = offset_secs % 60;
            let duration_ms = step.duration.as_millis();
            buf.push_str(&format!(
                "Step {:>3}  T+{:02}:{:02}  exit={:>3}  {}ms  $ {}\n",
                i + 1,
                mins,
                secs,
                step.exit_code,
                duration_ms,
                step.command,
            ));
            if !step.stderr_summary.is_empty() {
                for line in step.stderr_summary.lines() {
                    buf.push_str(&format!("           stderr> {}\n", line));
                }
            }
        }
        buf
    }

    /// Human-readable elapsed time since the session started.
    pub fn elapsed_display(&self) -> String {
        format_duration(self.start_instant.elapsed())
    }
}

// ── Prompt indicator ──────────────────────────────────────────────────────────

/// Short indicator string to embed in the shell prompt when incident is active.
/// The caller should dim this (e.g. wrap in `\x1b[90m`).
#[allow(dead_code)]
pub fn prompt_indicator(session: &IncidentSession) -> String {
    format!("[INC:{}]", session.id())
}

// ── Utility helpers ───────────────────────────────────────────────────────────

/// Convert seconds-since-epoch to a `YYYY-MM-DD` string using simple arithmetic.
/// Avoids pulling in a chrono dependency.
fn format_unix_date(secs: u64) -> String {
    // Days since 1970-01-01
    let days = secs / 86400;
    // Gregorian calendar approximation
    let (y, m, d) = days_to_ymd(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Minimal Gregorian date calculation from days-since-epoch.
fn days_to_ymd(mut days: u64) -> (u32, u32, u32) {
    let mut year = 1970u32;
    loop {
        let leap = is_leap(year);
        let days_in_year = if leap { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let leap = is_leap(year);
    let month_days: &[u32] = if leap {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1u32;
    for &md in month_days {
        if days < md as u64 {
            break;
        }
        days -= md as u64;
        month += 1;
    }
    (year, month, days as u32 + 1)
}

fn is_leap(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Convert a human label to a URL/filename safe slug (lowercase, hyphens).
fn slug(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Format a duration as a human-readable string (e.g. `3m 22s`).
pub fn format_duration(d: Duration) -> String {
    let total_secs = d.as_secs();
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{}h {:02}m {:02}s", hours, mins, secs)
    } else if mins > 0 {
        format!("{}m {:02}s", mins, secs)
    } else {
        format!("{}s", secs)
    }
}

// ── Markdown report builder ───────────────────────────────────────────────────

/// Build a standalone markdown post-incident report from the step log.
///
/// This is a purely synchronous fallback that can run without an AI call.
/// The AI-enhanced version (if enabled) is generated separately via
/// `ai::generate_incident_runbook`.
pub fn build_markdown_report(incident_id: &str, incident_name: &str, step_log: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let date = format_unix_date(now);

    format!(
        "# Post-Incident Report: {incident_name}\n\n\
         **Incident ID:** {incident_id}  \n\
         **Generated:** {date}  \n\n\
         ---\n\n\
         ## Command Journal\n\n\
         ```\n\
         {step_log}\
         ```\n\n\
         ---\n\n\
         ## Summary\n\n\
         > *AI-generated narrative not available. Run `shako incident report` \
         with AI enabled for a full post-mortem analysis.*\n"
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slug_basic() {
        assert_eq!(slug("payment-svc-latency"), "payment-svc-latency");
        // Trailing non-alphanumeric chars become hyphens then are stripped.
        assert_eq!(slug("my incident!"), "my-incident");
        assert_eq!(slug("Hello World"), "hello-world");
    }

    #[test]
    fn test_format_duration_secs() {
        assert_eq!(format_duration(Duration::from_secs(45)), "45s");
    }

    #[test]
    fn test_format_duration_mins() {
        assert_eq!(format_duration(Duration::from_secs(125)), "2m 05s");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(Duration::from_secs(3723)), "1h 02m 03s");
    }

    #[test]
    fn test_format_unix_date_epoch() {
        // 1970-01-01
        assert_eq!(format_unix_date(0), "1970-01-01");
    }

    #[test]
    fn test_format_unix_date_known() {
        // 2024-04-10  → days_since_epoch = 19823
        // 19823 * 86400 = 1712620800
        assert_eq!(format_unix_date(1712620800), "2024-04-09");
    }

    #[test]
    fn test_session_step_log_format() {
        let mut session = IncidentSession::new("test-incident");
        // Manually set start to a fixed timestamp so the offset is deterministic
        session.start_timestamp = 1000;
        session.steps.push(IncidentStep {
            timestamp: 1065, // T+01:05
            command: "kubectl get pods".to_string(),
            exit_code: 0,
            stderr_summary: String::new(),
            duration: Duration::from_millis(432),
        });
        let log = session.step_log();
        assert!(log.contains("kubectl get pods"));
        assert!(log.contains("exit=  0"));
        assert!(log.contains("432ms"));
    }

    #[test]
    fn test_session_id_format() {
        let mut session = IncidentSession::new("payment-svc");
        session.start_timestamp = 1712620800; // 2024-04-09
        let id = session.id();
        assert!(id.starts_with("INC-2024-04-09-payment-svc"), "got: {id}");
    }
}
