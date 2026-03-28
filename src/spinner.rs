use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

const BRAILLE_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub struct Spinner {
    running: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Spinner {
    pub fn start(message: &str) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();
        let msg = message.to_string();

        let handle = thread::spawn(move || {
            let mut i = 0;
            while running_clone.load(Ordering::Relaxed) {
                let frame = BRAILLE_FRAMES[i % BRAILLE_FRAMES.len()];
                eprint!("\r\x1b[90m{frame} {msg}\x1b[0m");
                io::stderr().flush().ok();
                i += 1;
                thread::sleep(Duration::from_millis(80));
            }
            eprint!("\r\x1b[K");
            io::stderr().flush().ok();
        });

        Self {
            running,
            handle: Some(handle),
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
    fn test_braille_frames_not_empty() {
        assert!(!BRAILLE_FRAMES.is_empty());
    }
}
