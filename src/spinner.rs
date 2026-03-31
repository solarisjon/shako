use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

// Themed spinner frames — dots that feel smooth and crafted
const SPINNER_FRAMES: &[&str] = &["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"];

// Gradient colors matching the startup banner: teal → cyan
const GRAD: &[u8] = &[30, 31, 32, 37, 38, 44, 45];

fn spinner_color(frame_idx: usize) -> u8 {
    GRAD[frame_idx % GRAD.len()]
}

pub struct Spinner {
    running: Arc<AtomicBool>,
    /// Shared message — can be updated via `set_phase` to show phase transitions.
    message: Arc<Mutex<String>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Spinner {
    pub fn start(message: &str) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let message = Arc::new(Mutex::new(message.to_string()));
        let running_clone = running.clone();
        let message_clone = message.clone();

        let handle = thread::spawn(move || {
            let mut i = 0usize;
            while running_clone.load(Ordering::Relaxed) {
                let frame = SPINNER_FRAMES[i % SPINNER_FRAMES.len()];
                let color = spinner_color(i);
                let msg = message_clone.lock().map(|g| g.clone()).unwrap_or_default();
                eprint!("\r\x1b[38;5;{color}m{frame}\x1b[0m \x1b[90m{msg}\x1b[0m\x1b[K");
                io::stderr().flush().ok();
                i += 1;
                thread::sleep(Duration::from_millis(80));
            }
            eprint!("\r\x1b[K");
            io::stderr().flush().ok();
        });

        Self {
            running,
            message,
            handle: Some(handle),
        }
    }

    /// Update the spinner message mid-flight (e.g. "translating..." → "executing...").
    #[allow(dead_code)]
    pub fn set_phase(&self, phase: &str) {
        if let Ok(mut msg) = self.message.lock() {
            *msg = phase.to_string();
        }
    }

    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        self.running.clone()
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            handle.join().ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spinner_starts_and_stops() {
        let spinner = Spinner::start("testing...");
        thread::sleep(Duration::from_millis(200));
        drop(spinner);
    }

    #[test]
    fn test_spinner_drop_stops() {
        let spinner = Spinner::start("drop test...");
        thread::sleep(Duration::from_millis(100));
        drop(spinner);
    }

    #[test]
    fn test_spinner_frames_not_empty() {
        assert!(!SPINNER_FRAMES.is_empty());
    }
}
