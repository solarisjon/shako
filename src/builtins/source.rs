use std::path::PathBuf;

use super::set::{builtin_set, handle_path_set, prepend_path, set_targets_path_var};
use super::state::{ShellFunction, ShellState};

pub(super) fn builtin_source(args: &[&str], state: &mut ShellState) {
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
            // Safety: strip_prefix always succeeds here because `starts_with` was just checked.
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
                } else if inner == "end" || inner.starts_with("end ") || inner.starts_with("end\t")
                {
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
                state.functions.insert(name, ShellFunction { body });
            }
            continue;
        }

        // Brace-style function definition
        if line.starts_with("function ") && line.contains('{') {
            super::try_define_function(line, state);
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
                let expanded =
                    crate::parser::parse_args(value.trim_matches('\'').trim_matches('"'));
                let value = expanded.join(" ");
                unsafe { std::env::set_var(key.trim(), &value) };
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
        // Parse with quote-awareness and strip inline comments so values
        // like `abbr ls "eza --icons"  # comment` work correctly.
        // We insert directly into state.abbreviations (no output during source).
        if line.starts_with("abbr ") {
            let cleaned = strip_inline_comment(line);
            let parsed = crate::parser::parse_args(&cleaned);
            // parsed[0] = "abbr", rest = flags + positional args
            let args: Vec<&str> = parsed.iter().skip(1).map(|s| s.as_str()).collect();
            // Determine mode and collect positional args (skip flags)
            let mut mode = "add";
            let mut positional = Vec::new();
            for arg in &args {
                match *arg {
                    "-a" | "--add" => mode = "add",
                    "-e" | "--erase" => mode = "erase",
                    "-l" | "--list" => mode = "list",
                    "--" => {}
                    _ if arg.starts_with('-') => {}
                    _ => positional.push(*arg),
                }
            }
            match mode {
                "erase" => {
                    for name in &positional {
                        state.abbreviations.remove(*name);
                    }
                }
                _ if !positional.is_empty() => {
                    if positional.len() >= 2 {
                        let name = positional[0].to_string();
                        let value = positional[1..].join(" ");
                        state.abbreviations.insert(name, value);
                    }
                }
                _ => {}
            }
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

/// Strip an inline comment from a shell line, respecting quotes.
/// `abbr rg rg # comment` → `abbr rg rg`
/// `abbr ls "eza --icons"  # comment` → `abbr ls "eza --icons"`
fn strip_inline_comment(line: &str) -> String {
    let mut in_single = false;
    let mut in_double = false;
    let chars: Vec<char> = line.chars().collect();

    for (i, &c) in chars.iter().enumerate() {
        if c == '\'' && !in_double {
            in_single = !in_single;
        } else if c == '"' && !in_single {
            in_double = !in_double;
        } else if c == '#' && !in_single && !in_double {
            return line[..i].trim_end().to_string();
        }
    }

    line.to_string()
}

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

/// Parse a fish-style function file (function name ... end) and return
/// the body as a semicolon-separated string suitable for ShellFunction.
pub(super) fn parse_fish_function_file(contents: &str) -> String {
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
