//! Integration with the [TUIOS](https://tuios.dev) terminal multiplexer.
//!
//! When running inside a TUIOS pane, shako reports command activity through
//! OSC 7501 (Program Status) so the pane shows working / done / error state,
//! and emits OSC 133 prompt marks so TUIOS clears stale `working` records when
//! a new prompt starts. Outside TUIOS nothing is written.

use std::io::{self, Write};
use std::sync::OnceLock;
use std::time::Duration;

/// Commands shorter than this don't leave a `done`/`error` record behind; the
/// prompt mark clears their `working` record instead, so quick commands like
/// `ls` don't fill TUIOS's inbox.
const NOTIFY_THRESHOLD: Duration = Duration::from_secs(5);

/// True when this process is running in a TUIOS pane.
///
/// TUIOS exports `TUIOS_SESSION` to panes; `TERM_PROGRAM=tuios` is accepted
/// too. Set `SHAKO_NO_TUIOS=1` to opt out.
pub fn active() -> bool {
    static ACTIVE: OnceLock<bool> = OnceLock::new();
    *ACTIVE.get_or_init(|| {
        std::env::var_os("SHAKO_NO_TUIOS").is_none()
            && (std::env::var_os("TUIOS_SESSION").is_some()
                || std::env::var("TERM_PROGRAM").is_ok_and(|v| v.eq_ignore_ascii_case("tuios")))
    })
}

fn emit(seq: &str) {
    let mut out = io::stdout();
    let _ = out.write_all(seq.as_bytes());
    let _ = out.flush();
}

/// OSC 133;A — a new prompt is about to be drawn.
pub fn prompt_start() {
    if active() {
        emit("\x1b]133;A\x1b\\");
    }
}

/// OSC 7501 `working` — a command has started.
pub fn command_started() {
    if active() {
        emit("\x1b]7501;state=working:app=shako\x1b\\");
    }
}

/// OSC 7501 `done`/`error` for a finished command that ran long enough to matter.
pub fn command_finished(status: i32, elapsed: Duration) {
    if active() && elapsed >= NOTIFY_THRESHOLD {
        emit(&finished_sequence(status));
    }
}

fn finished_sequence(status: i32) -> String {
    let state = if status == 0 { "done" } else { "error" };
    format!("\x1b]7501;state={state}:app=shako\x1b\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finished_sequence_maps_status_to_state() {
        assert_eq!(finished_sequence(0), "\x1b]7501;state=done:app=shako\x1b\\");
        assert_eq!(
            finished_sequence(2),
            "\x1b]7501;state=error:app=shako\x1b\\"
        );
    }
}
