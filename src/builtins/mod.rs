mod jobs;
mod set;
pub mod source;
pub mod state;

pub use source::{load_functions_dir, source_conf_d, source_fish_string};
pub use state::{ShellFunction, ShellState};

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
        "set" => set::builtin_set(&parts[1..]),
        "history" => builtin_history(&parts[1..], state),
        "alias" => builtin_alias(&parts[1..], state),
        "unalias" => builtin_unalias(&parts[1..], state),
        "abbr" => builtin_abbr(&parts[1..], state),
        "fish-import" => crate::fish_import::run_import(),
        "source" => source::builtin_source(&parts[1..], state),
        "type" => builtin_type(&parts[1..], state),
        "jobs" => jobs::builtin_jobs(state),
        "fg" => jobs::builtin_fg(&parts[1..], state),
        "bg" => jobs::builtin_bg(&parts[1..], state),
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
    use std::env;
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
        match std::env::var("OLDPWD") {
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

    if let Ok(current) = std::env::current_dir() {
        unsafe { std::env::set_var("OLDPWD", current) };
    }

    if let Err(e) = std::env::set_current_dir(&target) {
        eprintln!("shako: cd: {}: {e}", target.display());
    } else if let Ok(cwd) = std::env::current_dir() {
        // Keep PWD in sync — Starship and most Unix tools read PWD rather than
        // resolving the kernel cwd, so without this the prompt stays stale.
        unsafe { std::env::set_var("PWD", &cwd) };
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
            unsafe { std::env::set_var(key, value) };
        } else {
            match std::env::var(arg) {
                Ok(val) => println!("{arg}={val}"),
                Err(_) => eprintln!("shako: export: {arg}: not set"),
            }
        }
    }
}

fn builtin_unset(args: &[&str]) {
    for arg in args {
        unsafe { std::env::remove_var(arg) };
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
