# shako

A fish-inspired shell where natural language and shell commands coexist. Type `ls` — it runs instantly. Type `list all python files modified today` — the AI translates, confirms, and executes.

**Shell-first, AI-augmented** — real commands execute with zero latency; only unrecognized input goes to the LLM.

![shako demo](demo.gif)

## Quick Start

```bash
cargo build --release
make install        # installs to ~/.local/bin/shako
shako               # first run launches interactive setup wizard
```

## How It Works

```
$ ls -la                          # runs immediately (via eza if installed)
$ show me disk usage by folder    # → AI translates → "dust" → [Y/n/e] → runs
$ ? grep                          # explains what grep does
$ grep -rn?                       # explains what -rn flags do
$ ? find all large files          # AI translates → fd/find command → [Y/n/e]
$ gti status                      # typo → "did you mean git status? [Y/n]"
$ gcc bad.c                       # fails → "ask AI for help? [y/N]" → suggests fix
$ z projects                      # zoxide smart jump
```

Every input is classified in order:

1. Shell function → run it
2. `?` or `ai:` prefix → AI mode (explain if bare command, translate otherwise)
3. Trailing `?` → explain command without executing
4. Builtin (`cd`, `exit`, `z`, `set`, etc.) → handle internally
5. Found in `$PATH` → execute directly (unless args look like prose → AI)
6. Close to a known command → typo suggestion
7. Everything else → AI translation

## Key Features

| Feature | Description |
|---|---|
| **AI translation** | Natural language → shell commands with `[Y/n/e]` confirmation |
| **Explain mode** | `? grep` or `chmod 755?` — AI explains without executing |
| **Error recovery** | Failed commands get AI diagnosis with suggested fix |
| **Smart defaults** | Auto-detects eza, bat, fd, rg, zoxide, fzf, dust, delta, duf, and more |
| **Git shortcuts** | `gs`, `gl`, `gd`, `gp`, `gco`, `gcm` (auto-created if git installed) |
| **Project context** | `.shako.toml` gives the AI project-specific instructions |
| **Git-aware AI** | AI sees your current branch, dirty/clean status, recent commits |
| **History context** | AI sees your recent commands for follow-up queries |
| **Syntax highlighting** | Full-line: commands, flags, strings, pipes, variables, comments |
| **Tab completion** | git, cargo, docker, kubectl, make targets, paths, commands |
| **Typo correction** | Levenshtein distance detection with `[Y/n]` prompt |
| **Fish compatibility** | `set -x`, fish config import, conf.d, functions directory |
| **Starship prompt** | Native integration with parallel left/right rendering |

## Documentation

| Guide | What's covered |
|---|---|
| [Getting Started](docs/getting-started.md) | Installation, first run, configuration wizard |
| [AI Features](docs/ai-features.md) | Translation, explain mode, error recovery, project context |
| [Smart Defaults](docs/smart-defaults.md) | Tool detection, auto-aliases, AI tool preferences |
| [Configuration](docs/configuration.md) | Full config reference, LLM providers, behavior settings |
| [Shell Features](docs/shell-features.md) | Builtins, pipes, redirects, job control, functions, history |
| [ROADMAP](ROADMAP.md) | Planned features and architecture improvements |
| [SCOPE](SCOPE.md) | Original design document |

## Building

```bash
make build          # cargo build
make test           # cargo test
make lint           # cargo clippy -- -W warnings
make install        # release build + copy to ~/.local/bin
make register-shell # add to /etc/shells (requires sudo)
```

Requires Rust 1.85.0+ (edition 2024).

## License

MIT
