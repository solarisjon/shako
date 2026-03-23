use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

use crate::smart_defaults;

pub const BUILTINS: &[&str] = &[
    "cd",
    "exit",
    "export",
    "unset",
    "set",
    "source",
    "alias",
    "unalias",
    "history",
    "type",
    "z",
    "zi",
    "jobs",
    "fg",
    "bg",
    "function",
    "functions",
];

/// A background job tracked by the shell.
pub struct Job {
    pub id: usize,
    pub pid: u32,
    pub command: String,
    pub child: std::process::Child,
}

/// Shared shell state accessible to builtins and the classifier.
pub struct ShellState {
    pub aliases: HashMap<String, String>,
    pub functions: HashMap<String, ShellFunction>,
    pub history_path: PathBuf,
    pub jobs: Vec<Job>,
    next_job_id: usize,
}

/// A user-defined shell function.
#[derive(Clone, Debug)]
pub struct ShellFunction {
    pub body: String,
}

impl ShellState {
    pub fn new(history_path: PathBuf) -> Self {
        Self {
            aliases: HashMap::new(),
            functions: HashMap::new(),
            history_path,
            jobs: Vec::new(),
            next_job_id: 1,
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

    /// Expand aliases in the input. Returns the expanded string if the
    /// first token is an alias, otherwise returns None.
    pub fn expand_alias(&self, input: &str) -> Option<String> {
        let first_token = input.split_whitespace().next()?;
        let replacement = self.aliases.get(first_token)?;
        let rest = input[first_token.len()..].to_string();
        Some(format!("{replacement}{rest}"))
    }
}

/// Check if a token is a builtin command name.
pub fn is_builtin(token: &str) -> bool {
    BUILTINS.contains(&token)
}

/// Run a shell builtin command.
pub fn run_builtin(input: &str, state: &mut ShellState) {
    // Use parse_args so builtins get glob expansion, tilde expansion,
    // env var expansion, and quoting — same as regular commands.
    let parsed = crate::parser::parse_args(input);
    let parts: Vec<&str> = parsed.iter().map(|s| s.as_str()).collect();
    if parts.is_empty() {
        return;
    }

    match parts[0] {
        "cd" => builtin_cd(&parts[1..]),
        "z" => builtin_z(&parts[1..]),
        "zi" => builtin_zi(),
        "exit" => std::process::exit(0),
        "export" => builtin_export(&parts[1..]),
        "unset" => builtin_unset(&parts[1..]),
        "set" => builtin_set(&parts[1..]),
        "history" => builtin_history(&parts[1..], state),
        "alias" => builtin_alias(&parts[1..], state),
        "unalias" => builtin_unalias(&parts[1..], state),
        "source" => builtin_source(&parts[1..], state),
        "type" => builtin_type(&parts[1..], state),
        "jobs" => builtin_jobs(state),
        "fg" => builtin_fg(&parts[1..], state),
        "bg" => builtin_bg(&parts[1..], state),
        "functions" => builtin_functions(state),
        other => eprintln!("shako: unknown builtin: {other}"),
    }
}

/// Try to parse and register a function definition.
/// Returns true if the input was a function definition.
/// Supports: `function name() { body }` and `function name { body }`
pub fn try_define_function(input: &str, state: &mut ShellState) -> bool {
    let trimmed = input.trim();

    // Match "function name() { body }" or "function name { body }"
    let rest = match trimmed.strip_prefix("function ") {
        Some(r) => r.trim(),
        None => return false,
    };

    // Extract function name
    let name_end = rest
        .find(|c: char| c == '(' || c == '{' || c.is_whitespace())
        .unwrap_or(rest.len());
    let name = rest[..name_end].trim().to_string();
    if name.is_empty() {
        eprintln!("shako: function: missing name");
        return true;
    }

    let after_name = rest[name_end..].trim();

    // Strip optional "()"
    let after_parens = after_name
        .strip_prefix("()")
        .map(|s| s.trim())
        .unwrap_or(after_name);

    // Extract body between { }
    let body = if let Some(inner) = after_parens.strip_prefix('{') {
        if let Some(body) = inner.strip_suffix('}') {
            body.trim().to_string()
        } else {
            eprintln!("shako: function: missing closing '}}' for {name}");
            return true;
        }
    } else {
        eprintln!("shako: function: missing '{{' for {name}");
        return true;
    };

    state.functions.insert(name.clone(), ShellFunction { body });
    true
}

/// Run a shell function by executing each line of its body.
pub fn run_function(func: &ShellFunction, args: &[&str]) {
    // Set positional parameters as env vars
    for (i, arg) in args.iter().enumerate() {
        unsafe { env::set_var(format!("{}", i + 1), arg) };
    }
    unsafe { env::set_var("@", args.join(" ")) };

    for line in func.body.split(';') {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        crate::executor::execute_command(line);
    }

    // Clean up positional parameters
    for i in 1..=args.len() {
        unsafe { env::remove_var(format!("{i}")) };
    }
    unsafe { env::remove_var("@") };
}

fn builtin_cd(args: &[&str]) {
    let target = if args.is_empty() {
        match dirs::home_dir() {
            Some(home) => home,
            None => {
                eprintln!("shako: cd: HOME not set");
                return;
            }
        }
    } else if args[0] == "-" {
        match env::var("OLDPWD") {
            Ok(old) => {
                println!("{old}");
                std::path::PathBuf::from(old)
            }
            Err(_) => {
                eprintln!("shako: cd: OLDPWD not set");
                return;
            }
        }
    } else {
        let path = args[0];
        // If the path still contains glob metacharacters, it means the glob
        // expansion found no matches and returned the pattern literally.
        if path.chars().any(|c| c == '*' || c == '?' || c == '[') {
            eprintln!("shako: cd: {path}: no matches found");
            return;
        }
        if path.starts_with('~') {
            match dirs::home_dir() {
                Some(home) => home.join(path.trim_start_matches('~').trim_start_matches('/')),
                None => {
                    eprintln!("shako: cd: HOME not set");
                    return;
                }
            }
        } else {
            std::path::PathBuf::from(path)
        }
    };

    if let Ok(current) = env::current_dir() {
        unsafe { env::set_var("OLDPWD", current) };
    }

    if let Err(e) = env::set_current_dir(&target) {
        eprintln!("shako: cd: {}: {e}", target.display());
    } else if let Ok(cwd) = env::current_dir() {
        // Keep PWD in sync — Starship and most Unix tools read PWD rather than
        // resolving the kernel cwd, so without this the prompt stays stale.
        unsafe { env::set_var("PWD", &cwd) };
        if smart_defaults::has_zoxide() {
            smart_defaults::zoxide_add(&cwd.display().to_string());
        }
    }
}

/// `z` — zoxide-powered smart cd. Falls back to regular cd if zoxide is not installed.
fn builtin_z(args: &[&str]) {
    if args.is_empty() {
        builtin_cd(args);
        return;
    }

    if !smart_defaults::has_zoxide() {
        builtin_cd(args);
        return;
    }

    match smart_defaults::zoxide_query(args) {
        Some(path) => {
            builtin_cd(&[path.as_str()]);
        }
        None => {
            eprintln!("shako: z: no match for {:?}", args.join(" "));
        }
    }
}

/// `zi` — interactive zoxide selection via fzf.
fn builtin_zi() {
    if !smart_defaults::has_zoxide() {
        eprintln!("shako: zi: zoxide not installed");
        return;
    }

    let output = std::process::Command::new("zoxide")
        .args(["query", "--list", "--score"])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let list = String::from_utf8_lossy(&out.stdout).to_string();
            if list.trim().is_empty() {
                eprintln!("shako: zi: no directories tracked yet");
                return;
            }

            if smart_defaults::has_fzf() {
                if let Some(selected) = smart_defaults::fzf_select(&list, "z>") {
                    // zoxide output format: "  score /path/to/dir"
                    let path = selected.split_whitespace().last().unwrap_or("");
                    if !path.is_empty() {
                        builtin_cd(&[path]);
                    }
                }
            } else {
                // No fzf — just print the list
                print!("{list}");
            }
        }
        _ => eprintln!("shako: zi: failed to query zoxide"),
    }
}

fn builtin_export(args: &[&str]) {
    for arg in args {
        if let Some((key, value)) = arg.split_once('=') {
            unsafe { env::set_var(key, value) };
        } else {
            match env::var(arg) {
                Ok(val) => println!("{arg}={val}"),
                Err(_) => eprintln!("shako: export: {arg}: not set"),
            }
        }
    }
}

fn builtin_unset(args: &[&str]) {
    for arg in args {
        unsafe { env::remove_var(arg) };
    }
}

/// Fish-compatible `set` builtin.
///   set VAR value         — set variable
///   set -x VAR value      — export variable
///   set -gx VAR value     — export variable (global, same as -x)
///   set -Ux VAR value     — export variable (universal, treated as -x)
///   set -e VAR            — erase variable
///   set -ex VAR           — erase exported variable
///   set                   — list all environment variables
fn builtin_set(args: &[&str]) {
    if args.is_empty() {
        // List all env vars
        let mut vars: Vec<(String, String)> = env::vars().collect();
        vars.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, value) in vars {
            println!("{key} {value}");
        }
        return;
    }

    // Parse flags
    let mut export = false;
    let mut erase = false;
    let mut var_start = 0;

    for (i, arg) in args.iter().enumerate() {
        if let Some(flags) = arg.strip_prefix('-') {
            for ch in flags.chars() {
                match ch {
                    'x' => export = true,
                    'g' | 'U' => {} // global/universal — treat as default
                    'e' => erase = true,
                    _ => {
                        eprintln!("shako: set: unknown option: -{ch}");
                        return;
                    }
                }
            }
            var_start = i + 1;
        } else {
            break;
        }
    }

    let remaining = &args[var_start..];

    if remaining.is_empty() {
        eprintln!("shako: set: missing variable name");
        return;
    }

    let var_name = remaining[0];

    if erase {
        unsafe { env::remove_var(var_name) };
        return;
    }

    let value = if remaining.len() > 1 {
        remaining[1..].join(" ")
    } else {
        String::new()
    };

    unsafe { env::set_var(var_name, &value) };

    if export {
        // Already set via set_var — env vars are inherited by child processes
        log::debug!("exported {var_name}={value}");
    }
}

fn builtin_history(args: &[&str], state: &ShellState) {
    let limit: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(25);

    if !state.history_path.exists() {
        eprintln!("shako: history: no history file");
        return;
    }

    match std::fs::read_to_string(&state.history_path) {
        Ok(contents) => {
            let lines: Vec<&str> = contents.lines().collect();
            let start = lines.len().saturating_sub(limit);
            for (i, line) in lines[start..].iter().enumerate() {
                println!("{:>5}  {}", start + i + 1, line);
            }
        }
        Err(e) => eprintln!("shako: history: {e}"),
    }
}

fn builtin_alias(args: &[&str], state: &mut ShellState) {
    if args.is_empty() {
        if state.aliases.is_empty() {
            return;
        }
        let mut sorted: Vec<_> = state.aliases.iter().collect();
        sorted.sort_by_key(|(k, _)| (*k).clone());
        for (name, value) in sorted {
            println!("alias {name}='{value}'");
        }
        return;
    }

    for arg in args {
        if let Some((name, value)) = arg.split_once('=') {
            let value = value.trim_matches('\'').trim_matches('"');
            state.aliases.insert(name.to_string(), value.to_string());
        } else {
            match state.aliases.get(*arg) {
                Some(value) => println!("alias {arg}='{value}'"),
                None => eprintln!("shako: alias: {arg}: not found"),
            }
        }
    }
}

fn builtin_unalias(args: &[&str], state: &mut ShellState) {
    for arg in args {
        if *arg == "-a" {
            state.aliases.clear();
            return;
        }
        if state.aliases.remove(*arg).is_none() {
            eprintln!("shako: unalias: {arg}: not found");
        }
    }
}

fn builtin_source(args: &[&str], state: &mut ShellState) {
    if args.is_empty() {
        eprintln!("shako: source: filename argument required");
        return;
    }

    for path in args {
        let expanded = if path.starts_with('~') {
            dirs::home_dir()
                .map(|h| h.join(path.trim_start_matches('~').trim_start_matches('/')))
                .unwrap_or_else(|| PathBuf::from(path))
        } else {
            PathBuf::from(path)
        };

        let contents = match std::fs::read_to_string(&expanded) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("shako: source: {}: {e}", expanded.display());
                return;
            }
        };

        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some(rest) = line.strip_prefix("alias ") {
                if let Some((name, value)) = rest.split_once('=') {
                    let value = value.trim_matches('\'').trim_matches('"');
                    state
                        .aliases
                        .insert(name.trim().to_string(), value.to_string());
                }
            } else if let Some(rest) = line.strip_prefix("export ") {
                if let Some((key, value)) = rest.split_once('=') {
                    let value = value.trim_matches('\'').trim_matches('"');
                    unsafe { env::set_var(key.trim(), value) };
                }
            } else if line.starts_with("set ") {
                // Fish-style: set -x VAR value, set -gx VAR value, set VAR value
                let parts: Vec<&str> = line.split_whitespace().collect();
                builtin_set(&parts[1..]);
            } else if line.starts_with("function ") {
                try_define_function(line, state);
            }
        }
    }
}

fn builtin_type(args: &[&str], state: &ShellState) {
    for arg in args {
        if BUILTINS.contains(arg) {
            println!("{arg} is a shell builtin");
        } else if let Some(func) = state.functions.get(*arg) {
            println!("{arg} is a function: {{ {} }}", func.body);
        } else if let Some(value) = state.aliases.get(*arg) {
            println!("{arg} is aliased to '{value}'");
        } else if let Ok(path) = which::which(arg) {
            println!("{arg} is {}", path.display());
        } else {
            eprintln!("shako: type: {arg}: not found");
        }
    }
}

fn builtin_functions(state: &ShellState) {
    if state.functions.is_empty() {
        return;
    }
    let mut sorted: Vec<_> = state.functions.iter().collect();
    sorted.sort_by_key(|(k, _)| (*k).clone());
    for (name, func) in sorted {
        println!("function {name}() {{ {} }}", func.body);
    }
}

fn builtin_jobs(state: &mut ShellState) {
    state.reap_jobs();
    if state.jobs.is_empty() {
        return;
    }
    for job in &state.jobs {
        println!("[{}]  running  {} (pid {})", job.id, job.command, job.pid);
    }
}

fn builtin_fg(args: &[&str], state: &mut ShellState) {
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

fn builtin_bg(args: &[&str], state: &mut ShellState) {
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
