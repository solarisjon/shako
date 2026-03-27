use std::io::{self, Write};

use anyhow::Result;
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
mod smart_defaults;

use builtins::ShellState;
use classifier::{Classification, Classifier};
use config::ShakoConfig;
use shell::prompt::{self, CommandTimer, StarshipPrompt};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let quiet = args.iter().any(|a| a == "--quiet" || a == "-q");
    let init = args.iter().any(|a| a == "--init");
    let cmd_mode = args.iter().position(|a| a == "-c")
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

    let (config, first_run) = ShakoConfig::load()?;
    let rt = tokio::runtime::Runtime::new()?;

    if !quiet {
        print_banner(&config);
        let ai_status = rt.block_on(ai::client::check_ai_session(
            config.active_llm(),
            config.behavior.ai_enabled,
        ));
        print_ai_status(&ai_status, config.active_llm());
    }

    if first_run {
        setup::check_recommended_tools();
    }

    let path_cache = path_cache::PathCache::new();
    let classifier = Classifier::new(path_cache.clone());

    let highlighter = shell::highlighter::ShakoHighlighter::new(path_cache.clone());
    let extra_completions: std::sync::Arc<std::sync::RwLock<Vec<String>>> =
        std::sync::Arc::new(std::sync::RwLock::new(vec![]));
    let completer = shell::completer::ShakoCompleter::new(path_cache, std::sync::Arc::clone(&extra_completions));
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

    let completion_menu = Box::new(
        ColumnarMenu::default()
            .with_name("completion_menu")
            .with_columns(4)
            .with_column_padding(2),
    );

    let tab_completion_binding = ReedlineEvent::UntilFound(vec![
        ReedlineEvent::Menu("completion_menu".to_string()),
        ReedlineEvent::MenuNext,
    ]);

    let edit_mode: Box<dyn EditMode> = if config.behavior.edit_mode == "vi" {
        // Add Tab completion to vi insert mode — Vi::default() has no completion binding.
        let mut insert_kb = default_vi_insert_keybindings();
        insert_kb.add_binding(KeyModifiers::NONE, KeyCode::Tab, tab_completion_binding.clone());
        insert_kb.add_binding(KeyModifiers::SHIFT, KeyCode::BackTab, ReedlineEvent::MenuPrevious);
        Box::new(Vi::new(insert_kb, default_vi_normal_keybindings()))
    } else {
        let mut keybindings = default_emacs_keybindings();
        keybindings.add_binding(KeyModifiers::NONE, KeyCode::Tab, tab_completion_binding);
        keybindings.add_binding(KeyModifiers::SHIFT, KeyCode::BackTab, ReedlineEvent::MenuPrevious);
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
    let smart_aliases = smart_defaults::detect_smart_defaults(&state.aliases);
    for (name, value) in smart_aliases {
        state.aliases.entry(name).or_insert(value);
    }

    // Track shell nesting level
    let shlvl: i32 = std::env::var("SHLVL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    unsafe { std::env::set_var("SHLVL", (shlvl + 1).to_string()) };

    // ── Fish-like startup order ──────────────────────────────────────
    //  1. Source ~/.config/shako/conf.d/*.{fish,sh}  (config snippets)
    //  2. Source ~/.config/shako/config.shako         (main user config)
    //  3. Load  ~/.config/shako/functions/            (autoloaded functions)
    //  4. Optionally source fish config               (if [fish] source_config = true)

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
        let fish_config_dir = dirs::home_dir()
            .map(|h| h.join(".config").join("fish"));

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
                let mut input = input.trim().to_string();
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
                                input.pop(); // remove trailing backslash
                            }
                            input.push(' ');
                            input.push_str(next.trim());
                        }
                        _ => break,
                    }
                }

                // History expansion: !! (last command), !$ (last arg)
                let input = expand_history_bangs(&input, &last_command);

                // Expand aliases before classification
                let input = state.expand_alias(&input).unwrap_or(input);

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
                            state.add_job(child, bg_cmd.to_string());
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
                        let (status, stderr_output) =
                            executor::execute_command_with_stderr(&cmd);
                        set_exit_code(status);
                        if let Some(s) = status {
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
                            if stop { break; }
                        }
                        prompt::set_last_status(last_code);
                    }
                    Classification::NaturalLanguage(text) => {
                        if !config.behavior.ai_enabled {
                            eprintln!("shako: AI is disabled (ai_enabled = false in config)");
                        } else {
                        let history = read_recent_history(&history_path, config.behavior.history_context_lines);
                        match rt.block_on(ai::translate_and_execute(&text, &config, history, &mut state.ai_session_memory)) {
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
                            print!("\x1b[90mexplaining...\x1b[0m");
                            io::stdout().flush().ok();
                            rt.block_on(async {
                                match ai::explain_command(&text, &config).await {
                                    Ok(explanation) => {
                                        print!("\r\x1b[K");
                                        eprintln!("\x1b[36m{text}\x1b[0m");
                                        eprintln!("{explanation}");
                                    }
                                    Err(e) => {
                                        print!("\r\x1b[K");
                                        eprintln!("shako: ai error: {e}");
                                        prompt::set_last_status(1);
                                    }
                                }
                            });
                        } else {
                            let history = read_recent_history(&history_path, config.behavior.history_context_lines);
                            match rt.block_on(ai::translate_and_execute(&text, &config, history, &mut state.ai_session_memory)) {
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
                            print!(
                                "\x1b[33mshako: did you mean \x1b[1m{suggestion}\x1b[0m\x1b[33m? [Y/n]\x1b[0m "
                            );
                            io::stdout().flush().ok();
                            let mut answer = String::new();
                            io::stdin().read_line(&mut answer).ok();
                            let answer = answer.trim().to_lowercase();
                            if answer.is_empty() || answer == "y" || answer == "yes" {
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
                            let history = read_recent_history(&history_path, config.behavior.history_context_lines);
                            match rt.block_on(ai::translate_and_execute(&suggestion, &config, history, &mut state.ai_session_memory)) {
                                Ok(_) => prompt::set_last_status(0),
                                Err(e) => {
                                    eprintln!("shako: ai error: {e}");
                                    prompt::set_last_status(1);
                                }
                            }
                        }
                    }
                    Classification::Empty => {}
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
                        print!("\x1b[90mexplaining...\x1b[0m");
                        io::stdout().flush().ok();
                        rt.block_on(async {
                            match ai::explain_command(&cmd, &config).await {
                                Ok(explanation) => {
                                    print!("\r\x1b[K");
                                    eprintln!("\x1b[36m{cmd}\x1b[0m");
                                    eprintln!("{explanation}");
                                }
                                Err(e) => {
                                    print!("\r\x1b[K");
                                    eprintln!("shako: ai error: {e}");
                                }
                            }
                        });
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

    print!("\x1b[33mshako: command failed (exit {exit_code}). ask AI for help? [y/N]\x1b[0m ");
    io::stdout().flush().ok();

    let mut answer = String::new();
    io::stdin().read_line(&mut answer).ok();
    let answer = answer.trim().to_lowercase();

    if answer != "y" && answer != "yes" {
        return;
    }

    print!("\x1b[90mthinking...\x1b[0m");
    io::stdout().flush().ok();

    rt.block_on(async {
        let history = read_recent_history(history_path, config.behavior.history_context_lines);
        match ai::diagnose_error(command, exit_code, stderr_output, config, history).await {
            Ok(response) => {
                // Clear the "thinking..." text
                print!("\r\x1b[K");

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
                    eprintln!("\x1b[36m  cause:\x1b[0m {cause}");
                }

                if fix.is_empty() || fix == "SHAKO_NO_FIX" {
                    return;
                }

                // Show suggested fix and offer to run it
                println!("\x1b[36m  fix:\x1b[0m \x1b[1m{fix}\x1b[0m");
                print!("\x1b[90m  [Y]es / [n]o / [e]dit:\x1b[0m ");
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
                        print!("\x1b[36m  ❯\x1b[0m ");
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
                print!("\r\x1b[K");
                eprintln!("shako: ai error: {e}");
            }
        }
    });
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
            "fi" | "done" => depth -= 1,
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

fn print_banner(config: &ShakoConfig) {
    let version = env!("CARGO_PKG_VERSION");
    let llm = config.active_llm();

    // Derive a display name for the provider.
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
        // No named provider — infer a friendly label from the endpoint.
        endpoint_label(&llm.endpoint)
    };

    // Show the normalized endpoint so users see what URL will actually be used.
    let endpoint = ai::client::normalize_endpoint(&llm.endpoint);
    let endpoint_display = if endpoint.len() > 60 {
        format!("{}…", &endpoint[..60])
    } else {
        endpoint
    };

    eprintln!(
        "\x1b[1;36mshako\x1b[0m \x1b[90mv{version}\x1b[0m  \x1b[90m·\x1b[0m  \
         \x1b[33m{provider_name}\x1b[0m  {model}  \x1b[90m{endpoint_display}\x1b[0m",
        model = llm.model,
    );
}

fn print_ai_status(status: &ai::client::AiCheckResult, llm: &config::LlmConfig) {
    match status {
        ai::client::AiCheckResult::Ready => {
            eprintln!("\x1b[90m  AI\x1b[0m  \x1b[32m✓ session ready\x1b[0m");
        }
        ai::client::AiCheckResult::Disabled => {}
        ai::client::AiCheckResult::NoApiKey(env_var) => {
            eprintln!(
                "\x1b[90m  AI\x1b[0m  \x1b[33m⚠ no API key\x1b[0m  \x1b[90m({env_var} not set — AI will be unavailable)\x1b[0m"
            );
        }
        ai::client::AiCheckResult::AuthFailed(code) => {
            eprintln!(
                "\x1b[90m  AI\x1b[0m  \x1b[31m✗ auth failed\x1b[0m  \x1b[90m(HTTP {code} — check {})\x1b[0m",
                llm.api_key_env
            );
        }
        ai::client::AiCheckResult::Unreachable(reason) => {
            eprintln!(
                "\x1b[90m  AI\x1b[0m  \x1b[31m✗ unreachable\x1b[0m  \x1b[90m({reason})\x1b[0m"
            );
        }
    }
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

    let last_arg = last_command
        .split_whitespace()
        .last()
        .unwrap_or("");

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
            lines[start..]
                .iter()
                .map(|l| l.to_string())
                .collect()
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
