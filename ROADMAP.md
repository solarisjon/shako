# ROADMAP.md — shako

Living document tracking gaps, bugs, feature ideas, and priorities for making shako a daily-driver shell.

---

## Bugs / Broken Wiring

- [x] **Stderr never captured for AI diagnosis** — fixed; `execute_command_with_stderr` captures stderr and passes last 20 lines to `diagnose_error()`.
- [x] **`history_context_lines` declared but never used** — fixed; `read_recent_history()` now reads this from config and passes to AI context on every query.
- [x] **`pre_exec` collision on `2>&1`** — fixed; `setup_child_signals()` combines setpgid and stderr-dup into a single `pre_exec` closure. Comment in `executor.rs` documents the fix.

---

## Quick Wins (high value, low effort)

- [x] **Git context for AI** — branch, dirty/clean status, and recent commits are sent to the AI on every query.
- [x] **Project type detection** — detects `Cargo.toml`, `package.json`, `go.mod`, `pyproject.toml`, `Makefile`, `Dockerfile` in cwd and informs the AI which ecosystem tools to use.
- [x] **More smart defaults** — the following are now detected and aliased:
  - `duf` → `df`
  - `tokei` → `cloc`
  - `doggo` → `dig`
  - `xh` → `curl`
  - `delta` → `diff`
  - `procs` → `ps`
  - `btop` / `bottom` → `top`
  - Git shortcuts: `gs`, `gl`, `gd`, `gp`, `gpl`, `gco`, `gcm`
  - Docker shortcuts: `dps`, `dex`, `dlog`
  - rg-powered: `rgf` → `rg -l`
- [x] **`edit_mode` config option** — `"emacs"` (default) or `"vi"` keybindings via `[behavior] edit_mode`.
- [x] **`-c` flag** — `shako -c "command"` runs a command non-interactively and exits.
- [x] **More subcommand completions** — `npm`/`pnpm`/`yarn`/`bun`, `brew`, `go`, `just` (parses justfile targets), `rustup`, `terraform`, `helm` (#35).
- [x] **SSH host completion** — parse `~/.ssh/config` for hostnames on `ssh <Tab>`.

---

## Missing Shell Essentials

### Builtins

- [x] **`echo`** — every script uses it; external `/bin/echo` has cross-platform behaviour differences.
- [x] **`read`** — can't do interactive prompts in functions without it. `read -p "prompt" VAR`.
- [x] **`test` / `[`** — can't write conditionals. `[[ -f file ]]` doesn't work at all.
- [x] **`pushd` / `popd` / `dirs`** — directory stack, very common workflow.
- [x] **`return`** — functions can't set exit status or return early.
- [x] **`disown`** — remove background job from shell tracking so it survives shell exit.
- [x] **`wait`** — wait for background jobs to finish.
- [x] **`pwd`** — avoid exec overhead for trivial operation.
- [x] **`command`** — run a command bypassing aliases/functions (like fish's `command`).
- [ ] **`eval`** — evaluate a string as a command.
- [x] **`true` / `false`** — trivial builtins for scripting.
- [x] **`local`** — local variable scoping in functions.

### Parser / Expansion

- [x] **`${VAR:-default}`** — parameter expansion with defaults. Breaks many sourced scripts without it.
- [x] **`${VAR:+alt}`** — use alternate value if set.
- [x] **`${VAR:?error}`** — error if unset.
- [x] **`${VAR#pattern}` / `${VAR%pattern}`** — prefix/suffix stripping.
- [x] **`${VAR/old/new}`** — string replacement.
- [x] **`${#VAR}`** — string length.
- [x] **`$((arithmetic))`** — inline math expressions.
- [x] **Brace expansion** — `{a,b,c}` and `{1..10}`. Fish has this.
- [ ] **Heredoc `<<EOF`** — pass multi-line input to commands.
- [x] **Herestring `<<<`** — `grep foo <<< "$var"`.
- [ ] **`$0`, `$#`, `$$`, `$!`** — special variables (script name, arg count, PID, last background PID).
- [ ] **ANSI-C quoting `$'...'`** — `$'\n'`, `$'\t'` escape sequences.

### Control Flow

- [x] **`if` / `else` / `elif` / `fi`** — interactive and in functions.
- [x] **`for` / `while` / `do` / `done`** — loops.
- [ ] **`case` / `esac`** — pattern matching.
- [x] **`break` / `continue`** — loop control.
- [x] **Local variables** — `local VAR=value` in functions.

### Job Control

- [ ] **`Ctrl-Z` suspend/resume** — SIGTSTP is ignored by the shell, but there's no SIGCHLD handler to detect when a child is stopped and add it to the jobs list.
- [ ] **`fg` terminal ownership** — `fg` doesn't call `tcsetpgrp`, so foregrounded jobs don't properly receive Ctrl-C/Ctrl-Z. (Partially addressed for pipelines but not for `fg`.)

### Completion Gaps

- [x] **Flag completion** — `git commit --am<Tab>` completes `--amend`; cargo subcommand flags also supported.
- [x] **Git branch completion** — `git checkout <Tab>` should list branches, not files.
- [x] **Alias/function completion** — user-defined aliases and functions should appear in first-token tab completion. (Requires passing `ShellState` to the completer.)
- [ ] **Env var completion** — `$PA<Tab>` should complete to `$PATH`.
- [ ] **Dynamic completions** — protocol for tools to register their own completions (like fish's `complete` command).
- [ ] **Fuzzy matching** — not just prefix matching; `gitp` could match `git-push`.

---

## AI Enhancements

### Context Improvements

- [x] **Wire up `history_context_lines`** — recent command history is now sent to the AI on every query.
- [x] **Git state in AI context** — branch, dirty/clean, recent commits included in every AI prompt.
- [x] **Project type in AI context** — build system / language detected from files in cwd.
- [x] **Per-project AI context (`.shako.toml`)** — drop a `.shako.toml` in any project root with `[ai] context = "..."` to inject project-specific instructions into every prompt.
- [ ] **Shell aliases in AI context** — AI should know what `ll`, `gs`, etc. map to.
- [ ] **File sizes in directory context** — useful for size-related queries.
- [ ] **Running processes** — useful for "kill the node process" queries.

### UX Improvements

- [x] **Stream AI responses** — tokens stream live to the terminal (#37).
- [ ] **Edit mode with readline** — AI confirm `[e]dit` currently uses raw `stdin.read_line()`. Should use reedline with history/completion/cursor movement.
- [x] **`[w]hy` option** — `[Y]es / [n]o / [e]dit / [w]hy` lets users understand before executing (#36).
- [x] **Multi-command guard** — `collapse_multiline()` rejects multi-line AI responses, picks the best single command, and warns the user.
- [x] **Retry/refine** — if the AI generates the wrong command, `[r]efine` lets you clarify without starting over.
- [ ] **AI-generated commands in history** — add to reedline history after confirmation so they're recallable.

### Innovation Ideas (differentiators)

- [x] **`?` suffix for inline explain** — `git rebase -i?` explains the flags without executing. Implemented and working.
- [x] **Per-project AI context (`.shako.toml`)** — drop a file in your project root with instructions the AI reads:
  ```toml
  [ai]
  context = "Rust project using actix-web. Tests: cargo nextest run."
  ```
- [x] **Session memory** — AI remembers the conversation within a session (last 5 exchanges). `ai reset` clears it.
- [x] **AI-powered history search** — `?? rsync command last week` does semantic search over shell history.
- [x] **Proactive suggestions** — after `git add`, shako offers an AI-generated commit message from the staged diff (#43).
- [ ] **AI pipe builder** — `? take output.json, extract emails, sort unique, count` builds the pipeline step-by-step with intermediate previews.
- [x] **Watch-and-learn** — edits to AI suggestions are logged to `~/.config/shako/learned_prefs.toml`; preferences (e.g. "prefer rg over grep") are injected into future AI prompts (#42).
- [ ] **Natural language aliases** — `alias "deploy to staging" = "kubectl apply -f k8s/staging/".

---

## Modern CLI Tools to Detect

Tools to add to `smart_defaults.rs` as the ecosystem grows:

| Tool | Replaces | Category |
|---|---|---|
| `duf` | `df` | Disk usage |
| `ouch` | `tar`/`unzip`/`gzip` | Compression |
| `tokei` | `cloc` | Code statistics |
| `doggo` | `dig` | DNS lookup |
| `xh` | `curl` | HTTP client |
| `yazi` | `ranger` | File manager |
| `jaq` | `jq` | JSON processor |
| `uv` | `pip` | Python packages |
| `mise` | `asdf`/`nvm` | Runtime manager |
| `just` | `make` | Command runner |
| `zellij` | `tmux` | Multiplexer |
| `lazygit` | — | Git TUI |
| `gitui` | — | Git TUI |
| `broot` | `tree` | Interactive tree |
| `xcp` | `cp` | Extended copy |
| `hyperfine` | `time` | Benchmarking |
| `gping` | `ping` | Graphical ping |
| `tealdeer` | `man` | Simplified man pages |

---

## Configuration Gaps

Missing config options for power users:

| Category | Option | Purpose |
|---|---|---|
| Behavior | `history_size` | Currently hardcoded to 10,000 |
| Behavior | `history_dedup` | Deduplicate consecutive identical commands |
| AI | `ai_enabled` | Global kill switch for AI features |
| AI | `ai_system_prompt_extra` | User-injected system prompt context |
| AI | `ai_preferred_tools` | Override tool preferences ("always use rg not grep") |
| Shell | `env` section | Set env vars at startup from config |
| Shell | `path_prepend` / `path_append` | Modify PATH from config |
| Shell | `abbreviations` section | Define abbreviations in config file |
| Smart Defaults | `smart_defaults_enabled` | Disable auto-aliasing entirely |
| Smart Defaults | `smart_defaults_exclude` | Skip specific tool upgrades |

> `edit_mode` (emacs/vi keybindings) is implemented — see `[behavior] edit_mode` in the [Configuration guide](docs/configuration.md).

---

## Architecture / Code Health

- [x] **Split `builtins.rs`** — split into `builtins/state.rs` (ShellState/Job/ShellFunction), `builtins/jobs.rs` (fg/bg/jobs), `builtins/set.rs` (set + PATH helpers), `builtins/source.rs` (source_fish_string, source_conf_d, load_functions_dir), `builtins/mod.rs` (dispatch + remaining builtins).
- [x] **Rename `Jbosh*` → `Shako*`** — `ShakoConfig`, `ShakoHighlighter`, `ShakoCompleter` throughout. No more `Jbosh` prefix anywhere.
- [ ] **Feature-gate `fish_import.rs`** (683 lines) — one-time migration utility; put behind `--features fish-import`.
- [x] **Integration tests** — `tests/integration.rs` has 107 end-to-end tests covering pipes, chains, redirects, env, glob, quoting, builtins, control flow, brace expansion, and herestrings. 119 unit tests + 107 integration = 226 total (and growing). Note: builtins (cd, alias, export, set) can only be tested via the interactive REPL loop, not via `-c` mode (which bypasses `ShellState`).

---

## Competitive Features from Other Shells

### From fish (not yet in shako)
- `string` builtin (match, replace, split, join, trim, length)
- Declarative `complete` command for custom completions
- `math` builtin with floating point
- `argparse` for function argument parsing
- Universal variables (persisted across sessions)
- Private mode (`--private`)

### From zsh (not yet in shako)
- Programmable completion system (`compdef`)
- Glob qualifiers (`*.rs(.)` = files only)
- `precmd`/`preexec` hooks
- Associative arrays

### From nushell (inspiration)
- Structured data pipelines (tables not strings)
- Built-in `open` that auto-parses JSON/YAML/CSV/TOML
- Plugin system
- Duration/filesize as first-class types

---

## Suggested Priority Order

### Phase 1 — Fix What's Broken ✅ Complete
1. ~~Fix `pre_exec` collision on `2>&1`~~ — done
2. ~~Capture stderr for AI diagnosis~~ — done
3. ~~Wire up `history_context_lines`~~ — done

### Phase 2 — Essential Shell Features ✅ Complete
4. ~~`echo`, `read`, `test` builtins~~ — done
5. ~~`${VAR:-default}` parameter expansion~~ — done
6. ~~`pushd`/`popd`/`dirs`~~ — done
7. ~~Git branch + state in AI context~~ — done

### Phase 3 — UX Polish ✅ Complete
8. ~~More smart defaults (duf, git shortcuts, docker shortcuts)~~ — done
9. ~~`npm`/`brew`/`go`/`just`/`helm`/`terraform` completions~~ — done (#35)
10. ~~Stream AI responses~~ — done (#37)
11. ~~`[w]hy` option in AI confirmation~~ — done (#36)

### Phase 4 — Differentiators ✅ Complete
12. ~~`?` suffix explain mode~~ — done
13. ~~Per-project `.shako.toml` AI context~~ — done
14. ~~Session memory for AI~~ — done (#47)
15. ~~AI-powered history search~~ — done (`??` prefix, #47)

### Phase 5 — Advanced Shell ✅ Complete
16. ~~`if`/`for`/`while` control flow~~ — done
17. ~~Brace expansion~~ — done (#49)
18. ~~Herestrings~~ — done (#49). Heredocs (`<<EOF`) not yet implemented.
19. ~~Flag + branch completion~~ — done (#47)
