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
use crate::env_context::{self, ContextTracker};
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

    // ── Environment drift detection ──────────────────────────────────────────
    // Create a context tracker seeded from the current config.
    let mut ctx_tracker = ContextTracker::new(
        config.behavior.context_warn_window_secs,
        config.behavior.production_contexts.clone(),
    );
    // Initialise the production-context prompt indicator.
    prompt::set_production_context_active(
        ctx_tracker.is_currently_production(&config.behavior.production_contexts),
    );

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

                // Handle `incident report` specially so we have access to rt and config.
                if input.trim() == "incident report" {
                    handle_incident_report(&mut state, &config, &rt);
                    last_command = input.to_string();
                    ctx_tracker.post_command();
                    prompt::set_production_context_active(
                        ctx_tracker.is_currently_production(&config.behavior.production_contexts),
                    );
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
                    ctx_tracker.post_command();
                    prompt::set_production_context_active(
                        ctx_tracker.is_currently_production(&config.behavior.production_contexts),
                    );
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
                    ctx_tracker.post_command();
                    prompt::set_production_context_active(
                        ctx_tracker.is_currently_production(&config.behavior.production_contexts),
                    );
                    continue;
                }

                // ── Environment drift check (pre-execution) ──────────────────
                // Only run when safety_mode != "off".
                if config.behavior.safety_mode != "off" {
                    if let Some(warning) =
                        ctx_tracker.check_command(&input, &config.behavior.production_contexts)
                    {
                        if !show_context_drift_warning(&warning, &input) {
                            // User aborted — skip execution, record the attempt.
                            last_command = input.to_string();
                            timer.stop();
                            ctx_tracker.post_command();
                            prompt::set_production_context_active(
                                ctx_tracker.is_currently_production(
                                    &config.behavior.production_contexts,
                                ),
                            );
                            continue;
                        }
                    }
                }

                match classifier.classify(&input) {
                    Classification::Command(cmd) => {
                        ran_foreground = true;
                        // Pre-execution snapshot for directly typed dangerous commands.
                        if config.behavior.safety_mode != "off" {
                            ai::maybe_take_snapshot(&cmd, &config);
                        }
                        let cmd_start = std::time::Instant::now();
                        let (status, stderr_output) =
                            executor::execute_command_with_stderr(&cmd);
                        let cmd_duration = cmd_start.elapsed();
                        set_exit_code(status);

                        // Record step in active incident session.
                        if let Some(ref mut session) = state.incident_session {
                            let exit_code = status.and_then(|s| s.code()).unwrap_or(0);
                            session.record(
                                cmd.as_str(),
                                exit_code,
                                &stderr_output,
                                cmd_duration,
                            );
                        }

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
                        let builtin_start = std::time::Instant::now();
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
                        // Record builtin step in active incident session
                        // (skip `incident` itself to avoid meta-noise).
                        let first_token = cmd.split_whitespace().next().unwrap_or("");
                        if first_token != "incident" {
                            if let Some(ref mut session) = state.incident_session {
                                session.record(
                                    cmd.as_str(),
                                    last_code,
                                    "",
                                    builtin_start.elapsed(),
                                );
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
                        match slash::run(&name, &args, &mut config, &rt, &history_path) {
                            slash::SlashOutcome::Code(code) => {
                                prompt::set_last_status(code);
                            }
                            slash::SlashOutcome::Prefill(selected) => {
                                // The user picked a history entry.  Show it in
                                // a styled panel with [Y/n/e] so they can run,
                                // cancel, or edit it before execution.
                                let ran = offer_history_selection(
                                    &selected,
                                    &mut state,
                                    &history_path,
                                    &config,
                                    &rt,
                                );
                                if !ran {
                                    prompt::set_last_status(0);
                                }
                            }
                        }
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
                    Classification::UndoRequest(query) => {
                        match ai::handle_undo_request(&query, &config) {
                            Ok(true) => prompt::set_last_status(0),
                            Ok(false) => prompt::set_last_status(0),
                            Err(e) => {
                                eprintln!("shako: undo error: {e}");
                                prompt::set_last_status(1);
                            }
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

                // ── Post-command: update context snapshot ────────────────────
                ctx_tracker.post_command();
                prompt::set_production_context_active(
                    ctx_tracker.is_currently_production(&config.behavior.production_contexts),
                );

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

/// Show a styled confirm panel for a history-picked command.
///
/// Prints the selected command in a box and prompts `[Y/n/e]`:
/// - `Y` / Enter — run it immediately
/// - `n` — cancel
/// - `e` — edit: re-prompt the user with the command pre-printed so they can
///          retype/modify it (limited to one-shot since we cannot seed reedline)
///
/// Returns `true` if the command was executed.
fn offer_history_selection(
    cmd: &str,
    state: &mut crate::builtins::ShellState,
    _history_path: &Path,
    _config: &ShakoConfig,
    _rt: &Runtime,
) -> bool {
    use std::io::Write;

    const GRAD: &[u8] = &[30, 31, 32, 37, 38, 44, 45];
    let mid_color = GRAD[GRAD.len() / 2];
    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);

    let grad_line = |width: usize| -> String {
        (0..width)
            .map(|i| {
                let idx = if width <= 1 { 0 } else { i * (GRAD.len() - 1) / (width - 1) };
                format!("\x1b[38;5;{}m─\x1b[0m", GRAD[idx])
            })
            .collect()
    };
    let border = |c: char| format!("\x1b[38;5;{mid_color}m{c}\x1b[0m");

    let visible_len = |s: &str| -> usize {
        let mut len = 0;
        let mut in_esc = false;
        for c in s.chars() {
            if c == '\x1b' { in_esc = true; }
            else if in_esc { if c.is_ascii_alphabetic() { in_esc = false; } }
            else { len += 1; }
        }
        len
    };

    let cmd_styled = format!("\x1b[1;36m{cmd}\x1b[0m");
    let opts_styled = "\x1b[90m[Y]es  [n]o  [e]dit\x1b[0m";
    let cmd_vis = cmd.len();
    let opts_vis = visible_len(opts_styled);
    let content_width = cmd_vis.max(opts_vis).max(36);
    let inner_width = (content_width + 4).min(term_width.saturating_sub(2));
    let content_inner = inner_width.saturating_sub(4);

    let box_line = |content: &str| -> String {
        let vl = visible_len(content);
        let pad = content_inner.saturating_sub(vl);
        format!(" {}  {}{}  {}", border('│'), content, " ".repeat(pad), border('│'))
    };

    let label = format!("\x1b[38;5;{mid_color}m history \x1b[0m");
    let label_vis = 10usize;
    let rest_width = inner_width.saturating_sub(label_vis + 2);

    eprintln!(
        " {tl}{sep}{label}{rest}{tr}",
        tl = border('╭'),
        sep = grad_line(2),
        rest = grad_line(rest_width),
        tr = border('╮'),
    );
    eprintln!("{}", box_line(&cmd_styled));
    eprintln!(
        " {bl}{sep}{br}",
        bl = border('├'),
        sep = grad_line(inner_width),
        br = border('┤'),
    );

    let opts_pad = content_inner.saturating_sub(opts_vis);
    eprint!(
        " {}  {}{}  {} ",
        border('│'),
        opts_styled,
        " ".repeat(opts_pad),
        border('│'),
    );
    io::stdout().flush().ok();

    let mut answer = String::new();
    io::stdin().read_line(&mut answer).ok();
    let answer = answer.trim().to_lowercase();

    eprintln!(
        " {bl}{bot}{br}",
        bl = border('╰'),
        bot = grad_line(inner_width),
        br = border('╯'),
    );

    match answer.as_str() {
        "" | "y" | "yes" => {
            // Run through the normal executor / builtin dispatch.
            let first = cmd.split_whitespace().next().unwrap_or("");
            if crate::builtins::is_builtin(first) {
                let code = crate::builtins::run_builtin(cmd, state);
                prompt::set_last_status(code);
            } else {
                let status = executor::execute_command(cmd);
                set_exit_code(status);
            }
            true
        }
        "e" | "edit" => {
            // Print the command so the user can see it, then open a plain
            // prompt pre-annotated with the command text for manual editing.
            eprintln!("\x1b[90m  (edit then press Enter)\x1b[0m");
            eprint!("  \x1b[38;5;{mid_color}m❯\x1b[0m {cmd}");
            io::stdout().flush().ok();
            // We can't seed reedline here, so we read a raw line from stdin.
            let mut edited = String::new();
            io::stdin().read_line(&mut edited).ok();
            let edited = edited.trim();
            if !edited.is_empty() {
                let first = edited.split_whitespace().next().unwrap_or("");
                if crate::builtins::is_builtin(first) {
                    let code = crate::builtins::run_builtin(edited, state);
                    prompt::set_last_status(code);
                } else {
                    let status = executor::execute_command(edited);
                    set_exit_code(status);
                }
                true
            } else {
                false
            }
        }
        _ => false,
    }
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

    // ── styled error header panel ────────────────────────────────────────────
    const GRAD: &[u8] = &[30, 31, 32, 37, 38, 44, 45];
    let mid_color = GRAD[GRAD.len() / 2];

    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);

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

    let visible_len = |s: &str| -> usize {
        let mut len = 0;
        let mut in_esc = false;
        for c in s.chars() {
            if c == '\x1b' {
                in_esc = true;
            } else if in_esc {
                if c.is_ascii_alphabetic() {
                    in_esc = false;
                }
            } else {
                len += 1;
            }
        }
        len
    };

    let err_styled = format!(
        "\x1b[1;31m✗\x1b[0m \x1b[90mexit {exit_code}\x1b[0m  \x1b[1m{command}\x1b[0m"
    );
    let hint_styled = "\x1b[90mask AI for help? [y/N]\x1b[0m";

    let err_vis = visible_len(&err_styled);
    let hint_vis = visible_len(hint_styled);
    let content_width = err_vis.max(hint_vis).max(36);
    let inner_width = (content_width + 4).min(term_width.saturating_sub(2));
    let content_inner = inner_width.saturating_sub(4);

    let box_line = |content: &str| -> String {
        let vl = visible_len(content);
        let pad = content_inner.saturating_sub(vl);
        format!(
            " {b}  {content}{}  {b}",
            " ".repeat(pad),
            b = border('│')
        )
    };

    let label = format!("\x1b[38;5;{mid_color}m error \x1b[0m");
    let label_vis = 8usize;
    let rest_width = inner_width.saturating_sub(label_vis + 2);

    eprintln!(
        " {tl}{sep}{label}{rest}{tr}",
        tl = border('╭'),
        sep = grad_line(2),
        label = label,
        rest = grad_line(rest_width),
        tr = border('╮'),
    );
    eprintln!("{}", box_line(&err_styled));
    eprintln!(
        " {bl}{sep}{br}",
        bl = border('├'),
        sep = grad_line(inner_width),
        br = border('┤'),
    );

    // Print the ask-AI prompt inside the panel
    let ask_pad = content_inner.saturating_sub(hint_vis);
    eprint!(
        " {b}  {hint}{}  {b} ",
        " ".repeat(ask_pad),
        b = border('│'),
        hint = hint_styled,
    );
    io::stdout().flush().ok();

    let mut answer = String::new();
    io::stdin().read_line(&mut answer).ok();
    let answer = answer.trim().to_lowercase();

    eprintln!(
        " {bl}{bot}{br}",
        bl = border('╰'),
        bot = grad_line(inner_width),
        br = border('╯'),
    );

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
        Ok(diagnosis) => {
            // Show the cause to orient the user even if there's no fix.
            if !diagnosis.explanation.is_empty() {
                eprintln!("\x1b[90mshako: cause: {}\x1b[0m", diagnosis.explanation);
            }

            let fix = match diagnosis.suggested_command {
                Some(ref cmd) => cmd.clone(),
                None => {
                    // AI could not suggest a corrective command.
                    return;
                }
            };

            // Route the suggested fix through the shared confirm_command loop.
            // This gives the user the full [Y]es / [n]o / [e]dit / [w]hy / [r]efine
            // panel — identical UX to a normal AI-translated command.
            let mut current_fix = fix.clone();
            loop {
                match ai::confirm::confirm_command(&current_fix) {
                    Ok(ai::confirm::ConfirmAction::Execute) => {
                        ai::maybe_take_snapshot(&current_fix, config);
                        let status = executor::execute_command(&current_fix);
                        set_exit_code(status);
                        break;
                    }
                    Ok(ai::confirm::ConfirmAction::Edit(edited)) => {
                        ai::maybe_take_snapshot(&edited, config);
                        let status = executor::execute_command(&edited);
                        set_exit_code(status);
                        break;
                    }
                    Ok(ai::confirm::ConfirmAction::Cancel) => break,
                    Ok(ai::confirm::ConfirmAction::Why) => {
                        // Explain why the *original* failed command broke —
                        // reuse the cause line we already have.
                        if !diagnosis.explanation.is_empty() {
                            eprintln!("\x1b[90m{}\x1b[0m", diagnosis.explanation);
                        } else {
                            eprintln!("\x1b[90mNo additional explanation available.\x1b[0m");
                        }
                        // loop continues — re-shows the confirm panel
                    }
                    Ok(ai::confirm::ConfirmAction::Refine) => {
                        // Refine is not meaningful in error-recovery context;
                        // re-show the panel so the user can choose another option.
                    }
                    Err(_) => break,
                }
                // Refresh current_fix in case the user edits inline; since
                // ConfirmAction::Edit already breaks, this is a no-op for now
                // but guards against future loop changes.
                current_fix = fix.clone();
            }
        }
        Err(e) => {
            eprintln!("shako: ai error: {e}");
        }
    }
}

// ─── Environment drift warning ────────────────────────────────────────────────

/// Show a styled context-drift warning panel and prompt the user to confirm.
///
/// Returns `true` if the user chose to proceed, `false` to abort.
fn show_context_drift_warning(warning: &env_context::ContextWarning<'_>, command: &str) -> bool {
    use std::io::Write;

    // ── colour palette matching the rest of the shell UI ──────────────────
    // Amber gradient for production risk.
    const GRAD: &[u8] = &[208, 214, 220, 226, 220, 214, 208];
    let mid_color = GRAD[GRAD.len() / 2];
    let border_color = 214u8; // amber

    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);

    let border = |c: char| format!("\x1b[38;5;{border_color}m{c}\x1b[0m");

    let grad_line = |width: usize| -> String {
        (0..width)
            .map(|i| {
                let idx = if width <= 1 { 0 } else { i * (GRAD.len() - 1) / (width - 1) };
                format!("\x1b[38;5;{}m─\x1b[0m", GRAD[idx])
            })
            .collect()
    };

    let visible_len = |s: &str| -> usize {
        let mut len = 0usize;
        let mut in_esc = false;
        for c in s.chars() {
            if c == '\x1b' { in_esc = true; }
            else if in_esc { if c.is_ascii_alphabetic() { in_esc = false; } }
            else { len += 1; }
        }
        len
    };

    // ── Compose warning lines ─────────────────────────────────────────────
    let kind_label = warning.kind.label();
    let switched_ago = env_context::format_duration(warning.switched_ago);
    let from_label = warning.switch.from.label().unwrap_or_else(|| "unknown".to_string());
    let to_label = warning.switch.to.label().unwrap_or_else(|| "unknown".to_string());

    let context_line = format!(
        "\x1b[1;38;5;214m⚠\x1b[0m  {kind_label} context switched \x1b[1m{switched_ago}\x1b[0m ago\
         \x1b[90m  {from_label} → {to_label}\x1b[0m"
    );
    let command_line = format!("   \x1b[1m{command}\x1b[0m \x1b[38;5;196mwill run in PRODUCTION\x1b[0m");
    let opts_line = "\x1b[90m[y]es  [n]o  (default: abort)\x1b[0m";

    let lines = [context_line.as_str(), command_line.as_str()];
    let max_vis = lines
        .iter()
        .chain(std::iter::once(&opts_line))
        .map(|s| visible_len(s))
        .max()
        .unwrap_or(40);

    let content_width = max_vis.max(40);
    let inner_width = (content_width + 4).min(term_width.saturating_sub(2));
    let content_inner = inner_width.saturating_sub(4);

    let box_line = |content: &str| -> String {
        let vl = visible_len(content);
        let pad = content_inner.saturating_sub(vl);
        format!(" {}  {}{}  {}", border('│'), content, " ".repeat(pad), border('│'))
    };

    let label = format!("\x1b[38;5;{mid_color}m context drift \x1b[0m");
    let label_vis = 16usize;
    let rest_width = inner_width.saturating_sub(label_vis + 2);

    eprintln!(
        " {tl}{sep}{label}{rest}{tr}",
        tl = border('╭'),
        sep = grad_line(2),
        rest = grad_line(rest_width),
        tr = border('╮'),
    );
    for l in &lines {
        eprintln!("{}", box_line(l));
    }
    eprintln!(
        " {bl}{sep}{br}",
        bl = border('├'),
        sep = grad_line(inner_width),
        br = border('┤'),
    );

    let opts_vis = visible_len(opts_line);
    let opts_pad = content_inner.saturating_sub(opts_vis);
    eprint!(
        " {}  {}{}  {} ",
        border('│'),
        opts_line,
        " ".repeat(opts_pad),
        border('│'),
    );
    io::stdout().flush().ok();

    let mut answer = String::new();
    io::stdin().read_line(&mut answer).ok();
    let answer = answer.trim().to_lowercase();

    eprintln!(
        " {bl}{bot}{br}",
        bl = border('╰'),
        bot = grad_line(inner_width),
        br = border('╯'),
    );

    answer == "y" || answer == "yes"
}

// ─── Incident report handler ──────────────────────────────────────────────────

/// Handle `incident report` with access to the AI runtime and config.
///
/// Ends the active session, emits the step journal, then calls the AI
/// to generate a structured post-mortem runbook.
fn handle_incident_report(state: &mut ShellState, config: &ShakoConfig, rt: &Runtime) {
    use crate::incident;

    let session = match state.incident_session.take() {
        Some(s) => s,
        None => {
            eprintln!("shako: no active incident session");
            return;
        }
    };

    prompt::set_incident_active(false);

    eprintln!(
        "\x1b[1;31m⚡\x1b[0m Incident {} ended after {} ({} steps).",
        session.id(),
        session.elapsed_display(),
        session.steps.len()
    );

    if session.steps.is_empty() {
        eprintln!("  No steps were recorded — nothing to report.");
        return;
    }

    let step_log = session.step_log();
    let incident_name = session.name.clone();
    let incident_id = session.id();

    // Print the raw step journal.
    eprintln!("\n\x1b[90m─── Step Journal ───────────────────────────────────\x1b[0m");
    for line in step_log.lines() {
        eprintln!("  \x1b[90m{line}\x1b[0m");
    }
    eprintln!("\x1b[90m────────────────────────────────────────────────────\x1b[0m\n");

    // If AI is enabled, generate an enhanced runbook.
    let report = if config.behavior.ai_enabled {
        let sp = crate::spinner::Spinner::start("generating runbook...");
        let result = rt.block_on(ai::generate_incident_runbook(
            &incident_name,
            &step_log,
            config,
        ));
        drop(sp);
        match result {
            Ok(runbook) => runbook,
            Err(e) => {
                eprintln!("shako: AI runbook generation failed: {e}");
                eprintln!("  Falling back to plain step log report.");
                incident::build_markdown_report(&incident_id, &incident_name, &step_log)
            }
        }
    } else {
        incident::build_markdown_report(&incident_id, &incident_name, &step_log)
    };

    // Emit the report to stdout.
    println!("{report}");

    // Auto-save to configured runbook_dir.
    if let Some(save_path) = incident_save_path(&incident_id) {
        match std::fs::write(&save_path, &report) {
            Ok(_) => eprintln!("\x1b[90m  Saved: {}\x1b[0m", save_path.display()),
            Err(e) => eprintln!("shako: could not save runbook: {e}"),
        }
    }
}

/// Determine where to save the runbook markdown file.
/// Reads `.shako.toml` in the current directory for `[incident] runbook_dir`.
fn incident_save_path(incident_id: &str) -> Option<std::path::PathBuf> {
    use std::io::BufRead;
    let toml_path = std::env::current_dir().ok()?.join(".shako.toml");
    if !toml_path.exists() {
        return None;
    }
    let file = std::fs::File::open(&toml_path).ok()?;
    let reader = std::io::BufReader::new(file);
    let mut in_section = false;
    let mut runbook_dir: Option<String> = None;
    for line in reader.lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed == "[incident]" {
            in_section = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_section = false;
        }
        if in_section {
            if let Some(val) = trimmed.strip_prefix("runbook_dir") {
                let val = val.trim_start_matches(|c: char| c == ' ' || c == '=');
                let val = val.trim().trim_matches('"').trim_matches('\'');
                runbook_dir = Some(val.to_string());
            }
        }
    }
    let dir_str = runbook_dir?;
    let dir_str = if dir_str.starts_with('~') {
        let home = std::env::var("HOME").unwrap_or_default();
        dir_str.replacen('~', &home, 1)
    } else {
        dir_str
    };
    let dir = std::path::PathBuf::from(dir_str);
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(format!("{incident_id}.md")))
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
