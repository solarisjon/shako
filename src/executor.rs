use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader};
use std::process::{Command, ExitStatus, Stdio};

use crate::parser;

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
            use nix::sys::signal::{SigHandler, Signal, signal};
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
fn foreground_wait(mut child: std::process::Child) -> std::process::ExitStatus {
    let child_pid = nix::unistd::Pid::from_raw(child.id() as i32);
    let shell_pgid = nix::unistd::getpgrp();

    // Close the TOCTOU race: set the child's pgid in the parent too.
    // (The child also does this in pre_exec; whichever runs first wins.)
    let _ = nix::unistd::setpgid(child_pid, child_pid);

    // Give the terminal to the child so Ctrl-C/Ctrl-Z reach it.
    let _ = nix::unistd::tcsetpgrp(std::io::stdin(), child_pid);

    let status = child.wait().unwrap_or_else(|_| fake_status(1));

    // Restore terminal ownership to the shell.
    let _ = nix::unistd::tcsetpgrp(std::io::stdin(), shell_pgid);

    // Drain any pending terminal responses (e.g. vim's OSC background color
    // query) so they don't leak into the next reedline prompt as garbage text.
    drain_pending_input();

    status
}

/// Discard any bytes waiting on stdin after a foreground process exits.
///
/// Programs like vim/neovim send terminal queries (OSC 11 for background
/// color, DCS for capabilities) whose responses arrive asynchronously.
/// If the response arrives after the program exits, it appears as typed
/// input in the next prompt. This drains those stale responses.
#[cfg(unix)]
pub fn drain_pending_input() {
    use std::io::Read;
    use std::os::unix::io::AsRawFd;

    let fd = std::io::stdin().as_raw_fd();

    // Get current flags
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 {
        return;
    }

    // Set non-blocking
    unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };

    // Read and discard all pending bytes
    let mut buf = [0u8; 1024];
    let mut stdin = std::io::stdin();
    while stdin.read(&mut buf).is_ok_and(|n| n > 0) {}

    // Restore original flags
    unsafe { libc::fcntl(fd, libc::F_SETFL, flags) };
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
        Ok(child) => Some(child),
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
            Ok(f) => { cmd.stdin(Stdio::from(f)); }
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
            Ok(f) => { cmd.stdout(Stdio::from(f)); }
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
                let mut collected = Vec::new();
                if let Some(pipe) = stderr_pipe {
                    let reader = BufReader::new(pipe);
                    for line in reader.lines() {
                        match line {
                            Ok(line) => {
                                eprintln!("{line}");
                                collected.push(line);
                                if collected.len() > 20 {
                                    collected.remove(0);
                                }
                            }
                            Err(_) => break,
                        }
                    }
                }
                collected.join("\n")
            });

            let status = foreground_wait(child);
            let stderr_output = stderr_thread.join().unwrap_or_default();

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
        Ok(child) => {
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
        let _ = nix::unistd::tcsetpgrp(
            std::io::stdin(),
            nix::unistd::Pid::from_raw(pipeline_pgid),
        );
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
}

/// Parse redirects from a command string, preserving quotes.
/// Supports: >, >>, <, 2>, 2>>, 2>&1
fn parse_redirects(input: &str) -> Redirects {
    let mut cmd_parts = Vec::new();
    let mut stdout_path = None;
    let mut stderr_path = None;
    let mut stdin_path = None;
    let mut stdout_append = false;
    let mut stderr_append = false;
    let mut stderr_to_stdout = false;

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
    }
}

/// Create a fake ExitStatus with the given code.
/// On Unix, we do this by running `sh -c "exit N"`.
fn fake_status(code: i32) -> ExitStatus {
    Command::new("sh")
        .args(["-c", &format!("exit {code}")])
        .status()
        .expect("failed to create exit status")
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
