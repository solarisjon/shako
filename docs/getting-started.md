# Getting Started

## Installation

### From Source

```bash
git clone https://github.com/solarisjon/shako.git
cd shako
make install        # builds release binary, copies to ~/.local/bin/shako
```

Requires **Rust 1.85.0+** (edition 2024).

### Register as Login Shell

```bash
make register-shell   # adds ~/.local/bin/shako to /etc/shells (requires sudo)
chsh -s ~/.local/bin/shako
```

### Recommended Tools

Install these for the best experience. shako detects them automatically — no configuration needed:

```bash
# macOS
brew install starship eza bat fd ripgrep zoxide fzf dust delta procs duf

# Ubuntu/Debian
sudo apt install eza bat fd-find ripgrep zoxide fzf
```

See [Smart Defaults](smart-defaults.md) for the full list of detected tools.

## First Run

On first launch, shako runs an interactive setup wizard:

1. **LLM provider selection**:
   - **LM Studio** (local) — connects to `localhost:1234`
   - **Work/custom proxy** — any OpenAI-compatible endpoint
   - **Skip** — creates a template config for manual editing

2. **Fish config import** — if `~/.config/fish/` exists, offers to import your aliases, abbreviations, environment variables, PATH entries, and functions

3. **Recommended tools audit** — shows which modern CLI tools are installed and which are missing, with a one-line install command for your package manager

The wizard creates `~/.config/shako/config.toml`. You can re-run it anytime with:

```bash
shako --init
```

## Directory Structure

shako uses `~/.config/shako/` for all configuration:

```
~/.config/shako/
├── config.toml          # main configuration (LLM providers, behavior, aliases)
├── config.shako         # startup script (sourced at launch)
├── starship.toml        # shako-specific Starship prompt config
├── learned_prefs.toml   # AI tool preferences learned from your edits (auto-generated)
├── conf.d/              # config snippets, sourced alphabetically at startup
│   ├── 00-env.sh
│   └── 10-aliases.sh
└── functions/           # autoloaded shell functions (one per file)
    ├── deploy.sh
    └── mkcd.sh
```

History is stored at the platform data directory:
- **macOS**: `~/Library/Application Support/shako/history.txt`
- **Linux**: `~/.local/share/shako/history.txt`

## Startup Order

1. Load `config.toml` (aliases, provider config)
2. Apply smart defaults (modern tool aliases — user config aliases win)
3. Source `conf.d/*.sh` alphabetically
4. Source `config.shako` (main startup script, or `init.sh`/`init.fish` for backwards compat)
5. Register functions from `functions/` directory
6. Optionally source fish config (if `[fish] source_config = true`)

## Startup Banner

On launch, shako displays a styled banner showing:
- Version number
- AI status: `✓ ready`, `⚠ no api key`, `✗ auth failed`, or `✗ unreachable`
- Active provider name and model
- Current safety mode, edit mode, and typo-fix setting

Suppress the banner with `--quiet` or `-q`.

## Runtime Flags

| Flag | Effect |
|---|---|
| `--quiet` / `-q` | Suppress startup banner |
| `--timings` | Print startup phase timing breakdown (config, AI check, PATH scan, etc.) |
| `--init` | Re-run the setup wizard (removes existing config files and recreates them) |
| `-c "cmd"` | Run command non-interactively and exit |

## Next Steps

- [AI Features](ai-features.md) — translation, explain mode, error recovery
- [Configuration](configuration.md) — full config reference
- [Shell Features](shell-features.md) — builtins, pipes, job control
