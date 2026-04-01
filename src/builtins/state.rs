use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// A background job tracked by the shell.
pub struct Job {
    pub id: usize,
    pub pid: u32,
    /// Process group ID for the job (used by `fg`/`bg` to send signals and
    /// transfer terminal ownership).
    pub pgid: i32,
    pub command: String,
    /// `true` when the job was suspended by SIGTSTP (Ctrl-Z); `false` when
    /// it is running in the background.
    pub stopped: bool,
    /// `None` for stopped foreground jobs whose `Child` handle was forgotten.
    pub child: Option<std::process::Child>,
}

/// A user-defined shell function.
#[derive(Clone, Debug)]
pub struct ShellFunction {
    pub body: String,
}

/// Shared shell state accessible to builtins and the classifier.
pub struct ShellState {
    pub aliases: HashMap<String, String>,
    pub abbreviations: HashMap<String, String>,
    pub functions: HashMap<String, ShellFunction>,
    pub functions_dir: Option<PathBuf>,
    pub history_path: PathBuf,
    pub jobs: Vec<Job>,
    next_job_id: usize,
    pub dir_stack: Vec<PathBuf>,
    /// Rolling AI session memory: (user NL input, AI command response)
    pub ai_session_memory: Vec<(String, String)>,
}

impl ShellState {
    pub fn new(history_path: PathBuf) -> Self {
        Self {
            aliases: HashMap::new(),
            abbreviations: HashMap::new(),
            functions: HashMap::new(),
            functions_dir: None,
            history_path,
            jobs: Vec::new(),
            next_job_id: 1,
            dir_stack: Vec::new(),
            ai_session_memory: Vec::new(),
        }
    }

    /// Add a background job and print its job ID.
    pub fn add_job(&mut self, child: std::process::Child, command: String) {
        let id = self.next_job_id;
        self.next_job_id += 1;
        let pid = child.id();
        // Use the child's PID as its process-group ID (background jobs are
        // spawned with setpgid(0,0) making each one its own process-group leader).
        let pgid = pid as i32;
        eprintln!("[{id}] {pid}");
        self.jobs.push(Job {
            id,
            pid,
            pgid,
            command,
            stopped: false,
            child: Some(child),
        });
    }

    /// Add a stopped foreground job (Ctrl-Z) and print its job ID.
    ///
    /// Stopped jobs don't have a live `Child` handle (it was forgotten in
    /// `foreground_wait`), so `child` is `None`.
    pub fn add_stopped_job(&mut self, pid: u32, pgid: i32, command: String) {
        let id = self.next_job_id;
        self.next_job_id += 1;
        eprintln!("\n[{id}]  Stopped  {command}");
        self.jobs.push(Job {
            id,
            pid,
            pgid,
            command,
            stopped: true,
            child: None,
        });
    }

    /// Reap finished background jobs and report their completion.
    pub fn reap_jobs(&mut self) {
        // HashSet so the retain predicate below is O(1) per job.
        let mut completed: HashSet<usize> = HashSet::new();
        for job in &mut self.jobs {
            // Stopped jobs are not "running" and cannot be reaped until resumed.
            if job.stopped {
                continue;
            }
            if let Some(status) = job.child.as_mut().and_then(|c| c.try_wait().ok().flatten()) {
                let code = status.code().unwrap_or(-1);
                if status.success() {
                    eprintln!("[{}] done  {}", job.id, job.command);
                } else {
                    eprintln!("[{}] exit {code}  {}", job.id, job.command);
                }
                completed.insert(job.id);
            }
            // Also reap if the child handle is missing (shouldn't happen for bg jobs).
            if job.child.is_none() {
                completed.insert(job.id);
            }
        }
        self.jobs.retain(|j| !completed.contains(&j.id));
    }

    /// Expand aliases and abbreviations in the input. Returns the expanded
    /// string if the first token matches, otherwise returns None.
    /// Aliases are checked first, then abbreviations.
    pub fn expand_alias(&self, input: &str) -> Option<String> {
        let first_token = input.split_whitespace().next()?;
        let replacement = self
            .aliases
            .get(first_token)
            .or_else(|| self.abbreviations.get(first_token))?;
        // first_token is a sub-slice of input (returned by split_whitespace).
        // Use pointer arithmetic to find its byte-end so the slice is correct
        // even when first_token contains multi-byte characters.
        let token_end = first_token.as_ptr() as usize - input.as_ptr() as usize + first_token.len();
        let rest = &input[token_end..];
        Some(format!("{replacement}{rest}"))
    }

    /// Try to autoload a function from the functions directory.
    /// Returns true if the function was loaded.
    pub fn try_autoload_function(&mut self, name: &str) -> bool {
        if self.functions.contains_key(name) {
            return true;
        }

        let dir = match &self.functions_dir {
            Some(d) if d.is_dir() => d.clone(),
            _ => return false,
        };

        // Try name.fish, then name.sh
        for ext in &["fish", "sh"] {
            let path = dir.join(format!("{name}.{ext}"));
            if path.exists() {
                if let Ok(contents) = std::fs::read_to_string(&path) {
                    let body = super::source::parse_fish_function_file(&contents);
                    if !body.is_empty() {
                        self.functions
                            .insert(name.to_string(), ShellFunction { body });
                        return true;
                    }
                }
            }
        }

        false
    }
}
