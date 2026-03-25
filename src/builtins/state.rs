use std::collections::HashMap;
use std::path::PathBuf;

/// A background job tracked by the shell.
pub struct Job {
    pub id: usize,
    pub pid: u32,
    pub command: String,
    pub child: std::process::Child,
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
        }
    }

    /// Add a background job and print its job ID.
    pub fn add_job(&mut self, child: std::process::Child, command: String) {
        let id = self.next_job_id;
        self.next_job_id += 1;
        let pid = child.id();
        eprintln!("[{id}] {pid}");
        self.jobs.push(Job {
            id,
            pid,
            command,
            child,
        });
    }

    /// Reap finished background jobs and report their completion.
    pub fn reap_jobs(&mut self) {
        let mut completed = Vec::new();
        for job in &mut self.jobs {
            match job.child.try_wait() {
                Ok(Some(status)) => {
                    let code = status.code().unwrap_or(-1);
                    if status.success() {
                        eprintln!("[{}] done  {}", job.id, job.command);
                    } else {
                        eprintln!("[{}] exit {code}  {}", job.id, job.command);
                    }
                    completed.push(job.id);
                }
                Ok(None) => {} // still running
                Err(_) => {
                    completed.push(job.id);
                }
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
        let rest = input[first_token.len()..].to_string();
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
                        self.functions.insert(
                            name.to_string(),
                            ShellFunction { body },
                        );
                        return true;
                    }
                }
            }
        }

        false
    }
}
