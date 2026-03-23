use anyhow::Result;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Run the interactive first-time setup wizard and write config.toml.
/// Returns the TOML string that was written.
pub fn run_wizard(config_path: &Path) -> Result<String> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    writeln!(out, "\n\x1b[1;36m welcome to shako!\x1b[0m")?;
    writeln!(
        out,
        " No config found at \x1b[33m{}\x1b[0m",
        config_path.display()
    )?;
    writeln!(out, " Let's get you set up.\n")?;

    writeln!(out, " Which AI provider would you like to use?")?;
    writeln!(
        out,
        "   \x1b[1m[1]\x1b[0m LM Studio  \x1b[90m(local, http://localhost:1234)\x1b[0m"
    )?;
    writeln!(out, "   \x1b[1m[2]\x1b[0m Work / custom proxy")?;
    writeln!(
        out,
        "   \x1b[1m[3]\x1b[0m Skip — write a template I'll edit manually\n"
    )?;

    let choice = prompt_line(&mut out, " Choice [1]: ", "1")?;

    let toml = match choice.trim() {
        "2" => wizard_custom_proxy(&mut out)?,
        "3" => template_config(),
        _ => wizard_lm_studio(&mut out)?,
    };

    // Create config directory
    if let Some(dir) = config_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(config_path, &toml)?;

    writeln!(out)?;
    writeln!(
        out,
        " \x1b[32m✓\x1b[0m Config written to \x1b[33m{}\x1b[0m",
        config_path.display()
    )?;
    writeln!(
        out,
        " Edit it any time to change providers or add aliases.\n"
    )?;

    Ok(toml)
}

fn wizard_lm_studio(out: &mut impl Write) -> Result<String> {
    writeln!(out)?;
    writeln!(out, "\x1b[1m LM Studio setup\x1b[0m")?;
    writeln!(
        out,
        " Make sure LM Studio is running with the local server enabled.\n"
    )?;

    let endpoint = prompt_line(
        out,
        " Endpoint [http://localhost:1234/v1/chat/completions]: ",
        "http://localhost:1234/v1/chat/completions",
    )?;
    let model = prompt_line(out, " Model name (from LM Studio): ", "")?;
    let model = if model.trim().is_empty() {
        "local-model".to_string()
    } else {
        model.trim().to_string()
    };

    Ok(format!(
        r#"# shako configuration
# Docs: https://github.com/solarisjon/shako

active_provider = "lm_studio"

[providers.lm_studio]
endpoint = "{endpoint}"
model = "{model}"
# LM Studio doesn't require an API key — leave api_key_env unset or empty

[behavior]
confirm_ai_commands = true
auto_correct_typos = true
safety_mode = "warn"  # "warn" | "block" | "off"

# [aliases]
# gs = "git status"
# ll = "ls -la"
"#,
        endpoint = endpoint.trim(),
        model = model,
    ))
}

fn wizard_custom_proxy(out: &mut impl Write) -> Result<String> {
    writeln!(out)?;
    writeln!(out, "\x1b[1m Custom / work proxy setup\x1b[0m\n")?;

    writeln!(
        out,
        " \x1b[90m(full URL, e.g. https://proxy.company.com/v1/chat/completions)\x1b[0m"
    )?;
    let endpoint = prompt_line(out, " Endpoint URL: ", "")?;
    if endpoint.trim().is_empty() {
        writeln!(
            out,
            " \x1b[33m(no endpoint entered — writing template)\x1b[0m"
        )?;
        return Ok(template_config());
    }

    // Normalize: add scheme if missing, warn about bare hostnames
    let mut endpoint = endpoint.trim().to_string();
    if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
        endpoint = format!("https://{endpoint}");
        writeln!(
            out,
            " \x1b[90m(added https:// → {endpoint})\x1b[0m"
        )?;
    }
    if let Ok(parsed) = reqwest::Url::parse(&endpoint) {
        if parsed.path() == "/" || parsed.path().is_empty() {
            endpoint = format!("{}/v1/chat/completions", endpoint.trim_end_matches('/'));
            writeln!(
                out,
                " \x1b[90m(added API path → {endpoint})\x1b[0m"
            )?;
        }
    }

    let model = prompt_line(out, " Model name: ", "gpt-4")?;
    let api_key_env = prompt_line(out, " API key env var [SHAKO_LLM_KEY]: ", "SHAKO_LLM_KEY")?;

    let verify_ssl_ans = prompt_line(out, " Verify SSL? [Y/n]: ", "y")?;
    let verify_ssl = !matches!(verify_ssl_ans.trim().to_lowercase().as_str(), "n" | "no");

    Ok(format!(
        r#"# shako configuration
# Docs: https://github.com/solarisjon/shako

active_provider = "work_proxy"

[providers.work_proxy]
endpoint = "{endpoint}"
model = "{model}"
api_key_env = "{api_key_env}"
verify_ssl = {verify_ssl}

# Add more providers here, then change active_provider to switch:
# [providers.lm_studio]
# endpoint = "http://localhost:1234/v1/chat/completions"
# model = "your-local-model"

[behavior]
confirm_ai_commands = true
auto_correct_typos = true
safety_mode = "warn"  # "warn" | "block" | "off"

# [aliases]
# gs = "git status"
# ll = "ls -la"
"#,
        endpoint = endpoint.trim(),
        model = model.trim(),
        api_key_env = api_key_env.trim(),
        verify_ssl = verify_ssl,
    ))
}

fn template_config() -> String {
    r#"# shako configuration
# Docs: https://github.com/solarisjon/shako
#
# Uncomment and fill in a provider, then set active_provider to use it.

# active_provider = "lm_studio"

# [providers.lm_studio]
# endpoint = "http://localhost:1234/v1/chat/completions"
# model = "your-model-name"

# [providers.work_proxy]
# endpoint = "https://your-proxy.company.com/v1/chat/completions"
# model = "claude-sonnet-4.5"
# api_key_env = "SHAKO_LLM_KEY"
# verify_ssl = false

[behavior]
confirm_ai_commands = true
auto_correct_typos = true
safety_mode = "warn"  # "warn" | "block" | "off"

# [aliases]
# gs = "git status"
# ll = "ls -la"
"#
    .to_string()
}

/// Ensure `~/.config/shako/starship.toml` exists with shako shell indicator set.
///
/// Starship's `shell` module only knows bash/fish/zsh/etc — shako maps to "unknown"
/// and uses `unknown_indicator` (default: ""). This function creates a shako-specific
/// Starship config (merging the user's global config) so the prompt shows "shako".
///
/// Returns the path to the shako Starship config if successfully created/existing,
/// so the caller can set `STARSHIP_CONFIG` accordingly.
pub fn ensure_starship_config(shako_config_dir: &Path) -> Option<PathBuf> {
    if which::which("starship").is_err() {
        return None;
    }

    let dest = shako_config_dir.join("starship.toml");
    if dest.exists() {
        return Some(dest);
    }

    let base = find_user_starship_config()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();

    let merged = match merge_shako_shell_settings(&base) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("could not merge starship config: {e}");
            return None;
        }
    };

    if let Err(e) = std::fs::create_dir_all(shako_config_dir) {
        log::warn!("could not create shako config dir: {e}");
        return None;
    }

    if let Err(e) = std::fs::write(&dest, merged) {
        log::warn!("could not write starship config: {e}");
        return None;
    }

    log::info!("wrote shako starship config to {}", dest.display());
    Some(dest)
}

/// Find the user's global Starship config, respecting `STARSHIP_CONFIG` env var.
fn find_user_starship_config() -> Option<PathBuf> {
    // Respect the env var Starship itself honours.
    if let Ok(path) = std::env::var("STARSHIP_CONFIG") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    // XDG default
    let p = dirs::home_dir()?.join(".config").join("starship.toml");
    if p.exists() { Some(p) } else { None }
}

/// Merge shako shell indicator settings into an existing Starship TOML config string.
fn merge_shako_shell_settings(base: &str) -> Result<String> {
    use toml::Value;

    let mut config: toml::map::Map<String, Value> = if base.is_empty() {
        Default::default()
    } else {
        toml::from_str(base)?
    };

    let shell = config
        .entry("shell".to_string())
        .or_insert_with(|| Value::Table(Default::default()));

    let table = shell
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[shell] is not a TOML table"))?;

    // Always enable the shell module under shako.
    table.insert("disabled".to_string(), Value::Boolean(false));

    // Set unknown_indicator to "shako" only if the user hasn't customised it.
    table
        .entry("unknown_indicator".to_string())
        .or_insert_with(|| Value::String("shako".to_string()));

    Ok(toml::to_string_pretty(&Value::Table(config))?)
}

fn prompt_line(out: &mut impl Write, prompt: &str, default: &str) -> Result<String> {
    write!(out, "{}", prompt)?;
    out.flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim().to_string();

    if input.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(input)
    }
}
