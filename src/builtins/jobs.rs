use super::state::ShellState;

pub(super) fn builtin_jobs(state: &mut ShellState) {
    state.reap_jobs();
    if state.jobs.is_empty() {
        return;
    }
    for job in &state.jobs {
        let status_label = if job.stopped { "Stopped" } else { "Running" };
        println!(
            "[{}]  {}  {} (pid {})",
            job.id, status_label, job.command, job.pid
        );
    }
}

pub(super) fn builtin_disown(args: &[&str], state: &mut ShellState) -> i32 {
    state.reap_jobs();
    if state.jobs.is_empty() {
        eprintln!("shako: disown: no current job");
        return 1;
    }

    let idx = if args.is_empty() {
        // Most recent job
        state.jobs.len() - 1
    } else {
        let target_id: usize = match args[0].trim_start_matches('%').parse() {
            Ok(id) => id,
            Err(_) => {
                eprintln!("shako: disown: {}: invalid job spec", args[0]);
                return 1;
            }
        };
        match state.jobs.iter().position(|j| j.id == target_id) {
            Some(i) => i,
            None => {
                eprintln!("shako: disown: %{target_id}: no such job");
                return 1;
            }
        }
    };

    let job = state.jobs.remove(idx);
    log::debug!("disowned job [{}] pid {}", job.id, job.pid);
    0
}

pub(super) fn builtin_wait(args: &[&str], state: &mut ShellState) -> i32 {
    state.reap_jobs();

    if args.is_empty() {
        // Wait for ALL background/stopped jobs
        let mut last_code = 0i32;
        let jobs = std::mem::take(&mut state.jobs);
        for mut job in jobs {
            if let Some(ref mut child) = job.child {
                match child.wait() {
                    Ok(status) => {
                        last_code = status.code().unwrap_or(0);
                    }
                    Err(e) => {
                        eprintln!("shako: wait: {e}");
                        last_code = 1;
                    }
                }
            }
        }
        return last_code;
    }

    let spec = args[0];

    // %N — wait for job by number
    if let Some(stripped) = spec.strip_prefix('%') {
        let target_id: usize = match stripped.parse() {
            Ok(id) => id,
            Err(_) => {
                eprintln!("shako: wait: {spec}: invalid job spec");
                return 1;
            }
        };
        let idx = match state.jobs.iter().position(|j| j.id == target_id) {
            Some(i) => i,
            None => {
                eprintln!("shako: wait: %{target_id}: no such job");
                return 1;
            }
        };
        let mut job = state.jobs.remove(idx);
        if let Some(ref mut child) = job.child {
            return match child.wait() {
                Ok(status) => status.code().unwrap_or(0),
                Err(e) => {
                    eprintln!("shako: wait: {e}");
                    1
                }
            };
        }
        return 0;
    }

    // PID — find by pid in job list
    if let Ok(pid) = spec.parse::<u32>() {
        let idx = state.jobs.iter().position(|j| j.pid == pid);
        if let Some(idx) = idx {
            let mut job = state.jobs.remove(idx);
            if let Some(ref mut child) = job.child {
                return match child.wait() {
                    Ok(status) => status.code().unwrap_or(0),
                    Err(e) => {
                        eprintln!("shako: wait: {e}");
                        1
                    }
                };
            }
            return 0;
        } else {
            // PID not in our job table — use nix waitpid if available
            #[cfg(unix)]
            {
                use nix::sys::wait::{waitpid, WaitPidFlag};
                use nix::unistd::Pid;
                let p = Pid::from_raw(pid as i32);
                return match waitpid(p, Some(WaitPidFlag::empty())) {
                    Ok(nix::sys::wait::WaitStatus::Exited(_, code)) => code,
                    Ok(_) => 0,
                    Err(e) => {
                        eprintln!("shako: wait: {pid}: {e}");
                        1
                    }
                };
            }
            #[cfg(not(unix))]
            {
                eprintln!("shako: wait: {pid}: not found in job table");
                return 1;
            }
        }
    }

    eprintln!("shako: wait: {spec}: invalid job spec");
    1
}

pub(super) fn builtin_fg(args: &[&str], state: &mut ShellState) {
    state.reap_jobs();

    let job_idx = if args.is_empty() {
        // Default to most recent job (prefer stopped jobs over running ones).
        if state.jobs.is_empty() {
            eprintln!("shako: fg: no current job");
            return;
        }
        // Prefer the most recent stopped job; fall back to most recent running.
        state
            .jobs
            .iter()
            .rposition(|j| j.stopped)
            .unwrap_or(state.jobs.len() - 1)
    } else {
        let target_id: usize = match args[0].trim_start_matches('%').parse() {
            Ok(id) => id,
            Err(_) => {
                eprintln!("shako: fg: {}: no such job", args[0]);
                return;
            }
        };
        match state.jobs.iter().position(|j| j.id == target_id) {
            Some(idx) => idx,
            None => {
                eprintln!("shako: fg: %{target_id}: no such job");
                return;
            }
        }
    };

    let job = state.jobs.remove(job_idx);
    eprintln!("{}", job.command);

    #[cfg(unix)]
    {
        use nix::sys::signal::{kill, Signal};
        use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
        use nix::unistd::Pid;

        let job_pgid = Pid::from_raw(job.pgid);
        let job_pid = Pid::from_raw(job.pid as i32);
        let shell_pgid = nix::unistd::getpgrp();

        // Give terminal ownership to the job's process group.
        let _ = nix::unistd::tcsetpgrp(std::io::stdin(), job_pgid);

        // Resume the stopped job.
        if job.stopped {
            let _ = kill(job_pgid, Signal::SIGCONT);
        }

        // Wait for the job to exit or be stopped again (WUNTRACED).
        let mut last_code = 0i32;
        loop {
            match waitpid(job_pid, Some(WaitPidFlag::WUNTRACED)) {
                Ok(WaitStatus::Exited(_, code)) => {
                    last_code = code;
                    break;
                }
                Ok(WaitStatus::Signaled(_, sig, _)) => {
                    last_code = 128 + sig as i32;
                    break;
                }
                Ok(WaitStatus::Stopped(pid, _)) => {
                    // Ctrl-Z again — re-add as stopped job.
                    let _ = nix::unistd::tcsetpgrp(std::io::stdin(), shell_pgid);
                    state.add_stopped_job(pid.as_raw() as u32, job.pgid, job.command.clone());
                    crate::shell::prompt::set_last_status(148);
                    return;
                }
                Err(nix::errno::Errno::EINTR) => continue,
                Err(e) => {
                    eprintln!("shako: fg: waitpid: {e}");
                    break;
                }
                Ok(_) => continue,
            }
        }

        // Restore terminal to shell.
        let _ = nix::unistd::tcsetpgrp(std::io::stdin(), shell_pgid);
        crate::shell::prompt::set_last_status(last_code);
    }

    #[cfg(not(unix))]
    {
        if let Some(mut child) = job.child {
            match child.wait() {
                Ok(status) => {
                    let code = status.code().unwrap_or(0);
                    crate::shell::prompt::set_last_status(code);
                }
                Err(e) => eprintln!("shako: fg: {e}"),
            }
        }
    }
}

pub(super) fn builtin_bg(args: &[&str], state: &mut ShellState) {
    state.reap_jobs();

    if args.is_empty() {
        if state.jobs.is_empty() {
            eprintln!("shako: bg: no current job");
            return;
        }
        // Prefer most recent stopped job.
        let idx = state
            .jobs
            .iter()
            .rposition(|j| j.stopped)
            .unwrap_or(state.jobs.len() - 1);
        #[cfg(unix)]
        {
            let job = &mut state.jobs[idx];
            // Send SIGCONT to the job's process group.
            let pgid = nix::unistd::Pid::from_raw(job.pgid);
            let _ = nix::sys::signal::kill(pgid, nix::sys::signal::Signal::SIGCONT);
            job.stopped = false;
            eprintln!("[{}] {} &", job.id, job.command);
        }
    } else {
        let target_id: usize = match args[0].trim_start_matches('%').parse() {
            Ok(id) => id,
            Err(_) => {
                eprintln!("shako: bg: {}: no such job", args[0]);
                return;
            }
        };
        #[cfg(unix)]
        {
            if let Some(job) = state.jobs.iter_mut().find(|j| j.id == target_id) {
                let pgid = nix::unistd::Pid::from_raw(job.pgid);
                let _ = nix::sys::signal::kill(pgid, nix::sys::signal::Signal::SIGCONT);
                job.stopped = false;
                eprintln!("[{}] {} &", job.id, job.command);
            } else {
                eprintln!("shako: bg: %{target_id}: no such job");
            }
        }
    }
}
