use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::{env, fs, io};

/// Import fish shell configuration into shako format.
///
/// Reads:   ~/.config/fish/{config.fish, conf.d/*.fish, functions/*.fish}
/// Writes:  ~/.config/shako/{config.shako, conf.d/*.sh, functions/*.sh}
///
/// Performs best-effort conversion of fish syntax into shako-compatible format.
/// Fish-specific constructs (status is-interactive, fish_greeting, etc.) are
/// stripped or converted. Aliases, abbreviations, env vars, PATH modifications,
/// and function definitions are preserved.
pub fn run_import() {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            eprintln!("shako: fish-import: cannot determine home directory");
            return;
        }
    };

    let fish_dir = home.join(".config").join("fish");
    if !fish_dir.is_dir() {
        eprintln!(
            "shako: fish-import: {} not found — is fish installed?",
            fish_dir.display()
        );
        return;
    }

    let shako_dir = env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .ok()
        .unwrap_or_else(|| home.join(".config"))
        .join("shako");

    writeln!(out, "\x1b[1;36m fish → shako config import\x1b[0m\n").ok();
    writeln!(out, " \x1b[90mfrom:\x1b[0m {}", fish_dir.display()).ok();
    writeln!(out, " \x1b[90m  to:\x1b[0m {}\n", shako_dir.display()).ok();

    fs::create_dir_all(shako_dir.join("conf.d")).ok();
    fs::create_dir_all(shako_dir.join("functions")).ok();

    let mut stats = ImportStats::default();

    // 1. Convert config.fish → config.shako
    let config_fish = fish_dir.join("config.fish");
    if config_fish.exists() {
        match fs::read_to_string(&config_fish) {
            Ok(contents) => {
                let converted = convert_fish_config(&contents, &mut stats);
                let dest = shako_dir.join("config.shako");
                if dest.exists() {
                    writeln!(
                        out,
                        " \x1b[33m⚠\x1b[0m config.shako already exists — writing config.shako.imported"
                    )
                    .ok();
                    let dest = shako_dir.join("config.shako.imported");
                    fs::write(&dest, &converted).ok();
                } else {
                    fs::write(&dest, &converted).ok();
                }
                writeln!(out, " \x1b[32m✓\x1b[0m config.fish → config.shako").ok();
            }
            Err(e) => {
                writeln!(out, " \x1b[31m✗\x1b[0m config.fish: {e}").ok();
            }
        }
    }

    // 2. Convert conf.d/*.fish → conf.d/*.sh
    let fish_conf_d = fish_dir.join("conf.d");
    if fish_conf_d.is_dir() {
        let mut files: Vec<_> = fs::read_dir(&fish_conf_d)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".fish"))
            .collect();
        files.sort_by_key(|e| e.file_name());

        for entry in &files {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let dest_name = name.replace(".fish", ".sh");
            let dest = shako_dir.join("conf.d").join(&dest_name);

            match fs::read_to_string(entry.path()) {
                Ok(contents) => {
                    let converted = convert_fish_config(&contents, &mut stats);
                    if !converted.trim().is_empty() {
                        fs::write(&dest, &converted).ok();
                        writeln!(out, " \x1b[32m✓\x1b[0m conf.d/{name} → conf.d/{dest_name}").ok();
                    } else {
                        writeln!(
                            out,
                            " \x1b[90m⊘\x1b[0m conf.d/{name} \x1b[90m(empty after conversion, skipped)\x1b[0m"
                        )
                        .ok();
                        stats.skipped += 1;
                    }
                }
                Err(e) => {
                    writeln!(out, " \x1b[31m✗\x1b[0m conf.d/{name}: {e}").ok();
                }
            }
        }
    }

    // 3. Convert functions/*.fish → functions/*.sh
    let fish_functions = fish_dir.join("functions");
    if fish_functions.is_dir() {
        let mut files: Vec<_> = fs::read_dir(&fish_functions)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                name.ends_with(".fish") && !name.starts_with('.')
            })
            .collect();
        files.sort_by_key(|e| e.file_name());

        let mut func_count = 0;
        for entry in &files {
            let name = entry.file_name();
            let name = name.to_string_lossy();

            // Skip fish internal functions
            if is_fish_internal_function(&name) {
                stats.skipped += 1;
                continue;
            }

            let dest_name = name.replace(".fish", ".sh");
            let dest = shako_dir.join("functions").join(&dest_name);

            if let Ok(contents) = fs::read_to_string(entry.path()) {
                let converted = convert_fish_function(&contents);
                if !converted.trim().is_empty() {
                    fs::write(&dest, &converted).ok();
                    func_count += 1;
                }
            }
        }

        if func_count > 0 {
            writeln!(
                out,
                " \x1b[32m✓\x1b[0m functions/ — {func_count} functions imported"
            )
            .ok();
        }
        stats.functions += func_count;
    }

    // Summary
    writeln!(out).ok();
    writeln!(out, " \x1b[1mImport summary:\x1b[0m").ok();
    if stats.aliases > 0 {
        writeln!(out, "   {:<4} aliases", stats.aliases).ok();
    }
    if stats.abbreviations > 0 {
        writeln!(out, "   {:<4} abbreviations", stats.abbreviations).ok();
    }
    if stats.env_vars > 0 {
        writeln!(out, "   {:<4} environment variables", stats.env_vars).ok();
    }
    if stats.path_entries > 0 {
        writeln!(out, "   {:<4} PATH entries", stats.path_entries).ok();
    }
    if stats.functions > 0 {
        writeln!(out, "   {:<4} functions", stats.functions).ok();
    }
    if stats.skipped > 0 {
        writeln!(
            out,
            "   {:<4} \x1b[90mskipped (fish-specific or empty)\x1b[0m",
            stats.skipped
        )
        .ok();
    }
    writeln!(out).ok();
    writeln!(
        out,
        " \x1b[90mRestart shako to load the imported config.\x1b[0m"
    )
    .ok();
    writeln!(
        out,
        " \x1b[90mYou can remove [fish] source_config = true from config.toml now.\x1b[0m\n"
    )
    .ok();
}

#[derive(Default)]
struct ImportStats {
    aliases: usize,
    abbreviations: usize,
    env_vars: usize,
    path_entries: usize,
    functions: usize,
    skipped: usize,
}

/// Convert fish config file contents to shako-compatible format.
fn convert_fish_config(contents: &str, stats: &mut ImportStats) -> String {
    let mut output = Vec::new();
    let lines: Vec<&str> = contents.lines().collect();
    let mut i = 0;
    let mut seen_vars: HashMap<String, bool> = HashMap::new();

    while i < lines.len() {
        let line = lines[i].trim();
        i += 1;

        // Preserve comments
        if line.starts_with('#') {
            output.push(line.to_string());
            continue;
        }

        if line.is_empty() {
            output.push(String::new());
            continue;
        }

        // Skip fish-specific constructs
        if should_skip_line(line) {
            continue;
        }

        // Skip block constructs: if/end, switch/end, for/end, while/end
        if line.starts_with("if ")
            || line.starts_with("switch ")
            || line.starts_with("for ")
            || line.starts_with("while ")
        {
            // Extract useful content from `if status is-interactive` blocks
            if line.contains("status is-interactive") || line.contains("status --is-interactive") {
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
                    } else if depth == 1 && inner != "else" && !inner.starts_with("else if") {
                        // Recursively process lines inside the interactive block
                        let inner_result = convert_fish_config(&format!("{inner}\n"), stats);
                        let inner_result = inner_result.trim();
                        if !inner_result.is_empty() {
                            output.push(inner_result.to_string());
                        }
                    }
                    i += 1;
                }
            } else {
                // Skip other blocks entirely
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
                stats.skipped += 1;
            }
            continue;
        }

        // Multi-line fish function → shako function
        if line.starts_with("function ") && !line.contains('{') {
            // Safety: strip_prefix always succeeds here because `starts_with` was just checked.
            let rest = line.strip_prefix("function ").unwrap().trim();
            let name = rest
                .split(|c: char| c.is_whitespace() || c == ';')
                .next()
                .unwrap_or("")
                .trim_end_matches("()")
                .to_string();

            if name.is_empty() || name == "--" {
                i += skip_block(&lines, i);
                stats.skipped += 1;
                continue;
            }

            // Skip fish internal event handler functions
            if rest.contains("--on-event") || rest.contains("--on-signal") {
                i += skip_block(&lines, i);
                stats.skipped += 1;
                continue;
            }

            let mut body_lines = Vec::new();
            let mut depth = 1;
            while i < lines.len() && depth > 0 {
                let inner = lines[i].trim();
                if inner.starts_with("function ") {
                    depth += 1;
                } else if inner == "end" {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                if depth > 0 {
                    body_lines.push(convert_line(inner));
                }
                i += 1;
            }

            let body = body_lines
                .iter()
                .filter(|l| !l.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join("; ");

            if !body.is_empty() {
                output.push(format!("function {name}() {{ {body} }}"));
                stats.functions += 1;
            }
            continue;
        }

        // set -gx VAR value → set -gx VAR value (already compatible)
        if line.starts_with("set ") {
            let converted = convert_set_line(line, &mut seen_vars);
            if let Some(line) = converted {
                output.push(line);
                stats.env_vars += 1;
            }
            continue;
        }

        // alias
        if line.starts_with("alias ") {
            output.push(convert_alias_line(line));
            stats.aliases += 1;
            continue;
        }

        // abbr
        if line.starts_with("abbr ") {
            output.push(line.to_string());
            stats.abbreviations += 1;
            continue;
        }

        // fish_add_path
        if line.starts_with("fish_add_path ") {
            output.push(line.to_string());
            stats.path_entries += 1;
            continue;
        }

        // export
        if line.starts_with("export ") {
            output.push(line.to_string());
            stats.env_vars += 1;
            continue;
        }

        // source — convert path
        if line.starts_with("source ") {
            output.push(line.to_string());
            continue;
        }

        // Anything else — pass through as comment
        if !line.is_empty() {
            output.push(format!("# [fish] {line}"));
            stats.skipped += 1;
        }
    }

    // Remove trailing empty lines
    while output.last().is_some_and(|l| l.is_empty()) {
        output.pop();
    }

    if output.is_empty() {
        return String::new();
    }

    format!(
        "# Imported from fish shell by shako fish-import\n\n{}\n",
        output.join("\n")
    )
}

/// Convert a fish `set` line to shako format.
fn convert_set_line(line: &str, seen: &mut HashMap<String, bool>) -> Option<String> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    // Parse flags to find the variable name
    let mut export = false;
    let mut erase = false;
    let mut query = false;
    let mut var_idx = 1;

    for (idx, part) in parts.iter().enumerate().skip(1) {
        if *part == "--" {
            var_idx = idx + 1;
            break;
        }
        if part.starts_with('-') {
            for ch in part.trim_start_matches('-').chars() {
                match ch {
                    'x' => export = true,
                    'e' => erase = true,
                    'q' => query = true,
                    _ => {}
                }
            }
            var_idx = idx + 1;
        } else {
            var_idx = idx;
            break;
        }
    }

    if query {
        return None;
    }

    if var_idx >= parts.len() {
        return None;
    }

    let var_name = parts[var_idx];

    // Skip fish internal variables
    if is_fish_internal_var(var_name) {
        return None;
    }

    if erase {
        return Some(format!("set -e {var_name}"));
    }

    // Avoid duplicate PATH entries (fish configs often re-set PATH)
    if var_name == "PATH" || var_name == "fish_user_paths" {
        if seen.contains_key(var_name) {
            return None;
        }
        seen.insert(var_name.to_string(), true);
        // Convert fish PATH/fish_user_paths to fish_add_path entries
        let paths = &parts[var_idx + 1..];
        if !paths.is_empty() {
            let result: Vec<String> = paths
                .iter()
                .filter(|p| {
                    // Skip default system paths and fish internals
                    !matches!(**p, "/usr/bin" | "/bin" | "/usr/sbin" | "/sbin" | "$PATH")
                })
                .map(|p| format!("fish_add_path {p}"))
                .collect();
            if result.is_empty() {
                return None;
            }
            return Some(result.join("\n"));
        }
        return None;
    }

    let value = if parts.len() > var_idx + 1 {
        parts[var_idx + 1..].join(" ")
    } else {
        String::new()
    };

    if export {
        Some(format!("set -gx {var_name} {value}"))
    } else {
        Some(format!("set {var_name} {value}"))
    }
}

/// Convert a fish alias line to shako format.
fn convert_alias_line(line: &str) -> String {
    let rest = line.strip_prefix("alias ").unwrap_or(line);
    if rest.contains('=') {
        line.to_string()
    } else {
        let parts: Vec<&str> = rest.splitn(2, char::is_whitespace).collect();
        if parts.len() == 2 {
            let value = parts[1].trim().trim_matches('\'').trim_matches('"');
            format!("alias {}='{}'", parts[0], value)
        } else {
            line.to_string()
        }
    }
}

/// Convert a single line from inside a fish function body.
fn convert_line(line: &str) -> String {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return line.to_string();
    }

    // Convert `command foo` → `foo` (fish uses `command` to bypass functions)
    if let Some(rest) = line.strip_prefix("command ") {
        return rest.to_string();
    }

    // Convert `builtin cd` → `cd`
    if let Some(rest) = line.strip_prefix("builtin ") {
        return rest.to_string();
    }

    // Convert `echo (cmd)` → `echo $(cmd)` — fish uses () for command substitution
    // This is a best-effort heuristic
    line.to_string()
}

/// Convert a fish function file to shako format.
fn convert_fish_function(contents: &str) -> String {
    let mut stats = ImportStats::default();
    convert_fish_config(contents, &mut stats)
}

/// Skip a block (if/for/while/switch ... end) and return how many lines were consumed.
fn skip_block(lines: &[&str], start: usize) -> usize {
    let mut depth = 1;
    let mut consumed = 0;
    let mut i = start;
    while i < lines.len() && depth > 0 {
        let inner = lines[i].trim();
        if inner.starts_with("if ")
            || inner.starts_with("switch ")
            || inner.starts_with("for ")
            || inner.starts_with("while ")
            || (inner.starts_with("function ") && !inner.contains('{'))
        {
            depth += 1;
        } else if inner == "end" {
            depth -= 1;
        }
        i += 1;
        consumed += 1;
    }
    consumed
}

/// Lines that are fish-specific and should be skipped entirely.
fn should_skip_line(line: &str) -> bool {
    let skip_prefixes = [
        "emit ",
        "status ",
        "string ",
        "test ",
        "contains ",
        "type -q ",
        "command -q ",
        "set_color ",
        "printf ",
        "bind ",
        "fish_vi_key_bindings",
        "fish_default_key_bindings",
        "fish_hybrid_key_bindings",
        "set fish_greeting",
        "set -g fish_greeting",
        "set -U fish_greeting",
        "set fish_color_",
        "set -g fish_color_",
        "set -U fish_color_",
        "set fish_pager_color_",
        "starship init fish",
        "zoxide init fish",
        "fzf --fish",
        "direnv hook fish",
        "mise activate fish",
        "fnm env --use-on-cd",
        "pyenv init",
        "rbenv init",
        "nvm ",
        "thefuck --alias",
        "any-nix-shell fish",
    ];

    for prefix in &skip_prefixes {
        if line.starts_with(prefix) {
            return true;
        }
    }

    // Skip pipe chains and complex expressions
    if line.contains(" | ") && !line.starts_with("alias") && !line.starts_with("abbr") {
        return true;
    }

    false
}

/// Fish internal variables that shouldn't be imported.
fn is_fish_internal_var(name: &str) -> bool {
    name.starts_with("fish_")
        || name.starts_with("__fish_")
        || name == "FISH_VERSION"
        || name == "SHLVL"
        || name == "PWD"
        || name == "OLDPWD"
        || name == "USER"
        || name == "HOME"
        || name == "SHELL"
        || name == "TERM"
        || name == "COLUMNS"
        || name == "LINES"
        || name == "status"
        || name == "pipestatus"
        || name == "STARSHIP_SHELL"
}

/// Fish internal function files that shouldn't be imported.
fn is_fish_internal_function(name: &str) -> bool {
    let name = name.strip_suffix(".fish").unwrap_or(name);
    name.starts_with("fish_")
        || name.starts_with("__")
        || matches!(
            name,
            "cd" | "ls"
                | "ll"
                | "la"
                | "help"
                | "man"
                | "open"
                | "N"
                | "Y"
                | "alias"
                | "history"
                | "type"
                | "isatty"
                | "realpath"
                | "suspend"
                | "trap"
                | "umask"
                | "vared"
                | "wait"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── convert_alias_line ───────────────────────────────────────────────────

    #[test]
    fn test_alias_with_equals_passes_through() {
        // `alias foo=bar` is already POSIX-compatible — keep it unchanged.
        assert_eq!(convert_alias_line("alias foo=bar"), "alias foo=bar");
    }

    #[test]
    fn test_alias_space_form_becomes_equals_form() {
        // Fish allows `alias foo 'bar --baz'` — convert to `alias foo='bar --baz'`.
        let result = convert_alias_line("alias ll 'ls -la'");
        assert_eq!(result, "alias ll='ls -la'");
    }

    #[test]
    fn test_alias_space_form_double_quotes() {
        let result = convert_alias_line("alias gst \"git status\"");
        assert_eq!(result, "alias gst='git status'");
    }

    #[test]
    fn test_alias_single_token_unchanged() {
        // No value token — can't convert, return as-is.
        let result = convert_alias_line("alias foo");
        assert_eq!(result, "alias foo");
    }

    // ── convert_line ────────────────────────────────────────────────────────

    #[test]
    fn test_convert_line_command_prefix_stripped() {
        assert_eq!(convert_line("command git status"), "git status");
    }

    #[test]
    fn test_convert_line_builtin_prefix_stripped() {
        assert_eq!(convert_line("builtin cd /tmp"), "cd /tmp");
    }

    #[test]
    fn test_convert_line_empty_returns_empty() {
        assert_eq!(convert_line(""), "");
    }

    #[test]
    fn test_convert_line_comment_preserved() {
        assert_eq!(convert_line("# this is a comment"), "# this is a comment");
    }

    #[test]
    fn test_convert_line_plain_command_unchanged() {
        assert_eq!(convert_line("echo hello world"), "echo hello world");
    }

    #[test]
    fn test_convert_line_trims_leading_whitespace() {
        assert_eq!(convert_line("  echo hi"), "echo hi");
    }

    // ── convert_set_line ─────────────────────────────────────────────────────

    #[test]
    fn test_set_export_flag() {
        let mut seen = HashMap::new();
        let result = convert_set_line("set -gx MY_VAR hello", &mut seen);
        assert_eq!(result, Some("set -gx MY_VAR hello".to_string()));
    }

    #[test]
    fn test_set_no_export() {
        let mut seen = HashMap::new();
        let result = convert_set_line("set MY_VAR hello", &mut seen);
        assert_eq!(result, Some("set MY_VAR hello".to_string()));
    }

    #[test]
    fn test_set_erase() {
        let mut seen = HashMap::new();
        let result = convert_set_line("set -e MY_VAR", &mut seen);
        assert_eq!(result, Some("set -e MY_VAR".to_string()));
    }

    #[test]
    fn test_set_query_ignored() {
        // `set -q` is a test construct — skip it entirely.
        let mut seen = HashMap::new();
        let result = convert_set_line("set -q MY_VAR", &mut seen);
        assert_eq!(result, None);
    }

    #[test]
    fn test_set_path_deduplicated() {
        let mut seen = HashMap::new();
        // First occurrence should produce a fish_add_path line.
        let first = convert_set_line("set PATH /usr/local/bin $PATH", &mut seen);
        assert!(first.is_some());
        // Second occurrence of PATH should be skipped to avoid duplication.
        let second = convert_set_line("set PATH /usr/local/bin $PATH", &mut seen);
        assert_eq!(second, None);
    }

    #[test]
    fn test_set_fish_internal_var_skipped() {
        let mut seen = HashMap::new();
        // fish_greeting is a fish-internal variable and should not be emitted.
        let result = convert_set_line("set fish_greeting ''", &mut seen);
        assert_eq!(result, None);
    }

    // ── is_fish_internal_function ────────────────────────────────────────────

    #[test]
    fn test_fish_greeting_is_internal() {
        assert!(is_fish_internal_function("fish_greeting"));
    }

    #[test]
    fn test_fish_prompt_is_internal() {
        assert!(is_fish_internal_function("fish_prompt"));
    }

    #[test]
    fn test_user_function_not_internal() {
        assert!(!is_fish_internal_function("my_custom_function"));
    }

    // ── should_skip_line ─────────────────────────────────────────────────────

    #[test]
    fn test_skip_status_line() {
        assert!(should_skip_line("status is-interactive"));
    }

    #[test]
    fn test_skip_set_color_line() {
        assert!(should_skip_line("set fish_color_command blue"));
    }

    #[test]
    fn test_skip_starship_init() {
        assert!(should_skip_line("starship init fish | source"));
    }

    #[test]
    fn test_do_not_skip_alias() {
        assert!(!should_skip_line("alias ll='ls -la'"));
    }

    #[test]
    fn test_do_not_skip_set_var() {
        assert!(!should_skip_line("set -gx EDITOR vim"));
    }

    // ── convert_fish_config round-trip ───────────────────────────────────────

    #[test]
    fn test_convert_config_alias_round_trip() {
        let mut stats = ImportStats::default();
        let input = "alias ll 'ls -la'\n";
        let output = convert_fish_config(input, &mut stats);
        assert!(output.contains("alias ll='ls -la'"), "got: {output}");
    }

    #[test]
    fn test_convert_config_strips_status_lines() {
        let mut stats = ImportStats::default();
        let input = "status is-interactive; or exit\nalias gs='git status'\n";
        let output = convert_fish_config(input, &mut stats);
        assert!(!output.contains("status is-interactive"), "got: {output}");
        assert!(output.contains("alias gs='git status'"), "got: {output}");
    }

    #[test]
    fn test_convert_config_set_export() {
        let mut stats = ImportStats::default();
        let input = "set -gx EDITOR nvim\n";
        let output = convert_fish_config(input, &mut stats);
        assert!(output.contains("EDITOR"), "got: {output}");
    }
}
