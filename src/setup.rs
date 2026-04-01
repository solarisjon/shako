use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal,
};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

// ── shared wizard styling constants ──────────────────────────────────────────

/// Teal→cyan gradient palette used for bordered panels throughout the wizard.
const GRAD: &[u8] = &[30, 31, 32, 37, 38, 44, 45];

/// Format a single `─` with the gradient color at position `i` of `total`.
fn grad_char(i: usize, total: usize) -> String {
    let idx = if total <= 1 {
        0
    } else {
        i * (GRAD.len() - 1) / (total - 1)
    };
    format!("\x1b[38;5;{}m─\x1b[0m", GRAD[idx])
}

/// Build a horizontal gradient rule of `width` dashes.
fn grad_line(width: usize) -> String {
    (0..width).map(|i| grad_char(i, width)).collect()
}

/// Color a border character with the mid-gradient color.
fn border(c: char) -> String {
    let mid = GRAD[GRAD.len() / 2];
    format!("\x1b[38;5;{mid}m{c}\x1b[0m")
}

/// Print a full-width bordered panel header with a `label` in the top bar.
/// `label_vis` is the visible (un-escaped) character count of `label`.
fn print_panel_header(out: &mut impl Write, label: &str, label_vis: usize, width: usize) {
    let rest = width.saturating_sub(label_vis + 4);
    writeln!(
        out,
        " {tl}{sep}{label}{rest}{tr}",
        tl = border('╭'),
        sep = grad_line(2),
        label = label,
        rest = grad_line(rest),
        tr = border('╮'),
    )
    .ok();
}

/// Print a divider inside an open panel (├─…─┤).
fn print_panel_div(out: &mut impl Write, width: usize) {
    writeln!(
        out,
        " {bl}{sep}{br}",
        bl = border('├'),
        sep = grad_line(width),
        br = border('┤'),
    )
    .ok();
}

/// Print the bottom of a panel (╰─…─╯).
fn print_panel_footer(out: &mut impl Write, width: usize) {
    writeln!(
        out,
        " {bl}{bot}{br}",
        bl = border('╰'),
        bot = grad_line(width),
        br = border('╯'),
    )
    .ok();
}

/// Print a content row inside a panel, padded to `inner` visible chars.
fn print_panel_row(out: &mut impl Write, content: &str, content_vis: usize, inner: usize) {
    let pad = inner.saturating_sub(content_vis);
    writeln!(
        out,
        " {b}  {content}{}  {b}",
        " ".repeat(pad),
        b = border('│'),
    )
    .ok();
}

// ── wizard entry point ────────────────────────────────────────────────────────

/// Run the interactive first-time setup wizard and write config.toml.
/// Returns the TOML string that was written.
pub fn run_wizard(config_path: &Path) -> Result<String> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);
    let panel_width = (60usize).min(term_width.saturating_sub(4));
    let inner = panel_width.saturating_sub(4);
    let mid_color = GRAD[GRAD.len() / 2];

    // ── welcome header panel  [step 1 of 2] ─────────────────────────────────
    let title_label = format!("\x1b[38;5;{mid_color}m  shako setup  \x1b[0m");
    let title_label_vis = 16usize;
    print_panel_header(&mut out, &title_label, title_label_vis, panel_width);
    let welcome = "\x1b[1;36mwelcome to shako!\x1b[0m".to_string();
    let welcome_vis = 17usize;
    print_panel_row(&mut out, &welcome, welcome_vis, inner);
    let cfg_line = format!("\x1b[90mconfig: {}\x1b[0m", config_path.display());
    let cfg_vis = 8 + config_path.display().to_string().len();
    print_panel_row(&mut out, &cfg_line, cfg_vis, inner);
    // Step progress dots: ● ● ○ (step 1 of 2 visible steps)
    let steps_row =
        format!("\x1b[38;5;{mid_color}m●\x1b[0m \x1b[90m○\x1b[0m  \x1b[90mstep 1 of 2\x1b[0m");
    let steps_vis = 14usize;
    print_panel_row(&mut out, &steps_row, steps_vis, inner);
    print_panel_footer(&mut out, panel_width);
    writeln!(out)?;

    // ── provider selection panel  [step 2 of 2] ─────────────────────────────
    let sel_label = format!("\x1b[38;5;{mid_color}m  AI provider  \x1b[0m");
    let sel_label_vis = 16usize;
    print_panel_header(&mut out, &sel_label, sel_label_vis, panel_width);

    let providers: &[(&str, &str, &str)] = &[
        ("1", "LM Studio", "local, http://localhost:1234"),
        ("2", "Work / custom proxy", "any OpenAI-compatible endpoint"),
        (
            "3",
            "Anthropic Claude",
            "api.anthropic.com  (needs ANTHROPIC_API_KEY)",
        ),
        ("4", "Skip", "write a template I'll edit manually"),
    ];

    // Step progress: ● ● (step 2 of 2)
    let steps2_row = format!(
        "\x1b[38;5;{mid_color}m●\x1b[0m \x1b[38;5;{mid_color}m●\x1b[0m  \x1b[90mstep 2 of 2\x1b[0m"
    );
    let steps2_vis = 14usize;
    print_panel_row(&mut out, &steps2_row, steps2_vis, inner);
    print_panel_div(&mut out, panel_width);

    for (key, name, hint) in providers {
        let row = format!("\x1b[1m[{key}]\x1b[0m \x1b[1m{name:<22}\x1b[0m \x1b[90m{hint}\x1b[0m");
        let row_vis = 5 + name.len().max(22) + 1 + hint.len();
        print_panel_row(&mut out, &row, row_vis, inner);
    }

    print_panel_div(&mut out, panel_width);

    // Prompt inside the panel
    let prompt_str = "\x1b[90mChoice [1]:\x1b[0m";
    let prompt_vis = 11usize;
    let pad = inner.saturating_sub(prompt_vis + 4);
    write!(
        out,
        " {b}  {prompt}{}    {b} ",
        " ".repeat(pad),
        b = border('│'),
        prompt = prompt_str,
    )?;
    out.flush()?;

    let mut choice_raw = String::new();
    io::stdin().read_line(&mut choice_raw).ok();
    print_panel_footer(&mut out, panel_width);
    writeln!(out)?;

    let choice = choice_raw.trim().to_string();
    let choice = if choice.is_empty() {
        "1".to_string()
    } else {
        choice
    };

    let toml = match choice.trim() {
        "2" => wizard_custom_proxy(&mut out)?,
        "3" => wizard_anthropic(&mut out)?,
        "4" => template_config(),
        _ => wizard_lm_studio(&mut out)?,
    };

    // Create config directory and fish-like subdirectories
    if let Some(dir) = config_path.parent() {
        std::fs::create_dir_all(dir)?;
        std::fs::create_dir_all(dir.join("conf.d"))?;
        std::fs::create_dir_all(dir.join("functions"))?;
    }
    std::fs::write(config_path, &toml)?;

    writeln!(out)?;
    writeln!(
        out,
        " \x1b[32m✓\x1b[0m Config written to \x1b[33m{}\x1b[0m",
        config_path.display()
    )?;

    if let Some(dir) = config_path.parent() {
        writeln!(
            out,
            " \x1b[32m✓\x1b[0m Created \x1b[33m{}/conf.d/\x1b[0m  \x1b[90m(drop .fish or .sh config snippets here)\x1b[0m",
            dir.display()
        )?;
        writeln!(
            out,
            " \x1b[32m✓\x1b[0m Created \x1b[33m{}/functions/\x1b[0m  \x1b[90m(autoloaded fish-style functions)\x1b[0m",
            dir.display()
        )?;
    }

    // Offer fish import if fish config exists
    let fish_dir = dirs::home_dir().map(|h| h.join(".config").join("fish"));
    if let Some(ref fd) = fish_dir {
        if fd.is_dir() {
            writeln!(out)?;
            writeln!(
                out,
                " \x1b[36m?\x1b[0m Found fish config at \x1b[33m{}\x1b[0m",
                fd.display()
            )?;
            let answer = prompt_line(&mut out, " Import fish config into shako? [Y/n]: ", "y")?;
            if matches!(answer.trim().to_lowercase().as_str(), "" | "y" | "yes") {
                writeln!(out)?;
                drop(out);
                #[cfg(feature = "fish-import")]
                crate::fish_import::run_import();
                #[cfg(not(feature = "fish-import"))]
                eprintln!("shako: fish-import: not compiled in");
            } else {
                writeln!(
                    out,
                    " \x1b[90m(skipped — run `fish-import` later to import)\x1b[0m"
                )?;
            }
        }
    }

    writeln!(
        io::stdout().lock(),
        " Edit config any time to change providers or add aliases.\n"
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
        writeln!(out, " \x1b[90m(added https:// → {endpoint})\x1b[0m")?;
    }
    if let Ok(parsed) = reqwest::Url::parse(&endpoint) {
        if parsed.path() == "/" || parsed.path().is_empty() {
            endpoint = format!("{}/v1/chat/completions", endpoint.trim_end_matches('/'));
            writeln!(out, " \x1b[90m(added API path → {endpoint})\x1b[0m")?;
        }
    }

    let model = prompt_line(out, " Model name: ", "gpt-4")?;
    let api_key_env = prompt_line(out, " API key env var [LLMPROXY_KEY]: ", "LLMPROXY_KEY")?;
    let api_key_env = api_key_env.trim().to_string();

    // Optional: capture actual API key with masked echo for this session.
    writeln!(
        out,
        "\n \x1b[90mPaste your API key to set it now (or press Enter to skip):\x1b[0m"
    )?;
    let api_key_val = prompt_secret(out, &format!(" {api_key_env} (masked): "), "")?;
    let api_key_val = api_key_val.trim().to_string();
    if !api_key_val.is_empty() {
        // SAFETY: setup wizard runs single-threaded before the REPL starts.
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var(&api_key_env, &api_key_val);
        }
        writeln!(
            out,
            " \x1b[32m✓\x1b[0m \x1b[90mSet {api_key_env} for this session.\x1b[0m"
        )?;
    }

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
        api_key_env = api_key_env,
        verify_ssl = verify_ssl,
    ))
}

fn wizard_anthropic(out: &mut impl Write) -> Result<String> {
    writeln!(out)?;
    writeln!(out, "\x1b[1m Anthropic Claude setup\x1b[0m\n")?;
    writeln!(
        out,
        " \x1b[90mYou'll need an API key from https://console.anthropic.com\x1b[0m\n"
    )?;

    let model = prompt_line(out, " Model [claude-sonnet-4-6]: ", "claude-sonnet-4-6")?;
    let model = if model.trim().is_empty() {
        "claude-sonnet-4-6".to_string()
    } else {
        model.trim().to_string()
    };

    let api_key_env = prompt_line(
        out,
        " API key env var [ANTHROPIC_API_KEY]: ",
        "ANTHROPIC_API_KEY",
    )?;
    let api_key_env = if api_key_env.trim().is_empty() {
        "ANTHROPIC_API_KEY".to_string()
    } else {
        api_key_env.trim().to_string()
    };

    // Optional: enter actual API key value with masked echo for this session.
    writeln!(
        out,
        "\n \x1b[90mPaste your API key to set it now (or press Enter to skip):\x1b[0m"
    )?;
    let api_key_val = prompt_secret(out, &format!(" {api_key_env} (masked): "), "")?;
    let api_key_val = api_key_val.trim().to_string();

    if !api_key_val.is_empty() {
        // Export into the current process so child processes inherit it.
        // The user can also add it to their shell profile permanently.
        // SAFETY: setup wizard runs single-threaded before the REPL starts.
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var(&api_key_env, &api_key_val);
        }
        writeln!(
            out,
            " \x1b[32m✓\x1b[0m \x1b[90mSet {api_key_env} for this session.\x1b[0m"
        )?;
        writeln!(
            out,
            " \x1b[90m  Add `export {api_key_env}=<key>` to your shell profile to persist it.\x1b[0m"
        )?;
    } else {
        writeln!(
            out,
            " \x1b[90mMake sure to export {api_key_env}=sk-ant-... in your shell config.\x1b[0m"
        )?;
    }

    Ok(format!(
        r#"# shako configuration
# Docs: https://github.com/solarisjon/shako

active_provider = "anthropic"

[providers.anthropic]
endpoint = "https://api.anthropic.com"
model = "{model}"
api_key_env = "{api_key_env}"
provider_type = "anthropic"

[behavior]
confirm_ai_commands = true
auto_correct_typos = true
safety_mode = "warn"  # "warn" | "block" | "off"

# [aliases]
# gs = "git status"
# ll = "ls -la"
"#,
        model = model,
        api_key_env = api_key_env,
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
# model = "claude-sonnet-4-6"
# api_key_env = "LLMPROXY_KEY"
# verify_ssl = false

# [providers.anthropic]
# endpoint = "https://api.anthropic.com"
# model = "claude-sonnet-4-6"
# api_key_env = "ANTHROPIC_API_KEY"
# provider_type = "anthropic"

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
    if p.exists() {
        Some(p)
    } else {
        None
    }
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

/// Prompt for a secret value (e.g. API key env var name) with masked echo.
///
/// Each character typed is shown as `·` so the user knows input is being
/// received, while the actual value stays hidden. Backspace is supported.
/// Returns the default if the user presses Enter without typing anything.
///
/// Falls back to plain `prompt_line` if raw-mode cannot be enabled.
fn prompt_secret(out: &mut impl Write, prompt: &str, default: &str) -> Result<String> {
    write!(out, "{}", prompt)?;
    out.flush()?;

    // Enter raw mode so we get each keypress without line buffering.
    if terminal::enable_raw_mode().is_err() {
        // Fallback: read normally (terminal might not support raw mode).
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        writeln!(out)?;
        out.flush()?;
        let input = input.trim().to_string();
        return Ok(if input.is_empty() {
            default.to_string()
        } else {
            input
        });
    }

    let mut secret = String::new();

    loop {
        if let Ok(Event::Key(key)) = event::read() {
            match (key.code, key.modifiers) {
                // Enter: finish input
                (KeyCode::Enter, _) => {
                    break;
                }
                // Ctrl-C / Ctrl-D: abort
                (KeyCode::Char('c'), KeyModifiers::CONTROL)
                | (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                    terminal::disable_raw_mode().ok();
                    writeln!(out)?;
                    out.flush()?;
                    anyhow::bail!("setup cancelled by user");
                }
                // Backspace: remove last char
                (KeyCode::Backspace, _) => {
                    if secret.pop().is_some() {
                        // Erase one mask character from the display.
                        write!(out, "\x08 \x08")?;
                        out.flush()?;
                    }
                }
                // Printable character: append to secret, show mask dot.
                (KeyCode::Char(c), _) => {
                    secret.push(c);
                    write!(out, "·")?;
                    out.flush()?;
                }
                _ => {}
            }
        }
    }

    terminal::disable_raw_mode().ok();
    // Move to next line after masked input.
    writeln!(out)?;
    out.flush()?;

    Ok(if secret.is_empty() {
        default.to_string()
    } else {
        secret
    })
}

/// Recommended tools with (binary, name, description, impact level).
/// Impact: "core" = significant shako experience uplift, "nice" = useful but optional.
const RECOMMENDED_TOOLS: &[(&str, &str, &str, &str)] = &[
    (
        "starship",
        "Starship",
        "cross-shell prompt with git, rust, node info",
        "core",
    ),
    (
        "eza",
        "eza",
        "modern ls with icons, git status, tree view",
        "core",
    ),
    (
        "bat",
        "bat",
        "cat with syntax highlighting and line numbers",
        "core",
    ),
    ("fd", "fd", "faster find with simpler syntax", "core"),
    (
        "rg",
        "ripgrep",
        "faster grep that respects .gitignore",
        "core",
    ),
    (
        "zoxide",
        "zoxide",
        "smart cd that learns your habits (powers z/zi)",
        "core",
    ),
    (
        "fzf",
        "fzf",
        "fuzzy finder for interactive selection (powers zi)",
        "core",
    ),
    ("dust", "dust", "visual disk usage (replaces du)", "nice"),
    (
        "delta",
        "delta",
        "side-by-side diff with syntax highlighting",
        "nice",
    ),
    ("procs", "procs", "modern ps with color and search", "nice"),
    ("sd", "sd", "simpler sed for find-and-replace", "nice"),
];

/// Check which recommended tools are installed and print a summary.
/// Only called on first run (after the setup wizard).
pub fn check_recommended_tools() {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut missing_core: Vec<&str> = Vec::new();
    let mut missing_nice: Vec<&str> = Vec::new();
    let mut all_installed = true;

    writeln!(out, "\x1b[1m Recommended tools:\x1b[0m\n").ok();

    for &(binary, _name, desc, impact) in RECOMMENDED_TOOLS {
        if which::which(binary).is_ok() {
            writeln!(
                out,
                "   \x1b[32m✓\x1b[0m \x1b[1m{binary:<10}\x1b[0m \x1b[90m{desc}\x1b[0m"
            )
            .ok();
        } else {
            all_installed = false;
            writeln!(
                out,
                "   \x1b[31m✗\x1b[0m \x1b[1m{binary:<10}\x1b[0m \x1b[90m{desc}\x1b[0m"
            )
            .ok();
            match impact {
                "core" => missing_core.push(binary),
                _ => missing_nice.push(binary),
            }
        }
    }

    writeln!(out).ok();

    if all_installed {
        writeln!(out, " \x1b[32m✓ All recommended tools installed!\x1b[0m\n").ok();
        return;
    }

    // Detect package manager and show install command
    let (pm, install_cmd) = detect_package_manager();

    if !missing_core.is_empty() {
        let tools = missing_core.join(" ");
        writeln!(
            out,
            " \x1b[33mInstall recommended:\x1b[0m\n   {install_cmd} {tools}"
        )
        .ok();
    }

    if !missing_nice.is_empty() {
        let tools = missing_nice.join(" ");
        if missing_core.is_empty() {
            writeln!(out, " \x1b[90mOptional:\x1b[0m\n   {install_cmd} {tools}").ok();
        } else {
            writeln!(out, "\n \x1b[90mOptional:\x1b[0m\n   {install_cmd} {tools}").ok();
        }
    }

    writeln!(out).ok();
    writeln!(
        out,
        " \x1b[90mshako works without these, but they unlock smart aliases and better AI commands.\x1b[0m\n"
    )
    .ok();

    // Suggest the pm name if we fell back to generic
    if pm == "unknown" {
        writeln!(
            out,
            " \x1b[90mUse your system package manager to install the tools above.\x1b[0m\n"
        )
        .ok();
    }
}

/// Detect the system package manager and return (name, install_prefix).
fn detect_package_manager() -> (&'static str, &'static str) {
    if which::which("brew").is_ok() {
        ("brew", "brew install")
    } else if which::which("apt").is_ok() {
        ("apt", "sudo apt install")
    } else if which::which("dnf").is_ok() {
        ("dnf", "sudo dnf install")
    } else if which::which("pacman").is_ok() {
        ("pacman", "sudo pacman -S")
    } else if which::which("apk").is_ok() {
        ("apk", "sudo apk add")
    } else if which::which("pkg").is_ok() {
        ("pkg", "pkg install")
    } else if which::which("nix-env").is_ok() {
        ("nix", "nix-env -iA nixpkgs.")
    } else {
        ("unknown", "# install:")
    }
}
