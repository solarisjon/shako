use super::state::ShellState;

pub(super) fn builtin_jobs(state: &mut ShellState) {
    state.reap_jobs();
    if state.jobs.is_empty() {
        return;
    }
    for job in &state.jobs {
        println!("[{}]  running  {} (pid {})", job.id, job.command, job.pid);
    }
}

pub(super) fn builtin_fg(args: &[&str], state: &mut ShellState) {
    state.reap_jobs();

    let job_idx = if args.is_empty() {
        // Default to most recent job
        if state.jobs.is_empty() {
            eprintln!("shako: fg: no current job");
            return;
        }
        state.jobs.len() - 1
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

    let mut job = state.jobs.remove(job_idx);
    eprintln!("{}", job.command);
    match job.child.wait() {
        Ok(status) => {
            let code = status.code().unwrap_or(0);
            crate::shell::prompt::set_last_status(code);
        }
        Err(e) => eprintln!("shako: fg: {e}"),
    }
}

pub(super) fn builtin_bg(args: &[&str], state: &mut ShellState) {
    state.reap_jobs();

    if args.is_empty() {
        if state.jobs.is_empty() {
            eprintln!("shako: bg: no current job");
            return;
        }
        // On Unix, send SIGCONT to the most recent job
        #[cfg(unix)]
        {
            let job = state.jobs.last().unwrap();
            let pid = nix::unistd::Pid::from_raw(job.pid as i32);
            let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGCONT);
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
            if let Some(job) = state.jobs.iter().find(|j| j.id == target_id) {
                let pid = nix::unistd::Pid::from_raw(job.pid as i32);
                let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGCONT);
                eprintln!("[{}] {} &", job.id, job.command);
            } else {
                eprintln!("shako: bg: %{target_id}: no such job");
            }
        }
    }
}
