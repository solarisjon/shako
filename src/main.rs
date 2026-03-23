use std::io::{self, Write};

use anyhow::Result;
use reedline::{
    ColumnarMenu, Emacs, FileBackedHistory, KeyCode, KeyModifiers, MenuBuilder, Reedline,
    ReedlineEvent, ReedlineMenu, Signal, default_emacs_keybindings,
};

mod ai;
mod builtins;
mod classifier;
mod config;
mod executor;
mod parser;
mod safety;
mod setup;
mod shell;
mod smart_defaults;

use builtins::ShellState;
use classifier::{Classification, Classifier};
use config::JboshConfig;
use shell::prompt::{self, CommandTimer, StarshipPrompt};

fn main() -> Result<()> {
    let quiet = std::env::args().any(|a| a == "--quiet" || a == "-q");

    env_logger::init();
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

    let config = JboshConfig::load()?;

    if !quiet {
        print_banner(&config);
    }

    let classifier = Classifier::new();

    let highlighter = shell::highlighter::JboshHighlighter::new();
    let completer = shell::completer::JboshCompleter::new();
    let hinter = shell::hinter::JboshHinter::new();

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

    let mut keybindings = default_emacs_keybindings();
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );
    keybindings.add_binding(
        KeyModifiers::SHIFT,
        KeyCode::BackTab,
        ReedlineEvent::MenuPrevious,
    );

    let edit_mode = Box::new(Emacs::new(keybindings));

    let mut line_editor = Reedline::create()
        .with_history(history)
        .with_highlighter(Box::new(highlighter))
        .with_completer(Box::new(completer))
        .with_hinter(Box::new(hinter))
        .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
        .with_edit_mode(edit_mode);

    let prompt = StarshipPrompt::new();
    let rt = tokio::runtime::Runtime::new()?;
    let mut state = ShellState::new(history_path);

    // Load aliases from config
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

    // Auto-source init file if it exists
    let init_path = dirs::config_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .map(|d| d.join("shako").join("init.sh"));
    if let Some(ref path) = init_path {
        if path.exists() {
            builtins::run_builtin(&format!("source {}", path.display()), &mut state);
        }
    }

    loop {
        // Reap finished background jobs before each prompt
        state.reap_jobs();
        prompt::set_job_count(state.jobs.len());

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

                // Expand aliases before classification
                let input = state.expand_alias(&input).unwrap_or(input);

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

                let timer = CommandTimer::start();

                // Check if first token is a shell function
                let first_token = input.split_whitespace().next().unwrap_or("");
                if let Some(func) = state.functions.get(first_token).cloned() {
                    let args: Vec<&str> = input.split_whitespace().skip(1).collect();
                    builtins::run_function(&func, &args);
                    timer.stop();
                    continue;
                }

                match classifier.classify(&input) {
                    Classification::Command(cmd) => {
                        let status = executor::execute_command(&cmd);
                        set_exit_code(status);
                        if let Some(s) = status {
                            if !s.success() {
                                offer_ai_recovery(&cmd, s.code().unwrap_or(1), &config, &rt);
                            }
                        }
                    }
                    Classification::Builtin(cmd) => {
                        builtins::run_builtin(&cmd, &mut state);
                        prompt::set_last_status(0);
                    }
                    Classification::NaturalLanguage(text) => {
                        rt.block_on(async {
                            match ai::translate_and_execute(&text, &config).await {
                                Ok(_) => prompt::set_last_status(0),
                                Err(e) => {
                                    eprintln!("shako: ai error: {e}");
                                    prompt::set_last_status(1);
                                }
                            }
                        });
                    }
                    Classification::ForcedAI(text) => {
                        rt.block_on(async {
                            match ai::translate_and_execute(&text, &config).await {
                                Ok(_) => prompt::set_last_status(0),
                                Err(e) => {
                                    eprintln!("shako: ai error: {e}");
                                    prompt::set_last_status(1);
                                }
                            }
                        });
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
                                let status = executor::execute_command(&suggestion);
                                set_exit_code(status);
                            }
                        } else {
                            rt.block_on(async {
                                match ai::translate_and_execute(&suggestion, &config).await {
                                    Ok(_) => prompt::set_last_status(0),
                                    Err(e) => {
                                        eprintln!("shako: ai error: {e}");
                                        prompt::set_last_status(1);
                                    }
                                }
                            });
                        }
                    }
                    Classification::Empty => {}
                }

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
    config: &JboshConfig,
    rt: &tokio::runtime::Runtime,
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
        match ai::diagnose_error(command, exit_code, "", config).await {
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

/// Check if the input line needs continuation (trailing \ or unclosed quotes).
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

    in_single || in_double
}

fn print_banner(config: &JboshConfig) {
    let version = env!("CARGO_PKG_VERSION");
    let llm = config.active_llm();

    let provider_name = config.active_provider.as_deref().unwrap_or("llm");

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
