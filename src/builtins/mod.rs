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
    // Phase 2
    "echo",
    "read",
    "test",
    "[",
    "pwd",
    "pushd",
    "popd",
    "dirs",
    "true",
    "false",
    // Phase 3
    "return",
    "command",
];

// Thread-local signal for early return from user-defined functions.
// Set by `return [code]`; cleared by `run_function` after the loop.
std::thread_local! {
    static FUNCTION_RETURN: std::cell::Cell<Option<i32>> = const { std::cell::Cell::new(None) };
}

/// Check if a token is a builtin command name.
pub fn is_builtin(token: &str) -> bool {
    BUILTINS.contains(&token)
}

/// Run a shell builtin command. Returns the exit code (0 = success).
pub fn run_builtin(input: &str, state: &mut ShellState) -> i32 {
    // Use parse_args so builtins get glob expansion, tilde expansion,
    // env var expansion, and quoting — same as regular commands.
    let parsed = crate::parser::parse_args(input);
    let parts: Vec<&str> = parsed.iter().map(|s| s.as_str()).collect();
    if parts.is_empty() {
        return 0;
    }

    match parts[0] {
        "cd" => builtin_cd(&parts[1..]),
        "z" => { builtin_z(&parts[1..]); 0 }
        "zi" => { builtin_zi(); 0 }
        "exit" => std::process::exit(0),
        "export" => { builtin_export(&parts[1..]); 0 }
        "unset" => { builtin_unset(&parts[1..]); 0 }
        "set" => { set::builtin_set(&parts[1..]); 0 }
        "history" => { builtin_history(&parts[1..], state); 0 }
        "alias" => { builtin_alias(&parts[1..], state); 0 }
        "unalias" => { builtin_unalias(&parts[1..], state); 0 }
        "abbr" => { builtin_abbr(&parts[1..], state); 0 }
        "fish-import" => { crate::fish_import::run_import(); 0 }
        "source" => { source::builtin_source(&parts[1..], state); 0 }
        "type" => builtin_type(&parts[1..], state),
        "jobs" => { jobs::builtin_jobs(state); 0 }
        "fg" => { jobs::builtin_fg(&parts[1..], state); 0 }
        "bg" => { jobs::builtin_bg(&parts[1..], state); 0 }
        "functions" => { builtin_functions(state); 0 }
        // Phase 2
        "echo" => builtin_echo(&parts[1..]),
        "read" => builtin_read(&parts[1..]),
        "test" => builtin_test(&parts[1..]),
        "[" => {
            let args: Vec<&str> = parts[1..].iter().copied().filter(|a| *a != "]").collect();
            builtin_test(&args)
        }
        "pwd" => { println!("{}", std::env::current_dir().unwrap_or_default().display()); 0 }
        "pushd" => builtin_pushd(&parts[1..], state),
        "popd" => builtin_popd(&parts[1..], state),
        "dirs" => { builtin_dirs(state); 0 }
        "true" => 0,
        "false" => 1,
        "return" => {
            let code = parts.get(1).and_then(|n| n.parse::<i32>().ok()).unwrap_or(0);
            FUNCTION_RETURN.with(|r| r.set(Some(code)));
            code
        }
        "command" => {
            // Run a command bypassing aliases/functions (like fish's `command`).
            // Simply exec the remainder as an external command.
            if parts.len() < 2 {
                eprintln!("shako: command: missing command name");
                return 1;
            }
            let cmd = parts[1..].join(" ");
            crate::executor::execute_command(&cmd)
                .and_then(|s| s.code())
                .unwrap_or(0)
        }
        other => { eprintln!("shako: unknown builtin: {other}"); 1 }
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
/// Returns the exit code: the argument to `return`, the last command's exit
/// code, or 0 if the body was empty.
pub fn run_function(func: &ShellFunction, args: &[&str]) -> i32 {
    use std::env;
    // Set positional parameters as env vars
    for (i, arg) in args.iter().enumerate() {
        unsafe { env::set_var(format!("{}", i + 1), arg) };
    }
    unsafe { env::set_var("@", args.join(" ")) };
    unsafe { env::set_var("#", args.len().to_string()) };

    let mut last_code = 0i32;

    'body: for line in func.body.split(';') {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Dispatch builtins (including `return`) inside function bodies.
        let first = line.split_whitespace().next().unwrap_or("");
        if is_builtin(first) {
            // `return` signals early exit; the FUNCTION_RETURN cell is set
            // inside run_builtin, so we just need to check it after.
            last_code = run_builtin_no_state(first, line);
        } else {
            last_code = crate::executor::execute_command(line)
                .and_then(|s| s.code())
                .unwrap_or(0);
        }
        // Check for early return signal
        if let Some(code) = FUNCTION_RETURN.with(|r| r.get()) {
            last_code = code;
            break 'body;
        }
    }

    // Clear any pending return signal
    FUNCTION_RETURN.with(|r| r.set(None));

    // Clean up positional parameters
    for i in 1..=args.len() {
        unsafe { env::remove_var(format!("{i}")) };
    }
    unsafe { env::remove_var("@") };
    unsafe { env::remove_var("#") };

    last_code
}

/// Dispatch a builtin that does not need `ShellState` (usable inside function
/// bodies where we don't have access to the REPL state).
fn run_builtin_no_state(first: &str, line: &str) -> i32 {
    let parsed = crate::parser::parse_args(line);
    let parts: Vec<&str> = parsed.iter().map(|s| s.as_str()).collect();
    match first {
        "echo" => builtin_echo(&parts[1..]),
        "read" => builtin_read(&parts[1..]),
        "test" => builtin_test(&parts[1..]),
        "[" => {
            let args: Vec<&str> = parts[1..].iter().copied().filter(|a| *a != "]").collect();
            builtin_test(&args)
        }
        "pwd" => { println!("{}", std::env::current_dir().unwrap_or_default().display()); 0 }
        "true" => 0,
        "false" => 1,
        "return" => {
            let code = parts.get(1).and_then(|n| n.parse::<i32>().ok()).unwrap_or(0);
            FUNCTION_RETURN.with(|r| r.set(Some(code)));
            code
        }
        "exit" => std::process::exit(0),
        "cd" => builtin_cd(&parts[1..]),
        other => {
            eprintln!("shako: {other}: builtin not available inside function body");
            127
        }
    }
}

fn builtin_cd(args: &[&str]) -> i32 {
    let target = if args.is_empty() {
        match dirs::home_dir() {
            Some(home) => home,
            None => {
                eprintln!("shako: cd: HOME not set");
                return 1;
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
                return 1;
            }
        }
    } else {
        let path = args[0];
        // If the path still contains glob metacharacters, it means the glob
        // expansion found no matches and returned the pattern literally.
        if path.chars().any(|c| c == '*' || c == '?' || c == '[') {
            eprintln!("shako: cd: {path}: no matches found");
            return 1;
        }
        if path.starts_with('~') {
            match dirs::home_dir() {
                Some(home) => home.join(path.trim_start_matches('~').trim_start_matches('/')),
                None => {
                    eprintln!("shako: cd: HOME not set");
                    return 1;
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
        1
    } else if let Ok(cwd) = std::env::current_dir() {
        // Keep PWD in sync — Starship and most Unix tools read PWD rather than
        // resolving the kernel cwd, so without this the prompt stays stale.
        unsafe { std::env::set_var("PWD", &cwd) };
        if smart_defaults::has_zoxide() {
            smart_defaults::zoxide_add(&cwd.display().to_string());
        }
        0
    } else {
        0
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

fn builtin_type(args: &[&str], state: &ShellState) -> i32 {
    let mut found = true;
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
            found = false;
        }
    }
    if found { 0 } else { 1 }
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

// ─── Phase 2 Builtins ────────────────────────────────────────────────────────

/// `echo` — print arguments to stdout.
///   -n   no trailing newline
///   -e   interpret backslash escapes (\n \t \\ \a \b \r)
fn builtin_echo(args: &[&str]) -> i32 {
    let mut newline = true;
    let mut interpret = false;
    let mut arg_start = 0;

    for (i, arg) in args.iter().enumerate() {
        match *arg {
            "-n" => { newline = false; arg_start = i + 1; }
            "-e" => { interpret = true; arg_start = i + 1; }
            "-ne" | "-en" => { newline = false; interpret = true; arg_start = i + 1; }
            _ => break,
        }
    }

    let output = args[arg_start..].join(" ");
    let output = if interpret { unescape_echo(&output) } else { output };

    if newline {
        println!("{output}");
    } else {
        print!("{output}");
        use std::io::Write;
        std::io::stdout().flush().ok();
    }
    0
}

fn unescape_echo(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some('a') => result.push('\x07'),
                Some('b') => result.push('\x08'),
                Some('\\') => result.push('\\'),
                Some('0') => result.push('\0'),
                Some(other) => { result.push('\\'); result.push(other); }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// `read` — read a line from stdin into a variable.
///   -p prompt   print prompt before reading
///   -r          raw mode (accepted, currently default)
///   VAR         variable name to store result (default: REPLY)
fn builtin_read(args: &[&str]) -> i32 {
    let mut prompt = "";
    let mut var_name = "REPLY";
    let mut i = 0;

    while i < args.len() {
        match args[i] {
            "-p" => {
                i += 1;
                if i < args.len() { prompt = args[i]; }
            }
            "-r" => {}
            arg if !arg.starts_with('-') => { var_name = arg; }
            _ => {}
        }
        i += 1;
    }

    if !prompt.is_empty() {
        use std::io::Write;
        print!("{prompt}");
        std::io::stdout().flush().ok();
    }

    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) => return 1,
        Ok(_) => {}
        Err(e) => { eprintln!("shako: read: {e}"); return 1; }
    }

    let value = line.trim_end_matches('\n').trim_end_matches('\r');
    unsafe { std::env::set_var(var_name, value) };
    0
}

/// `test`/`[` — evaluate a conditional expression. Returns 0 (true) or 1 (false).
fn builtin_test(args: &[&str]) -> i32 {
    if test_eval(args) { 0 } else { 1 }
}

fn test_eval(args: &[&str]) -> bool {
    match args {
        [] => false,
        ["!", rest @ ..] => !test_eval(rest),
        [op, operand] => test_unary(op, operand),
        [lhs, op, rhs] => test_binary(lhs, op, rhs),
        _ => {
            if let Some(pos) = args.iter().position(|a| *a == "-o") {
                return test_eval(&args[..pos]) || test_eval(&args[pos + 1..]);
            }
            if let Some(pos) = args.iter().position(|a| *a == "-a") {
                return test_eval(&args[..pos]) && test_eval(&args[pos + 1..]);
            }
            args.len() == 1 && !args[0].is_empty()
        }
    }
}

fn test_unary(op: &str, operand: &str) -> bool {
    use std::path::Path;
    let path = Path::new(operand);
    match op {
        "-e" => path.exists(),
        "-f" => path.is_file(),
        "-d" => path.is_dir(),
        "-r" => {
            use std::os::unix::fs::PermissionsExt;
            path.metadata().map(|m| m.permissions().mode() & 0o444 != 0).unwrap_or(false)
        }
        "-w" => {
            use std::os::unix::fs::PermissionsExt;
            path.metadata().map(|m| m.permissions().mode() & 0o222 != 0).unwrap_or(false)
        }
        "-x" => {
            use std::os::unix::fs::PermissionsExt;
            path.metadata().map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false)
        }
        "-s" => path.metadata().map(|m| m.len() > 0).unwrap_or(false),
        "-L" | "-h" => path.symlink_metadata().map(|m| m.file_type().is_symlink()).unwrap_or(false),
        "-z" => operand.is_empty(),
        "-n" => !operand.is_empty(),
        _ => !op.is_empty(),
    }
}

fn test_binary(lhs: &str, op: &str, rhs: &str) -> bool {
    match op {
        "=" | "==" => lhs == rhs,
        "!=" => lhs != rhs,
        "-eq" => parse_int(lhs) == parse_int(rhs),
        "-ne" => parse_int(lhs) != parse_int(rhs),
        "-lt" => parse_int(lhs) < parse_int(rhs),
        "-le" => parse_int(lhs) <= parse_int(rhs),
        "-gt" => parse_int(lhs) > parse_int(rhs),
        "-ge" => parse_int(lhs) >= parse_int(rhs),
        _ => false,
    }
}

fn parse_int(s: &str) -> i64 {
    s.trim().parse().unwrap_or(0)
}

/// `pushd` — push cwd onto the directory stack and cd to the new dir.
fn builtin_pushd(args: &[&str], state: &mut ShellState) -> i32 {
    if args.is_empty() {
        eprintln!("shako: pushd: too few arguments");
        return 1;
    }
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => { eprintln!("shako: pushd: {e}"); return 1; }
    };
    let code = builtin_cd(args);
    if code == 0 {
        state.dir_stack.push(cwd);
        builtin_dirs(state);
    }
    code
}

/// `popd` — pop the top directory off the stack and cd there.
fn builtin_popd(_args: &[&str], state: &mut ShellState) -> i32 {
    match state.dir_stack.pop() {
        Some(dir) => {
            let dir_str = dir.display().to_string();
            let code = builtin_cd(&[dir_str.as_str()]);
            if code == 0 { builtin_dirs(state); }
            code
        }
        None => { eprintln!("shako: popd: directory stack empty"); 1 }
    }
}

/// `dirs` — print the directory stack (cwd first, then stack).
fn builtin_dirs(state: &ShellState) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut parts = vec![cwd.display().to_string()];
    for dir in state.dir_stack.iter().rev() {
        parts.push(dir.display().to_string());
    }
    println!("{}", parts.join("  "));
}
