//! Command execution engine for shako.
//!
//! This module handles the full lifecycle of running shell commands:
//! process spawning, pipe plumbing, I/O redirection (heredoc, herestring,
//! `>`, `>>`, `<`, `2>`, `2>&1`), chain operators (`&&`, `||`, `;`), and
//! job-control primitives (foreground wait, background spawn, stopped-job
//! notifications).
//!
//! The main entry points are [`execute_command`] and [`execute_command_with_stderr`].

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, ExitStatus, Stdio};

use crate::parser;

/// Information about a foreground job that was stopped (Ctrl-Z / SIGTSTP)
/// rather than exited.  Posted to the thread-local [`STOPPED_JOB`] so that
/// the REPL loop in `main.rs` can add it to `ShellState.jobs`.
pub struct StoppedJob {
    /// Process ID of the stopped child.
    pub pid: u32,
    /// Process group ID — used to send signals to the entire job pipeline.
    pub pgid: i32,
}

thread_local! {
    /// Set by [`foreground_wait`] when the child is stopped instead of exited.
    /// Cleared by the REPL loop after each command dispatch.
    pub static STOPPED_JOB: RefCell<Option<StoppedJob>> = const { RefCell::new(None) };
}

/// Take any stopped-job notification posted by the last foreground command.
/// Returns `None` if the foreground process exited normally.
pub fn take_stopped_job() -> Option<StoppedJob> {
    STOPPED_JOB.with(|cell| cell.borrow_mut().take())
}

#[cfg(unix)]
use std::os::unix::process::CommandExt;

/// Set up a command for proper signal handling on Unix.
///
/// `pgid` controls the process group:
///   - `0`  → child becomes its own process-group leader (`setpgid(0, 0)`)
///   - `N`  → child joins an existing process group N (`setpgid(0, N)`)
///
/// `dup_stderr_to_stdout` merges stderr into stdout (for `2>&1`).
///
/// Both operations are combined in a single `pre_exec` closure because
/// calling `pre_exec` multiple times replaces the previous closure.
#[cfg(unix)]
fn setup_child_signals(cmd: &mut Command, pgid: i32, dup_stderr_to_stdout: bool) {
    unsafe {
        cmd.pre_exec(move || {
            nix::unistd::setpgid(
                nix::unistd::Pid::from_raw(0),
                nix::unistd::Pid::from_raw(pgid),
            )
            .ok();
            use nix::sys::signal::{signal, SigHandler, Signal};
            for sig in [
                Signal::SIGINT,
                Signal::SIGQUIT,
                Signal::SIGTSTP,
                Signal::SIGTTOU,
                Signal::SIGTTIN,
            ] {
                signal(sig, SigHandler::SigDfl).ok();
            }
            if dup_stderr_to_stdout && libc::dup2(1, 2) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn setup_child_signals(_cmd: &mut Command, _pgid: i32, _dup_stderr_to_stdout: bool) {}

/// Run a foreground child: hand the terminal to the child's process group,
/// wait for it to finish, then reclaim the terminal for the shell.
///
/// On non-Unix the function just waits normally.
#[cfg(unix)]
fn foreground_wait(child: std::process::Child) -> std::process::ExitStatus {
    use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};

    let child_pid = nix::unistd::Pid::from_raw(child.id() as i32);
    let child_pgid = child_pid; // each foreground child is its own process-group leader
    let shell_pgid = nix::unistd::getpgrp();

    // Close the TOCTOU race: set the child's pgid in the parent too.
    // (The child also does this in pre_exec; whichever runs first wins.)
    let _ = nix::unistd::setpgid(child_pid, child_pid);

    // Give the terminal to the child so Ctrl-C/Ctrl-Z reach it.
    let _ = nix::unistd::tcsetpgrp(std::io::stdin(), child_pid);

    // Wait for the child to exit *or* be stopped (WUNTRACED).
    // Using nix::waitpid so we can distinguish exit from stop.
    let wait_flags = WaitPidFlag::WUNTRACED;
    let wait_result = loop {
        match waitpid(child_pid, Some(wait_flags)) {
            Ok(status) => break status,
            Err(nix::errno::Errno::EINTR) => continue, // retry on signal interrupt
            Err(e) => {
                log::warn!("waitpid() failed: {e}");
                break WaitStatus::Exited(child_pid, 1);
            }
        }
    };

    // Restore terminal ownership to the shell.
    let _ = nix::unistd::tcsetpgrp(std::io::stdin(), shell_pgid);

    // Immediately disable echo to prevent late-arriving terminal responses
    // (vim's OSC/DCS queries) from being displayed during prompt rendering.
    suppress_echo();

    drain_pending_input();

    match wait_result {
        WaitStatus::Stopped(pid, _signal) => {
            // The child was suspended (Ctrl-Z / SIGTSTP).
            // Notify the REPL loop so it can add this to the jobs list.
            // We intentionally leak `child` here — the process is still alive
            // and will be resumed via SIGCONT; the jobs list owns it from now on.
            STOPPED_JOB.with(|cell| {
                *cell.borrow_mut() = Some(StoppedJob {
                    pid: pid.as_raw() as u32,
                    pgid: child_pgid.as_raw(),
                });
            });
            // Forget the Child so Rust does not wait/kill it on drop.
            std::mem::forget(child);
            // Return exit code 148 (128 + SIGTSTP) to signal "stopped".
            fake_status(148)
        }
        WaitStatus::Exited(_, code) => fake_status(code),
        WaitStatus::Signaled(_, signal, _) => fake_status(128 + signal as i32),
        _ => fake_status(0),
    }
}

/// Temporarily disable terminal echo so late-arriving escape sequences
/// from programs like vim aren't displayed during prompt rendering.
/// Must be paired with restore_echo() before the next read_line() call.
#[cfg(unix)]
fn suppress_echo() {
    use std::os::unix::io::AsRawFd;
    let fd = std::io::stdin().as_raw_fd();
    unsafe {
        let mut termios: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut termios) == 0 {
            termios.c_lflag &= !libc::ECHO;
            libc::tcsetattr(fd, libc::TCSANOW, &termios);
        }
    }
}

/// Re-enable terminal echo before handing control back to reedline.
///
/// `suppress_echo()` disables ECHO so that late-arriving terminal responses
/// (e.g. vim's OSC queries) don't appear on screen. We must re-enable it
/// before reedline's `read_line()` — otherwise reedline saves ECHO=0 as its
/// "normal" baseline and the ColumnarMenu (tab completion) can end up in a
/// broken state on some terminals.
#[cfg(unix)]
pub fn restore_echo() {
    use std::os::unix::io::AsRawFd;
    let fd = std::io::stdin().as_raw_fd();
    unsafe {
        let mut termios: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut termios) == 0 {
            termios.c_lflag |= libc::ECHO | libc::ECHOE | libc::ECHOK | libc::ECHONL;
            libc::tcsetattr(fd, libc::TCSANOW, &termios);
        }
    }
}

/// Discard any bytes waiting on stdin after a foreground process exits.
///
/// Programs like vim/neovim send terminal queries (OSC 11 for background
/// color, DCS for capabilities) whose responses arrive asynchronously.
/// If the response arrives after the program exits, it appears as typed
/// input in the next prompt. This uses `tcflush(TCIFLUSH)` to discard
/// the terminal driver's input buffer at the kernel level.
#[cfg(unix)]
pub fn drain_pending_input() {
    use std::os::unix::io::AsRawFd;
    let fd = std::io::stdin().as_raw_fd();
    unsafe { libc::tcflush(fd, libc::TCIFLUSH) };
}

#[cfg(not(unix))]
fn foreground_wait(mut child: std::process::Child) -> std::process::ExitStatus {
    child.wait().unwrap_or_else(|_| fake_status(1))
}

/// Spawn a command in the background, returning the Child.
pub fn spawn_background(input: &str) -> Option<std::process::Child> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    let redir = parse_redirects(input);
    let args = parser::parse_args(&redir.cmd);
    if args.is_empty() {
        return None;
    }

    let program = &args[0];
    let cmd_args = &args[1..];

    let mut cmd = Command::new(program);
    cmd.args(cmd_args);
    setup_child_signals(&mut cmd, 0, redir.stderr_to_stdout);

    if let Some(ref path) = redir.stdin_path {
        match File::open(path) {
            Ok(f) => {
                cmd.stdin(Stdio::from(f));
            }
            Err(e) => {
                eprintln!("shako: {path}: {e}");
                return None;
            }
        }
    } else if redir.herestring.is_some() {
        cmd.stdin(Stdio::piped());
    }

    if let Some(ref path) = redir.stdout_path {
        let file = if redir.stdout_append {
            OpenOptions::new().create(true).append(true).open(path)
        } else {
            File::create(path)
        };
        match file {
            Ok(f) => {
                cmd.stdout(Stdio::from(f));
            }
            Err(e) => {
                eprintln!("shako: {path}: {e}");
                return None;
            }
        }
    }

    apply_stderr_redirect(&mut cmd, &redir);

    match cmd.spawn() {
        Ok(mut child) => {
            if let Some(ref content) = redir.herestring {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(content.as_bytes());
                    let _ = stdin.write_all(b"\n");
                }
            }
            Some(child)
        }
        Err(e) => {
            eprintln!("shako: {program}: {e}");
            None
        }
    }
}

/// Execute a shell command string, handling chains, pipes, redirects,
/// quoting, and expansion.
pub fn execute_command(input: &str) -> Option<ExitStatus> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    let chains = parser::split_chains(input);
    let mut last_status: Option<ExitStatus> = None;
    let mut prev_op = parser::ChainOp::None;

    for (cmd, op) in &chains {
        let should_run = match prev_op {
            parser::ChainOp::None | parser::ChainOp::Semi => true,
            parser::ChainOp::And => last_status.is_some_and(|s| s.success()),
            parser::ChainOp::Or => last_status.is_some_and(|s| !s.success()),
        };

        if should_run {
            last_status = Some(execute_chain_segment(cmd));
        }

        prev_op = *op;
    }

    last_status
}

/// Like `execute_command` but captures stderr (last 20 lines) while still
/// printing it to the terminal in real time. Returns (exit_status, stderr_tail).
pub fn execute_command_with_stderr(input: &str) -> (Option<ExitStatus>, String) {
    let input = input.trim();
    if input.is_empty() {
        return (None, String::new());
    }

    let chains = parser::split_chains(input);
    if chains.len() > 1 {
        return (execute_command(input), String::new());
    }

    let segments = parser::split_pipes(input);
    if segments.len() > 1 {
        return (execute_command(input), String::new());
    }

    let redir = parse_redirects(input);

    if redir.stderr_path.is_some() || redir.stderr_to_stdout {
        return (execute_command(input), String::new());
    }

    let args = parser::parse_args(&redir.cmd);
    if args.is_empty() {
        return (Some(fake_status(1)), String::new());
    }

    let program = &args[0];
    let cmd_args = &args[1..];

    let mut cmd = Command::new(program);
    cmd.args(cmd_args);
    setup_child_signals(&mut cmd, 0, false);

    if let Some(ref path) = redir.stdin_path {
        match File::open(path) {
            Ok(f) => {
                cmd.stdin(Stdio::from(f));
            }
            Err(e) => {
                eprintln!("shako: {path}: {e}");
                return (Some(fake_status(1)), String::new());
            }
        }
    }

    if let Some(ref path) = redir.stdout_path {
        let file = if redir.stdout_append {
            OpenOptions::new().create(true).append(true).open(path)
        } else {
            File::create(path)
        };
        match file {
            Ok(f) => {
                cmd.stdout(Stdio::from(f));
            }
            Err(e) => {
                eprintln!("shako: {path}: {e}");
                return (Some(fake_status(1)), String::new());
            }
        }
    }

    cmd.stderr(Stdio::piped());

    match cmd.spawn() {
        Ok(mut child) => {
            let stderr_pipe = child.stderr.take();

            let stderr_thread = std::thread::spawn(move || {
                // VecDeque so pop_front is O(1) instead of Vec::remove(0) O(n).
                let mut collected: VecDeque<String> = VecDeque::new();
                if let Some(pipe) = stderr_pipe {
                    let reader = BufReader::new(pipe);
                    for line in reader.lines() {
                        match line {
                            Ok(line) => {
                                eprintln!("{line}");
                                collected.push_back(line);
                                if collected.len() > 20 {
                                    collected.pop_front();
                                }
                            }
                            Err(_) => break,
                        }
                    }
                }
                collected.into_iter().collect::<Vec<_>>().join("\n")
            });

            let status = foreground_wait(child);
            let stderr_output = match stderr_thread.join() {
                Ok(s) => s,
                Err(_) => {
                    log::warn!(
                        "stderr capture thread panicked; AI error diagnosis may be incomplete"
                    );
                    String::new()
                }
            };

            if !status.success() {
                if let Some(code) = status.code() {
                    log::debug!("command exited with status {code}");
                }
            }
            (Some(status), stderr_output)
        }
        Err(e) => {
            eprintln!("shako: {program}: {e}");
            (Some(fake_status(127)), format!("{e}"))
        }
    }
}

/// Execute a single chain segment (may contain pipes).
fn execute_chain_segment(input: &str) -> ExitStatus {
    let segments = parser::split_pipes(input);

    if segments.len() > 1 {
        execute_pipeline(&segments)
    } else {
        execute_single(input)
    }
}

/// Apply stderr file redirects (`2>file` / `2>>file`) to a Command.
/// Note: `2>&1` is handled in `setup_child_signals` via `dup_stderr_to_stdout`
/// to avoid the pre_exec collision.
fn apply_stderr_redirect(cmd: &mut Command, redir: &Redirects) {
    if let Some(ref path) = redir.stderr_path {
        let file = if redir.stderr_append {
            OpenOptions::new().create(true).append(true).open(path)
        } else {
            File::create(path)
        };
        match file {
            Ok(f) => {
                cmd.stderr(Stdio::from(f));
            }
            Err(e) => {
                eprintln!("shako: {path}: {e}");
            }
        }
    }
}

/// Execute a single command with optional redirects.
fn execute_single(input: &str) -> ExitStatus {
    let redir = parse_redirects(input);

    let args = parser::parse_args(&redir.cmd);
    if args.is_empty() {
        return fake_status(1);
    }

    let program = &args[0];
    let cmd_args = &args[1..];

    let mut cmd = Command::new(program);
    cmd.args(cmd_args);
    setup_child_signals(&mut cmd, 0, redir.stderr_to_stdout);

    if let Some(ref path) = redir.stdin_path {
        match File::open(path) {
            Ok(f) => {
                cmd.stdin(Stdio::from(f));
            }
            Err(e) => {
                eprintln!("shako: {path}: {e}");
                return fake_status(1);
            }
        }
    } else if redir.herestring.is_some() {
        cmd.stdin(Stdio::piped());
    }

    if let Some(ref path) = redir.stdout_path {
        let file = if redir.stdout_append {
            OpenOptions::new().create(true).append(true).open(path)
        } else {
            File::create(path)
        };
        match file {
            Ok(f) => {
                cmd.stdout(Stdio::from(f));
            }
            Err(e) => {
                eprintln!("shako: {path}: {e}");
                return fake_status(1);
            }
        }
    }

    apply_stderr_redirect(&mut cmd, &redir);

    match cmd.spawn() {
        Ok(mut child) => {
            if let Some(ref content) = redir.herestring {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(content.as_bytes());
                    let _ = stdin.write_all(b"\n");
                }
            }
            let status = foreground_wait(child);
            if !status.success() {
                if let Some(code) = status.code() {
                    log::debug!("command exited with status {code}");
                }
            }
            status
        }
        Err(e) => {
            eprintln!("shako: {program}: {e}");
            fake_status(127)
        }
    }
}

/// Execute a pipeline of commands connected by pipes.
///
/// All processes in the pipeline share a single process group (the first
/// child's pid becomes the pgid for the rest), and the terminal is handed
/// to that group for the duration of the pipeline so that Ctrl-C/Ctrl-Z
/// are delivered to every process in the pipe, not to the shell.
fn execute_pipeline(segments: &[String]) -> ExitStatus {
    let mut children = Vec::new();
    let mut prev_stdout: Option<std::process::ChildStdout> = None;
    // pid of the first child; all subsequent children join this process group.
    let mut pipeline_pgid: i32 = 0;

    for (i, segment) in segments.iter().enumerate() {
        let is_last = i == segments.len() - 1;
        let redir = parse_redirects(segment);

        let args = parser::parse_args(&redir.cmd);
        if args.is_empty() {
            eprintln!("shako: empty command in pipeline");
            return fake_status(1);
        }

        let program = &args[0];
        let cmd_args = &args[1..];

        let mut cmd = Command::new(program);
        cmd.args(cmd_args);
        setup_child_signals(&mut cmd, pipeline_pgid, redir.stderr_to_stdout);

        if let Some(prev) = prev_stdout.take() {
            cmd.stdin(Stdio::from(prev));
        } else if let Some(ref path) = redir.stdin_path {
            match File::open(path) {
                Ok(f) => {
                    cmd.stdin(Stdio::from(f));
                }
                Err(e) => {
                    eprintln!("shako: {path}: {e}");
                    return fake_status(1);
                }
            }
        } else if redir.herestring.is_some() {
            cmd.stdin(Stdio::piped());
        }

        if !is_last {
            cmd.stdout(Stdio::piped());
        } else if let Some(ref path) = redir.stdout_path {
            let file = if redir.stdout_append {
                OpenOptions::new().create(true).append(true).open(path)
            } else {
                File::create(path)
            };
            match file {
                Ok(f) => {
                    cmd.stdout(Stdio::from(f));
                }
                Err(e) => {
                    eprintln!("shako: {path}: {e}");
                    return fake_status(1);
                }
            }
        }

        apply_stderr_redirect(&mut cmd, &redir);

        match cmd.spawn() {
            Ok(mut child) => {
                if let Some(ref content) = redir.herestring {
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = stdin.write_all(content.as_bytes());
                        let _ = stdin.write_all(b"\n");
                    }
                }
                let child_pid = child.id() as i32;
                #[cfg(unix)]
                {
                    // Parent-side setpgid closes the TOCTOU race with the
                    // child's own pre_exec setpgid call.
                    if pipeline_pgid == 0 {
                        pipeline_pgid = child_pid;
                        let _ = nix::unistd::setpgid(
                            nix::unistd::Pid::from_raw(child_pid),
                            nix::unistd::Pid::from_raw(child_pid),
                        );
                    } else {
                        let _ = nix::unistd::setpgid(
                            nix::unistd::Pid::from_raw(child_pid),
                            nix::unistd::Pid::from_raw(pipeline_pgid),
                        );
                    }
                }
                if !is_last {
                    prev_stdout = child.stdout.take();
                }
                children.push(child);
            }
            Err(e) => {
                eprintln!("shako: {program}: {e}");
                for mut child in children {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                return fake_status(127);
            }
        }
    }

    // Hand the terminal to the pipeline's process group so Ctrl-C/Ctrl-Z
    // reach all processes in the pipe.
    #[cfg(unix)]
    let shell_pgid = nix::unistd::getpgrp();
    #[cfg(unix)]
    if pipeline_pgid != 0 {
        let _ = nix::unistd::tcsetpgrp(std::io::stdin(), nix::unistd::Pid::from_raw(pipeline_pgid));
    }

    let mut last_status = fake_status(0);
    for mut child in children {
        match child.wait() {
            Ok(status) => last_status = status,
            Err(e) => {
                eprintln!("shako: wait: {e}");
                last_status = fake_status(1);
            }
        }
    }

    // Restore terminal ownership to the shell.
    #[cfg(unix)]
    let _ = nix::unistd::tcsetpgrp(std::io::stdin(), shell_pgid);

    #[cfg(unix)]
    suppress_echo();

    #[cfg(unix)]
    drain_pending_input();

    last_status
}

/// Parsed redirect state for a single command.
struct Redirects {
    cmd: String,
    stdout_path: Option<String>,
    stderr_path: Option<String>,
    stdout_append: bool,
    stderr_append: bool,
    stdin_path: Option<String>,
    stderr_to_stdout: bool,
    /// Content for herestring (`<<<`) redirect.
    herestring: Option<String>,
}

/// Parse redirects from a command string, preserving quotes.
/// Supports: >, >>, <, <<<, <<MARKER (heredoc), 2>, 2>>, 2>&1
fn parse_redirects(input: &str) -> Redirects {
    let mut cmd_parts = Vec::new();
    let mut stdout_path = None;
    let mut stderr_path = None;
    let mut stdin_path = None;
    let mut stdout_append = false;
    let mut stderr_append = false;
    let mut stderr_to_stdout = false;

    // Heredoc (`<<MARKER`) takes priority over herestring (`<<<`).
    // The heredoc body must be embedded in the input string following a newline.
    let (herestring, input) = if let Some((body, stripped)) = extract_heredoc(input) {
        (Some(body), std::borrow::Cow::Owned(stripped))
    } else {
        // Use quote-aware tokenizer to extract herestring content correctly.
        let hs = extract_herestring(input);
        let cleaned = if hs.is_some() {
            std::borrow::Cow::Owned(strip_herestring_from_input(input))
        } else {
            std::borrow::Cow::Borrowed(input)
        };
        (hs, cleaned)
    };
    let input: &str = &input;
    let tokens: Vec<&str> = input.split_whitespace().collect();
    let mut i = 0;

    while i < tokens.len() {
        match tokens[i] {
            "2>&1" => {
                stderr_to_stdout = true;
                i += 1;
            }
            "2>>" => {
                stderr_append = true;
                if i + 1 < tokens.len() {
                    stderr_path = Some(tokens[i + 1].to_string());
                    i += 2;
                } else {
                    eprintln!("shako: syntax error near 2>>");
                    i += 1;
                }
            }
            "2>" => {
                if i + 1 < tokens.len() {
                    stderr_path = Some(tokens[i + 1].to_string());
                    i += 2;
                } else {
                    eprintln!("shako: syntax error near 2>");
                    i += 1;
                }
            }
            ">>" => {
                stdout_append = true;
                if i + 1 < tokens.len() {
                    stdout_path = Some(tokens[i + 1].to_string());
                    i += 2;
                } else {
                    eprintln!("shako: syntax error near >>");
                    i += 1;
                }
            }
            ">" => {
                if i + 1 < tokens.len() {
                    stdout_path = Some(tokens[i + 1].to_string());
                    i += 2;
                } else {
                    eprintln!("shako: syntax error near >");
                    i += 1;
                }
            }
            "<" => {
                if i + 1 < tokens.len() {
                    stdin_path = Some(tokens[i + 1].to_string());
                    i += 2;
                } else {
                    eprintln!("shako: syntax error near <");
                    i += 1;
                }
            }
            token => {
                if token == "2>&1" {
                    stderr_to_stdout = true;
                } else if let Some(path) = token.strip_prefix("2>>") {
                    stderr_append = true;
                    if !path.is_empty() {
                        stderr_path = Some(path.to_string());
                    }
                } else if let Some(path) = token.strip_prefix("2>") {
                    if path == "&1" {
                        stderr_to_stdout = true;
                    } else if !path.is_empty() {
                        stderr_path = Some(path.to_string());
                    }
                } else if let Some(path) = token.strip_prefix(">>") {
                    stdout_append = true;
                    if !path.is_empty() {
                        stdout_path = Some(path.to_string());
                    }
                } else if let Some(path) = token.strip_prefix('>') {
                    if !path.is_empty() {
                        stdout_path = Some(path.to_string());
                    }
                } else if let Some(path) = token.strip_prefix('<') {
                    if !path.is_empty() {
                        stdin_path = Some(path.to_string());
                    }
                } else {
                    cmd_parts.push(token);
                }
                i += 1;
            }
        }
    }

    Redirects {
        cmd: cmd_parts.join(" "),
        stdout_path,
        stderr_path,
        stdout_append,
        stderr_append,
        stdin_path,
        stderr_to_stdout,
        herestring,
    }
}

/// Extract and expand the herestring word from a raw command string.
///
/// Uses the parser tokenizer for proper quote handling, so
/// `cat <<< "hello world"` correctly produces `hello world`.
/// Also handles the no-space form: `cat <<<word`.
fn extract_herestring(input: &str) -> Option<String> {
    let tokens = parser::tokenize(input);
    let mut i = 0;
    while i < tokens.len() {
        if !tokens[i].quoted {
            if tokens[i].value == "<<<" {
                if i + 1 < tokens.len() {
                    // Value is already quote-stripped and env-expanded by the tokenizer
                    return Some(tokens[i + 1].value.clone());
                }
            } else if let Some(word) = tokens[i].value.strip_prefix("<<<") {
                if !word.is_empty() {
                    return Some(expand_herestring_word(word));
                }
            }
        }
        i += 1;
    }
    None
}

/// Expand a single herestring word token (simple no-space case).
fn expand_herestring_word(raw: &str) -> String {
    parser::parse_args(raw).join(" ")
}

/// Remove `<<<` and its argument from the input string for redirect re-parsing.
///
/// Scans character-by-character to respect quoted sections, then strips
/// the `<<<` operator and the following word so split_whitespace doesn't
/// accidentally put herestring content into `cmd_parts`.
fn strip_herestring_from_input(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;

    while i < chars.len() {
        match chars[i] {
            '\'' if !in_double => {
                in_single = !in_single;
                i += 1;
            }
            '"' if !in_single => {
                in_double = !in_double;
                i += 1;
            }
            '<' if !in_single
                && !in_double
                && i + 2 < chars.len()
                && chars[i + 1] == '<'
                && chars[i + 2] == '<' =>
            {
                let start = i;
                i += 3; // skip <<<
                        // Skip whitespace between <<< and the word
                while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
                    i += 1;
                }
                // Skip the word, respecting quotes
                let mut ws = false;
                let mut wd = false;
                while i < chars.len() {
                    match chars[i] {
                        '\'' if !wd => ws = !ws,
                        '"' if !ws => wd = !wd,
                        ' ' | '\t' if !ws && !wd => break,
                        _ => {}
                    }
                    i += 1;
                }
                let before: String = chars[..start].iter().collect();
                let after: String = chars[i..].iter().collect();
                return format!("{}{}", before.trim_end(), after);
            }
            _ => {
                i += 1;
            }
        }
    }
    input.to_string()
}

/// Extract a heredoc (`<<MARKER`) from a potentially multi-line input string.
///
/// Returns `Some((body, stripped_cmd))` when a heredoc is found, where:
/// - `body` is the collected heredoc content (without the terminator line)
/// - `stripped_cmd` is the command with the `<<MARKER` operator removed
///
/// The function handles:
/// - Standard heredoc: `<<EOF`
/// - Indented heredoc: `<<-EOF` (strips leading tabs from each body line)
/// - Quoted terminators: `<<"EOF"` or `<<'EOF'` (suppress variable expansion; we
///   use the raw terminator for matching but return the body as-is)
///
/// Variable expansion in the body is performed unless the marker is quoted.
/// In the embedded single-string case (e.g. `shako -c "cmd <<EOF\nbody\nEOF"`),
/// the newlines are already embedded in the input string.
fn extract_heredoc(input: &str) -> Option<(String, String)> {
    // Fast path: no heredoc operator present.
    if !input.contains("<<") {
        return None;
    }
    // Reject herestring (<<<) before further processing.
    // We need to find `<<` that is NOT followed by another `<`.
    let lines: Vec<&str> = input.splitn(2, '\n').collect();
    let first_line = lines.first().copied().unwrap_or("");

    // Scan the first line for `<<[-]marker` (not `<<<`).
    let chars: Vec<char> = first_line.chars().collect();
    let mut i = 0;
    let mut heredoc_start = None;
    let mut strip_tabs = false;

    while i < chars.len() {
        if chars[i] == '<'
            && i + 1 < chars.len()
            && chars[i + 1] == '<'
            && !(i + 2 < chars.len() && chars[i + 2] == '<')
        {
            i += 2; // skip <<
            if i < chars.len() && chars[i] == '-' {
                strip_tabs = true;
                i += 1;
            }
            // Skip optional whitespace before marker.
            while i < chars.len() && chars[i] == ' ' {
                i += 1;
            }
            heredoc_start = Some(i);
            break;
        }
        i += 1;
    }

    let marker_start = heredoc_start?;

    // Extract the marker (possibly quoted).
    let mut marker_raw = String::new();
    let mut j = marker_start;
    let mut quoted = false;
    let quote_char = if j < chars.len() && (chars[j] == '\'' || chars[j] == '"') {
        let q = chars[j];
        j += 1;
        quoted = true;
        Some(q)
    } else {
        None
    };
    while j < chars.len() {
        let c = chars[j];
        if let Some(qc) = quote_char {
            if c == qc {
                break; // skip closing quote; j will advance in outer loop
            }
        } else if c == ' ' || c == '\t' || c == ';' || c == '|' || c == '&' {
            break;
        }
        marker_raw.push(c);
        j += 1;
    }

    if marker_raw.is_empty() {
        return None;
    }

    // The body lives after the first newline.
    let rest = if lines.len() > 1 { lines[1] } else { "" };

    // Collect lines until the terminator.
    let mut body_lines: Vec<&str> = Vec::new();
    let mut found_terminator = false;

    for line in rest.split('\n') {
        let trimmed = if strip_tabs {
            line.trim_start_matches('\t')
        } else {
            line
        };
        if trimmed == marker_raw {
            found_terminator = true;
            break;
        }
        body_lines.push(if strip_tabs { trimmed } else { line });
    }

    if !found_terminator {
        return None;
    }

    // Expand variables in the body unless the marker was quoted.
    let body = if quoted {
        body_lines.join("\n")
    } else {
        body_lines
            .iter()
            .map(|line| parser::parse_args(line).join(" "))
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Build the stripped command (first line with <<MARKER removed).
    let cmd_part: String = chars[..marker_start.saturating_sub(if strip_tabs { 3 } else { 2 })]
        .iter()
        .collect();
    let cmd_stripped = cmd_part.trim_end().to_string();

    Some((body, cmd_stripped))
}

/// Create a fake ExitStatus with the given code.
/// Tries `sh -c "exit N"` first; falls back to `/bin/true` or `/bin/false`
/// in restricted environments where `sh` is unavailable (containers, sandboxes).
fn fake_status(code: i32) -> ExitStatus {
    if let Ok(s) = Command::new("sh")
        .args(["-c", &format!("exit {code}")])
        .status()
    {
        return s;
    }
    let fallback = if code == 0 { "/bin/true" } else { "/bin/false" };
    if let Ok(s) = Command::new(fallback).status() {
        return s;
    }
    // Last resort: `true` or `false` from PATH
    if let Ok(s) = Command::new(if code == 0 { "true" } else { "false" }).status() {
        return s;
    }
    eprintln!("shako: could not construct exit status for code {code}");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_redirects_none() {
        let r = parse_redirects("ls -la");
        assert_eq!(r.cmd, "ls -la");
        assert!(r.stdout_path.is_none());
        assert!(!r.stdout_append);
        assert!(r.stdin_path.is_none());
        assert!(r.stderr_path.is_none());
        assert!(!r.stderr_to_stdout);
    }

    #[test]
    fn test_parse_redirects_stdout() {
        let r = parse_redirects("echo hello > output.txt");
        assert_eq!(r.cmd, "echo hello");
        assert_eq!(r.stdout_path.unwrap(), "output.txt");
        assert!(!r.stdout_append);
    }

    #[test]
    fn test_parse_redirects_append() {
        let r = parse_redirects("echo hello >> output.txt");
        assert_eq!(r.cmd, "echo hello");
        assert_eq!(r.stdout_path.unwrap(), "output.txt");
        assert!(r.stdout_append);
    }

    #[test]
    fn test_parse_redirects_stdin() {
        let r = parse_redirects("sort < input.txt");
        assert_eq!(r.cmd, "sort");
        assert_eq!(r.stdin_path.unwrap(), "input.txt");
    }

    #[test]
    fn test_parse_redirects_no_space() {
        let r = parse_redirects("echo hello >output.txt");
        assert_eq!(r.cmd, "echo hello");
        assert_eq!(r.stdout_path.unwrap(), "output.txt");
        assert!(!r.stdout_append);
    }

    #[test]
    fn test_parse_redirects_append_no_space() {
        let r = parse_redirects("echo hello >>output.txt");
        assert_eq!(r.cmd, "echo hello");
        assert_eq!(r.stdout_path.unwrap(), "output.txt");
        assert!(r.stdout_append);
    }

    #[test]
    fn test_parse_redirects_stderr() {
        let r = parse_redirects("make 2> errors.log");
        assert_eq!(r.cmd, "make");
        assert_eq!(r.stderr_path.unwrap(), "errors.log");
        assert!(!r.stderr_append);
    }

    #[test]
    fn test_parse_redirects_stderr_append() {
        let r = parse_redirects("make 2>> errors.log");
        assert_eq!(r.cmd, "make");
        assert_eq!(r.stderr_path.unwrap(), "errors.log");
        assert!(r.stderr_append);
    }

    #[test]
    fn test_parse_redirects_stderr_to_stdout() {
        let r = parse_redirects("make 2>&1");
        assert_eq!(r.cmd, "make");
        assert!(r.stderr_to_stdout);
    }

    #[test]
    fn test_parse_redirects_stderr_no_space() {
        let r = parse_redirects("make 2>errors.log");
        assert_eq!(r.cmd, "make");
        assert_eq!(r.stderr_path.unwrap(), "errors.log");
    }

    #[test]
    fn test_parse_redirects_stderr_to_stdout_no_space() {
        let r = parse_redirects("make 2>&1");
        assert_eq!(r.cmd, "make");
        assert!(r.stderr_to_stdout);
    }

    #[test]
    fn test_parse_redirects_combined() {
        let r = parse_redirects("make > out.log 2> err.log");
        assert_eq!(r.cmd, "make");
        assert_eq!(r.stdout_path.unwrap(), "out.log");
        assert_eq!(r.stderr_path.unwrap(), "err.log");
    }

    #[test]
    fn test_parse_redirects_herestring() {
        let r = parse_redirects("cat <<< hello");
        assert_eq!(r.cmd, "cat");
        assert_eq!(r.herestring.unwrap(), "hello");
        assert!(r.stdin_path.is_none());
    }

    #[test]
    fn test_parse_redirects_herestring_no_space() {
        let r = parse_redirects("cat <<<hello");
        assert_eq!(r.cmd, "cat");
        assert_eq!(r.herestring.unwrap(), "hello");
    }

    #[test]
    fn test_execute_chain_simple() {
        let status = execute_command("true");
        assert!(status.unwrap().success());
    }

    #[test]
    fn test_execute_chain_and() {
        let status = execute_command("true && true");
        assert!(status.unwrap().success());
    }

    #[test]
    fn test_execute_chain_semicolon() {
        let status = execute_command("true; true");
        assert!(status.unwrap().success());
    }
}
