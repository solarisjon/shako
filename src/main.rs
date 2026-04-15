use std::time::Instant;

use anyhow::Result;
use nu_ansi_term::{Color as NuColor, Style as NuStyle};
use reedline::{
    ColumnarMenu, EditMode, Emacs, FileBackedHistory, KeyCode, KeyModifiers, MenuBuilder, Reedline,
    ReedlineEvent, ReedlineMenu, Vi, default_emacs_keybindings, default_vi_insert_keybindings,
    default_vi_normal_keybindings,
};

mod ai;
mod audit;
mod behavioral_profile;
mod builtins;
mod classifier;
mod config;
mod control;
mod env_context;
mod executor;
#[cfg(feature = "fish-import")]
mod fish_import;
mod incident;
mod journal;
mod learned_prefs;
mod parser;
mod path_cache;
mod pipe_builder;
mod proactive;
mod safety;
mod setup;
mod shell;
mod slash;
mod smart_defaults;
mod spinner;
mod undo;

use builtins::ShellState;
use classifier::Classifier;
use config::ShakoConfig;
use shell::prompt::StarshipPrompt;
use shell::repl::is_pure_builtin_call;

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
    let (config, first_run) = ShakoConfig::load()?;
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
        FileBackedHistory::with_file(config.behavior.history_size, history_path.clone())
            .unwrap_or_else(|e| {
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
            .with_text_style(NuColor::Fixed(242).normal())
            .with_selected_text_style(NuStyle::new().bold().fg(NuColor::Fixed(38)))
            .with_description_text_style(NuColor::Fixed(30).normal())
            .with_match_text_style(NuStyle::new().underline().fg(NuColor::Fixed(38)))
            .with_selected_match_text_style(
                NuStyle::new().bold().underline().fg(NuColor::Fixed(45)),
            ),
    );

    let tab_completion_binding = ReedlineEvent::UntilFound(vec![
        ReedlineEvent::Menu("completion_menu".to_string()),
        ReedlineEvent::MenuNext,
    ]);

    let edit_mode: Box<dyn EditMode> = if config.behavior.edit_mode == "vi" {
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

    let line_editor = Reedline::create()
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

    // Load abbreviations from config.toml [abbreviations] section.
    for (name, value) in &config.abbreviations {
        state.abbreviations.insert(name.clone(), value.clone());
    }

    // Apply [env] section — set startup environment variables.
    // Safety: called before any threads exist; no concurrent env readers.
    for (key, value) in &config.env {
        unsafe { std::env::set_var(key, value) };
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

        // 3. functions/ — autoloaded function files
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
            let fish_conf_d = fish_dir.join("conf.d");
            if fish_conf_d.is_dir() {
                builtins::source_conf_d(&fish_conf_d, &mut state);
            }

            let config_fish = fish_dir.join("config.fish");
            if config_fish.exists() {
                if let Ok(contents) = std::fs::read_to_string(&config_fish) {
                    builtins::source_fish_string(&contents, &mut state);
                }
            }

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

    // Hand off to the REPL event loop.
    shell::repl::run_repl(
        line_editor,
        prompt,
        state,
        classifier,
        config,
        rt,
        history_path,
        extra_completions,
    );

    Ok(())
}

// ─── Startup UI helpers ───────────────────────────────────────────────────────

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

    let line1 = format!("\x1b[1;36mshako\x1b[0m \x1b[90m{version} \"Mako\"\x1b[0m");
    let line2 = ai_line;
    let line3 = config_line;

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
