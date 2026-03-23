use std::fs::{File, OpenOptions};
use std::process::{Command, ExitStatus, Stdio};

use crate::parser;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

/// Set up a command for proper signal handling on Unix.
/// Puts the child in its own process group so Ctrl-C goes to the child,
/// not the shell.
#[cfg(unix)]
fn setup_child_signals(cmd: &mut Command) {
    unsafe {
        cmd.pre_exec(|| {
            // Put this process into its own process group
            let _ = nix::unistd::setpgid(nix::unistd::Pid::from_raw(0), nix::unistd::Pid::from_raw(0));
            // Reset signal handlers to defaults (shell may have ignored them)
            nix::sys::signal::signal(
                nix::sys::signal::Signal::SIGINT,
                nix::sys::signal::SigHandler::SigDfl,
            )
            .ok();
            nix::sys::signal::signal(
                nix::sys::signal::Signal::SIGQUIT,
                nix::sys::signal::SigHandler::SigDfl,
            )
            .ok();
            nix::sys::signal::signal(
                nix::sys::signal::Signal::SIGTSTP,
                nix::sys::signal::SigHandler::SigDfl,
            )
            .ok();
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn setup_child_signals(_cmd: &mut Command) {}

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
    setup_child_signals(&mut cmd);

    if let Some(ref path) = stdin_redirect {
        match File::open(path) {
            Ok(f) => { cmd.stdin(Stdio::from(f)); }
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
            Ok(f) => { cmd.stdout(Stdio::from(f)); }
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
    setup_child_signals(&mut cmd);

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

    match cmd.status() {
        Ok(status) => {
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
fn execute_pipeline(segments: &[String]) -> ExitStatus {
    let mut children = Vec::new();
    let mut prev_stdout: Option<std::process::ChildStdout> = None;

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
        setup_child_signals(&mut cmd);

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
