# AGENTS.md — jbosh

## What This Is

**jbosh** (Jon's Brilliant Operating Shell) — a fish-inspired interactive shell written in Rust where natural language and shell commands coexist. Type `ls` and it runs instantly (via `eza` if installed). Type `list all python files modified today` and it routes through an LLM, translates to a shell command, confirms with the user, and executes.

**Design philosophy**: Shell-first, AI-augmented — not an AI agent that happens to have shell access. Real commands execute with zero latency; only unrecognized input goes to the LLM.

## Commands

```bash
make build          # cargo build
make test           # cargo test (34 tests)
make run            # cargo run
make check          # cargo check
make fmt            # cargo fmt
make lint           # cargo clippy -- -W warnings
make release        # cargo build --release
make install        # release build + copy to ~/.local/bin/jbosh
make register-shell # add to /etc/shells (requires sudo)
make clean          # cargo clean
```

**Minimum Rust version**: 1.85.0 (edition 2024)

**Logging**: Set `RUST_LOG=debug` (or `info`, `trace`) to enable `env_logger` output.

## Project Structure

```
src/
├── main.rs              # Entry point, REPL loop, signal handling, multiline input
├── classifier.rs        # Input classification with typo detection (strsim/Damerau-Levenshtein)
│                        #   caches PATH commands at startup
├── executor.rs          # Process execution: pipes, redirects, chains, background spawning
│                        #   child process groups via pre_exec/setpgid
├── parser.rs            # Tokenizer: quoting, env expansion, globs, tilde, command substitution
│                        #   handles $(), backticks, nested substitution, chain/pipe splitting
├── builtins.rs          # Builtins, ShellState (aliases, functions, jobs), job control
├── safety.rs            # Dangerous command pattern matching (wired into AI pipeline)
├── smart_defaults.rs    # Modern tool detection (eza, bat, fd, rg, zoxide, fzf) + auto-aliasing
├── ai/
│   ├── mod.rs           # Orchestrator: translate_and_execute(), diagnose_error()
│   ├── client.rs        # OpenAI-compatible LLM HTTP client (rustls-tls-native-roots)
│   ├── context.rs       # Shell context (OS, arch, cwd, user)
│   ├── prompt.rs        # System prompts: translation + error recovery
│   └── confirm.rs       # Confirmation UX: [Y]es / [n]o / [e]dit
├── shell/
│   ├── mod.rs           # Re-exports
│   ├── prompt.rs        # Starship integration, exit code + duration tracking (atomics)
│   ├── highlighter.rs   # Syntax highlighting (green/cyan/purple/yellow/red)
│   ├── completer.rs     # Smart tab completion (git, cargo, docker, kubectl, make, sudo, dirs)
│   └── hinter.rs        # History-based autosuggestions
└── config/
    ├── mod.rs           # Re-exports
    └── schema.rs        # Config types, XDG-aware path resolution, serde defaults, [aliases] map
```

## Architecture & Data Flow

```
User Input → Reedline → Multiline continuation (if trailing \ or unclosed quotes)
           → Alias expansion
           → Function check → run_function() if match
           → Background check (trailing &) → spawn_background()
           → Classifier → ?
  ├── Classification::Command(...)       → executor::execute_command()
  │                                        → on failure: offer_ai_recovery()
  ├── Classification::Builtin(...)       → builtins::run_builtin()
  ├── Classification::Typo{suggestion}   → prompt "did you mean X?" → execute if yes
  ├── Classification::NaturalLanguage(.) → ai::translate_and_execute()
  ├── Classification::ForcedAI(...)      → ai::translate_and_execute()
  └── Classification::Empty              → (skip)
```

### Classification Logic (classifier.rs)

Order matters:

1. Empty input → `Empty`
2. Starts with `? ` or `ai:` or `?<text>` → `ForcedAI`
3. First token is in `BUILTINS` list → `Builtin`
4. First token starts with `/` or `./` (explicit path) → `Command`
5. First token found via `which` (in `$PATH`) → `Command`
6. First token is within edit distance 2 of a known command AND input is ≤3 words → `Typo`
7. Everything else → `NaturalLanguage` (routed to LLM)

### AI Pipeline (ai/)

1. `context::build_context()` — gathers OS, arch, cwd, user
2. `prompt::system_prompt()` — formats system prompt with context
3. `client::query_llm()` — sends to OpenAI-compatible endpoint
4. If response is `JBOSH_CANNOT_TRANSLATE` or empty → error message
5. **Safety check** — `safety::is_dangerous()` blocks/warns based on `safety_mode`
6. **Extra warning** — `safety::needs_extra_confirmation()` for sudo/rm/chmod
7. If `confirm_ai_commands` is true → `confirm::confirm_command()` → Y/n/e
8. Execute via `executor::execute_command()`

### AI Error Recovery

When a command exits with code ≥2 (skips 1 and signals):
1. User gets `[y/N]` prompt (default no — never slows you down)
2. AI receives command + exit code
3. Returns structured `CAUSE:` + `FIX:` response
4. Fix is offered with `[Y]es / [n]o / [e]dit`

### Executor (executor.rs)

Handles:
- **Simple commands** — tokenized via parser (quoting, expansion, globs)
- **Pipelines** — `ls | grep foo | wc -l` → chained with piped stdout/stdin
- **Redirects** — `>`, `>>`, `<` with or without spaces
- **Chains** — `&&`, `||`, `;` with correct short-circuit logic
- **Background** — `spawn_background()` returns a `Child` tracked in `ShellState.jobs`
- **Signal handling** — children get own process group via `setpgid`/`pre_exec`

### Parser (parser.rs)

Full tokenizer and expansion engine:
- Single quotes: literal, no expansion
- Double quotes: env var + command substitution, no glob
- Backslash escapes
- `$VAR`, `${VAR}`, `$?` expansion
- `$(cmd)` and backtick command substitution (with nesting)
- Tilde expansion (`~` → `$HOME`)
- Glob expansion (`*.rs`) via the `glob` crate
- Chain splitting (`;`, `&&`, `||`) respecting quotes
- Pipe splitting (`|` but not `||`) respecting quotes

## Key Patterns & Conventions

### Error Handling

- `anyhow::Result` throughout for propagation
- User-facing errors: `eprintln!("jbosh: {context}: {e}")`
- Config missing → falls back to defaults silently

### Async

- Tokio runtime created **once** in `main()`, used with `rt.block_on()` only for AI calls
- REPL loop is synchronous
- Only `ai::client::query_llm()` is async

### Configuration

- Config path searched: `$XDG_CONFIG_HOME/jbosh/config.toml` → `~/.config/jbosh/config.toml` → platform default
- All fields have serde defaults — works with no config file
- `[aliases]` section loaded at startup, user config overrides smart defaults
- Auto-sources `~/.config/jbosh/init.sh` if it exists

### State Management

- `ShellState` holds: aliases, functions, jobs, history path
- Exit code tracked via atomics in `shell::prompt` (for starship + `$?`)
- Command duration tracked via `CommandTimer` (for starship)
- `SHLVL` incremented on entry, `STARSHIP_SHELL` set to `jbosh`

### Smart Defaults (smart_defaults.rs)

At startup, detects modern CLI tools and creates aliases:
- `eza` → replaces `ls`, `ll`, `la`, `lt`
- `bat` → replaces `cat`
- `fd` → replaces `find`, adds `ff`, `fdir`
- `rg` → replaces `grep`
- `zoxide` → powers `z` and `zi` builtins, `cd` tracks visits
- `fzf` → powers `zi` interactive picker
- User config aliases always win over smart defaults

### Naming Conventions

- Structs: `PascalCase` with `Jbosh` prefix for reedline traits
- Functions: `snake_case`
- Builtin handlers: `builtin_cd()`, `builtin_export()`, etc.
- Constants: `SCREAMING_SNAKE_CASE`
- Single source of truth: `pub const BUILTINS` in `builtins.rs`

### Dependencies

| Crate | Purpose |
|---|---|
| `reedline` | Line editor (Highlighter, Completer, Hinter, FileBackedHistory, Prompt traits) |
| `crossterm` | Terminal size for starship |
| `tokio` | Async runtime for LLM calls |
| `reqwest` | HTTP client (rustls-tls-native-roots — uses system CA store) |
| `serde` / `serde_json` | LLM API serialization |
| `toml` | Config file parsing |
| `dirs` | XDG/platform directory resolution |
| `anyhow` / `thiserror` | Error handling |
| `which` | Binary lookup in `$PATH` |
| `strsim` | Damerau-Levenshtein distance for typo detection |
| `glob` | Filename glob expansion |
| `nu-ansi-term` | ANSI styling for reedline highlighter |
| `nix` | Unix process groups, signals (job control) |

## Testing

```bash
cargo test                      # all 34 tests
cargo test classifier           # classifier + typo tests
cargo test executor             # redirect parsing + chain tests
cargo test parser               # tokenizer, expansion, command substitution tests
```

Tests use `assert!(matches!(...))` for enum variants and direct equality for strings.

## Gotchas

1. **Edition 2024** — `env::set_var`/`remove_var` require `unsafe`. This is correct.
2. **Config path on macOS** — `dirs::config_dir()` returns `~/Library/Application Support`. The loader checks `~/.config` first.
3. **reqwest uses native roots** — `rustls-tls-native-roots` loads system CA store. Required for corporate proxies.
4. **Typo vs NL heuristic** — only fires for ≤3 word inputs. Prevents `list all files` matching `lint`.
5. **AI recovery skips exit 1** — exit code 1 is too common (grep no-match, test failures). Only exit ≥2 triggers the prompt.
6. **Smart defaults never override** — user's `[aliases]` config always wins.
7. **Functions use `;` as separator** — function bodies split on `;` for multi-statement execution.
8. **Background `&` check** — `input.ends_with('&') && !input.ends_with("&&")` to avoid matching `&&`.
9. **History on macOS** — stored at `~/Library/Application Support/jbosh/history.txt` via `dirs::data_dir()`.
10. **Starship shell name** — `STARSHIP_SHELL=jbosh` is set at startup so starship shows the correct shell.
