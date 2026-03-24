# ROADMAP.md — shako

Living document tracking gaps, bugs, feature ideas, and priorities for making shako a daily-driver shell.

---

## Bugs / Broken Wiring

- [ ] **Stderr never captured for AI diagnosis** — `main.rs` calls `diagnose_error()` with `""` for stderr. The AI can't see *why* a command failed. Capture stderr from child processes and pass it through.
- [ ] **`history_context_lines` declared but never used** — `config/schema.rs` has the field (marked `dead_code`), but it's never read. AI should see recent command history for follow-up queries.
- [ ] **`pre_exec` collision on `2>&1`** — `apply_stderr_redirect()` calls `cmd.pre_exec()` which *replaces* the one set by `setup_child_signals()`. Commands with `2>&1` lose signal handling (SIGINT/SIGQUIT/SIGTSTP not reset to defaults in child). Fix: merge both closures into a single `pre_exec`.

---

## Quick Wins (high value, low effort)

- [ ] **Git context for AI** — send current branch, dirty/clean status, and recent `git log --oneline -5` to the AI. Most AI queries are git-related.
- [ ] **Project type detection** — detect `Cargo.toml`, `package.json`, `go.mod`, `pyproject.toml`, `Makefile`, `Dockerfile`, etc. in cwd and tell the AI what ecosystem tools to use.
- [ ] **More smart defaults** — detect and alias:
  - `duf` → `df` (disk usage with colors)
  - `ouch` → auto-detect for compress/decompress
  - `tokei` → `cloc` (code statistics)
  - `doggo` → `dig` (DNS lookup)
  - `xh` → `curl` (HTTP client)
  - Git shortcuts: `gs` → `git status`, `gl` → `git log --oneline -20`, `gd` → `git diff`, `gp` → `git push`
  - Docker shortcuts: `dps` → `docker ps`, `dex` → `docker exec -it`
  - rg-powered: `rgf` → `rg -l` (filenames only)
- [ ] **More subcommand completions** — `npm`/`pnpm`/`yarn`/`bun`, `brew`, `go`, `just` (parse justfile targets like Makefile), `python`/`pip`/`uv`, `rustup`, `terraform`, `helm`.
- [ ] **SSH host completion** — parse `~/.ssh/config` for hostnames on `ssh <Tab>`.

---

## Missing Shell Essentials

### Builtins

- [ ] **`echo`** — every script uses it; external `/bin/echo` has cross-platform behaviour differences.
- [ ] **`read`** — can't do interactive prompts in functions without it. `read -p "prompt" VAR`.
- [ ] **`test` / `[`** — can't write conditionals. `[[ -f file ]]` doesn't work at all.
- [ ] **`pushd` / `popd` / `dirs`** — directory stack, very common workflow.
- [ ] **`return`** — functions can't set exit status or return early.
- [ ] **`disown`** — remove background job from shell tracking so it survives shell exit.
- [ ] **`wait`** — wait for background jobs to finish.
- [ ] **`pwd`** — avoid exec overhead for trivial operation.
- [ ] **`command`** — run a command bypassing aliases/functions (like fish's `command`).
- [ ] **`eval`** — evaluate a string as a command.

### Parser / Expansion

- [ ] **`${VAR:-default}`** — parameter expansion with defaults. Breaks many sourced scripts without it.
- [ ] **`${VAR:+alt}`** — use alternate value if set.
- [ ] **`${VAR:?error}`** — error if unset.
- [ ] **`${VAR#pattern}` / `${VAR%pattern}`** — prefix/suffix stripping.
- [ ] **`${VAR/old/new}`** — string replacement.
- [ ] **`${#VAR}`** — string length.
- [ ] **`$((arithmetic))`** — inline math expressions.
- [ ] **Brace expansion** — `{a,b,c}` and `{1..10}`. Fish has this.
- [ ] **Heredoc `<<EOF`** — pass multi-line input to commands.
- [ ] **Herestring `<<<`** — `grep foo <<< "$var"`.
- [ ] **`$0`, `$#`, `$$`, `$!`** — special variables (script name, arg count, PID, last background PID).
- [ ] **ANSI-C quoting `$'...'`** — `$'\n'`, `$'\t'` escape sequences.

### Control Flow

- [ ] **`if` / `else` / `fi`** — interactive and in functions.
- [ ] **`for` / `while` / `done`** — loops.
- [ ] **`case` / `esac`** — pattern matching.
- [ ] **`break` / `continue`** — loop control.
- [ ] **Local variables** — `local VAR=value` in functions.

### Job Control

- [ ] **`Ctrl-Z` suspend/resume** — SIGTSTP is ignored by the shell, but there's no SIGCHLD handler to detect when a child is stopped and add it to the jobs list.
- [ ] **`fg` terminal ownership** — `fg` doesn't call `tcsetpgrp`, so foregrounded jobs don't properly receive Ctrl-C/Ctrl-Z. (Partially addressed for pipelines but not for `fg`.)

### Completion Gaps

- [ ] **Flag completion** — `git commit --am<Tab>` should suggest `--amend`.
- [ ] **Git branch completion** — `git checkout <Tab>` should list branches, not files.
- [ ] **Alias/function completion** — user-defined aliases and functions should appear in first-token tab completion. (Requires passing `ShellState` to the completer.)
- [ ] **Env var completion** — `$PA<Tab>` should complete to `$PATH`.
- [ ] **Dynamic completions** — protocol for tools to register their own completions (like fish's `complete` command).
- [ ] **Fuzzy matching** — not just prefix matching; `gitp` could match `git-push`.

---

## AI Enhancements

### Context Improvements

- [ ] **Wire up `history_context_lines`** — send recent command history to the AI so it can understand "do that again" or follow-up queries.
- [ ] **Git state in AI context** — branch, dirty/clean, recent commits.
- [ ] **Project type in AI context** — detect build system / language from files in cwd.
- [ ] **Shell aliases in AI context** — AI should know what `ll`, `gs`, etc. map to.
- [ ] **File sizes in directory context** — useful for size-related queries.
- [ ] **Running processes** — useful for "kill the node process" queries.

### UX Improvements

- [ ] **Stream AI responses** — show tokens as they arrive instead of blocking with "thinking...". Much better perceived performance.
- [ ] **Edit mode with readline** — AI confirm `[e]dit` currently uses raw `stdin.read_line()`. Should use reedline with history/completion/cursor movement.
- [ ] **Add `[w]hy` option** — `[Y]es / [n]o / [e]dit / [w]hy` lets users understand before executing.
- [ ] **Multi-command preview** — multi-line AI commands should show as numbered steps, not one blob.
- [ ] **Retry/refine** — if the AI generates the wrong command, allow "no, I meant..." without starting over.
- [ ] **AI-generated commands in history** — add to reedline history after confirmation so they're recallable.

### Innovation Ideas (differentiators)

- [ ] **`?` suffix for inline explain** — `git rebase -i?` explains the flag without executing. Different from `? git rebase -i` which translates NL.
- [ ] **Session memory** — AI remembers the conversation. After `fd *.log`, say `"now delete the ones over 1GB"` and it knows what you mean.
- [ ] **AI-powered history search** — `? what was that rsync command I used last week` does semantic search over shell history.
- [ ] **Proactive suggestions** — after `git add .`, suggest `git commit -m "..."` with an AI-generated message from the staged diff.
- [ ] **Per-project AI context (`.shako.toml`)** — drop a file in your project root with instructions the AI reads:
  ```toml
  [ai]
  context = "Rust project using actix-web. Tests: cargo nextest run."
  ```
- [ ] **AI pipe builder** — `? take output.json, extract emails, sort unique, count` builds the pipeline step-by-step with intermediate previews.
- [ ] **Watch-and-learn** — when the user edits an AI suggestion, log the correction to a local preferences file. Over time: "user prefers rg over grep", "user uses fd not find".
- [ ] **Smart history search** — `? what was that command I used to resize images` does semantic search.
- [ ] **Natural language aliases** — `alias "deploy to staging" = "kubectl apply -f k8s/staging/"`.

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
| Behavior | `vi_mode` / `edit_mode` | Enable vi keybindings (reedline supports this) |
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

---

## Architecture / Code Health

- [ ] **Split `builtins.rs`** (1,183 lines) — extract `ShellState`, `Job`/job-control, and `set` builtin into separate modules.
- [ ] **Rename `Jbosh*` → `Shako*`** — `JboshConfig`, `JboshHighlighter`, `JboshCompleter`, `JboshHinter` still use the pre-rename prefix.
- [ ] **Feature-gate `fish_import.rs`** (683 lines) — one-time migration utility; put behind `--features fish-import`.
- [ ] **Integration tests** — end-to-end tests that pipe input through shako and check output. Currently all 54 tests are unit tests.
- [ ] **Startup time instrumentation** — add `--timings` flag or log startup duration at `RUST_LOG=info`.

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

### Phase 1 — Fix What's Broken
1. Fix `pre_exec` collision on `2>&1`
2. Capture stderr for AI diagnosis
3. Wire up `history_context_lines`

### Phase 2 — Essential Shell Features
4. `echo`, `read`, `test` builtins
5. `${VAR:-default}` parameter expansion
6. `pushd`/`popd`/`dirs`
7. Git branch + state in AI context

### Phase 3 — UX Polish
8. More smart defaults (duf, ouch, git shortcuts)
9. `npm`/`brew`/`go`/`just` completions
10. Stream AI responses
11. `[w]hy` option in AI confirmation

### Phase 4 — Differentiators
12. `?` suffix explain mode
13. Per-project `.shako.toml` AI context
14. Session memory for AI
15. AI-powered history search

### Phase 5 — Advanced Shell
16. `if`/`for`/`while` control flow
17. Brace expansion
18. Heredocs / herestrings
19. Flag + branch completion
