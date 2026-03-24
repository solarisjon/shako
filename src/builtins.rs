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
    "abbr",
    "fish-import",
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
    pub abbreviations: HashMap<String, String>,
    pub functions: HashMap<String, ShellFunction>,
    pub functions_dir: Option<PathBuf>,
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
            abbreviations: HashMap::new(),
            functions: HashMap::new(),
            functions_dir: None,
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
                    let body = parse_fish_function_file(&contents);
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
        "abbr" => builtin_abbr(&parts[1..], state),
        "fish-import" => crate::fish_import::run_import(),
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
        if *arg == "--" {
            var_start = i + 1;
            break;
        }
        if let Some(flags) = arg.strip_prefix('-') {
            for ch in flags.chars() {
                match ch {
                    'x' => export = true,
                    'g' | 'U' | 'l' | 'f' | 'q' => {}
                    'e' => erase = true,
                    _ => {
                        log::debug!("set: ignoring unknown flag: -{ch}");
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
        let values = &remaining[1..];
        if is_path_variable(var_name) {
            values.join(":")
        } else {
            values.join(" ")
        }
    } else {
        String::new()
    };

    unsafe { env::set_var(var_name, &value) };

    if export {
        // Already set via set_var — env vars are inherited by child processes
        log::debug!("exported {var_name}={value}");
    }
}

/// Fish convention: variables whose names end in PATH are colon-separated
/// lists (PATH, MANPATH, CDPATH, CLASSPATH, etc.). All others are
/// space-separated when multiple values are provided.
fn is_path_variable(name: &str) -> bool {
    name.ends_with("PATH")
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

/// Fish-compatible `abbr` builtin.
///   abbr --add name 'expansion'   (or -a)
///   abbr --erase name             (or -e)
///   abbr --list                   (or -l, or no args)
///   abbr name 'expansion'         (shorthand for --add)
fn builtin_abbr(args: &[&str], state: &mut ShellState) {
    if args.is_empty() {
        let mut sorted: Vec<_> = state.abbreviations.iter().collect();
        sorted.sort_by_key(|(k, _)| (*k).clone());
        for (name, value) in sorted {
            println!("abbr -a {name} '{value}'");
        }
        return;
    }

    let mut mode = "add";
    let mut positional = Vec::new();

    for arg in args {
        match *arg {
            "-a" | "--add" => mode = "add",
            "-e" | "--erase" => mode = "erase",
            "-l" | "--list" => mode = "list",
            _ if arg.starts_with('-') => {}
            _ => positional.push(*arg),
        }
    }

    match mode {
        "list" => {
            let mut sorted: Vec<_> = state.abbreviations.iter().collect();
            sorted.sort_by_key(|(k, _)| (*k).clone());
            for (name, value) in sorted {
                println!("abbr -a {name} '{value}'");
            }
        }
        "erase" => {
            for name in &positional {
                state.abbreviations.remove(*name);
            }
        }
        _ => {
            if positional.len() >= 2 {
                let name = positional[0].to_string();
                let value = positional[1..]
                    .join(" ")
                    .trim_matches('\'')
                    .trim_matches('"')
                    .to_string();
                state.abbreviations.insert(name, value);
            } else if positional.len() == 1 {
                if let Some(value) = state.abbreviations.get(positional[0]) {
                    println!("abbr -a {} '{}'", positional[0], value);
                }
            }
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
                log::debug!("source: {}: {e}", expanded.display());
                continue;
            }
        };

        source_fish_string(&contents, state);
    }
}

/// Source a string of fish/sh config lines into ShellState.
/// Handles: alias, export, set, function, abbr, fish_add_path,
/// and skips if/end, switch/end, for/end blocks.
pub fn source_fish_string(contents: &str, state: &mut ShellState) {
    let lines: Vec<&str> = contents.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let raw_line = lines[i].trim();
        i += 1;

        if raw_line.is_empty() || raw_line.starts_with('#') {
            continue;
        }

        // Convert fish command substitution (cmd) → $(cmd) so the parser
        // can handle it. Fish uses bare parens; POSIX/shako uses $().
        let converted = fish_cmdsub_to_posix(raw_line);
        let line = converted.as_str();

        // Skip block constructs: if/end, switch/end, for/end, while/end
        if line.starts_with("if ")
            || line.starts_with("switch ")
            || line.starts_with("for ")
            || line.starts_with("while ")
        {
            let mut depth = 1;
            while i < lines.len() && depth > 0 {
                let inner = lines[i].trim();
                if inner.starts_with("if ")
                    || inner.starts_with("switch ")
                    || inner.starts_with("for ")
                    || inner.starts_with("while ")
                {
                    depth += 1;
                } else if inner == "end" {
                    depth -= 1;
                }
                i += 1;
            }
            continue;
        }

        // Multi-line fish function: function name ... end
        if line.starts_with("function ") && !line.contains('{') {
            let rest = line.strip_prefix("function ").unwrap().trim();
            let name = rest
                .split(|c: char| c.is_whitespace() || c == ';' || c == '\n')
                .next()
                .unwrap_or("")
                .to_string();
            if name.is_empty() || name == "--" {
                continue;
            }

            let mut body_lines = Vec::new();
            let mut depth = 1;
            while i < lines.len() && depth > 0 {
                let inner = lines[i].trim();
                // Track ALL fish block openers, not just `function`, so that
                // inner switch/if/for/while/begin blocks don't prematurely
                // terminate the function body and leak their contents as
                // top-level statements (which caused `atuin search -i` to run
                // at startup when _atuin_search was being collected).
                if inner.starts_with("function ")
                    || inner.starts_with("if ")
                    || inner == "if"
                    || inner.starts_with("switch ")
                    || inner.starts_with("for ")
                    || inner.starts_with("while ")
                    || inner == "begin"
                    || inner.starts_with("begin ")
                {
                    depth += 1;
                } else if inner == "end" || inner.starts_with("end ") || inner.starts_with("end\t") {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                body_lines.push(inner);
                i += 1;
            }

            let body = body_lines.join("; ");
            if !body.is_empty() {
                state
                    .functions
                    .insert(name, ShellFunction { body });
            }
            continue;
        }

        // Brace-style function definition
        if line.starts_with("function ") && line.contains('{') {
            try_define_function(line, state);
            continue;
        }

        // alias — both `alias name='value'` and fish-style `alias name 'value'`
        if let Some(rest) = line.strip_prefix("alias ") {
            if let Some((name, value)) = rest.split_once('=') {
                let value = value.trim_matches('\'').trim_matches('"');
                state
                    .aliases
                    .insert(name.trim().to_string(), value.to_string());
            } else {
                let parts: Vec<&str> = rest.splitn(2, char::is_whitespace).collect();
                if parts.len() == 2 {
                    let value = parts[1].trim().trim_matches('\'').trim_matches('"');
                    state
                        .aliases
                        .insert(parts[0].to_string(), value.to_string());
                }
            }
            continue;
        }

        // export KEY=VALUE — expand command substitution and env vars
        if let Some(rest) = line.strip_prefix("export ") {
            if let Some((key, value)) = rest.split_once('=') {
                let expanded = crate::parser::parse_args(
                    value.trim_matches('\'').trim_matches('"'),
                );
                let value = expanded.join(" ");
                unsafe { env::set_var(key.trim(), &value) };
            }
            continue;
        }

        // set (fish-style) — intercept PATH modifications
        // Run through parse_args so $(), env vars, tilde, etc. are expanded.
        if line.starts_with("set ") {
            let expanded = crate::parser::parse_args(line);
            let parts: Vec<&str> = expanded.iter().map(|s| s.as_str()).collect();
            if set_targets_path_var(&parts) {
                handle_path_set(&parts);
            } else {
                builtin_set(&parts[1..]);
            }
            continue;
        }

        // abbr --add name 'expansion' (or -a, or shorthand)
        if line.starts_with("abbr ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            builtin_abbr(&parts[1..], state);
            continue;
        }

        // fish_add_path /path — prepend to PATH
        if let Some(rest) = line.strip_prefix("fish_add_path ") {
            let path = rest.trim().trim_matches('\'').trim_matches('"');
            if !path.is_empty() {
                prepend_path(path);
            }
            continue;
        }

        // source another file (expand $VAR and ~ in path)
        if let Some(rest) = line.strip_prefix("source ") {
            let raw = rest.trim().trim_matches('\'').trim_matches('"');
            if !raw.is_empty() {
                let expanded = crate::parser::parse_args(raw);
                if let Some(path) = expanded.first() {
                    builtin_source(&[path.as_str()], state);
                }
            }
            continue;
        }
    }
}

/// Check if a `set` command targets a PATH variable (PATH, MANPATH, etc.).
fn set_targets_path_var(parts: &[&str]) -> bool {
    for part in parts.iter().skip(1) {
        if *part == "--" {
            continue;
        }
        if part.starts_with('-') {
            continue;
        }
        return part.ends_with("PATH");
    }
    false
}

/// Handle `set ... PATH ...` safely: convert to prepend operations
/// instead of replacing PATH. Skips `set -e PATH` (erase) entirely.
fn handle_path_set(parts: &[&str]) {
    let mut erase = false;
    let mut var_idx = 1;

    for (i, part) in parts.iter().enumerate().skip(1) {
        if *part == "--" {
            var_idx = i + 1;
            break;
        }
        if part.starts_with('-') {
            for ch in part.chars().skip(1) {
                if ch == 'e' {
                    erase = true;
                }
            }
            var_idx = i + 1;
        } else {
            var_idx = i;
            break;
        }
    }

    if var_idx >= parts.len() {
        return;
    }

    let var_name = parts[var_idx];

    // Never erase PATH — fish rebuilds it from scratch, but shako inherits
    // a working PATH from the parent shell.
    if erase {
        log::debug!("skipping set -e {var_name} (preserving inherited PATH)");
        return;
    }

    // Convert each value to a prepend_path call, skipping $PATH (self-reference)
    // and standard system dirs that are already present.
    for value in &parts[var_idx + 1..] {
        let v = value.trim_matches('\'').trim_matches('"');
        if v == "$PATH" || v == "${PATH}" || v.is_empty() {
            continue;
        }
        prepend_path(v);
    }
}

/// Prepend a directory to $PATH if not already present.
/// Convert fish command substitution `(cmd)` to POSIX `$(cmd)`.
/// Only converts `(` that is NOT already preceded by `$`.
/// Handles nested parentheses correctly.
fn fish_cmdsub_to_posix(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut result = String::with_capacity(line.len() + 8);
    let mut i = 0;
    let mut in_single_quote = false;

    while i < chars.len() {
        let c = chars[i];

        // Don't convert inside single quotes
        if c == '\'' {
            in_single_quote = !in_single_quote;
            result.push(c);
            i += 1;
            continue;
        }
        if in_single_quote {
            result.push(c);
            i += 1;
            continue;
        }

        // Already POSIX-style $(
        if c == '$' && i + 1 < chars.len() && chars[i + 1] == '(' {
            result.push(c);
            i += 1;
            continue;
        }

        // Fish-style ( not preceded by $ → convert to $(
        if c == '(' && (i == 0 || chars[i - 1] != '$') {
            // Check that this looks like a command substitution:
            // must contain at least one non-paren character
            let remaining: String = chars[i + 1..].iter().collect();
            if remaining.contains(')') {
                result.push('$');
            }
        }

        result.push(c);
        i += 1;
    }

    result
}

fn prepend_path(dir: &str) {
    let dir = if dir.starts_with('~') {
        dirs::home_dir()
            .map(|h| {
                h.join(dir.trim_start_matches('~').trim_start_matches('/'))
                    .display()
                    .to_string()
            })
            .unwrap_or_else(|| dir.to_string())
    } else {
        dir.to_string()
    };

    let current = env::var("PATH").unwrap_or_default();
    if current.split(':').any(|p| p == dir) {
        return;
    }
    unsafe { env::set_var("PATH", format!("{dir}:{current}")) };
}

/// Parse a fish-style function file (function name ... end) and return
/// the body as a semicolon-separated string suitable for ShellFunction.
fn parse_fish_function_file(contents: &str) -> String {
    let mut body_lines = Vec::new();
    let mut in_function = false;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            if in_function {
                continue;
            }
            continue;
        }

        if trimmed.starts_with("function ") {
            in_function = true;
            continue;
        }

        if trimmed == "end" && in_function {
            break;
        }

        if in_function {
            body_lines.push(trimmed);
        }
    }

    body_lines.join("; ")
}

/// Source all files in a conf.d/ directory (sorted alphabetically).
pub fn source_conf_d(dir: &std::path::Path, state: &mut ShellState) {
    let mut files: Vec<_> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .filter(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                (name.ends_with(".fish") || name.ends_with(".sh")) && !name.starts_with('.')
            })
            .collect(),
        Err(_) => return,
    };

    files.sort_by_key(|e| e.file_name());

    for entry in files {
        if let Ok(contents) = std::fs::read_to_string(entry.path()) {
            source_fish_string(&contents, state);
        }
    }
}

/// Load all function files from a functions/ directory.
pub fn load_functions_dir(dir: &std::path::Path, state: &mut ShellState) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }

        let func_name = if let Some(n) = name.strip_suffix(".fish") {
            n.to_string()
        } else if let Some(n) = name.strip_suffix(".sh") {
            n.to_string()
        } else {
            continue;
        };

        if let Ok(contents) = std::fs::read_to_string(entry.path()) {
            let body = parse_fish_function_file(&contents);
            if !body.is_empty() {
                state
                    .functions
                    .entry(func_name)
                    .or_insert(ShellFunction { body });
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
        } else if let Some(value) = state.abbreviations.get(*arg) {
            println!("{arg} is an abbreviation for '{value}'");
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
