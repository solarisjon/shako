use crate::ai;
use crate::config::ShakoConfig;

pub const SLASH_COMMANDS: &[(&str, &str)] = &[
    ("help", "List available slash commands"),
    ("validate", "Validate the AI endpoint connection"),
    ("config", "Show current configuration"),
    ("model", "Show or switch the active AI model/provider"),
    ("safety", "Show or change safety mode (warn/block/off)"),
    ("provider", "Show or switch the active LLM provider"),
];

pub fn run(name: &str, args: &str, config: &mut ShakoConfig, rt: &tokio::runtime::Runtime) -> i32 {
    match name {
        "help" => cmd_help(),
        "validate" => cmd_validate(config, rt),
        "config" => cmd_config(config),
        "model" => cmd_model(args, config),
        "safety" => cmd_safety(args, config),
        "provider" => cmd_provider(args, config),
        _ => {
            eprintln!("shako: unknown command /{name}");
            eprintln!("       run /help to see available commands");
            1
        }
    }
}

fn cmd_help() -> i32 {
    eprintln!("\x1b[1mshako slash commands\x1b[0m\n");
    for (name, desc) in SLASH_COMMANDS {
        eprintln!("  \x1b[36m/{name:<12}\x1b[0m {desc}");
    }
    eprintln!();
    0
}

fn cmd_validate(config: &ShakoConfig, rt: &tokio::runtime::Runtime) -> i32 {
    let llm = config.active_llm();
    let provider_label = config.active_provider.as_deref().unwrap_or("llm (default)");

    eprintln!("\x1b[90mvalidating provider \x1b[1m{provider_label}\x1b[0m\x1b[90m...\x1b[0m");
    eprintln!("  endpoint:  {}", llm.endpoint);
    eprintln!("  model:     {}", llm.model);
    eprintln!(
        "  api key:   {} {}",
        llm.api_key_env,
        if std::env::var(&llm.api_key_env).is_ok() {
            "(set)"
        } else {
            "(not set)"
        }
    );

    let result = rt.block_on(ai::client::check_ai_session(
        llm,
        config.behavior.ai_enabled,
    ));

    match result {
        ai::client::AiCheckResult::Ready => {
            eprintln!("\x1b[32m  status:    ready\x1b[0m");
            0
        }
        ai::client::AiCheckResult::Disabled => {
            eprintln!("\x1b[33m  status:    AI disabled (ai_enabled = false)\x1b[0m");
            0
        }
        ai::client::AiCheckResult::NoApiKey(env_var) => {
            eprintln!("\x1b[31m  status:    no API key (set ${env_var})\x1b[0m");
            1
        }
        ai::client::AiCheckResult::AuthFailed(code) => {
            eprintln!("\x1b[31m  status:    auth failed (HTTP {code})\x1b[0m");
            1
        }
        ai::client::AiCheckResult::Unreachable(reason) => {
            eprintln!("\x1b[31m  status:    unreachable ({reason})\x1b[0m");
            1
        }
    }
}

fn cmd_config(config: &ShakoConfig) -> i32 {
    let llm = config.active_llm();
    let provider_label = config.active_provider.as_deref().unwrap_or("llm (default)");

    eprintln!("\x1b[1mshako configuration\x1b[0m\n");

    eprintln!("\x1b[36m[active provider: {provider_label}]\x1b[0m");
    eprintln!("  endpoint         = {}", llm.endpoint);
    eprintln!("  model            = {}", llm.model);
    eprintln!("  api_key_env      = {}", llm.api_key_env);
    eprintln!("  timeout_secs     = {}", llm.timeout_secs);
    eprintln!("  max_tokens       = {}", llm.max_tokens);
    eprintln!("  temperature      = {}", llm.temperature);
    eprintln!("  verify_ssl       = {}", llm.verify_ssl);

    if !config.providers.is_empty() {
        eprintln!("\n\x1b[36m[providers]\x1b[0m");
        for name in config.providers.keys() {
            let marker = if config.active_provider.as_deref() == Some(name) {
                " (active)"
            } else {
                ""
            };
            eprintln!("  {name}{marker}");
        }
    }

    eprintln!("\n\x1b[36m[behavior]\x1b[0m");
    eprintln!("  ai_enabled           = {}", config.behavior.ai_enabled);
    eprintln!(
        "  confirm_ai_commands  = {}",
        config.behavior.confirm_ai_commands
    );
    eprintln!(
        "  auto_correct_typos   = {}",
        config.behavior.auto_correct_typos
    );
    eprintln!("  safety_mode          = {}", config.behavior.safety_mode);
    eprintln!("  edit_mode            = {}", config.behavior.edit_mode);
    eprintln!(
        "  history_context      = {}",
        config.behavior.history_context_lines
    );

    if !config.aliases.is_empty() {
        eprintln!("\n\x1b[36m[aliases]\x1b[0m");
        for (k, v) in &config.aliases {
            eprintln!("  {k} = {v}");
        }
    }

    eprintln!();
    0
}

fn cmd_model(args: &str, config: &ShakoConfig) -> i32 {
    if args.is_empty() {
        let llm = config.active_llm();
        let provider_label = config.active_provider.as_deref().unwrap_or("llm (default)");
        eprintln!("\x1b[36m{provider_label}\x1b[0m: {}", llm.model);
        return 0;
    }
    eprintln!("shako: runtime model switching not yet supported");
    eprintln!("       edit ~/.config/shako/config.toml to change models");
    1
}

fn cmd_safety(args: &str, config: &mut ShakoConfig) -> i32 {
    if args.is_empty() {
        eprintln!(
            "safety_mode = \x1b[1m{}\x1b[0m",
            config.behavior.safety_mode
        );
        eprintln!("  warn  — show warning for dangerous commands");
        eprintln!("  block — block dangerous commands entirely");
        eprintln!("  off   — no safety checks");
        return 0;
    }

    match args {
        "warn" | "block" | "off" => {
            config.behavior.safety_mode = args.to_string();
            eprintln!("safety_mode = \x1b[1m{args}\x1b[0m (session only)");
            0
        }
        _ => {
            eprintln!("shako: invalid safety mode '{args}'");
            eprintln!("       valid modes: warn, block, off");
            1
        }
    }
}

fn cmd_provider(args: &str, config: &mut ShakoConfig) -> i32 {
    if args.is_empty() {
        let current = config
            .active_provider
            .as_deref()
            .unwrap_or("(default [llm])");
        eprintln!("active provider: \x1b[1m{current}\x1b[0m");
        if !config.providers.is_empty() {
            eprintln!("\navailable providers:");
            for (name, p) in &config.providers {
                let marker = if config.active_provider.as_deref() == Some(name.as_str()) {
                    " \x1b[32m(active)\x1b[0m"
                } else {
                    ""
                };
                eprintln!("  \x1b[36m{name}\x1b[0m — {}{marker}", p.model);
            }
        }
        return 0;
    }

    if config.providers.contains_key(args) {
        config.active_provider = Some(args.to_string());
        let model = &config.providers[args].model;
        eprintln!("switched to \x1b[1m{args}\x1b[0m ({model}) (session only)");
        0
    } else {
        eprintln!("shako: unknown provider '{args}'");
        if !config.providers.is_empty() {
            let names: Vec<&str> = config.providers.keys().map(|s| s.as_str()).collect();
            eprintln!("       available: {}", names.join(", "));
        }
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Runtime::new()
            .expect("failed to create tokio runtime for slash command test")
    }

    #[test]
    fn test_slash_commands_list_not_empty() {
        assert!(!SLASH_COMMANDS.is_empty());
    }

    #[test]
    fn test_unknown_command_returns_1() {
        let mut config = ShakoConfig::default();
        let rt = make_runtime();
        assert_eq!(run("nonexistent", "", &mut config, &rt), 1);
    }

    #[test]
    fn test_help_returns_0() {
        let mut config = ShakoConfig::default();
        let rt = make_runtime();
        assert_eq!(run("help", "", &mut config, &rt), 0);
    }

    #[test]
    fn test_config_returns_0() {
        let mut config = ShakoConfig::default();
        let rt = make_runtime();
        assert_eq!(run("config", "", &mut config, &rt), 0);
    }

    #[test]
    fn test_model_no_args_returns_0() {
        let mut config = ShakoConfig::default();
        let rt = make_runtime();
        assert_eq!(run("model", "", &mut config, &rt), 0);
    }

    #[test]
    fn test_safety_no_args_returns_0() {
        let mut config = ShakoConfig::default();
        let rt = make_runtime();
        assert_eq!(run("safety", "", &mut config, &rt), 0);
    }

    #[test]
    fn test_safety_set_valid_mode() {
        let mut config = ShakoConfig::default();
        let rt = make_runtime();
        assert_eq!(run("safety", "off", &mut config, &rt), 0);
        assert_eq!(config.behavior.safety_mode, "off");
    }

    #[test]
    fn test_safety_set_invalid_mode() {
        let mut config = ShakoConfig::default();
        let rt = make_runtime();
        assert_eq!(run("safety", "invalid", &mut config, &rt), 1);
    }

    #[test]
    fn test_provider_no_args_returns_0() {
        let mut config = ShakoConfig::default();
        let rt = make_runtime();
        assert_eq!(run("provider", "", &mut config, &rt), 0);
    }

    #[test]
    fn test_provider_switch_unknown() {
        let mut config = ShakoConfig::default();
        let rt = make_runtime();
        assert_eq!(run("provider", "nonexistent", &mut config, &rt), 1);
    }
}
