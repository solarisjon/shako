use std::io::{self, Write};
use std::time::Instant;

use anyhow::Result;
use nu_ansi_term::{Color as NuColor, Style as NuStyle};
use reedline::{
    ColumnarMenu, EditMode, Emacs, FileBackedHistory, KeyCode, KeyModifiers, MenuBuilder, Reedline,
    ReedlineEvent, ReedlineMenu, Signal, Vi, default_emacs_keybindings,
    default_vi_insert_keybindings, default_vi_normal_keybindings,
};

mod ai;
mod builtins;
mod classifier;
mod config;
mod control;
mod executor;
mod fish_import;
mod learned_prefs;
mod parser;
mod path_cache;
mod proactive;
mod safety;
mod setup;
mod shell;
mod slash;
mod smart_defaults;
mod spinner;

use builtins::ShellState;
use classifier::{Classification, Classifier};
use config::ShakoConfig;
use shell::prompt::{self, CommandTimer, StarshipPrompt};

fn main() -> Result<()> {
    let t_total = Instant::now();

    let args: Vec<String> = std::env::args().collect();
    let quiet = args.iter().any(|a| a == "--quiet" || a == "-q");
    let init = args.iter().any(|a| a == "--init");
    let timings = args.iter().any(|a| a == "--timings");
    let cmd_mode = args
        .iter()
        .position(|a| a == "-c")
        .map(|i| args.get(i + 1).cloned().unwrap_or_default());

    env_logger::init();

    if init {
        eprintln!("\x1b[1;36mshako:\x1b[0m reinitializing...");
        if let Err(e) = ShakoConfig::reset() {
            eprintln!("shako: reset failed: {e}");
            std::process::exit(1);
        }
        eprintln!();
    }
    // Tell Starship which shell is running so its shell module displays correctly.
    // Safety: called at startup before any threads exist.
    unsafe { std::env::set_var("STARSHIP_SHELL", "shako") };
    // Ensure PWD reflects the real cwd at startup — Starship and subprocesses read it.
    if let Ok(cwd) = std::env::current_dir() {
        unsafe { std::env::set_var("PWD", cwd) };
    }

    // Create ~/.config/shako/starship.toml (merging user's global config) so Starship's
    // shell module shows "shako" instead of nothing/generic. Set STARSHIP_CONFIG so
    // all starship invocations from this session use it.
    let shako_config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .ok()
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .map(|d| d.join("shako"));
    if let Some(ref dir) = shako_config_dir {
        if let Some(starship_cfg) = setup::ensure_starship_config(dir) {
            // Safety: called at startup before any threads exist.
            unsafe { std::env::set_var("STARSHIP_CONFIG", starship_cfg) };
        }
    }

    // Non-interactive mode: shako -c "command" — skip wizard, just execute.
    // Use proper chain-aware dispatch so builtins work the same as interactive mode.
    if let Some(cmd_str) = cmd_mode {
        if cmd_str.is_empty() {
            eprintln!("shako: -c: option requires an argument");
            std::process::exit(2);
        }
        let mut state = ShellState::new(std::path::PathBuf::new());
        let last_code;

        if control::has_control_flow(&cmd_str) {
            let stmts = control::parse_body(&cmd_str);
            let mut locals = Vec::new();
            last_code = match control::exec_statements(&stmts, &mut locals) {
                control::ExecSignal::Normal(c) | control::ExecSignal::Return(c) => c,
                _ => 0,
            };
        } else {
            let mut code = 0i32;
            let chains = parser::split_chains(&cmd_str);
            let mut prev_op = parser::ChainOp::None;
            for (segment, op) in &chains {
                let should_run = match prev_op {
                    parser::ChainOp::None | parser::ChainOp::Semi => true,
                    parser::ChainOp::And => code == 0,
                    parser::ChainOp::Or => code != 0,
                };
                if should_run {
                    code = if is_pure_builtin_call(segment) {
                        builtins::run_builtin(segment, &mut state)
                    } else {
                        let status = executor::execute_command(segment);
                        status.and_then(|s| s.code()).unwrap_or(0)
                    };
                }
                prev_op = *op;
            }
            last_code = code;
        }
        std::process::exit(last_code);
    }

    let t_phase = Instant::now();
    let (mut config, first_run) = ShakoConfig::load()?;
    let rt = tokio::runtime::Runtime::new()?;
    let dt_config = t_phase.elapsed();

    let dt_ai_check;
    if !quiet {
        let t_phase = Instant::now();
        let ai_status = rt.block_on(ai::client::check_ai_session(
            config.active_llm(),
            config.behavior.ai_enabled,
        ));
        dt_ai_check = t_phase.elapsed();
        print_styled_banner(&config, &ai_status);
    } else {
        dt_ai_check = std::time::Duration::ZERO;
    }

    if first_run {
        setup::check_recommended_tools();
    }

    let t_phase = Instant::now();
    let path_cache = path_cache::PathCache::new();
    let classifier = Classifier::new(path_cache.clone());
    let dt_path_scan = t_phase.elapsed();

    let t_phase = Instant::now();
    let highlighter = shell::highlighter::ShakoHighlighter::new(path_cache.clone());
    let extra_completions: std::sync::Arc<std::sync::RwLock<Vec<String>>> =
        std::sync::Arc::new(std::sync::RwLock::new(vec![]));
    let completer = shell::completer::ShakoCompleter::new(
        path_cache,
        std::sync::Arc::clone(&extra_completions),
    );
    let hinter = shell::hinter::create_hinter();

    let history_path = dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("shako")
        .join("history.txt");

    let history = Box::new(
        FileBackedHistory::with_file(10_000, history_path.clone()).unwrap_or_else(|e| {
            eprintln!("shako: history: {e}, using in-memory only");
            FileBackedHistory::new(1000).expect("failed to create history")
        }),
    );

    // Themed completion menu — teal/cyan selection matching the brand gradient.
    let completion_menu = Box::new(
        ColumnarMenu::default()
            .with_name("completion_menu")
            .with_columns(4)
            .with_column_padding(2)
            // Normal suggestions: mid-gray, unobtrusive
            .with_text_style(NuColor::Fixed(242).normal())
            // Selected suggestion: bold teal — stands out without being harsh
            .with_selected_text_style(NuStyle::new().bold().fg(NuColor::Fixed(38)))
            // Description text: subtle dark-teal
            .with_description_text_style(NuColor::Fixed(30).normal())
            // Match highlight in unselected items: underlined teal
            .with_match_text_style(NuStyle::new().underline().fg(NuColor::Fixed(38)))
            // Match highlight in selected item: underlined bright cyan
            .with_selected_match_text_style(NuStyle::new().bold().underline().fg(NuColor::Fixed(45))),
    );

    let tab_completion_binding = ReedlineEvent::UntilFound(vec![
        ReedlineEvent::Menu("completion_menu".to_string()),
        ReedlineEvent::MenuNext,
    ]);

    let edit_mode: Box<dyn EditMode> = if config.behavior.edit_mode == "vi" {
        // Add Tab completion to vi insert mode — Vi::default() has no completion binding.
        let mut insert_kb = default_vi_insert_keybindings();
        insert_kb.add_binding(
            KeyModifiers::NONE,
            KeyCode::Tab,
            tab_completion_binding.clone(),
        );
        insert_kb.add_binding(
            KeyModifiers::SHIFT,
            KeyCode::BackTab,
            ReedlineEvent::MenuPrevious,
        );
        Box::new(Vi::new(insert_kb, default_vi_normal_keybindings()))
    } else {
        let mut keybindings = default_emacs_keybindings();
        keybindings.add_binding(KeyModifiers::NONE, KeyCode::Tab, tab_completion_binding);
        keybindings.add_binding(
            KeyModifiers::SHIFT,
            KeyCode::BackTab,
            ReedlineEvent::MenuPrevious,
        );
        Box::new(Emacs::new(keybindings))
    };

    let mut line_editor = Reedline::create()
        .with_history(history)
        .with_highlighter(Box::new(highlighter))
        .with_completer(Box::new(completer))
        .with_hinter(Box::new(hinter))
        .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
        .with_edit_mode(edit_mode);

    let prompt = StarshipPrompt::new();
    let mut state = ShellState::new(history_path.clone());
    let dt_reedline = t_phase.elapsed();

    // Interactive shell signal setup.
    //
    // The shell ignores the keyboard/job-control signals so that:
    //   • Ctrl-C  doesn't kill the shell while it's waiting on a child
    //   • Ctrl-\  doesn't dump core from the shell
    //   • Ctrl-Z  doesn't suspend the shell itself
    //   • SIGTTOU/SIGTTIN don't stop the shell when it calls tcsetpgrp
    //
    // Children have their signal dispositions reset to SIG_DFL in
    // setup_child_signals() (executor.rs), and the terminal is handed to each
    // foreground process group via tcsetpgrp so that the signals reach them.
    //
    // SIGTERM keeps its default disposition so that `kill <shell_pid>` still
    // terminates the shell cleanly.
    #[cfg(unix)]
    {
        use nix::sys::signal::{SigHandler, Signal, signal};
        unsafe {
            signal(Signal::SIGINT, SigHandler::SigIgn).ok();
            signal(Signal::SIGQUIT, SigHandler::SigIgn).ok();
            signal(Signal::SIGTSTP, SigHandler::SigIgn).ok();
            signal(Signal::SIGTTOU, SigHandler::SigIgn).ok();
            signal(Signal::SIGTTIN, SigHandler::SigIgn).ok();
        }
    }

    // Resolve the shako config directory (used for conf.d, functions, init)
    let shako_config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .ok()
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .map(|d| d.join("shako"));

    // Load aliases from config.toml
    for (name, value) in &config.aliases {
        state.aliases.insert(name.clone(), value.clone());
    }

    // Apply smart defaults (modern tools), user config takes priority
    let t_phase = Instant::now();
    let smart_aliases = smart_defaults::detect_smart_defaults(&state.aliases);
    for (name, value) in smart_aliases {
        state.aliases.entry(name).or_insert(value);
    }
    let dt_smart_defaults = t_phase.elapsed();

    // Track shell nesting level
    let shlvl: i32 = std::env::var("SHLVL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    // Safety: called before the tokio runtime is started and before any threads
    // exist; no concurrent readers of the process environment.
    unsafe { std::env::set_var("SHLVL", (shlvl + 1).to_string()) };

    // ── Fish-like startup order ──────────────────────────────────────
    //  1. Source ~/.config/shako/conf.d/*.{fish,sh}  (config snippets)
    //  2. Source ~/.config/shako/config.shako         (main user config)
    //  3. Load  ~/.config/shako/functions/            (autoloaded functions)
    //  4. Optionally source fish config               (if [fish] source_config = true)

    let t_phase = Instant::now();
    if let Some(ref dir) = shako_config_dir {
        // 1. conf.d/ — config snippets sourced alphabetically
        let conf_d = dir.join("conf.d");
        if conf_d.is_dir() {
            builtins::source_conf_d(&conf_d, &mut state);
        }

        // 2. config.shako — main user config (with backward compat)
        let config_shako = dir.join("config.shako");
        let init_sh = dir.join("init.sh");
        let init_fish = dir.join("init.fish");

        if config_shako.exists() {
            builtins::run_builtin(&format!("source {}", config_shako.display()), &mut state);
        } else if init_sh.exists() {
            builtins::run_builtin(&format!("source {}", init_sh.display()), &mut state);
        } else if init_fish.exists() {
            builtins::run_builtin(&format!("source {}", init_fish.display()), &mut state);
        }

        // 3. functions/ — autoloaded function files (lazy-loaded on call,
        //    but we also do an eager scan to register names)
        let functions_dir = dir.join("functions");
        if functions_dir.is_dir() {
            builtins::load_functions_dir(&functions_dir, &mut state);
            state.functions_dir = Some(functions_dir);
        }
    }

    // 4. Source fish config if enabled (reuse existing fish setup)
    if config.fish.source_config {
        let fish_config_dir = dirs::home_dir().map(|h| h.join(".config").join("fish"));

        if let Some(ref fish_dir) = fish_config_dir {
            // Fish conf.d/ snippets first
            let fish_conf_d = fish_dir.join("conf.d");
            if fish_conf_d.is_dir() {
                builtins::source_conf_d(&fish_conf_d, &mut state);
            }

            // Then config.fish
            let config_fish = fish_dir.join("config.fish");
            if config_fish.exists() {
                if let Ok(contents) = std::fs::read_to_string(&config_fish) {
                    builtins::source_fish_string(&contents, &mut state);
                }
            }

            // Load fish functions (don't overwrite shako-defined ones)
            let fish_functions = fish_dir.join("functions");
            if fish_functions.is_dir() {
                builtins::load_functions_dir(&fish_functions, &mut state);
            }
        }
    }

    let dt_shell_init = t_phase.elapsed();

    let dt_total = t_total.elapsed();

    if timings {
        eprintln!("\x1b[1mstartup timings\x1b[0m");
        eprintln!(
            "  config load      {:>7.1}ms",
            dt_config.as_secs_f64() * 1000.0
        );
        eprintln!(
            "  ai session check {:>7.1}ms",
            dt_ai_check.as_secs_f64() * 1000.0
        );
        eprintln!(
            "  PATH scan        {:>7.1}ms",
            dt_path_scan.as_secs_f64() * 1000.0
        );
        eprintln!(
            "  reedline setup   {:>7.1}ms",
            dt_reedline.as_secs_f64() * 1000.0
        );
        eprintln!(
            "  smart defaults   {:>7.1}ms",
            dt_smart_defaults.as_secs_f64() * 1000.0
        );
        eprintln!(
            "  shell init       {:>7.1}ms",
            dt_shell_init.as_secs_f64() * 1000.0
        );
        eprintln!("  ─────────────────────────");
        eprintln!(
            "  total            {:>7.1}ms",
            dt_total.as_secs_f64() * 1000.0
        );
        eprintln!();
    }
    log::info!(
        "startup: config={:.1}ms ai_check={:.1}ms path_scan={:.1}ms reedline={:.1}ms smart_defaults={:.1}ms shell_init={:.1}ms total={:.1}ms",
        dt_config.as_secs_f64() * 1000.0,
        dt_ai_check.as_secs_f64() * 1000.0,
        dt_path_scan.as_secs_f64() * 1000.0,
        dt_reedline.as_secs_f64() * 1000.0,
        dt_smart_defaults.as_secs_f64() * 1000.0,
        dt_shell_init.as_secs_f64() * 1000.0,
        dt_total.as_secs_f64() * 1000.0,
    );

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

                // Multiline continuation: trailing \ or unclosed quotes
                while needs_continuation(&input) {
                    let cont_prompt = reedline::DefaultPrompt::new(
                        reedline::DefaultPromptSegment::Basic("... ".to_string()),
                        reedline::DefaultPromptSegment::Empty,
                    );
                    match line_editor.read_line(&cont_prompt) {
                        Ok(Signal::Success(next)) => {
                            if input.ends_with('\\') {
                                input.pop(); // remove trailing backslash — join as one token
                                input.push(' ');
                            } else {
                                // Treat each continuation line as a new statement so
                                // that keywords like `done`/`fi` form their own segment
                                // and are recognised by control_depth / split_semicolons.
                                input.push_str("; ");
                            }
                            input.push_str(next.trim());
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

                if control::has_control_flow(&input) {
                    let stmts = control::parse_body(&input);
                    let mut locals = Vec::new();
                    let code = match control::exec_statements(&stmts, &mut locals) {
                        control::ExecSignal::Normal(c) | control::ExecSignal::Return(c) => c,
                        _ => 0,
                    };
                    prompt::set_last_status(code);
                    last_command = input.to_string();
                    timer.stop();
                    continue;
                }

                // Check if first token is a shell function (including autoload)
                // (timer was already started before the control-flow check above)
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
                        let (status, stderr_output) = executor::execute_command_with_stderr(&cmd);
                        set_exit_code(status);

                        // If the foreground process was stopped by Ctrl-Z,
                        // add it to the jobs list.
                        if let Some(stopped) = executor::take_stopped_job() {
                            // Use the actual command name instead of the generic "(pid N)".
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
                        // For segments with pipes or redirects, fall through to
                        // executor which handles them (so `echo hi | tr a-z A-Z` works).
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
                        } // end ai_enabled check
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
                                let first = suggestion.split_whitespace().next().unwrap_or("");
                                if builtins::is_builtin(first) {
                                    let code = builtins::run_builtin(&suggestion, &mut state);
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
                        } // end ai_enabled check
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

    Ok(())
}

fn set_exit_code(status: Option<std::process::ExitStatus>) {
    let code = status.and_then(|s| s.code()).unwrap_or(0);
    prompt::set_last_status(code);
}

fn offer_ai_recovery(
    command: &str,
    exit_code: i32,
    stderr_output: &str,
    config: &ShakoConfig,
    rt: &tokio::runtime::Runtime,
    history_path: &std::path::Path,
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
        let history = read_recent_history(history_path, config.behavior.history_context_lines);
        ai::diagnose_error(command, exit_code, stderr_output, config, history).await
    });

    drop(sp);

    match result {
        Ok(response) => {
            let response = response.trim();

            // Parse CAUSE and FIX from response
            let mut cause = String::new();
            let mut fix = String::new();

            for line in response.lines() {
                let line = line.trim();
                if let Some(c) = line.strip_prefix("CAUSE:") {
                    cause = c.trim().to_string();
                } else if let Some(f) = line.strip_prefix("FIX:") {
                    fix = f.trim().to_string();
                } else if !fix.is_empty() && !line.is_empty() && line != "SHAKO_NO_FIX" {
                    // Multi-line fix
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

            // Show suggested fix and offer to run it
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

/// Check if the input line needs continuation (trailing \, unclosed quotes,
/// or an unclosed if/for/while block).
fn needs_continuation(input: &str) -> bool {
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

    // Count unclosed control-flow blocks
    control_depth(input) > 0
}

/// Count nesting depth of control-flow keywords in a (possibly partial) input.
/// Positive → needs more `fi`/`done` to close.
fn control_depth(input: &str) -> i32 {
    let mut depth = 0i32;
    // Split on unquoted semicolons and check first word of each segment
    let mut in_single = false;
    let mut in_double = false;
    let mut seg_start = 0usize;
    let bytes = input.as_bytes();
    let mut i = 0usize;

    let mut check_seg = |seg: &str| {
        let first = seg.split_whitespace().next().unwrap_or("");
        match first {
            "if" | "for" | "while" => depth += 1,
            "end" | "fi" | "done" => depth -= 1, // end is canonical (fish); fi/done are bash compat
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

/// Print a styled panel for explain-mode output.
///
/// Renders the command in a teal header box, then the explanation text indented below.
fn print_styled_explain(cmd: &str, explanation: &str) {
    // Gradient palette matching the startup banner
    const GRAD: &[u8] = &[30, 31, 32, 37, 38, 44, 45];
    let mid_color = GRAD[GRAD.len() / 2];

    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);

    // Measure cmd visible length (no ANSI in raw cmd)
    let cmd_vis = cmd.len();
    let label = "explain";
    let label_vis = label.len() + 2; // " explain "
    let content_width = cmd_vis.max(32);
    let inner_width = (content_width + 4).min(term_width.saturating_sub(2));

    // Gradient horizontal line
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

    // Header bar: ╭─ explain ──────╮
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

    // Command line inside box
    let cmd_styled = format!("\x1b[1;36m{cmd}\x1b[0m");
    let content_inner = inner_width.saturating_sub(4);
    let pad = content_inner.saturating_sub(cmd_vis);
    eprintln!(
        " {b}  {cmd}{}  {b}",
        " ".repeat(pad),
        b = border('│'),
        cmd = cmd_styled,
    );

    // Bottom border
    eprintln!(
        " {bl}{bot}{br}",
        bl = border('╰'),
        bot = grad_line(inner_width),
        br = border('╯'),
    );

    // Explanation text — indented, dim prefix bar
    for line in explanation.trim().lines() {
        eprintln!("  \x1b[38;5;{mid_color}m│\x1b[0m {line}");
    }
    eprintln!();
}

fn print_styled_banner(config: &ShakoConfig, ai_status: &ai::client::AiCheckResult) {
    let version = proactive::format_minor_version(env!("CARGO_PKG_VERSION"));
    let llm = config.active_llm();

    let provider_name: String = if let Some(name) = &config.active_provider {
        if !config.providers.contains_key(name.as_str()) {
            eprintln!(
                "\x1b[33mwarning:\x1b[0m active_provider '{}' not found in config — using defaults.\
                 \n         Add a [providers.{}] block or remove active_provider.",
                name, name
            );
        }
        name.clone()
    } else {
        endpoint_label(&llm.endpoint)
    };

    let ai_line = match ai_status {
        ai::client::AiCheckResult::Ready => {
            format!(
                "\x1b[32m✓\x1b[0m ai ready  \x1b[90m{provider_name} · {}\x1b[0m",
                llm.model
            )
        }
        ai::client::AiCheckResult::Disabled => "\x1b[90m· ai disabled\x1b[0m".to_string(),
        ai::client::AiCheckResult::NoApiKey(env_var) => {
            format!("\x1b[33m⚠\x1b[0m no api key  \x1b[90m(set ${env_var})\x1b[0m")
        }
        ai::client::AiCheckResult::AuthFailed(code) => {
            format!("\x1b[31m✗\x1b[0m auth failed  \x1b[90m(HTTP {code})\x1b[0m")
        }
        ai::client::AiCheckResult::Unreachable(reason) => {
            format!("\x1b[31m✗\x1b[0m unreachable  \x1b[90m({reason})\x1b[0m")
        }
    };

    let config_line = format!(
        "\x1b[90msafety: {}  ·  edit: {}  ·  typo-fix: {}\x1b[0m",
        config.behavior.safety_mode,
        config.behavior.edit_mode,
        if config.behavior.auto_correct_typos {
            "on"
        } else {
            "off"
        },
    );

    let line1 = format!("\x1b[1;36mshako\x1b[0m \x1b[90m{version}\x1b[0m");
    let line2 = ai_line;
    let line3 = config_line;

    // Measure visible width of each line (strip ANSI escapes)
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
                len += unicode_display_width(c);
            }
        }
        len
    };

    let w1 = visible_len(&line1);
    let w2 = visible_len(&line2);
    let w3 = visible_len(&line3);
    let content_width = w1.max(w2).max(w3);
    let inner_width = content_width + 4;

    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);
    let inner_width = inner_width.min(term_width.saturating_sub(4));
    let content_width = inner_width.saturating_sub(4);

    let pad_line = |s: &str, visible: usize| -> String {
        let pad = content_width.saturating_sub(visible);
        format!("{s}{}", " ".repeat(pad))
    };

    // Gradient border colors: teal (38;5;30) → cyan (38;5;45)
    let grad: &[u8] = &[30, 31, 32, 37, 38, 44, 45];
    let top_bar = gradient_repeat('─', inner_width, grad);
    let bot_bar = gradient_repeat('─', inner_width, grad);

    let border = |c: char| -> String {
        let idx = grad.len() / 2;
        format!("\x1b[38;5;{}m{c}\x1b[0m", grad[idx])
    };

    eprintln!(
        " {tl}{top}{tr}",
        tl = border('╭'),
        top = top_bar,
        tr = border('╮'),
    );
    eprintln!(
        " {b}  {l1}  {b}",
        b = border('│'),
        l1 = pad_line(&line1, w1),
    );
    eprintln!(
        " {b}  {l2}  {b}",
        b = border('│'),
        l2 = pad_line(&line2, w2),
    );
    eprintln!(
        " {b}  {l3}  {b}",
        b = border('│'),
        l3 = pad_line(&line3, w3),
    );
    eprintln!(
        " {bl}{bot}{br}",
        bl = border('╰'),
        bot = bot_bar,
        br = border('╯'),
    );
}

fn unicode_display_width(c: char) -> usize {
    if ('\u{1100}'..='\u{115F}').contains(&c)
        || ('\u{2E80}'..='\u{9FFF}').contains(&c)
        || ('\u{F900}'..='\u{FAFF}').contains(&c)
        || ('\u{FE10}'..='\u{FE6F}').contains(&c)
        || ('\u{FF01}'..='\u{FF60}').contains(&c)
        || ('\u{FFE0}'..='\u{FFE6}').contains(&c)
        || ('\u{1F300}'..='\u{1F9FF}').contains(&c)
        || ('\u{20000}'..='\u{2FA1F}').contains(&c)
    {
        2
    } else {
        1
    }
}

fn gradient_repeat(ch: char, width: usize, colors: &[u8]) -> String {
    let mut out = String::new();
    for i in 0..width {
        let idx = if width <= 1 {
            0
        } else {
            i * (colors.len() - 1) / (width - 1)
        };
        out.push_str(&format!("\x1b[38;5;{}m{ch}", colors[idx]));
    }
    out.push_str("\x1b[0m");
    out
}

/// Infer a friendly backend label from an endpoint URL.
/// Maps well-known localhost ports to backend names; falls back to host:port.
fn endpoint_label(endpoint: &str) -> String {
    let url = endpoint.trim();
    let host = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url);

    if host.contains("openai.com") {
        "openai".to_string()
    } else if host.ends_with(":11434") || host == "localhost:11434" {
        "ollama".to_string()
    } else if host.ends_with(":1234") {
        "lm-studio".to_string()
    } else if host.ends_with(":8080") {
        "llama.cpp".to_string()
    } else {
        host.to_string()
    }
}

/// Expand `!!` (last command) and `!$` (last arg of last command) in the input.
fn expand_history_bangs(input: &str, last_command: &str) -> String {
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
fn read_recent_history(history_path: &std::path::Path, n: usize) -> Vec<String> {
    if n == 0 {
        return Vec::new();
    }
    match std::fs::read_to_string(history_path) {
        Ok(contents) => {
            let lines: Vec<&str> = contents.lines().collect();
            let start = lines.len().saturating_sub(n);
            lines[start..].iter().map(|l| l.to_string()).collect()
        }
        Err(_) => Vec::new(),
    }
}

/// Returns true if `segment` should be dispatched to `run_builtin`.
/// A pure builtin call has no pipes and no unquoted redirect operators (> <).
/// If pipes or redirects are present, the executor handles them (including
/// any builtin at the start of the pipeline).
fn is_pure_builtin_call(segment: &str) -> bool {
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
