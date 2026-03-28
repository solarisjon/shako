# shako Quick Reference

A concise cheat sheet for daily use. Print it, pin it, paste it somewhere handy.

---

## AI Modes

| What you type | What happens |
|---|---|
| `show me disk usage by folder` | Auto-detected as natural language → AI translates → confirm → run |
| `? show me disk usage` | Force AI mode (explicit prefix) |
| `ai: show me disk usage` | Force AI mode (alternate prefix) |
| `grep -rn?` | Explain what `-rn` flags do (no execution) |
| `? grep` | Explain what `grep` does (single known command → explain) |
| `chmod 755?` | Explain what `chmod 755` means |
| `? squash the last 3 commits` | AI translates → `git rebase -i HEAD~3` → confirm |
| `?? rsync command last week` | AI-powered semantic history search |
| `ai reset` | Clear AI session memory |

**Confirmation prompt:** `[Y]es / [n]o / [e]dit / [w]hy / [r]efine`  
Press `Y` or Enter to run, `n` to cancel, `e` to edit, `w` for explanation, `r` to refine your request.

---

## Shell Builtins

| Command | What it does |
|---|---|
| `cd [dir]` | Change directory. `cd -` returns to previous directory |
| `z <query>` | Smart jump (zoxide) — `z proj` → most-visited project dir |
| `zi` | Interactive directory picker (zoxide + fzf) |
| `exit` | Exit shako |
| `export KEY=value` | Set environment variable (POSIX style) |
| `set -x KEY value` | Set and export variable (fish style) |
| `set -gx KEY value` | Global export (fish style) |
| `set -e KEY` | Erase/unset variable (fish style) |
| `set` | List all environment variables |
| `unset KEY` | Remove environment variable |
| `alias name=value` | Define alias. `alias` alone lists all |
| `unalias name` | Remove alias. `unalias -a` clears all |
| `abbr add name value` | Add abbreviation (fish-style) |
| `abbr -e name` | Remove abbreviation |
| `abbr` | List abbreviations |
| `history [N]` | Show last N history entries (default 25) |
| `source file` | Load aliases, exports, functions from a file |
| `type name` | Show how a name resolves (builtin/function/alias/PATH) |
| `function name() { body }` | Define a shell function |
| `functions` | List all defined functions |
| `jobs` | List background jobs |
| `fg [%N]` | Bring job N to foreground |
| `bg [%N]` | Resume stopped job in background |
| `disown [%N]` | Remove job from shell tracking (survives shell exit) |
| `wait [%N]` | Wait for background job(s) to finish |
| `echo [args]` | Print arguments with escape support |
| `read [-p prompt] VAR` | Read a line from stdin into a variable |
| `test` / `[` | Conditional evaluation (`-f`, `-d`, `-z`, `-n`, `=`, `-eq`, etc.) |
| `pwd` | Print current working directory |
| `pushd [dir]` | Push directory onto stack and cd |
| `popd` | Pop directory from stack and cd |
| `dirs` | Display directory stack |
| `command name` | Run command bypassing aliases/functions |
| `true` / `false` | Return exit code 0 / 1 |
| `return [N]` | Return from function with exit code |
| `break` / `continue` | Loop control |
| `local VAR=val` | Function-local variable |
| `fish-import` | Import aliases/env/functions from `~/.config/fish/` |

---

## History Expansion

```bash
!!           # repeat last command
sudo !!      # run last command with sudo
echo !$      # insert last argument of last command
```

---

## Pipes, Redirects, and Chains

```bash
# Pipes
ls | grep foo | wc -l

# Stdout redirects
cmd > file.txt       # overwrite
cmd >> file.txt      # append

# Stdin redirect
cmd < input.txt

# Stderr redirects
cmd 2> errors.log    # stderr to file
cmd 2>&1             # merge stderr into stdout
cmd > out.log 2> err.log  # separate files
cmd 2>&1 | grep err  # pipe combined output

# Chains
cmd1 && cmd2         # run cmd2 only if cmd1 succeeds
cmd1 || cmd2         # run cmd2 only if cmd1 fails
cmd1; cmd2; cmd3     # run all regardless of exit codes
```

---

## Quoting and Expansion

```bash
echo "hello $USER"         # double quotes: variable expansion works
echo 'no $expansion'       # single quotes: everything literal
echo hello\ world          # backslash escapes the space

echo $HOME                 # env var expansion
echo ${USER}               # braced form
echo $?                    # last exit code

echo $(date)               # command substitution
echo `whoami`              # backtick substitution
echo $(dirname $(pwd))     # nested substitution

ls *.rs                    # glob expansion
ls "*.rs"                  # suppressed inside quotes
cd ~/projects              # tilde expansion

echo {a,b,c}               # brace expansion → a b c
echo {1..5}                # range expansion → 1 2 3 4 5
echo {01..10}              # zero-padded → 01 02 ... 10

grep foo <<< "$var"        # herestring → pipe to stdin

${VAR:-default}            # parameter expansion with default
${VAR#prefix}              # strip prefix
${VAR/old/new}             # string replacement
$((2 + 2))                 # arithmetic expansion
```

---

## Background Jobs

```bash
sleep 100 &    # start in background
jobs           # list running background jobs
fg %1          # bring job 1 to foreground
bg %1          # resume stopped job in background
```

---

## Multiline Input

```bash
echo hello \       # trailing backslash → continues on next line
world

echo "this is      # unclosed quote → continues
a multiline string"
```

The prompt changes to `... ` for continuation lines.

---

## Non-Interactive Mode

```bash
shako -c "ls -la"                      # run a single command and exit
shako -c "git status && git diff"      # run a chain
```

---

## Runtime Flags

| Flag | Effect |
|---|---|
| `--quiet` / `-q` | Suppress the startup banner |
| `--timings` | Print startup phase timing breakdown |
| `--init` | Re-run the setup wizard |
| `-c "cmd"` | Run command non-interactively and exit |

---

## Smart Aliases (auto-created if tool is installed)

### Modern tool upgrades

| If you have | Typing this | Runs this |
|---|---|---|
| eza | `ls` | `eza --icons --group-directories-first` |
| bat | `cat` | `bat --style=auto` |
| fd | `find` | `fd` |
| ripgrep | `grep` | `rg` |
| dust | `du` | `dust` |
| procs | `ps` | `procs` |
| sd | `sed` | `sd` |
| delta | `diff` | `delta` |
| btop/bottom | `top` | `btop` or `btm` |
| duf | `df` | `duf` |
| doggo | `dig` | `doggo` |
| xh | `curl` | `xh` |
| tokei | `cloc` | `tokei` |

### Compound aliases (eza)

| Alias | Expands to |
|---|---|
| `ll` | `eza -la --icons --group-directories-first` |
| `la` | `eza -a --icons --group-directories-first` |
| `lt` | `eza --tree --icons --level=2` |

### Git shortcuts

| Alias | Expands to |
|---|---|
| `gs` | `git status` |
| `gl` | `git log --oneline -20` |
| `gd` | `git diff` |
| `gp` | `git push` |
| `gpl` | `git pull` |
| `gco` | `git checkout` |
| `gcm` | `git commit -m` |

### Docker shortcuts

| Alias | Expands to |
|---|---|
| `dps` | `docker ps` |
| `dex` | `docker exec -it` |
| `dlog` | `docker logs -f` |

---

## Explain Mode (`?`)

```bash
git rebase -i?        # explain git rebase -i flags
tar xzf?              # explain tar xzf
? grep                # explain what grep is
? kubectl             # explain what kubectl is
```

---

## Error Recovery

When a command fails (exit code ≥ 2), shako offers AI help:

```
$ cargo build --featurse serde
error: unexpected argument '--featurse'
shako: command failed (exit 2). ask AI for help? [y/N] y
  cause: Typo — '--featurse' should be '--features'
  fix:   cargo build --features serde
  [Y]es / [n]o / [e]dit / [w]hy:
```

Note: exit code 1 is skipped (too common — grep no-match, test failure). Press **Enter at the `[y/N]`** to skip (default is no).

---

## Per-Project AI Context

Drop a `.shako.toml` in any project root to give the AI project-specific instructions:

```toml
[ai]
context = """
Rust project using actix-web and SQLx.
Tests: cargo nextest run
Database: PostgreSQL on localhost:5433
Deploy: make deploy-staging
"""
```

The AI reads this every time you're in that directory.

---

## Tab Completion

Subcommand completions for: `git` (branches+flags), `cargo` (flags), `docker`, `podman`, `kubectl`, `npm`/`pnpm`/`yarn`/`bun`, `brew`, `go`, `rustup`, `helm`, `terraform`, `make`/`just` (dynamic targets), `ssh` (hosts from config), `sudo`, `cd`/`z`/`pushd`.

All `$PATH` executables, builtins, aliases, functions, and file paths complete too. Filenames with spaces are auto-escaped.

---

## Syntax Highlighting

As you type, shako colors the input:

| Element | Color |
|---|---|
| Valid command | **Green** (bold) |
| Shell builtin | **Cyan** (bold) |
| AI prefix (`?`, `ai:`) | **Purple** (bold) |
| Explicit path | Yellow |
| Unknown command | Red |
| Flags (`-x`, `--flag`) | Blue |
| Strings | Yellow |
| Pipes and redirects | Cyan |
| Variables (`$VAR`) | Green |
| Comments (`# ...`) | Gray (italic) |

---

## Autosuggestions

Gray inline suggestions from history appear as you type.

- **Right arrow** — accept full suggestion
- **Ctrl+Right** — accept one word

---

## Config File

`~/.config/shako/config.toml` — key options:

```toml
active_provider = "work_proxy"

[providers.work_proxy]
endpoint = "https://llm-proxy.company.com"
model = "claude-sonnet-4.5"
api_key_env = "LLMPROXY_KEY"

[behavior]
confirm_ai_commands = true       # [Y/n/e] before running AI commands
auto_correct_typos = true        # typo suggestion prompt
history_context_lines = 20       # recent commands sent to AI
safety_mode = "warn"             # "warn" | "block" | "off"
edit_mode = "emacs"              # "emacs" | "vi"
```

---

## Signal Handling

| Key | Effect |
|---|---|
| `Ctrl-C` | Interrupt foreground process |
| `Ctrl-\\` | Quit foreground process (SIGQUIT) |
| `Ctrl-Z` | Suspend foreground process |
| `Ctrl-D` | Exit shell (on empty line) |

---

## Startup Files

| File | Purpose |
|---|---|
| `~/.config/shako/config.toml` | Main config (providers, behavior, aliases) |
| `~/.config/shako/config.shako` | Startup script (aliases, exports, functions) |
| `~/.config/shako/conf.d/*.sh` | Config snippets, sourced alphabetically |
| `~/.config/shako/functions/*.sh` | Autoloaded functions (name must match filename) |
| `~/.config/shako/starship.toml` | shako-specific Starship prompt config |

History: `~/Library/Application Support/shako/history.txt` (macOS) or `~/.local/share/shako/history.txt` (Linux)

---

## Slash Commands

Meta-commands for inspecting and configuring shako at runtime:

| Command | What it does |
|---|---|
| `/help` | List all slash commands |
| `/validate` | Validate AI endpoint (connectivity, auth, model) |
| `/config` | Show full current configuration |
| `/model` | Show active AI model and provider |
| `/safety [mode]` | Show or change safety mode (`warn`/`block`/`off`, session only) |
| `/provider [name]` | Show or switch LLM provider (session only) |

See [Slash Commands](slash-commands.md) for details.

---

## See Also

| Guide | What's covered |
|---|---|
| [Getting Started](getting-started.md) | Installation, setup wizard, directory structure |
| [AI Features](ai-features.md) | Translation, explain mode, error recovery, context |
| [Shell Features](shell-features.md) | Builtins, pipes, redirects, job control, functions |
| [Smart Defaults](smart-defaults.md) | Auto-aliasing, tool detection, AI tool awareness |
| [Configuration](configuration.md) | Full config reference, multi-provider setup |
