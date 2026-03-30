# New User Guide: Getting Started with shako

Welcome to shako — a shell that understands both commands and plain English. This guide walks you through everything you need to know, step by step. No prior experience with fish shell or AI-powered tools is required.

---

## What is shako?

shako is a modern interactive shell (like bash or zsh) with one important difference: **you can describe what you want in plain English, and shako translates it into a real shell command for you**.

Type `ls` — it runs instantly, just like any shell.  
Type `show me all large files` — shako asks an AI to translate it, shows you the command it would run, and asks if you want to execute it.

This means you get the **full power of a traditional shell** without having to memorize every command and its flags.

---

## Part 1: Installation

### Step 1: Install Rust

shako is written in Rust. If you don't have Rust installed:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Requires **Rust 1.85.0 or newer** (edition 2024).

### Step 2: Get shako

```bash
git clone https://github.com/solarisjon/shako.git
cd shako
```

### Step 3: Build and install

```bash
make install
```

This builds a release binary and installs it to `~/.local/bin/shako`. Make sure `~/.local/bin` is in your `PATH`.

### Step 4 (optional): Make shako your default shell

```bash
make register-shell       # adds shako to /etc/shells (requires sudo)
chsh -s ~/.local/bin/shako
```

After this, shako will launch every time you open a terminal.

---

## Part 2: Recommended Tools (Optional but Recommended)

shako automatically detects and uses modern replacements for classic Unix tools. Install them for a better experience — shako will use them without any configuration:

```bash
# macOS
brew install starship eza bat fd ripgrep zoxide fzf dust delta procs duf

# Ubuntu/Debian
sudo apt install eza bat fd-find ripgrep zoxide fzf
```

What these do:

| Tool | Replaces | What it does |
|---|---|---|
| `eza` | `ls` | Colorful file listings with icons |
| `bat` | `cat` | File viewer with syntax highlighting |
| `fd` | `find` | Faster, friendlier file search |
| `ripgrep` (rg) | `grep` | Faster, smarter text search |
| `zoxide` (z) | `cd` | Smart directory jumping |
| `fzf` | — | Fuzzy finder (used by `zi` for interactive dir picking) |
| `dust` | `du` | Visual disk usage |
| `starship` | — | Beautiful, informative prompt |

Once installed, shako creates aliases automatically. Typing `ls` will run `eza`, `cat` will run `bat`, etc.

---

## Part 3: First Run and Setup Wizard

Launch shako:

```bash
shako
```

On first launch, an interactive setup wizard runs automatically. It has three steps:

### Step 3.1: Choose an LLM provider

The AI features require a language model. You have options:

- **LM Studio (local)** — Download [LM Studio](https://lmstudio.ai/), load a model, and shako connects to `localhost:1234`. No internet required, no API key needed.
- **Work/custom proxy** — Enter a URL for any OpenAI-compatible endpoint (useful if your company has an internal LLM).
- **Skip** — Creates a config file you can edit manually later. AI features won't work until you configure a provider.

### Step 3.2: Import fish config (optional)

If you're coming from [fish shell](https://fishshell.com/) and have a `~/.config/fish/` directory, shako offers to import your:
- Aliases and abbreviations
- Environment variables and PATH entries
- Shell functions

If you're new to both fish and shako, just skip this step.

### Step 3.3: Tool audit

shako shows which recommended tools are installed and which are missing, along with a one-line install command for your package manager.

---

## Part 4: Your First Session

After setup, you'll see a startup banner like this:

```
shako v0.1.0   ✓ AI ready (work_proxy / claude-sonnet-4.5)
safety: warn   edit: emacs   typo-fix: on
❯
```

The `❯` prompt means shako is ready. Let's explore.

### Running regular commands

shako runs regular shell commands exactly like bash or zsh:

```
❯ ls
❯ pwd
❯ cd ~/Documents
❯ git status
❯ echo "hello world"
```

There's no difference here — shako is a complete shell.

### Using the AI: natural language translation

Now for the interesting part. Just type what you want in plain English:

```
❯ show me the 10 largest files in this directory
```

shako will think for a moment, then show you something like:

```
❯ fd --type f -x stat -f '%z %N' {} | sort -rn | head -10
[Y]es / [n]o / [e]dit / [w]hy:
```

The AI translated your request into a real shell command. Your options at the prompt:

| Key | What it does |
|---|---|
| `Y` or Enter | Run the command |
| `n` | Cancel — don't run it |
| `e` | Edit the command before running |
| `w` | Ask "why?" — the AI explains what the command does |
| `r` | Refine — add more detail and let the AI re-translate |

Try pressing `w` to see an explanation, then `Y` to run it.

---

## Part 5: The AI Modes

shako has several ways to invoke AI assistance:

### Natural language (automatic)

If your input doesn't look like a shell command, shako automatically routes it to the AI:

```
❯ find all python files modified in the last 7 days
❯ compress this directory into a tar.gz file
❯ show me memory usage by process
```

### Explicit AI mode (with `?` prefix)

You can force AI mode with a `?` prefix. This is useful when you want to be explicit or when a word like `find` would normally run the real `find` command:

```
❯ ? find all large files           # forces AI translation
❯ ? list open network connections  # forces AI translation
```

### Explain mode (with trailing `?`)

Append `?` to any command to get a plain-English explanation **without running it**:

```
❯ git rebase -i?
❯ chmod 755?
❯ tar xzf?
❯ rsync -avz?
```

You can also explain a command by name:

```
❯ ? grep
❯ ? awk
❯ ? kubectl
```

### History search (with `??` prefix)

Search your shell history using a description instead of trying to remember exact syntax:

```
❯ ?? the rsync command I used to deploy last week
❯ ?? docker command to remove stopped containers
```

---

## Part 6: Everyday Shell Usage

### Navigating directories

```bash
cd ~/projects          # change directory
cd -                   # go back to previous directory
z projects             # smart jump (zoxide) — goes to most-visited match
zi                     # interactive directory picker (requires fzf)
pwd                    # show current directory
```

### Listing files

```bash
ls                     # list files (uses eza if installed)
ll                     # long listing with details
la                     # show hidden files too
lt                     # tree view (2 levels deep)
```

### Viewing files

```bash
cat file.txt           # view file (uses bat if installed, with syntax highlighting)
```

### Searching

```bash
grep "pattern" file    # search in file (uses ripgrep if installed)
find . -name "*.py"    # find files (uses fd if installed)
```

### Git shortcuts

shako auto-creates these aliases if git is installed:

| Type | Runs |
|---|---|
| `gs` | `git status` |
| `gl` | `git log --oneline -20` |
| `gd` | `git diff` |
| `gp` | `git push` |
| `gpl` | `git pull` |
| `gco` | `git checkout` |
| `gcm` | `git commit -m` |

---

## Part 7: Smart Features

### Typo correction

If you mistype a command, shako catches it:

```
❯ gti status
shako: did you mean "git status"? [Y/n]
```

Press `Y` or Enter to run the corrected command, `n` to cancel.

### Error recovery

When a command fails with an error, shako offers AI-powered diagnosis:

```
❯ cargo build --featurse serde
error: unexpected argument '--featurse'
shako: command failed (exit 2). ask AI for help? [y/N]
```

Press `y` and the AI will:
1. Identify the problem ("Typo — `--featurse` should be `--features`")
2. Suggest the corrected command
3. Ask if you want to run the fix

The default at the `[y/N]` prompt is **no** — press Enter to skip if you don't need help.

### Proactive suggestions

After certain commands, shako offers helpful follow-ups:

**After `git add .`:**
```
shako: 3 files staged — suggest a commit message? [y/N]
```
Press `y` and the AI generates a commit message from your staged changes, then asks for confirmation before committing.

**After `git clone <url>`:**
```
tip: cd shako
```

**After `cd` into a directory with a Makefile:**
```
available targets: build, test, install
```

### Session memory

The AI remembers your recent exchanges within the session. Follow-up requests work naturally:

```
❯ find all rust files bigger than 1MB
❯ fd -e rs --size +1m
[Y] → runs

❯ now do the same but only in src/
❯ fd -e rs --size +1m src/
```

To clear the session memory: `ai reset`

---

## Part 8: History and Autosuggestions

### Command history

```bash
history        # show last 25 commands
history 50     # show last 50 commands
!!             # repeat the last command
sudo !!        # run last command with sudo
```

### Autosuggestions

As you type, shako shows gray suggestions from your history. You'll see them appear inline to the right of your cursor:

- **Right arrow** — accept the full suggestion
- **Ctrl+Right** — accept one word at a time

This works just like fish shell's autosuggestions and saves a lot of typing.

---

## Part 9: Pipes, Redirects, and Command Chaining

These work exactly like bash:

```bash
# Pipes: send output of one command to another
ls | grep ".rs" | wc -l

# Redirects: send output to a file
ls > files.txt         # overwrite
ls >> files.txt        # append

# Read from a file
sort < names.txt

# Capture stderr
make 2> errors.log
make 2>&1              # merge stderr into stdout

# Chain commands
mkdir foo && cd foo    # run second only if first succeeds
make || echo "failed"  # run second only if first fails
cmd1; cmd2; cmd3       # run all regardless of exit code
```

---

## Part 10: Configuring shako

The main configuration file is `~/.config/shako/config.toml`. You can edit it directly, or use slash commands (see below).

### Slash commands (in-shell configuration)

Type these at the prompt to inspect or change settings:

| Command | What it does |
|---|---|
| `/help` | List all slash commands |
| `/validate` | Test that the AI endpoint is reachable and working |
| `/config` | Show the current full configuration |
| `/model` | Show the active AI model and provider |
| `/safety warn` | Set safety mode (session only): `warn`, `block`, or `off` |
| `/provider work_proxy` | Switch LLM provider (session only) |

### Safety mode

shako checks AI-generated commands for danger before running them:

- **`warn`** (default) — Warns before running commands like `sudo`, `rm`, `chmod`
- **`block`** — Blocks dangerous commands like `rm -rf /` entirely
- **`off`** — No safety checks

Change for the session with `/safety warn` or set permanently in `config.toml`:

```toml
[behavior]
safety_mode = "warn"
```

### Changing the AI provider

To switch providers mid-session:

```
❯ /provider lm_studio
```

To switch permanently, edit `~/.config/shako/config.toml`:

```toml
active_provider = "lm_studio"
```

---

## Part 11: Per-Project AI Context

Drop a `.shako.toml` file in any project directory to give the AI project-specific knowledge:

```toml
[ai]
context = """
This is a Python web app using FastAPI and SQLAlchemy.
Tests: pytest tests/
Database: PostgreSQL on localhost:5432
Lint: ruff check .
Deploy: make deploy
"""
```

Now when you're in that directory:

```
❯ ? run the tests
❯ pytest tests/

❯ ? check the code style
❯ ruff check .
```

The AI uses the context for every query while you're in that directory. You can commit this file to your repo so the whole team benefits, or add it to `.gitignore` to keep it personal.

---

## Part 12: Tab Completion

Press **Tab** to complete commands, flags, file paths, and more. shako has built-in completions for:

- `git` — branches, tags, flags
- `cargo`, `rustup` — Rust toolchain commands
- `docker`, `podman` — container management
- `kubectl`, `helm`, `terraform` — infrastructure tools
- `npm`, `pnpm`, `yarn`, `bun` — JavaScript package managers
- `make`, `just` — build targets (dynamically read from your Makefile)
- `ssh` — hostnames from your SSH config
- All commands in `$PATH`, shell builtins, aliases, and functions
- File paths (with spaces auto-escaped)

---

## Part 13: If You're Coming from Fish Shell

shako was designed with fish users in mind. Many fish patterns work out of the box:

| fish syntax | In shako |
|---|---|
| `set -x VAR value` | Works the same |
| `set -gx VAR value` | Works the same |
| `set -e VAR` | Works the same |
| `set` (list all vars) | Works the same |
| `abbr add name value` | Works the same |
| Autosuggestions from history | Works the same |
| `conf.d/` directory | Works the same |
| `functions/` directory | Works the same |

### Importing your fish config

If you already have a fish setup you're happy with, import it:

```
❯ fish-import
```

Or re-run the setup wizard:

```bash
shako --init
```

This imports your fish aliases, abbreviations, environment variables, PATH entries, and functions into shako.

---

## Quick Troubleshooting

### AI says "unreachable" or "auth failed"

Run `/validate` at the shako prompt to test the connection. Then check:
1. Is your LLM provider running? (For LM Studio: is it open and a model is loaded?)
2. Is the endpoint URL correct in `~/.config/shako/config.toml`?
3. Is the API key environment variable set? (`echo $LLMPROXY_KEY`)

### AI features aren't working

Check the banner when shako starts. If it shows `✗ unreachable` or `⚠ no api key`, the AI isn't connected. Run `/config` to see what provider and endpoint is configured.

### I want to re-run the setup wizard

```bash
shako --init
```

This removes the existing config files and runs the wizard again.

### shako is running slowly at startup

Run `shako --timings` to see a breakdown of startup phases and identify what's slow.

---

## Summary: The Most Important Things to Remember

1. **Regular commands work normally** — shako is a full shell. `ls`, `cd`, `git`, etc. all work as expected.
2. **Just type in English** — If you don't know the command, describe what you want. shako will translate it.
3. **Always confirm** — AI-generated commands show you what they'll do before running. Press `Y` to run, `n` to cancel.
4. **Use `?` to learn** — Append `?` to any command to get an explanation: `chmod 755?`
5. **Use `??` to find history** — Search your history with plain English: `?? the docker command to stop all containers`
6. **Smart defaults just work** — Install recommended tools and shako uses them automatically.

---

## Next Steps

Now that you're up and running, explore these guides for deeper features:

| Guide | What you'll learn |
|---|---|
| [AI Features](ai-features.md) | Full AI translation, explain mode, error recovery, project context |
| [Shell Features](shell-features.md) | Pipes, redirects, functions, job control, control flow |
| [Configuration](configuration.md) | Provider setup, behavior options, all config keys |
| [Slash Commands](slash-commands.md) | In-shell configuration and diagnostics |
| [Smart Defaults](smart-defaults.md) | All auto-detected tools and generated aliases |
| [Quick Reference](quick-reference.md) | Printable cheat sheet for daily use |
