# shako

A fish-inspired shell where natural language and shell commands coexist. Type `ls` — it runs instantly. Type `list all python files modified today` — the AI translates, confirms, and executes.

**Shell-first, AI-augmented** — real commands execute with zero latency; only unrecognized input goes to the LLM.

![shako demo](demo.gif)

## Quick Start

```bash
cargo build --release
make install        # installs to ~/.local/bin/shako
shako               # first run launches interactive setup wizard
shako -c "ls -la"   # non-interactive: run one command and exit
shako --timings     # print startup phase breakdown
```

## How It Works

```
$ ls -la                          # runs immediately (via eza if installed)
$ show me disk usage by folder    # → AI translates → "dust" → [Y/n/e/w/r] → runs
$ ? grep                          # explains what grep does
$ grep -rn?                       # explains what -rn flags do
$ ? find all large files          # AI translates → fd/find command → [Y/n/e/w/r]
$ ?? rsync command last week      # AI-powered semantic history search
$ gti status                      # typo → "did you mean git status? [Y/n]"
$ gcc bad.c                       # fails → "ask AI for help? [y/N]" → suggests fix
$ git add .                       # → proactive: "commit with this message? [Y/n/e]"
$ z projects                      # zoxide smart jump
$ /validate                       # slash command → validate AI endpoint
$ /safety off                     # slash command → change safety mode (session)
```

Every input is classified in order:

1. Shell function → run it
2. `??` prefix → AI-powered history search
3. `?` or `ai:` prefix → AI mode (explain if bare command, translate otherwise)
4. Trailing `?` → explain command without executing
5. `/word` → slash command (shako meta-commands)
6. Builtin (`cd`, `exit`, `z`, `set`, etc.) → handle internally
7. Found in `$PATH` → execute directly (unless args look like prose → AI)
8. Close to a known command → typo suggestion
9. Everything else → AI translation

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
| **Slash commands** | `/validate`, `/config`, `/model`, `/safety`, `/provider`, `/history`, `/audit` |
| **Typo correction** | Levenshtein distance detection with `[Y/n]` prompt |
| **Fish compatibility** | `set -x`, fish config import, conf.d, functions directory |
| **Starship prompt** | Native integration with parallel left/right rendering |
| **AI Pipe Builder** | `\|? description` — step-by-step pipeline builder with live intermediate previews |
| **AI Audit Log** | Tamper-evident hash-chained JSONL journal of every AI interaction |
| **Secret Canary** | Scans AI-generated commands for credential exfiltration before confirmation |
| **Prompt injection firewall** | Sanitizes user-controlled strings before they reach the LLM system prompt |
| **Capability scoping** | Per-project `[ai.scope]` allowlist/denylist for AI-generated commands |
| **Behavioral fingerprinting** | Learns command patterns, flag preferences, and workflow habits for personalised AI |
| **Danger Replay / Undo** | Snapshots affected paths before risky commands; `undo that rm` to restore |
| **Environment drift detection** | Warns when destructive commands run shortly after a kubectl/AWS/Terraform context switch |
| **Incident mode** | `incident start/report` — timestamped runbook capture with AI post-mortem generation |

## Documentation

| Guide | What's covered |
|---|---|
| [New User Guide](docs/new-user-guide.md) | Step-by-step introduction for first-time users |
| [Quick Reference](docs/quick-reference.md) | Cheat sheet — syntax, modes, aliases, keybindings |
| [Getting Started](docs/getting-started.md) | Installation, first run, configuration wizard |
| [**Examples**](EXAMPLES.md) | **20 real-world use cases and feature showcases** |
| [AI Features](docs/ai-features.md) | Translation, explain mode, error recovery, project context |
| [Shell Features](docs/shell-features.md) | Builtins, pipes, redirects, job control, functions, history |
| [Smart Defaults](docs/smart-defaults.md) | Tool detection, auto-aliases, AI tool preferences |
| [Configuration](docs/configuration.md) | Full config reference, LLM providers, behavior settings |
| [Slash Commands](docs/slash-commands.md) | `/validate`, `/config`, `/model`, `/safety`, `/provider`, `/history`, `/audit` |
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

Apache-2.0
