# AGENTS.md — shako

## What This Is

**shako** — a fish-inspired interactive shell written in Rust where natural language and shell commands coexist. Type `ls` and it runs instantly (via `eza` if installed). Type `list all python files modified today` and it routes through an LLM, translates to a shell command, confirms with the user, and executes.

**Design philosophy**: Shell-first, AI-augmented — not an AI agent that happens to have shell access. Real commands execute with zero latency; only unrecognized input goes to the LLM.

## Commands

```bash
make build          # cargo build
make test           # cargo test < /dev/null (189+ tests)
make run            # cargo run
make check          # cargo check
make fmt            # cargo fmt
make lint           # cargo clippy -- -W warnings
make release        # cargo build --release
make install        # release build + copy to ~/.local/bin/shako
make register-shell # add to /etc/shells (requires sudo)
make clean          # cargo clean
```

**Minimum Rust version**: 1.85.0 (edition 2024)

**Logging**: Set `RUST_LOG=debug` (or `info`, `trace`) to enable `env_logger` output.

**Runtime flags**: `--quiet` / `-q` suppresses the startup banner. `--timings` / `-T` prints wall-clock execution time after each command.

## Project Structure

```
src/
├── main.rs              # Entry point, REPL loop, signal handling, multiline input,
│                        #   AI error recovery UX, startup banner, !! and !$ history expansion
run_function() → control::parse_body() + control::exec_statements() → ExecSignal
  │                                        propagates Return/Break/Continue upward
  └── restores local variables after function exit
├── classifier.rs        # Input classification with typo detection (strsim/Damerau-Levenshtein)
│                        #   uses shared PathCache; detects NL-looking args (looks_like_natural_language)
├── executor.rs          # Process execution: pipes, redirects (stdout, stderr, 2>&1), chains,
│                        #   background spawning, child process groups via pre_exec/setpgid
│                        #   pipeline child cleanup on spawn failure
├── parser.rs            # Tokenizer: quoting, env expansion, globs, tilde, command substitution
│                        #   handles $(), backticks, nested substitution, chain/pipe splitting
│                        #   $((arithmetic)): full recursive descent evaluator (+,-,*,/,%,**,cmp,&&,||,!)
│                        #   brace expansion: {a,b,c} and {1..10} sequences
│                        #   herestring: <<< word (expands to stdin string)
├── builtins/
│   ├── mod.rs           # Dispatch (run_builtin, is_builtin, BUILTINS, try_define_function,
│   │                    #   run_function -> i32), builtins: cd, z, zi, alias, unalias,
│   │                    #   abbr, export, unset, history, type, functions, return, command
│   │                    #   FUNCTION_RETURN thread-local for early return from function bodies
│   │                    #   run_builtin_no_state() dispatches echo/test/return/cd inside functions
│   ├── state.rs         # ShellState, Job, ShellFunction structs and impl
│   │                    #   ShellState.ai_session_memory: Vec<(String,String)> rolling AI context (cap 5)
│   ├── jobs.rs          # builtin_jobs, builtin_fg, builtin_bg, builtin_disown, builtin_wait
│   ├── set.rs           # fish-compatible `set` builtin (-x/-g/-U/-e flags), PATH helpers
│   └── source.rs        # source_fish_string, source_conf_d, load_functions_dir,
│                        #   fish parsing helpers (fish_cmdsub_to_posix, parse_fish_function_file)
├── control.rs           # Control flow engine: parse_body(), exec_statements(), is/has_control_flow()
│                        #   Statement enum (Simple, If, For, While, Break, Continue, Local)
│                        #   ExecSignal enum (Normal, Break, Continue, Return)
│                        #   Tokenizer: split_semicolons, leading_keyword, emit_segment
│                        #   Recursive-descent Parser struct; handles nested if/for/while
│                        #   exec_one dispatches to run_builtin_stateless or executor
│                        #   local variable save/restore via Vec<(String, Option<String>)>
├── safety.rs            # Dangerous command pattern matching (wired into AI pipeline)
├── setup.rs             # First-run wizard (interactive provider config)
│                        #   Starship config merging (ensure_starship_config)
├── smart_defaults.rs    # Modern tool detection (eza, bat, fd, rg, dust, procs, sd, delta,
│                        #   btop, bottom, duf, doggo, xh, tokei, zoxide, fzf) + auto-aliasing
│                        #   git shortcuts (gs, gl, gd, gp, gco, gcm), docker shortcuts
├── path_cache.rs        # Shared PATH command cache (Arc<PathCache>) used by classifier,
│                        #   completer, and highlighter — scanned once at startup
├── ai/
│   ├── mod.rs           # Orchestrator: translate_and_execute(), diagnose_error(),
│   │                    #   explain_command(), suggest_commit(), search_history(); collapse_multiline()
│   │                    #   guards against multi-line AI responses; session memory rolling context;
│   │                    #   multi-command step preview for && / ; / pipe chains; refine loop
│   ├── client.rs        # OpenAI-compatible LLM HTTP client (rustls-tls-native-roots)
│   │                    #   single retry with 2s delay on transient errors
│   ├── context.rs       # Shell context (OS, arch, cwd, user, dir listings, tool preferences,
│   │                    #   user_preferences from learned_prefs); git context; .shako.toml
│   │                    #   injects ai_session_memory as recent conversation history
│   ├── prompt.rs        # System prompts: translation (single-command rule), error recovery,
│   │                    #   explain, commit message; injects learned user preferences
│   └── confirm.rs       # Confirmation UX: [Y]es / [n]o / [e]dit / [w]hy / [r]efine
│                        #   [r]efine: prompts for clarification, re-queries LLM with amended input
├── shell/
│   ├── mod.rs           # Re-exports
│   ├── prompt.rs        # Starship integration, exit code + duration tracking (atomics)
│   │                    #   right prompt rendered in background thread
│   ├── highlighter.rs   # Rich syntax highlighting: command (green), builtin (cyan),
│   │                    #   AI prefix (purple), path (yellow), unknown (red),
│   │                    #   flags (blue), pipes/redirects (cyan), strings (yellow),
│   │                    #   variables (green), comments (gray) — uses PathCache
│   ├── completer.rs     # Smart tab completion (git branch/ref, git subcommands, cargo, docker,
│   │                    #   kubectl, make targets, just, npm/pnpm/yarn/bun, brew, go, rustup,
│   │                    #   helm, terraform, ssh/scp/sftp hosts from ~/.ssh/config, dirs, paths)
│   │                    #   first-token: PATH + builtins + aliases + functions via Arc<RwLock>
│   │                    #   flag completion: git (commit/push/log/diff/clone) and cargo subcommands
│   └── hinter.rs        # Autosuggestions via reedline DefaultHinter (gray inline hints)
├── proactive.rs         # Post-command hooks: after `git add`, offers AI commit message
│                        #   also triggers after `git clone` (suggests cd into repo) and
│                        #   after `cd` into a dir with Makefile/justfile (suggests make targets)
├── learned_prefs.rs     # Watch-and-learn: extracts tool substitutions from user edits,
│                        #   persists to ~/.config/shako/learned_prefs.toml, injects into
│                        #   AI context as "prefer rg over grep" style hints
└── config/
    ├── mod.rs           # Re-exports ShakoConfig, LlmConfig
    └── schema.rs        # Config types, XDG-aware path resolution, serde defaults,
                         #   multi-provider support ([providers.*] + active_provider)
```

## Architecture & Data Flow

```
User Input → Reedline → Multiline continuation (if trailing \ or unclosed quotes)
           → Alias expansion (state.expand_alias)
           → Function definition check (starts_with "function ")
           → Background check (trailing &) → spawn_background()
           → Function call check → run_function() if first token matches
           → Classifier → ?
  ├── Classification::Command(...)       → executor::execute_command()
  │                                        → on failure (exit ≥2): offer_ai_recovery()
  ├── Classification::Builtin(...)       → builtins::run_builtin()
  ├── Classification::Typo{suggestion}   → prompt "did you mean X?" → run as builtin or command
  ├── Classification::NaturalLanguage(.) → ai::translate_and_execute()
  ├── Classification::ForcedAI(...)      → explain if bare command, else translate_and_execute()
  ├── Classification::HistorySearch(...) → ai::search_history() (prefixed with `??`)
  ├── Classification::ExplainCommand(.)  → ai::explain_command() (trailing ? syntax)
  └── Classification::Empty              → (skip)

  Control flow shortcut (before classifier):
  has_control_flow(input) → control::parse_body() + control::exec_statements()
```

### Classification Logic (classifier.rs)

Order matters:

1. Empty input → `Empty`
2. Starts with `?? ` → `HistorySearch` (semantic history search via LLM)
3. Starts with `? ` or `ai:` or `?<text>` → `ForcedAI`
4. Ends with `?` → `ExplainCommand` (explain without executing)
5. First token is in `BUILTINS` list → `Builtin`
5. First token starts with `/` or `./` (explicit path) → `Command`
6. First token found via `which` (in `$PATH`) → `Command` **unless** remaining args look like natural language (`looks_like_natural_language()` detects prose words like "the", "all", "in", "files", "modified", "today", etc. — requires ≥2 args and no flags/paths)
7. First token is within edit distance 2 of a known command AND input is ≤3 words → `Typo`
8. Everything else → `NaturalLanguage` (routed to LLM)

### AI Pipeline (ai/)

1. `context::build_context()` — gathers OS, arch, cwd, user, directory listings (cwd + home subtree), detected modern tools with syntax guidance, git state (branch, status, recent commits), per-project .shako.toml instructions, recent command history, and recent AI session memory
2. `prompt::system_prompt()` — formats system prompt with context, tool preferences, and directory context
3. `client::query_llm()` — sends to OpenAI-compatible endpoint (temperature 0.1)
4. If response is `SHAKO_CANNOT_TRANSLATE` or empty → error message
5. **Safety check** — `safety::is_dangerous()` blocks/warns based on `safety_mode`
6. **Extra warning** — `safety::needs_extra_confirmation()` for sudo/rm/mv/chmod/chown
7. If command contains `&&` / `;` / pipes → print numbered step preview before confirming
8. If `confirm_ai_commands` is true → `confirm::confirm_command()` → Y/n/e/w/r
   - `[r]efine` prompts for clarification, appends it to original query, and re-queries LLM
9. Execute via `executor::execute_command()`
10. On success: push (query, command) pair to `ai_session_memory` (capped at 5 exchanges)
    - `ai reset` / `ai forget` clears session memory

### AI Error Recovery

When a command exits with code ≥2 (skips 1 and signals ≥128):
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
- **Signal handling** — children get own process group via `setpgid`/`pre_exec`, SIGINT/SIGQUIT/SIGTSTP reset to defaults
- **Fake exit status** — `fake_status(code)` uses `sh -c "exit N"` to create `ExitStatus` on Unix

### Parser (parser.rs)

Full tokenizer and expansion engine:
- Single quotes: literal, no expansion, no escapes
- Double quotes: env var + command substitution + backslash escapes (`"`, `\`, `$`, `` ` ``), no glob
- Backslash escapes (outside quotes)
- `$VAR`, `${VAR}`, `$?` expansion
- `${VAR:-word}` / `${VAR:+word}` / `${VAR:?word}` / `${VAR:=word}` — default/alternate/error/assign
- `${VAR#pat}` / `${VAR##pat}` — strip shortest/longest prefix glob
- `${VAR%pat}` / `${VAR%%pat}` — strip shortest/longest suffix glob
- `${VAR/old/new}` / `${VAR//old/new}` — first/all replacement
- `${#VAR}` — string length
- `$(cmd)` and backtick command substitution (with nesting via `extract_balanced`)
- Tilde expansion (`~` → `$HOME`)
- Glob expansion (`*.rs`) via the `glob` crate — suppressed for quoted tokens
- Chain splitting (`;`, `&&`, `||`) respecting quotes
- Pipe splitting (`|` but not `||`) respecting quotes

### First-Run Setup (setup.rs)

When no config file exists, `ShakoConfig::load()` launches an interactive wizard:
1. LM Studio (local, `localhost:1234`)
2. Custom/work proxy (endpoint, model, API key env var, SSL verify)
3. Template for manual editing

Also: `ensure_starship_config()` creates `~/.config/shako/starship.toml` (merging user's global Starship config) with `[shell] unknown_indicator = "shako"` so Starship displays the shell name correctly.

## Key Patterns & Conventions

### Error Handling

- `anyhow::Result` throughout for propagation
- User-facing errors: `eprintln!("shako: {context}: {e}")`
- Config missing → runs first-time setup wizard, then falls back to defaults

### Async

- Tokio runtime created **once** in `main()`, used with `rt.block_on()` only for AI calls
- REPL loop is synchronous
- Only `ai::client::query_llm()` is async

### Configuration

- Multi-provider config: `[providers.name]` blocks + `active_provider = "name"` to select
- Legacy single-provider: `[llm]` block (used when `active_provider` is unset)
- Config path searched: `$XDG_CONFIG_HOME/shako/config.toml` → `~/.config/shako/config.toml` → platform default
- All fields have serde defaults — works with no config file
- Default endpoint: `http://localhost:11434/v1/chat/completions` (Ollama)
- Default model: `claude-haiku-4.5`
- Default API key env var: `SHAKO_LLM_KEY`
- `behavior.ai_enabled = false` disables AI routing entirely (all NL input falls through as command)
- `[aliases]` section loaded at startup, user config overrides smart defaults
- Auto-sources `~/.config/shako/init.sh` if it exists (supports alias, export, set, function definitions)

### State Management

- `ShellState` holds: aliases (`HashMap<String, String>`), functions (`HashMap<String, ShellFunction>`), jobs (`Vec<Job>`), history path, `ai_session_memory: Vec<(String,String)>` (rolling AI context, max 5 pairs)
- Exit code tracked via `AtomicI32` in `shell::prompt` (for starship + `$?`)
- Command duration tracked via `CommandTimer` using `AtomicU64` (for starship)
- Job count tracked via `AtomicUsize` (for starship jobs module)
- `SHLVL` incremented on entry, `STARSHIP_SHELL` set to `shako`
- `STARSHIP_SESSION_KEY` generated at startup (PID + timestamp) for stateful Starship modules
- `STARSHIP_LOG=error` suppresses Starship debug output

### Smart Defaults (smart_defaults.rs)

At startup, detects modern CLI tools and creates aliases:
- `eza` → replaces `ls` (with `--icons --group-directories-first`), adds `ll`, `la`, `lt`
- `bat` → replaces `cat` (with `--style=auto`), adds `preview`
- `fd` → replaces `find`, adds `ff` (files), `fdir` (dirs)
- `rg` → replaces `grep`
- `dust` → replaces `du`
- `procs` → replaces `ps`
- `sd` → replaces `sed`
- `delta` → replaces `diff`
- `btop`/`bottom` → replaces `top`
- `zoxide` → powers `z` and `zi` builtins, `cd` tracks visits via `zoxide_add()`
- `fzf` → powers `zi` interactive picker
- User config aliases always win over smart defaults

### AI Context (ai/context.rs)

The AI receives rich context:
- OS, arch, cwd, user, shell name
- **Directory context**: contents of cwd and home directory (+ one level of home subdirectories), capped at 50 entries per dir, 200 total
- **Tool preferences**: for each detected modern tool (fd, rg, eza, bat, dust, sd, procs, delta), the AI gets concrete syntax guidance so it generates correct commands

### Naming Conventions

- Structs: `PascalCase`; reedline trait impls use `Shako` prefix (`ShakoHighlighter`, `ShakoCompleter`)
- Functions: `snake_case`
- Builtin handlers: `builtin_cd()`, `builtin_export()`, etc. (module-private in `builtins/mod.rs` or submodules)
- Constants: `SCREAMING_SNAKE_CASE`
- Single source of truth: `pub const BUILTINS` in `builtins/mod.rs`
- Config struct: `ShakoConfig` (in `config/schema.rs`)

### Shell Builtins

Full list (`builtins::BUILTINS`):
`cd`, `exit`, `export`, `unset`, `set`, `source`, `alias`, `unalias`, `abbr`, `fish-import`, `history`, `type`, `z`, `zi`, `jobs`, `fg`, `bg`, `disown`, `wait`, `function`, `functions`, `echo`, `read`, `test`, `[`, `pwd`, `pushd`, `popd`, `dirs`, `true`, `false`, `return`, `command`

Notable:
- `set` is fish-compatible: `set -x VAR val` (export), `set -gx VAR val`, `set -e VAR` (erase), `set` (list all)
- `source` processes `alias`, `export`, `set`, and `function` definitions from files
- `type` checks builtins → functions → aliases → PATH (like bash `type`)
- `z`/`zi` fall back to regular `cd` if zoxide not installed
- `disown <job>` removes a job from the job table (background process runs independently after)
- `wait [job]` blocks until specified (or all) background jobs finish
- `echo` supports `-n` (no newline), `-e` (escape sequences: `\n \t \r \a \b \\`)
- `read` supports `-p prompt` and reads into named VAR (default: `REPLY`)
- `test`/`[` implements POSIX: file tests (`-f -d -e -r -w -x -s -L -z -n`), string (`= != ==`), integer (`-eq -ne -lt -le -gt -ge`), boolean (`! -a -o`)
- `pushd`/`popd`/`dirs` maintain `ShellState.dir_stack`; `dirs` prints cwd-first like bash

### Dependencies

| Crate | Purpose |
|---|---|
| `reedline` 0.46 | Line editor (Highlighter, Completer, Hinter, FileBackedHistory, Prompt traits) |
| `crossterm` 0.29 | Terminal size for starship |
| `tokio` 1 (full) | Async runtime for LLM calls |
| `reqwest` 0.12 | HTTP client (`rustls-tls-native-roots` — uses system CA store, no OpenSSL) |
| `serde` 1 / `serde_json` 1 | LLM API serialization |
| `toml` 0.8 | Config file parsing |
| `dirs` 6 | XDG/platform directory resolution |
| `anyhow` 1 / `thiserror` 2 | Error handling |
| `log` 0.4 / `env_logger` 0.11 | Logging |
| `which` 8 | Binary lookup in `$PATH` |
| `strsim` 0.11 | Damerau-Levenshtein distance for typo detection |
| `glob` 0.3 | Filename glob expansion |
| `nu-ansi-term` 0.50 | ANSI styling for reedline highlighter |
| `nix` 0.30 | Unix process groups, signals (job control) — `cfg(target_family = "unix")` only |

### Release Profile

```toml
[profile.release]
opt-level = "s"      # optimize for size
strip = "debuginfo"  # strip debug info only
lto = "thin"         # thin link-time optimization
```

## Testing

```bash
cargo test < /dev/null              # all tests (189+ — count grows with new features)
cargo test --lib                    # unit tests only (inline #[cfg(test)] modules)
cargo test --test integration       # integration tests only (tests/integration.rs)
cargo test classifier               # classifier + typo + NL detection tests
cargo test executor             # redirect parsing + chain tests
cargo test parser               # tokenizer, expansion, command substitution, arithmetic tests
```

Unit test modules are inline (`#[cfg(test)] mod tests`) in `classifier.rs`, `executor.rs`, `parser.rs`, `ai/client.rs`, `shell/completer.rs`, `proactive.rs`, and `learned_prefs.rs`.

Integration tests live in `tests/integration.rs` and exercise the compiled binary via `shako -c "..."`. They cover: basic execution, pipes, chains (`&&`/`||`/`;`), redirects, env var expansion, glob expansion, quoting, command substitution, type-checking builtins, `$((arithmetic))`, and the `return`/`command` builtins. **Note**: builtins that require `ShellState` (cd, alias, export, set) cannot be tested via `-c` mode because that path calls `executor::execute_command` directly, bypassing the REPL's builtin dispatch. Those are best tested at the unit level.

Tests use `assert!(matches!(...))` for enum variants and direct equality for strings. Some parser tests use `unsafe { env::set_var() }` to set up test env vars (cleaned up after).

## Gotchas

1. **Edition 2024** — `env::set_var`/`remove_var` require `unsafe`. This is correct and intentional throughout the codebase.
2. **Config path on macOS** — `dirs::config_dir()` returns `~/Library/Application Support`. The loader checks `~/.config` first for XDG consistency.
3. **`-c` mode bypasses builtins** — `shako -c "..."` calls `executor::execute_command` directly. Builtins that need `ShellState` (cd, alias, export, set, source) are not dispatched; they fail as if they were unknown external commands. Only the interactive REPL loop handles builtins correctly.
4. **reqwest uses native roots** — `rustls-tls-native-roots` loads system CA store. Required for corporate proxies. `verify_ssl = false` disables cert verification.
5. **Typo vs NL heuristic** — typo detection only fires for ≤3 word inputs. Prevents `list all files` matching `lint`.
6. **Command + NL args** — even valid commands like `find` get routed to AI if args look like prose (detected by `looks_like_natural_language()`). Flags or path-like args override this.
7. **AI recovery skips exit 1** — exit code 1 is too common (grep no-match, test failures). Only exit ≥2 triggers the prompt. Signals (≥128) also skipped.
8. **Smart defaults never override** — user's `[aliases]` config always wins.
9. **Functions use `;` as separator** — function bodies split on `;` for multi-statement execution.
10. **Background `&` check** — `input.ends_with('&') && !input.ends_with("&&")` to avoid matching `&&`.
11. **History on macOS** — stored at `~/Library/Application Support/shako/history.txt` via `dirs::data_dir()`.
12. **Starship shell name** — `STARSHIP_SHELL=shako` is set at startup so starship shows the correct shell.
13. **Starship config merging** — `setup::ensure_starship_config()` creates `~/.config/shako/starship.toml` once, merging the user's global config with `[shell] unknown_indicator = "shako"`. `STARSHIP_CONFIG` env var points to this file.
14. **Right prompt threading** — `StarshipPrompt::render_prompt_left()` spawns a background thread for the right prompt render, joining it in `render_prompt_right()`. This parallelizes the two starship subprocess calls.
15. **CI** — `.github/workflows/ci.yml` runs `cargo test` + `cargo clippy` on push/PR (ubuntu + macOS).
16. **First-run wizard** — if no config file exists, the shell launches an interactive setup wizard before the REPL starts.
17. **LLM temperature** — configurable via `temperature` field in `LlmConfig` (default `0.1`). LLM client retries once with 2s delay on transient network errors.
18. **Vi mode Tab completion** — `Vi::default()` has no Tab binding. We use `Vi::new(insert_kb, normal_kb)` with Tab explicitly added to `insert_keybindings`. `edit_mode = "vi"` in config requires this.
19. **`suppress_echo()` / `restore_echo()` pairing** — `suppress_echo()` sets ECHO=0 after every foreground exit to silence late vim escape responses. `restore_echo()` re-enables ECHO before each `read_line()` call so reedline saves a clean baseline; otherwise the ColumnarMenu can break.
20. **`collapse_multiline()`** — if the LLM returns multiple lines (alternatives), only the first non-blank non-prose line is used. A yellow warning is printed. The system prompt also tells the model to return a single command.
21. **Watch-and-learn prefs path** — `~/.config/shako/learned_prefs.toml`. Safe to delete; defaults to empty on missing/corrupt file. Populated automatically when user edits an AI suggestion.
22. **Proactive commit fires only after `git add`** — `proactive::check()` is called in the `Classification::Command` success path only, not after AI translations, builtins, or background commands.
