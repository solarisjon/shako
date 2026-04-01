//! REPL loop and input dispatch for the interactive shell.
//!
//! This module owns:
//!  - `run_repl` — the main event loop; call it after setup is complete.
//!  - Continuation detection (`needs_continuation`, `heredoc_needs_continuation`,
//!    `control_depth`) used while building multi-line inputs.
//!  - History expansion (`expand_history_bangs`).
//!  - History file reading helpers (`read_recent_history`, `read_recent_history_with_dedup`).
//!  - The `is_pure_builtin_call` predicate used by chain dispatch.
//!  - AI error-recovery offer (`offer_ai_recovery`).

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use reedline::{Reedline, Signal};
use tokio::runtime::Runtime;

use crate::ai;
use crate::builtins::{self, ShellState};
use crate::classifier::{Classification, Classifier};
use crate::config::ShakoConfig;
use crate::executor;
use crate::parser;
use crate::proactive;
use crate::shell::prompt::{self, CommandTimer, StarshipPrompt};
use crate::slash;
use crate::spinner;

/// Run the interactive REPL until the user exits.
///
/// Consumes the `Reedline` instance and all configuration. Returns only when
/// the user types `exit` / Ctrl-D or an unrecoverable I/O error occurs.
pub fn run_repl(
    mut line_editor: Reedline,
    prompt: StarshipPrompt,
    mut state: ShellState,
    classifier: Classifier,
    mut config: ShakoConfig,
    rt: Runtime,
    history_path: PathBuf,
    extra_completions: std::sync::Arc<std::sync::RwLock<Vec<String>>>,
) {
    let mut last_command = String::new();
    let mut ran_foreground = false;

    loop {
        // Reap finished background jobs before each prompt
        state.reap_jobs();
        prompt::set_job_count(state.jobs.len());

        // Keep alias and function names available for tab completion.
        if let Ok(mut extra) = extra_completions.write() {
            extra.clear();
            extra.extend(state.aliases.keys().cloned());
            extra.extend(state.functions.keys().cloned());
        }

        #[cfg(unix)]
        if ran_foreground {
            std::thread::sleep(std::time::Duration::from_millis(50));
            executor::drain_pending_input();
            ran_foreground = false;
        }

        // Always restore ECHO before reedline reads — suppress_echo() may have
        // disabled it, and if reedline saves that as its baseline the tab
        // completion menu can break on some terminals.
        #[cfg(unix)]
        executor::restore_echo();

        let sig = line_editor.read_line(&prompt);
        match sig {
            Ok(Signal::Success(input)) => {
                // Trim in-place: truncate trailing whitespace first, then drain
                // leading whitespace — avoids allocating a second String when
                // there is no surrounding whitespace (the common case).
                let mut input = input;
                let start = input.len() - input.trim_start().len();
                input.drain(..start);
                let end = input.trim_end().len();
                input.truncate(end);
                if input.is_empty() {
                    continue;
                }

                // Multiline continuation: trailing \ or unclosed quotes or open heredoc
                while needs_continuation(&input) {
                    let in_heredoc = heredoc_needs_continuation(&input);
                    let cont_prompt = reedline::DefaultPrompt::new(
                        reedline::DefaultPromptSegment::Basic(if in_heredoc {
                            "heredoc> ".to_string()
                        } else {
                            "... ".to_string()
                        }),
                        reedline::DefaultPromptSegment::Empty,
                    );
                    match line_editor.read_line(&cont_prompt) {
                        Ok(Signal::Success(next)) => {
                            if in_heredoc {
                                // Heredoc lines must be joined with real newlines so that
                                // executor::parse_redirects can find the terminator line.
                                input.push('\n');
                                input.push_str(&next);
                            } else if input.ends_with('\\') {
                                input.pop(); // remove trailing backslash — join as one token
                                input.push(' ');
                                input.push_str(next.trim());
                            } else {
                                // Treat each continuation line as a new statement so
                                // that keywords like `done`/`fi` form their own segment
                                // and are recognised by control_depth / split_semicolons.
                                input.push_str("; ");
                                input.push_str(next.trim());
                            }
                        }
                        _ => break,
                    }
                }

                // History expansion: !! (last command), !$ (last arg)
                let input = expand_history_bangs(&input, &last_command);

                // Expand aliases before classification
                let input = state.expand_alias(&input).unwrap_or(input);

                // Publish current session context so that command substitutions
                // inside $(...) run under shako with the same aliases/functions.
                parser::set_subst_context(parser::SubstContext {
                    aliases: state.aliases.clone(),
                    functions: state
                        .functions
                        .iter()
                        .map(|(name, f)| (name.clone(), f.body.clone()))
                        .collect(),
                });

                // Handle AI session memory reset
                if input == "ai reset" || input == "ai forget" {
                    state.ai_session_memory.clear();
                    println!("AI session memory cleared.");
                    continue;
                }

                // Check for function definition
                if input.starts_with("function ") {
                    builtins::try_define_function(&input, &mut state);
                    continue;
                }

                // Check for trailing & (background execution)
                if input.ends_with('&') && !input.ends_with("&&") {
                    let bg_cmd = input.trim_end_matches('&').trim();
                    if !bg_cmd.is_empty() {
                        if let Some(child) = executor::spawn_background(bg_cmd) {
                            let pid = child.id();
                            state.add_job(child, bg_cmd.to_string());
                            // Update $! (last background PID).
                            parser::set_last_bg_pid(pid);
                        }
                    }
                    continue;
                }

                // Route control flow (if/for/while) through the control engine
                let timer = CommandTimer::start();

                if crate::control::has_control_flow(&input) {
                    let stmts = crate::control::parse_body(&input);
                    let mut locals = Vec::new();
                    let code = match crate::control::exec_statements(&stmts, &mut locals) {
                        crate::control::ExecSignal::Normal(c)
                        | crate::control::ExecSignal::Return(c) => c,
                        _ => 0,
                    };
                    prompt::set_last_status(code);
                    last_command = input.to_string();
                    timer.stop();
                    continue;
                }

                // Check if first token is a shell function (including autoload)
                let first_token = input.split_whitespace().next().unwrap_or("");
                if state.functions.contains_key(first_token)
                    || state.try_autoload_function(first_token)
                {
                    if let Some(func) = state.functions.get(first_token).cloned() {
                        let args: Vec<&str> = input.split_whitespace().skip(1).collect();
                        let code = builtins::run_function(&func, &args);
                        crate::shell::prompt::set_last_status(code);
                    }
                    timer.stop();
                    continue;
                }

                match classifier.classify(&input) {
                    Classification::Command(cmd) => {
                        ran_foreground = true;
                        let (status, stderr_output) =
                            executor::execute_command_with_stderr(&cmd);
                        set_exit_code(status);

                        // If the foreground process was stopped by Ctrl-Z,
                        // add it to the jobs list.
                        if let Some(stopped) = executor::take_stopped_job() {
                            state.add_stopped_job(stopped.pid, stopped.pgid, cmd.clone());
                        } else if let Some(s) = status {
                            if !s.success() {
                                if config.behavior.ai_enabled {
                                    offer_ai_recovery(
                                        &cmd,
                                        s.code().unwrap_or(1),
                                        &stderr_output,
                                        &config,
                                        &rt,
                                        &history_path,
                                    );
                                }
                            } else if config.behavior.ai_enabled {
                                proactive::check(&cmd, &config, &rt);
                            }
                        }
                    }
                    Classification::Builtin(cmd) => {
                        // Chain-aware builtin dispatch: split on ;/&&/|| so
                        // that `pushd /tmp && ls` works correctly.
                        let chains = parser::split_chains(&cmd);
                        let mut last_code = 0i32;
                        for (segment, op) in &chains {
                            let code = if is_pure_builtin_call(segment) {
                                builtins::run_builtin(segment, &mut state)
                            } else {
                                let status = executor::execute_command(segment);
                                status.and_then(|s| s.code()).unwrap_or(0)
                            };
                            last_code = code;
                            let stop = match op {
                                parser::ChainOp::And => last_code != 0,
                                parser::ChainOp::Or => last_code == 0,
                                _ => false,
                            };
                            if stop {
                                break;
                            }
                        }
                        prompt::set_last_status(last_code);
                    }
                    Classification::NaturalLanguage(text) => {
                        if !config.behavior.ai_enabled {
                            eprintln!("shako: AI is disabled (ai_enabled = false in config)");
                        } else {
                            let history = read_recent_history(
                                &history_path,
                                config.behavior.history_context_lines,
                            );
                            match rt.block_on(ai::translate_and_execute(
                                &text,
                                &config,
                                history,
                                &mut state.ai_session_memory,
                            )) {
                                Ok(_) => prompt::set_last_status(0),
                                Err(e) => {
                                    eprintln!("shako: ai error: {e}");
                                    prompt::set_last_status(1);
                                }
                            }
                        }
                    }
                    Classification::ForcedAI(text) => {
                        if !config.behavior.ai_enabled {
                            eprintln!("shako: AI is disabled (ai_enabled = false in config)");
                        } else {
                            let words: Vec<&str> = text.split_whitespace().collect();
                            let is_bare_command = words.len() == 1
                                && (which::which(words[0]).is_ok()
                                    || builtins::is_builtin(words[0]));

                            if is_bare_command {
                                let sp = spinner::Spinner::start("explaining...");
                                let result = rt.block_on(ai::explain_command(
                                    &text,
                                    &config,
                                    Some(sp.stop_flag()),
                                ));
                                drop(sp);
                                match result {
                                    Ok(explanation) => {
                                        print_styled_explain(&text, &explanation);
                                    }
                                    Err(e) => {
                                        eprintln!("shako: ai error: {e}");
                                        prompt::set_last_status(1);
                                    }
                                }
                            } else {
                                let history = read_recent_history(
                                    &history_path,
                                    config.behavior.history_context_lines,
                                );
                                match rt.block_on(ai::translate_and_execute(
                                    &text,
                                    &config,
                                    history,
                                    &mut state.ai_session_memory,
                                )) {
                                    Ok(_) => prompt::set_last_status(0),
                                    Err(e) => {
                                        eprintln!("shako: ai error: {e}");
                                        prompt::set_last_status(1);
                                    }
                                }
                            }
                        }
                    }
                    Classification::Typo { suggestion, .. } => {
                        if config.behavior.auto_correct_typos {
                            let should_run = if config.behavior.confirm_ai_commands {
                                print!(
                                    "\x1b[33mshako: did you mean \x1b[1m{suggestion}\x1b[0m\x1b[33m? [Y/n]\x1b[0m "
                                );
                                io::stdout().flush().ok();
                                let mut answer = String::new();
                                io::stdin().read_line(&mut answer).ok();
                                let answer = answer.trim().to_lowercase();
                                answer.is_empty() || answer == "y" || answer == "yes"
                            } else {
                                eprintln!(
                                    "\x1b[33mshako: auto-corrected to \x1b[1m{suggestion}\x1b[0m"
                                );
                                true
                            };
                            if should_run {
                                let first =
                                    suggestion.split_whitespace().next().unwrap_or("");
                                if builtins::is_builtin(first) {
                                    let code =
                                        builtins::run_builtin(&suggestion, &mut state);
                                    prompt::set_last_status(code);
                                } else {
                                    let status = executor::execute_command(&suggestion);
                                    set_exit_code(status);
                                }
                            }
                        } else {
                            let history = read_recent_history(
                                &history_path,
                                config.behavior.history_context_lines,
                            );
                            match rt.block_on(ai::translate_and_execute(
                                &suggestion,
                                &config,
                                history,
                                &mut state.ai_session_memory,
                            )) {
                                Ok(_) => prompt::set_last_status(0),
                                Err(e) => {
                                    eprintln!("shako: ai error: {e}");
                                    prompt::set_last_status(1);
                                }
                            }
                        }
                    }
                    Classification::Empty => {}
                    Classification::SlashCommand { name, args } => {
                        let code = slash::run(&name, &args, &mut config, &rt);
                        prompt::set_last_status(code);
                    }
                    Classification::HistorySearch(query) => {
                        if config.behavior.ai_enabled {
                            let history = read_recent_history(&history_path, 200);
                            match rt.block_on(ai::search_history(&query, &history, &config)) {
                                Ok(result) => println!("{result}"),
                                Err(e) => eprintln!("shako: history search failed: {e}"),
                            }
                        } else {
                            eprintln!("shako: AI is disabled (ai_enabled = false in config)");
                        }
                    }
                    Classification::ExplainCommand(cmd) => {
                        if !config.behavior.ai_enabled {
                            eprintln!("shako: AI is disabled (ai_enabled = false in config)");
                        } else {
                            let sp = spinner::Spinner::start("explaining...");
                            let result = rt.block_on(ai::explain_command(
                                &cmd,
                                &config,
                                Some(sp.stop_flag()),
                            ));
                            drop(sp);
                            match result {
                                Ok(explanation) => {
                                    print_styled_explain(&cmd, &explanation);
                                }
                                Err(e) => {
                                    eprintln!("shako: ai error: {e}");
                                }
                            }
                        }
                    }
                }

                last_command = input.to_string();
                timer.stop();
            }
            Ok(Signal::CtrlC) => {
                continue;
            }
            Ok(Signal::CtrlD) => {
                println!("exit");
                break;
            }
            Err(e) => {
                eprintln!("shako: input error: {e}");
                break;
            }
        }
    }
}

// ─── Helpers used by run_repl ─────────────────────────────────────────────────

fn set_exit_code(status: Option<std::process::ExitStatus>) {
    let code = status.and_then(|s| s.code()).unwrap_or(0);
    prompt::set_last_status(code);
}

/// Check if the input line needs continuation (trailing \, unclosed quotes,
/// or an unclosed if/for/while block).
pub fn needs_continuation(input: &str) -> bool {
    if input.ends_with('\\') {
        return true;
    }

    // Count unescaped quotes
    let mut in_single = false;
    let mut in_double = false;
    let mut prev_backslash = false;

    for c in input.chars() {
        if prev_backslash {
            prev_backslash = false;
            continue;
        }
        if c == '\\' {
            prev_backslash = true;
            continue;
        }
        if c == '\'' && !in_double {
            in_single = !in_single;
        } else if c == '"' && !in_single {
            in_double = !in_double;
        }
    }

    if in_single || in_double {
        return true;
    }

    // Detect open heredoc (<<MARKER without a matching MARKER terminator line).
    if heredoc_needs_continuation(input) {
        return true;
    }

    // Count unclosed control-flow blocks
    control_depth(input) > 0
}

/// Returns true when `input` contains a `<<MARKER` heredoc that has not yet
/// seen its terminator line.
pub fn heredoc_needs_continuation(input: &str) -> bool {
    // Quick exit: no heredoc operator
    if !input.contains("<<") {
        return false;
    }

    for (line_idx, line) in input.split('\n').enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i + 1 < chars.len() {
            if chars[i] == '<'
                && chars[i + 1] == '<'
                && !(i + 2 < chars.len() && chars[i + 2] == '<')
            {
                i += 2;
                if i < chars.len() && chars[i] == '-' {
                    i += 1;
                }
                while i < chars.len() && chars[i] == ' ' {
                    i += 1;
                }
                // Extract marker
                let mut marker = String::new();
                let quote_ch = if i < chars.len() && (chars[i] == '\'' || chars[i] == '"') {
                    let q = chars[i];
                    i += 1;
                    Some(q)
                } else {
                    None
                };
                while i < chars.len() {
                    let c = chars[i];
                    if let Some(qc) = quote_ch {
                        if c == qc {
                            break;
                        }
                    } else if c == ' ' || c == '\t' || c == ';' || c == '|' || c == '&' {
                        break;
                    }
                    marker.push(c);
                    i += 1;
                }
                if marker.is_empty() {
                    break;
                }
                let subsequent_lines: Vec<&str> =
                    input.split('\n').skip(line_idx + 1).collect();
                let found =
                    subsequent_lines.iter().any(|l| l.trim_start_matches('\t') == marker);
                if !found {
                    return true;
                }
                break;
            }
            i += 1;
        }
    }
    false
}

/// Count nesting depth of control-flow keywords in a (possibly partial) input.
/// Positive → needs more `fi`/`done` to close.
pub fn control_depth(input: &str) -> i32 {
    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    let mut seg_start = 0usize;
    let bytes = input.as_bytes();
    let mut i = 0usize;

    let mut check_seg = |seg: &str| {
        let first = seg.split_whitespace().next().unwrap_or("");
        match first {
            "if" | "for" | "while" => depth += 1,
            "end" | "fi" | "done" => depth -= 1,
            _ => {}
        }
    };

    while i < bytes.len() {
        let c = bytes[i] as char;
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ';' if !in_single && !in_double => {
                let seg = &input[seg_start..i];
                check_seg(seg);
                seg_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    let tail = &input[seg_start..];
    check_seg(tail);
    depth
}

/// Expand `!!` (last command) and `!$` (last arg of last command) in the input.
pub fn expand_history_bangs(input: &str, last_command: &str) -> String {
    if !input.contains('!') || last_command.is_empty() {
        return input.to_string();
    }

    let last_arg = last_command.split_whitespace().last().unwrap_or("");

    let mut result = input.replace("!!", last_command);
    result = result.replace("!$", last_arg);

    if result != input {
        eprintln!("\x1b[90m{result}\x1b[0m");
    }

    result
}

/// Read the last N lines from the history file for AI context.
///
/// When `dedup` is true, consecutive duplicate entries are removed.
pub fn read_recent_history(history_path: &Path, n: usize) -> Vec<String> {
    read_recent_history_with_dedup(history_path, n, true)
}

pub fn read_recent_history_with_dedup(
    history_path: &Path,
    n: usize,
    dedup: bool,
) -> Vec<String> {
    if n == 0 {
        return Vec::new();
    }
    match std::fs::read_to_string(history_path) {
        Ok(contents) => {
            let raw: Vec<&str> = contents.lines().collect();
            let lines: Vec<String> = if dedup {
                let mut deduped: Vec<&str> = Vec::with_capacity(raw.len());
                for line in &raw {
                    if deduped.last() != Some(line) {
                        deduped.push(line);
                    }
                }
                let start = deduped.len().saturating_sub(n);
                deduped[start..].iter().map(|l| l.to_string()).collect()
            } else {
                let start = raw.len().saturating_sub(n);
                raw[start..].iter().map(|l| l.to_string()).collect()
            };
            lines
        }
        Err(_) => Vec::new(),
    }
}

/// Returns true if `segment` should be dispatched to `run_builtin`.
/// A pure builtin call has no pipes and no unquoted redirect operators (> <).
pub fn is_pure_builtin_call(segment: &str) -> bool {
    if parser::split_pipes(segment).len() > 1 {
        return false;
    }
    let mut in_single = false;
    let mut in_double = false;
    for ch in segment.chars() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '>' | '<' if !in_single && !in_double => return false,
            _ => {}
        }
    }
    let first = segment.split_whitespace().next().unwrap_or("");
    builtins::is_builtin(first)
}

fn offer_ai_recovery(
    command: &str,
    exit_code: i32,
    stderr_output: &str,
    config: &ShakoConfig,
    rt: &Runtime,
    history_path: &Path,
) {
    // Don't offer for signals (128+) or trivial failures like grep no-match (1)
    if exit_code == 1 || exit_code > 128 {
        return;
    }

    // Styled error header panel
    let err_line = format!("\x1b[1;31m✗\x1b[0m exit {exit_code}  \x1b[90m{command}\x1b[0m");
    eprintln!(" \x1b[31m╷\x1b[0m {err_line}");
    print!(" \x1b[31m╰\x1b[0m \x1b[90mask AI for help? [y/N]\x1b[0m ");
    io::stdout().flush().ok();

    let mut answer = String::new();
    io::stdin().read_line(&mut answer).ok();
    let answer = answer.trim().to_lowercase();

    if answer != "y" && answer != "yes" {
        return;
    }

    let sp = spinner::Spinner::start("diagnosing...");

    let result = rt.block_on(async {
        let history = read_recent_history_with_dedup(
            history_path,
            config.behavior.history_context_lines,
            config.behavior.history_dedup,
        );
        ai::diagnose_error(command, exit_code, stderr_output, config, history).await
    });

    drop(sp);

    match result {
        Ok(response) => {
            let response = response.trim();

            let mut cause = String::new();
            let mut fix = String::new();

            for line in response.lines() {
                let line = line.trim();
                if let Some(c) = line.strip_prefix("CAUSE:") {
                    cause = c.trim().to_string();
                } else if let Some(f) = line.strip_prefix("FIX:") {
                    fix = f.trim().to_string();
                } else if !fix.is_empty() && !line.is_empty() && line != "SHAKO_NO_FIX" {
                    fix.push('\n');
                    fix.push_str(line);
                }
            }

            if !cause.is_empty() {
                eprintln!(" \x1b[36m╷\x1b[0m \x1b[90mcause:\x1b[0m {cause}");
            }

            if fix.is_empty() || fix == "SHAKO_NO_FIX" {
                return;
            }

            eprintln!(
                " \x1b[36m╷\x1b[0m \x1b[90mfix:\x1b[0m    \x1b[1;36m{fix}\x1b[0m"
            );
            print!(" \x1b[36m╰\x1b[0m \x1b[90m[Y]es  [n]o  [e]dit:\x1b[0m ");
            io::stdout().flush().ok();

            let mut answer = String::new();
            io::stdin().read_line(&mut answer).ok();
            let answer = answer.trim().to_lowercase();

            match answer.as_str() {
                "" | "y" | "yes" => {
                    for line in fix.lines() {
                        let line = line.trim();
                        if !line.is_empty() {
                            let status = executor::execute_command(line);
                            set_exit_code(status);
                        }
                    }
                }
                "e" | "edit" => {
                    print!(" \x1b[36m❯\x1b[0m ");
                    io::stdout().flush().ok();
                    let mut edited = String::new();
                    io::stdin().read_line(&mut edited).ok();
                    let edited = edited.trim();
                    if !edited.is_empty() {
                        let status = executor::execute_command(edited);
                        set_exit_code(status);
                    }
                }
                _ => {}
            }
        }
        Err(e) => {
            eprintln!("shako: ai error: {e}");
        }
    }
}

// ─── UI helpers (explain/banner) ─────────────────────────────────────────────

/// Print a styled panel for explain-mode output.
pub fn print_styled_explain(cmd: &str, explanation: &str) {
    const GRAD: &[u8] = &[30, 31, 32, 37, 38, 44, 45];
    let mid_color = GRAD[GRAD.len() / 2];

    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);

    let cmd_vis = cmd.len();
    let label = "explain";
    let label_vis = label.len() + 2;
    let content_width = cmd_vis.max(32);
    let inner_width = (content_width + 4).min(term_width.saturating_sub(2));

    let grad_line = |width: usize| -> String {
        (0..width)
            .map(|i| {
                let idx = if width <= 1 {
                    0
                } else {
                    i * (GRAD.len() - 1) / (width - 1)
                };
                format!("\x1b[38;5;{}m─\x1b[0m", GRAD[idx])
            })
            .collect()
    };

    let border = |c: char| format!("\x1b[38;5;{mid_color}m{c}\x1b[0m");

    let header_label = format!("\x1b[38;5;{mid_color}m {label} \x1b[0m");
    let rest_width = inner_width.saturating_sub(label_vis + 2);
    eprintln!(
        " {tl}{sep}{label}{rest}{tr}",
        tl = border('╭'),
        sep = grad_line(2),
        label = header_label,
        rest = grad_line(rest_width),
        tr = border('╮'),
    );

    let cmd_styled = format!("\x1b[1;36m{cmd}\x1b[0m");
    let content_inner = inner_width.saturating_sub(4);
    let pad = content_inner.saturating_sub(cmd_vis);
    eprintln!(
        " {b}  {cmd}{}  {b}",
        " ".repeat(pad),
        b = border('│'),
        cmd = cmd_styled,
    );

    eprintln!(
        " {bl}{bot}{br}",
        bl = border('╰'),
        bot = grad_line(inner_width),
        br = border('╯'),
    );

    for line in explanation.trim().lines() {
        eprintln!("  \x1b[38;5;{mid_color}m│\x1b[0m {line}");
    }
    eprintln!();
}
