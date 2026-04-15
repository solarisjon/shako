# Changelog

All notable changes to shako are documented here.

---

## [0.9.0] — 2026-04-14

Major security and intelligence release. Adds a layered AI security stack (audit log, prompt injection firewall, credential exfiltration guard, capability scoping), behavioral fingerprinting, the AI Pipe Builder, Danger Replay / Undo Graph, Incident Mode, Environment Drift Detection, and the `/history` and `/audit` slash commands.

### Added

#### AI Features
- **AI Pipe Builder** — `|? <description>` decomposes a natural-language pipeline goal into individual steps, executes each step incrementally against real data, and shows a live preview of intermediate output before the user commits to running the full command. Step descriptions appear in a gradient panel matching the existing confirmation UI (`src/pipe_builder.rs`).
- **DiagnosisResult + confirm_command flow** — the failure→fix loop is now fully closed: `DiagnosisResult` carries a structured `cause` and `fix` command that flow directly into the standard `[Y/n/e/w/r]` confirmation prompt, so the AI-suggested fix can be edited or refined before execution.
- **Behavioral Fingerprinting** — `BehavioralProfile` is built from command journal data and injected into every AI prompt as a compact hint (≤ 500 tokens). Tracked signals: command co-occurrence sequences, per-tool flag preferences, conventional-commit prefix style, and pre-command guard patterns (e.g. `source venv` always before `pip`). Persisted to `~/.config/shako/behavioral_profile.json`. Configurable via `[behavior] behavioral_fingerprinting = true/false` (`src/behavioral_profile.rs`).

#### Security Stack
- **Immutable AI Audit Log** — every AI query, generated command, and user decision (execute / edit / cancel / block) is appended to `~/.local/share/shako/audit.jsonl` as a JSONL record with a FNV-1a 64-bit hash chain. Any retroactive modification breaks the chain, detectable by `/audit verify`. Safety blocks and Secret Canary blocks are also recorded (`src/audit.rs`).
- **Prompt Injection Firewall** — user-controlled strings injected into the AI system prompt (`.shako.toml` `[ai].context`, `learned_prefs.toml` substitutions, `ai_system_prompt_extra`) are scanned for known injection phrases before insertion. Matched fields are stripped and the user is warned with the source path and pattern name. Clean fields are structurally wrapped in delimiter blocks to reduce blast radius even if the model ignores the strip (`src/ai/prompt_guard.rs`).
- **Secret Canary (ExfilGuard)** — scans every AI-generated command for credential exfiltration patterns before the confirmation prompt appears. `Critical` risk (secret-file access + outbound network in the same command) triggers a red ASCII warning box; `High` risk (secret access, no outbound command) triggers a yellow box. Both events are recorded in the audit log. Detection covers AWS credentials, SSH keys, GnuPG, `.netrc`, `.npmrc`, `.pypirc`, Docker config, kubeconfig, and common `$API_KEY`-style env var names (`src/ai/exfil_guard.rs`).
- **Capability-Scoped AI Sessions** — per-project `[ai.scope]` block in `.shako.toml` declares an explicit allowlist and denylist for AI-generated commands. Evaluation order: `deny_commands` wins over allow; then `allow_sudo`; then `allow_network`; then `allow_commands`. Violations are shown before the confirmation prompt (`src/ai/capability_scope.rs`).
- **Learned Prefs PATH validation** — paths injected via `learned_prefs.toml` substitutions are validated to prevent path-traversal abuse.

#### Shell Features
- **Danger Replay / Undo Graph** — before executing a confirmed dangerous command (e.g. `rm -rf old_build/`), shako optionally snapshots the affected paths to `~/.local/share/shako/snapshots/<sha>/` and records an entry in `~/.local/share/shako/undo_graph.json`. Natural-language undo requests (`undo that rm`, `restore what I deleted`, `go back`, `revert that`) are classified as `UndoRequest` and routed to the undo flow. Git-tracked paths are skipped. Max snapshot size: 50 MB (configurable). Snapshots GC'd after 7 days (`src/undo.rs`).
- **Environment Drift Detection** — `ContextTracker` snapshots `kubectl` context, `AWS_PROFILE`, `TF_WORKSPACE`, and `DOCKER_CONTEXT` after each command. When a destructive command is about to run within the configured warning window (default 5 min) of a context switch, a warning panel is shown. Production context detection is config-driven via `[safety] production_contexts` in `.shako.toml`. The prompt indicator turns amber in production contexts (`src/env_context.rs`).
- **Incident Mode** — structured runbook execution: `incident start <name>` activates incident mode and begins timestamping every command. `incident status` prints a live session summary. `incident end` closes the session. `incident report` closes the session and calls the AI to synthesise a post-mortem timeline and structured markdown runbook, optionally saving to `[incident] runbook_dir` from `.shako.toml` (`src/incident.rs`).
- **`/history` slash command** — fuzzy-browse shell history interactively. If `fzf` is in `$PATH`, history is piped through it for interactive selection; the chosen command is pre-filled in the readline buffer. Falls back to a native paginated picker when `fzf` is absent.
- **`/audit` slash command** — `audit verify` walks the entire JSONL audit log and reports whether the hash chain is intact. `audit search <query>` returns the most-recent matching entries across `nl_input`, `generated`, and `executed` fields.

### Fixed

- Filename completions now correctly escape single quotes and shell-special characters, preventing completion-time parse errors for paths containing `'`, `(`, `)`, `[`, `]`, `!`, `&`, `\`, and other metacharacters.

---

## [0.2.1] — 2026-03-30

Patch release adding slash commands, startup instrumentation, native Anthropic API support, and a styled startup banner, plus bug fixes and performance improvements.

### Added

- **Slash commands** — `/validate`, `/config`, `/model`, `/safety`, `/provider`, `/help` meta-commands configurable at runtime (#57)
- **`--timings` flag** — print startup phase breakdown (config, AI check, PATH scan, reedline setup, smart defaults, shell init) (#28)
- **Native Anthropic API engine** — set `provider_type = "anthropic"` in a provider block to use Anthropic's native API format instead of OpenAI-compatible
- **Styled startup banner** — box-drawing borders with teal-to-cyan gradient, AI status line, and config summary (#66)
- **Animated braille spinner** — shown during AI thinking/explaining/streaming to indicate progress (#61)
- **AI session validation on startup** — banner shows ✓ ready, ⚠ no api key, ✗ auth failed, or ✗ unreachable (#51)

### Fixed

- `??query` (no space after `??`) now correctly routes to history search instead of ForcedAI
- Spinner stops before the first streaming token arrives, preventing visual overlap (#61)
- Arithmetic `$((x/0))` and integer overflow now return an error instead of panicking (#71)
- Byte-safe token slicing using pointer arithmetic — prevents multi-byte character panics (#70)
- Thread panic now logged rather than silently swallowed; for-loop local scope fixed; false positive in classifier removed (#73)
- Startup banner right border alignment (#66)
- Safety hardening and UX consistency improvements (#56)

### Performance

- Replaced O(n) `Vec::remove(0)` and `Vec::contains` calls with O(1) equivalents in hot paths (#72)
- Reduced unnecessary clones and allocations in command dispatch (#54)

### Other

- Fish shell compliance improvements; multiline loop fix; `fd` flag constraint enforcement (#52)
- Renamed all `Jbosh*` prefixes to `Shako*` throughout the codebase (#53)

---

## [0.2.0] — 2026-03-26

Major feature release completing Phases 2–5 of the roadmap. shako now has full control flow, brace expansion, herestrings, 43 builtins, AI-powered history search, proactive suggestions, and 226 tests.

### Added

#### Shell Features
- **Control flow** — `if`/`elif`/`else`/`fi`, `for`/`while`/`do`/`done`, `break`, `continue`
- **Brace expansion** — `{a,b,c}` list form, `{1..10}` / `{a..z}` range form, zero-padding, reverse ranges (#49)
- **Herestring** — `grep foo <<< "$var"` pipes string to command stdin (#49)
- **Parameter expansion** — `${VAR:-default}`, `${VAR:+alt}`, `${VAR:?error}`, `${VAR#pat}`, `${VAR%pat}`, `${VAR/old/new}`, `${#VAR}`
- **Arithmetic expansion** — `$((2 + 2))` inline math
- **New builtins** — `echo`, `read`, `test`/`[`, `pwd`, `pushd`/`popd`/`dirs`, `return`, `command`, `break`, `continue`, `local`, `true`, `false`, `disown`, `wait` (43 builtins total)
- **User-defined functions** — `function name() { body }` with local variable scoping

#### AI Features
- **AI history search** — `?? rsync command last week` does semantic search over shell history (#47)
- **Proactive suggestions** — after `git add`, offers AI-generated commit message from staged diff (#43, #47)
- **Session memory** — AI remembers last 5 NL→command exchanges within a session; `ai reset` clears (#46)
- **Refine mode** — `[r]efine` in AI confirmation lets you clarify without starting over (#46)
- **Watch-and-learn** — edits to AI suggestions logged to `~/.config/shako/learned_prefs.toml`; preferences injected into future prompts (#42, #47)
- **Multi-step preview** — numbered preview for multi-command AI responses (#46)
- **`ai_enabled` config** — global kill switch for AI features in `[behavior]`

#### Completions
- **Flag completion** — `git commit --am<Tab>` → `--amend`; cargo subcommand flags (#47)
- **Git branch completion** — `git checkout <Tab>` lists branches (#34)
- **Alias/function completion** — user-defined aliases and functions appear in first-token completion
- **New tool completions** — npm/npx, pnpm, yarn, bun/bunx, brew, go, rustup, helm, terraform/tf, SSH hosts (#35)

#### Other
- **`[w]hy` option** — AI confirmation now has `[Y]es / [n]o / [e]dit / [w]hy / [r]efine` (#36)
- **Streaming AI responses** — tokens stream live to terminal (#37)
- **`-c` flag** — `shako -c "command"` for non-interactive execution
- **`edit_mode` config** — `"emacs"` (default) or `"vi"` keybindings
- **Proactive hints** — `cd` into dir with Makefile shows available targets; `git clone` suggests `cd <repo>`

### Fixed
- Stderr capture for AI diagnosis — `execute_command_with_stderr` captures last 20 lines
- `pre_exec` collision on `2>&1` — combined setpgid and stderr-dup into single closure
- `history_context_lines` wired up and used in AI context
- Clippy warnings resolved throughout

### Changed
- License corrected to Apache-2.0 (matching LICENSE file)
- Test suite expanded to 226 tests (119 unit + 107 integration)

---

## [0.1.0] — 2026-03-24

Initial release.

### Added
- Fish-inspired shell with natural language AI command translation
- Smart defaults (eza, bat, fd, rg, zoxide, fzf, dust, delta, procs, duf, and more)
- Syntax highlighting, autosuggestions, tab completion
- Typo correction with Levenshtein distance
- AI error recovery with stderr capture
- Starship prompt integration with parallel left/right rendering
- Fish config import (aliases, env, abbreviations, functions)
- Git/cargo/docker/kubectl/make subcommand completions
- `? command` explain mode and `command?` inline explain
- Per-project AI context via `.shako.toml`
- Safety layer for AI-generated commands (warn/block/off)
- Smart tool detection with AI syntax guidance
