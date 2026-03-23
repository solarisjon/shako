# jbosh — Jon's Brilliant Operating Shell

A fish-inspired shell with transparent AI command translation and modern CLI tool defaults.

Type `ls` — it runs (via `eza` if installed). Type `list all python files modified today` — the AI translates it, shows you the command, and runs it after confirmation. Type `gti status` — it asks "did you mean `git status`?"

## Quick Start

```bash
cargo build --release
make install        # installs to ~/.local/bin/jbosh
jbosh
```

## How It Works

```
$ ls -la                          # runs immediately (via eza if installed)
$ show me disk usage by folder    # → AI translates → "dust" (not du!) → [Y/n/e] → runs
$ ? grep                          # forced AI mode → explains what grep does
$ gti status                      # typo → "did you mean git status? [Y/n]"
$ gcc bad.c                       # fails → "ask AI for help? [y/N]" → suggests fix
$ z projects                      # zoxide smart jump
$ deploy staging                  # runs your shell function
```

### Input Classification

1. Shell function? → run the function body
2. Starts with `?` or `ai:`? → force AI mode
3. First token is a builtin (`cd`, `exit`, `z`, `set`, etc.)? → handle internally
4. First token resolves to a binary in `$PATH`? → execute directly (zero latency)
5. Close to a known command (≤2 edits, ≤3 words)? → typo suggestion
6. None of the above? → route to AI for translation

### Visual Feedback

Commands are syntax-highlighted as you type:
- **Green** — valid command (in `$PATH`)
- **Cyan** — shell builtin
- **Purple** — AI prefix (`?`, `ai:`)
- **Yellow** — explicit path (`./script.sh`)
- **Red** — unknown (will route to AI)

## Installation

```bash
# Build and install
make install                    # builds release, copies to ~/.local/bin/jbosh

# Register as a login shell (requires sudo)
make register-shell             # adds to /etc/shells

# Set as your default shell
chsh -s ~/.local/bin/jbosh
```

### Requirements

- Rust 1.85.0+ (edition 2024)
- [Starship](https://starship.rs/) prompt (optional but recommended — jbosh integrates natively)

### Recommended Tools

jbosh automatically detects and prefers modern CLI tools. Install any of these and they become the default — both as shell aliases **and** in AI-generated commands:

| Install | Replaces | What you get |
|---|---|---|
| [eza](https://eza.rocks/) | `ls` | Icons, git status, color |
| [bat](https://github.com/sharkdp/bat) | `cat` | Syntax highlighting |
| [fd](https://github.com/sharkdp/fd) | `find` | Faster, simpler syntax |
| [ripgrep](https://github.com/BurntSushi/ripgrep) | `grep` | Faster, respects .gitignore |
| [zoxide](https://github.com/ajeetdsouza/zoxide) | `cd` (via `z`) | Smart directory jumping |
| [fzf](https://github.com/junegunn/fzf) | — | Fuzzy finder for `zi` |
| [dust](https://github.com/bootandy/dust) | `du` | Visual disk usage |
| [delta](https://github.com/dandavison/delta) | `diff` | Side-by-side, syntax-aware |
| [procs](https://github.com/dalance/procs) | `ps` | Colored, searchable |

```bash
# macOS
brew install eza bat fd ripgrep zoxide fzf

# These aliases are set automatically — no config needed:
# ls  → eza --icons --group-directories-first
# cat → bat --style=auto
# ll  → eza -la --icons --group-directories-first
# lt  → eza --tree --icons --level=2
# ff  → fd --type f
# ... and more
```

The AI is also told which modern tools you have installed, so `find all rust files` generates `fd -e rs` instead of `find . -name "*.rs"`.

Your `[aliases]` in config.toml always take priority over smart defaults.

## Configuration

On first launch jbosh runs an interactive setup wizard that creates `~/.config/jbosh/config.toml` for you. You can also create or edit it manually.

### LLM Providers

jbosh supports multiple named providers. Set `active_provider` to switch between them:

```toml
active_provider = "lm_studio"   # switch to "work_proxy" to use that instead

[providers.lm_studio]
endpoint = "http://localhost:1234/v1/chat/completions"
model = "your-model-name"       # model loaded in LM Studio
# no api_key_env needed — LM Studio doesn't require auth

[providers.work_proxy]
endpoint = "https://your-llm-proxy.company.com/v1/chat/completions"
model = "claude-sonnet-4.5"
api_key_env = "JBOSH_LLM_KEY"  # name of env var holding your API key
verify_ssl = false              # set false for internal/self-signed CAs
```

Any OpenAI-compatible endpoint works. The legacy `[llm]` block is still supported as a fallback when `active_provider` is not set.

### Full Config Reference

```toml
active_provider = "lm_studio"

[providers.lm_studio]
endpoint = "http://localhost:1234/v1/chat/completions"
model = "your-model-name"

[providers.work_proxy]
endpoint = "https://your-llm-proxy.company.com/v1/chat/completions"
model = "claude-sonnet-4.5"
api_key_env = "JBOSH_LLM_KEY"
timeout_secs = 30
max_tokens = 512
verify_ssl = false

[behavior]
confirm_ai_commands = true          # show translated command before executing
auto_correct_typos = true           # suggest corrections for near-miss commands
history_context_lines = 20          # history lines sent as AI context
safety_mode = "warn"                # "warn" | "block" | "off"

[aliases]
ll = "ls -la"
la = "ls -A"
".." = "cd .."
"..." = "cd ../.."
gs = "git status"
gd = "git diff"
gl = "git log --oneline -20"
```

### Init File

If `~/.config/jbosh/init.sh` exists, it's sourced automatically at startup. Supports `alias`, `export`, `set` (fish-style), and `function` definitions:

```bash
# ~/.config/jbosh/init.sh

# POSIX style
alias k='kubectl'
export EDITOR=nvim

# Fish style — both work
set -x GOPATH ~/go
set -gx DOCKER_HOST unix:///var/run/docker.sock

# Functions
function deploy() { git push && ssh prod "cd /app && git pull" }
```

## Features

### Shell Basics
- **Pipes** — `ls | grep foo | wc -l`
- **Redirects** — `echo hello > file.txt`, `sort < input.txt`, `echo line >> log`
- **Command chaining** — `mkdir foo && cd foo`, `make || echo failed`, `cmd1; cmd2`
- **Quoting** — `echo "hello world"`, `echo 'no $expansion'`, `echo hello\ world`
- **Environment expansion** — `$HOME`, `${USER}`, `$?`
- **Glob expansion** — `ls *.rs`, `cat src/**/*.rs` (suppressed in quotes)
- **Tilde expansion** — `~/foo` → `/Users/you/foo`
- **Command substitution** — `echo $(date)`, `` echo `whoami` ``, `cd $(dirname $file)`, nested `$(echo $(pwd))`
- **Multiline input** — trailing `\` or unclosed quotes continue on next line
- **Background jobs** — `sleep 100 &`, `jobs`, `fg`, `bg %1`

### AI Integration
- **Natural language → command** — type what you want, AI translates, you confirm
- **Forced AI mode** — `? grep` or `ai: how do I find large files`
- **Tool-aware** — AI knows which modern tools you have and prefers them (fd over find, rg over grep, etc.)
- **Error recovery** — when a command fails (exit ≥2), jbosh offers AI diagnosis with a suggested fix
- **Safety layer** — dangerous AI-generated commands are blocked (`rm -rf /`) or warned (`sudo`, `chmod`)
- **Confirmation UX** — `[Y]es / [n]o / [e]dit` before any AI command runs

### Builtins

| Command | Description |
|---|---|
| `cd` | Change directory (tracks visits via zoxide if installed) |
| `z <query>` | Zoxide smart jump — `z proj` jumps to your projects dir |
| `zi` | Interactive directory picker (zoxide + fzf) |
| `exit` | Exit the shell |
| `export KEY=val` | Set environment variable (POSIX style) |
| `set -x KEY val` | Set/export environment variable (fish style) |
| `set -e KEY` | Erase environment variable (fish style) |
| `set` | List all environment variables |
| `unset KEY` | Remove environment variable |
| `alias name=value` | Define alias (or list all with no args) |
| `unalias name` | Remove alias (`-a` to clear all) |
| `history [N]` | Show last N history entries (default 25) |
| `source file` | Load aliases, exports, set, and functions from file |
| `type name` | Show how a name would be resolved (builtin/alias/function/path) |
| `function name() { body }` | Define a shell function |
| `functions` | List all defined functions |
| `jobs` | List background jobs |
| `fg [%N]` | Bring job to foreground |
| `bg [%N]` | Resume stopped job in background |

### Tab Completion

- **Commands** — all executables in `$PATH` + builtins
- **git** — subcommands (`status`, `commit`, `push`, `branch`, etc.)
- **cargo** — subcommands (`build`, `test`, `run`, `clippy`, etc.)
- **docker** — subcommands (`run`, `ps`, `build`, `exec`, etc.)
- **kubectl** — subcommands (`get`, `apply`, `describe`, `logs`, etc.)
- **make** — targets parsed from `Makefile` in current directory
- **sudo** — completes the next token as a command
- **cd/z** — directories only
- **Files** — path completion for everything else

### Prompt

jbosh integrates with [Starship](https://starship.rs/) for prompt rendering:

- Last command exit code, duration, and terminal width
- Background job count (drives Starship's jobs module)
- Current keymap (`emacs`)
- `STARSHIP_SESSION_KEY` set at startup for stateful modules
- Left and right prompts rendered in parallel (two `starship prompt` calls run simultaneously)

On first launch jbosh creates `~/.config/jbosh/starship.toml` — a copy of your global Starship config with the `[shell]` module enabled and `unknown_indicator = "jbosh"` so your prompt shows **jbosh** as the shell name. Edit that file to customise Starship specifically for jbosh.

If Starship isn't installed, a minimal `❯` prompt is used.

## Building

```bash
make build          # cargo build
make test           # cargo test (34 tests)
make run            # cargo run
make check          # cargo check
make fmt            # cargo fmt
make lint           # cargo clippy -- -W warnings
make release        # cargo build --release
make install        # release build + copy to ~/.local/bin
make register-shell # add to /etc/shells (requires sudo)
make clean          # cargo clean
```

## Project Structure

```
src/
├── main.rs              # Entry point, REPL loop, signal handling
├── classifier.rs        # Input classification with typo detection (strsim)
├── executor.rs          # Process execution: pipes, redirects, chains, background
├── parser.rs            # Tokenizer: quoting, expansion, globs, command substitution
├── builtins.rs          # Shell builtins, ShellState, job tracking, functions
├── safety.rs            # Dangerous command pattern matching
├── setup.rs             # First-run wizard: config + Starship setup
├── smart_defaults.rs    # Modern tool detection and auto-aliasing
├── ai/
│   ├── mod.rs           # AI orchestrator: translate, confirm, execute, diagnose
│   ├── client.rs        # OpenAI-compatible LLM HTTP client
│   ├── context.rs       # Shell context (OS, arch, cwd, user, available tools)
│   ├── prompt.rs        # System prompts for translation and error recovery
│   └── confirm.rs       # Confirmation UX: [Y]es / [n]o / [e]dit
├── shell/
│   ├── mod.rs           # Re-exports
│   ├── prompt.rs        # Starship integration, parallel rendering, job count
│   ├── highlighter.rs   # Syntax highlighting (green/cyan/purple/red)
│   ├── completer.rs     # Smart tab completion (git, cargo, docker, make, etc.)
│   └── hinter.rs        # History-based autosuggestions
└── config/
    ├── mod.rs           # Re-exports
    └── schema.rs        # Config types, multi-provider LLM, XDG-aware loading
```

## Roadmap

See [SCOPE.md](SCOPE.md) for the full design document and phased roadmap.

## License

MIT
