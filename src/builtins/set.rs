use std::env;

/// Fish-compatible `set` builtin.
///   set VAR value         — set variable
///   set -x VAR value      — export variable
///   set -gx VAR value     — export variable (global, same as -x)
///   set -Ux VAR value     — export variable (universal, treated as -x)
///   set -e VAR            — erase variable
///   set -ex VAR           — erase exported variable
///   set                   — list all environment variables
pub(super) fn builtin_set(args: &[&str]) {
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
pub(super) fn is_path_variable(name: &str) -> bool {
    name.ends_with("PATH")
}

/// Check if a `set` command targets a PATH variable (PATH, MANPATH, etc.).
pub(super) fn set_targets_path_var(parts: &[&str]) -> bool {
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
pub(super) fn handle_path_set(parts: &[&str]) {
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
pub(super) fn prepend_path(dir: &str) {
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
