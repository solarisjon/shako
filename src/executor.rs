use std::fs::{File, OpenOptions};
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
/// This, combined with `tcsetpgrp` in the parent, ensures keyboard signals
/// (Ctrl-C / Ctrl-\ / Ctrl-Z) are delivered to the child, not the shell.
#[cfg(unix)]
fn setup_child_signals(cmd: &mut Command, pgid: i32) {
    unsafe {
        cmd.pre_exec(move || {
            // Join or create the target process group.
            nix::unistd::setpgid(
                nix::unistd::Pid::from_raw(0),
                nix::unistd::Pid::from_raw(pgid),
            )
            .ok();
            // Reset all job-control and interrupt signals to their defaults.
            // The interactive shell ignores these; children must not inherit that.
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
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn setup_child_signals(_cmd: &mut Command, _pgid: i32) {}

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

    status
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

    let (cmd_str, stdout_redirect, append, stdin_redirect) = parse_redirects(input);
    let args = parser::parse_args(&cmd_str);
    if args.is_empty() {
        return None;
    }

    let program = &args[0];
    let cmd_args = &args[1..];

    let mut cmd = Command::new(program);
    cmd.args(cmd_args);
    // Background jobs get their own process group (pgid=0) but are never
    // handed terminal control, so signals from Ctrl-C don't reach them.
    setup_child_signals(&mut cmd, 0);

    if let Some(ref path) = stdin_redirect {
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

    if let Some(ref path) = stdout_redirect {
        let file = if append {
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

/// Execute a single chain segment (may contain pipes).
fn execute_chain_segment(input: &str) -> ExitStatus {
    let segments = parser::split_pipes(input);

    if segments.len() > 1 {
        execute_pipeline(&segments)
    } else {
        execute_single(input)
    }
}

/// Execute a single command with optional redirects.
fn execute_single(input: &str) -> ExitStatus {
    let (cmd_str, stdout_redirect, append, stdin_redirect) = parse_redirects(input);

    let args = parser::parse_args(&cmd_str);
    if args.is_empty() {
        return fake_status(1);
    }

    let program = &args[0];
    let cmd_args = &args[1..];

    let mut cmd = Command::new(program);
    cmd.args(cmd_args);
    setup_child_signals(&mut cmd, 0);

    if let Some(ref path) = stdin_redirect {
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

    if let Some(ref path) = stdout_redirect {
        let file = if append {
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
        let (cmd_str, stdout_redirect, append, stdin_redirect) = parse_redirects(segment);

        let args = parser::parse_args(&cmd_str);
        if args.is_empty() {
            eprintln!("shako: empty command in pipeline");
            return fake_status(1);
        }

        let program = &args[0];
        let cmd_args = &args[1..];

        let mut cmd = Command::new(program);
        cmd.args(cmd_args);
        // First child creates a new process group (pgid=0 → own pid as leader).
        // Subsequent children join the first child's group.
        setup_child_signals(&mut cmd, pipeline_pgid);

        if let Some(prev) = prev_stdout.take() {
            cmd.stdin(Stdio::from(prev));
        } else if let Some(ref path) = stdin_redirect {
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
        } else if let Some(ref path) = stdout_redirect {
            let file = if append {
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

    last_status
}

/// Parse redirects from a command string, preserving quotes.
/// Returns (command, stdout_path, is_append, stdin_path).
fn parse_redirects(input: &str) -> (String, Option<String>, bool, Option<String>) {
    let mut cmd_parts = Vec::new();
    let mut stdout_path = None;
    let mut stdin_path = None;
    let mut append = false;

    let tokens: Vec<&str> = input.split_whitespace().collect();
    let mut i = 0;

    while i < tokens.len() {
        match tokens[i] {
            ">>" => {
                append = true;
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
                if let Some(path) = token.strip_prefix(">>") {
                    append = true;
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

    (cmd_parts.join(" "), stdout_path, append, stdin_path)
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
        let (cmd, out, append, inp) = parse_redirects("ls -la");
        assert_eq!(cmd, "ls -la");
        assert!(out.is_none());
        assert!(!append);
        assert!(inp.is_none());
    }

    #[test]
    fn test_parse_redirects_stdout() {
        let (cmd, out, append, _) = parse_redirects("echo hello > output.txt");
        assert_eq!(cmd, "echo hello");
        assert_eq!(out.unwrap(), "output.txt");
        assert!(!append);
    }

    #[test]
    fn test_parse_redirects_append() {
        let (cmd, out, append, _) = parse_redirects("echo hello >> output.txt");
        assert_eq!(cmd, "echo hello");
        assert_eq!(out.unwrap(), "output.txt");
        assert!(append);
    }

    #[test]
    fn test_parse_redirects_stdin() {
        let (cmd, _, _, inp) = parse_redirects("sort < input.txt");
        assert_eq!(cmd, "sort");
        assert_eq!(inp.unwrap(), "input.txt");
    }

    #[test]
    fn test_parse_redirects_no_space() {
        let (cmd, out, append, _) = parse_redirects("echo hello >output.txt");
        assert_eq!(cmd, "echo hello");
        assert_eq!(out.unwrap(), "output.txt");
        assert!(!append);
    }

    #[test]
    fn test_parse_redirects_append_no_space() {
        let (cmd, out, append, _) = parse_redirects("echo hello >>output.txt");
        assert_eq!(cmd, "echo hello");
        assert_eq!(out.unwrap(), "output.txt");
        assert!(append);
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
