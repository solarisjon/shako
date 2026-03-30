# Changelog

All notable changes to shako are documented here.

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
