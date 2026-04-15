# AI Features

shako's AI integration is designed to augment your shell workflow — never to get in the way. Real commands execute instantly; the AI only activates when you need it.

## Natural Language Translation

Type what you want in plain English. shako translates it to a shell command, shows it inside a branded confirmation panel, and asks for your decision:

```
$ show me the 10 largest files in this directory

 ╭ shako ─────────────────────────────────────────────────────╮
 │  fd --type f -x stat -f '%z %N' {} | sort -rn | head -10  │
 ├────────────────────────────────────────────────────────────┤
 │  [Y]es  [n]o  [e]dit  [w]hy  [r]efine                     │
 ╰────────────────────────────────────────────────────────────╯
 ❯
```

The border is a teal-to-cyan gradient matching shako's startup banner, giving AI-generated commands a consistent visual identity distinct from direct shell output.

The AI is **tool-aware** — it knows which modern tools you have installed and prefers them:
- Has `fd` → uses `fd` instead of `find`
- Has `rg` → uses `rg` instead of `grep`
- Has `dust` → uses `dust` for disk usage instead of `du`

### How to trigger

Any input that doesn't match a known command, builtin, or function automatically routes to the AI. You can also force it:

| Syntax | Behavior |
|---|---|
| `show me large files` | Auto-detected as natural language → AI translates |
| `? show me large files` | Forced AI mode (explicit) |
| `ai: show me large files` | Forced AI mode (explicit) |
| `find all the .md files` | `find` is in PATH but args are prose → AI translates |
| `find . -name '*.md'` | `find` with real flags → executes directly (no AI) |

### Confirmation UX

After the AI generates a command:

- **`Y` or Enter** — execute the command
- **`n`** — cancel
- **`e`** — edit the command before executing (type your corrected version)
- **`w`** — explain what the command does, then re-present the prompt
- **`r`** — refine: add a clarification and let the AI re-translate without starting over

Edits made via `e` are tracked by the [watch-and-learn](#watch-and-learn) system.

**To validate this in your shell:**
```
$ list all python files modified today
```
You should see the teal-gradient `╭ shako ─╮` panel around the translated command. Type `w` to get an explanation, then `Y` to run.

### Safety Layer

AI-generated commands are checked before confirmation:

- **Blocked** (`safety_mode = "block"`): `rm -rf /`, `mkfs`, `dd if=`, fork bombs
- **Warned** (`safety_mode = "warn"`): `sudo`, `rm`, `mv /`, `chmod`, `chown`
- **Off** (`safety_mode = "off"`): no safety checks

## Session Memory

shako's AI remembers the last 5 natural-language → command exchanges within your session. This means follow-up requests work naturally:

```
$ find all rust files bigger than 1MB
❯ fd -e rs --size +1m
[Y] → runs

$ now do the same but only in src/
❯ fd -e rs --size +1m src/
```

To clear session memory:
```
$ ai reset
```

## AI History Search

Use the `??` prefix to semantically search your command history:

```
$ ?? rsync command I used last week
Found: rsync -avz --progress ./build/ deploy@prod:/var/www/
[Y]es / [n]o:
```

The AI searches your shell history for commands matching your description, even if you don't remember the exact syntax.

## Proactive Suggestions

shako offers context-aware suggestions after certain commands succeed:

| Trigger | Suggestion |
|---|---|
| `git add <files>` | Asks if you want an AI-generated commit message from the staged diff |
| `git clone <url>` | Prints `tip: cd <repo-name>` |
| `cd` into a dir with a Makefile | Prints available `make` targets (up to 3) |

Example after `git add .`:
```
shako: 3 files staged — suggest a commit message? [y/N] y
thinking...
 ╭ shako ──────────────────────────────────────────────╮
 │  git commit -m "fix: resolve null pointer in lookup" │
 ├──────────────────────────────────────────────────────┤
 │  [Y]es  [n]o  [e]dit  [w]hy  [r]efine               │
 ╰──────────────────────────────────────────────────────╯
```

Note: The commit suggestion prompt is optional (`[y/N]` defaults to no). The AI generates a message from `git diff --staged` — the diff is capped at 4 KB.

## Watch-and-Learn

When you edit an AI-suggested command (via `[e]dit`), shako records the correction in `~/.config/shako/learned_prefs.toml`. These preferences are injected into future AI prompts:

```
$ show me files
❯ find . -type f
[e]dit → fd --type f
# shako learns: "user prefers fd over find"

# Next time:
$ show me files
❯ fd --type f              # AI now uses fd by default
```

## AI Kill Switch

Disable all AI features globally:

```toml
[behavior]
ai_enabled = false
```

With AI disabled, natural language input prints `shako: AI is disabled` instead of querying the LLM. All non-AI shell features continue to work.

## Explain Mode

Append `?` to any command to get a plain-English explanation without executing it. The explanation appears inside a branded `╭─ explain ──╮` header panel:

```
$ git rebase -i?

 ╭─ explain ──────────────────╮
 │  git rebase -i             │
 ╰────────────────────────────╯
  │ Starts an interactive rebase. Opens your editor with a list of commits,
  │ letting you reorder, squash, edit, or drop them. The -i flag means
  │ "interactive" — without it, rebase runs automatically.

$ chmod 755?

 ╭─ explain ──────────────────╮
 │  chmod 755                 │
 ╰────────────────────────────╯
  │ Sets file permissions: owner can read/write/execute (7), group and
  │ others can read/execute (5). Common for scripts and executables.
```

You can also use the `?` prefix with a bare command name:

```
$ ? grep

 ╭─ explain ──────────────────╮
 │  grep                      │
 ╰────────────────────────────╯
  │ Searches for text patterns in files. Reads from stdin or files given
  │ as arguments. Use -r for recursive, -n for line numbers, -i for
  │ case-insensitive. Modern alternative: ripgrep (rg).
```

The rule: if `?` prefix + single known command → explain. If `?` prefix + multiple words → translate.

### Markdown Rendering

Explanations are rendered as styled terminal markdown via `termimad`:

- **Headers** appear in bold cyan
- **Inline code** and **code blocks** appear in yellow
- **Bold** text is bright white; **italic** text is dimmed
- **Lists** and other markdown structures are formatted appropriately

To disable styling (e.g. for scripts or pipes), set `NO_COLOR=1` or use `TERM=dumb`:

```
$ NO_COLOR=1 shako -c "? curl"   # plain text output
```

**To validate this in your shell:**
```
$ tar xzf?
$ ? curl
$ grep -rn?
```
Each should render a `╭─ explain ─╮` panel with the command name in the header and styled markdown explanation text.

## Error Recovery

When a command fails with exit code ≥ 2, shako offers AI-powered diagnosis using a structured vertical-rail layout:

```
$ cargo build --featurse serde
error: unexpected argument '--featurse'

 ╷ ✗ exit 2  cargo build --featurse serde
 ╰ ask AI for help? [y/N] y

 ╷ cause:  Typo in flag name — '--featurse' should be '--features'
 ╷ fix:    cargo build --features serde
 ╰ [Y]es  [n]o  [e]dit:
```

The `╷`/`╰` vertical rail keeps the diagnostic output visually separate from shell noise. The exit code and failed command appear in the header line; cause and fix are presented in a calm, aligned column.

The AI receives:
- The exact command that failed
- The exit code
- The **last 20 lines of stderr** (captured in real time)
- Your recent command history
- Current directory and git state

Exit code 1 is skipped (too common — grep no-match, test failures). Signals (≥ 128) are also skipped.

**To validate this in your shell:**
```
$ cargo build --featurse serde
```
When prompted `ask AI for help? [y/N]`, type `y`. You should see the `╷ cause:` / `╷ fix:` / `╰` layout.

Alternatively, trigger with any command that exits non-zero with code ≥ 2:
```
$ git commit --badoption
```

## Context Awareness

shako gives the AI rich context for every query:

### Always included
- OS and architecture
- Current working directory
- Username
- Directory listing (files in cwd and `~/`)
- Installed modern tools with syntax guidance

### Git context
If you're in a git repository, the AI sees:
- Current branch name
- Clean/dirty status (number of changed files)
- Last 5 commit messages

This makes git-related queries much more accurate:
```
$ ? squash the last 3 commits
❯ git rebase -i HEAD~3
```

### Command history
The last 20 commands (configurable via `history_context_lines`) are included in the AI prompt. This enables follow-up queries:

```
$ fd -e rs                           # finds all .rs files
$ ? do that again but only in src/   # AI sees the previous fd command
❯ fd -e rs src/
```

### Per-project context (`.shako.toml`)

Drop a `.shako.toml` file in any project directory to give the AI project-specific instructions:

```toml
[ai]
context = """
Rust project using actix-web and SQLx.
Tests: cargo nextest run
Database: PostgreSQL on localhost:5433
Deploy: make deploy-staging
The API lives in src/api/, frontend in web/.
"""
```

The AI includes this in every prompt while you're in that directory. This means:

```
$ ? run the tests
❯ cargo nextest run              # knows to use nextest, not cargo test

$ ? connect to the database
❯ psql -h localhost -p 5433      # knows the port from project context
```

The file is optional and gitignore-friendly — commit it for the team or keep it personal.

## AI Pipe Builder

Use `|? <description>` to build a pipeline step-by-step with live previews of intermediate output. The AI decomposes your goal into stages and runs each one before asking for confirmation:

```
$ |? top 10 IPs by request count in access.log

shako: building pipeline...
╭── pipe builder ──────────────────────────────────────╮
│ Step 1: grep pattern from access.log                  │
│       grep -E '^[0-9]+\.' access.log                 │
│       ▶ 42,891 lines total, showing first 5:          │
│       192.168.1.1 - - [11/Apr ...                    │
│   + awk '{print $1}'                                  │
│       extract IP from each line                       │
│       192.168.1.1                                     │
│   + sort | uniq -c | sort -rn | head -10              │
│       rank by frequency, top 10                       │
│       3214 192.168.1.1                                │
│ full:  grep -E ... | awk '{print $1}' | sort | ...   │
├──────────────────────────────────────────────────────┤
│ [Y]es  [n]o  [e]dit                                  │
╰──────────────────────────────────────────────────────╯
```

Each stage runs against your actual data so you see what each step produces before committing. Choosing `[e]dit` lets you replace the full assembled pipeline before it runs.

## AI Audit Log

Every AI interaction is recorded to `~/.local/share/shako/audit.jsonl` in a tamper-evident hash-chained format. Each line is a JSON object containing:

| Field | Description |
|---|---|
| `ts` | RFC 3339 timestamp |
| `kind` | `ai_query`, `direct_command`, `safety_block`, or `exfil_block` |
| `nl_input` | Natural-language input you typed |
| `generated` | Command the AI produced |
| `executed` | Command actually run (may differ after `[e]dit`) |
| `decision` | `execute`, `edit`, `cancel`, or `block` |
| `exit_code` | Exit code of the executed command |
| `hash` | FNV-1a 64-bit hash of this entry chained to the previous |

Use `/audit verify` to check the chain is intact, and `/audit search <query>` to find past AI interactions.

The hash chain is not cryptographic — it deters accidental corruption and naive editing. For cryptographic assurance, layer GPG signing on top of the file.

## Prompt Injection Firewall

User-controlled strings that reach the AI system prompt are sanitized before insertion:

- **Sources checked**: `.shako.toml` `[ai].context`, `learned_prefs.toml` substitutions, `ai_system_prompt_extra`
- **Detection**: scans for phrases like `ignore previous instructions`, `forget your instructions`, `act as`, and similar injection patterns
- **Response**: matched fields are stripped entirely; the user is warned with the source path and matched pattern name
- **Structural wrapping**: clean fields are embedded in delimiter blocks that instruct the model to treat them as data, not instructions — analogous to SQL parameterization

## Secret Canary

Every AI-generated command is scanned for credential exfiltration patterns *before* the confirmation panel appears. This guards against compromised or socially-engineered LLM endpoints.

**Risk levels:**

| Level | Trigger | UI |
|---|---|---|
| `Critical` | Secret-file pattern **and** outbound network command in the same pipeline | Red double-border warning box |
| `High` | Secret-file access without an obvious outbound network step | Yellow warning box |
| `None` | No sensitive patterns detected | No additional UI |

**Detected secret patterns** include: `~/.aws/credentials`, `~/.ssh/id_*`, `~/.gnupg/`, `~/.kube/config`, `~/.docker/config.json`, `~/.npmrc`, `~/.netrc`, `~/.pypirc`, and common env var names (`API_KEY`, `AWS_SECRET_ACCESS_KEY`, `GITHUB_TOKEN`, etc.).

**Detected exfiltration commands**: `curl`, `wget`, `nc`, `scp`, `rsync`, `ssh`, and Python/Ruby/Perl one-liners that open sockets.

Critical-risk commands are also recorded in the audit log as `exfil_block` events.

## Capability-Scoped AI Sessions

Add an `[ai.scope]` block to `.shako.toml` to restrict what commands the AI is allowed to generate for a project. This is an allowlist model — ideal for data science environments, CI workers, or any context where the AI should only touch a known set of tools.

```toml
# .shako.toml
[ai.scope]
allow_commands = ["python", "pip", "jupyter", "rg", "fd", "git", "ls", "cat"]
deny_commands  = ["sudo", "rm", "curl", "wget"]
allow_sudo     = false
allow_network  = true
```

**Evaluation order** (deny wins):
1. `deny_commands` — always blocked, even if also in `allow_commands`
2. `allow_sudo = false` — blocks any command containing `sudo`
3. `allow_network = false` — blocks `curl`, `wget`, `nc`, `rsync`, and similar outbound tools
4. `allow_commands` — if non-empty, blocks any base command not in this list

Scope violations are shown before the confirmation prompt. An empty `allow_commands` (the default) means all commands are allowed unless `deny_commands` or the other flags block them.

## Behavioral Fingerprinting

shako learns your workflow patterns from your command history and uses them to personalise AI suggestions. The behavioral profile is rebuilt in a background thread after each session and injected into every AI prompt as a compact hint (≤ 500 tokens).

**What is tracked (locally only):**

| Signal | Example |
|---|---|
| Command co-occurrence | `cargo test` is almost always followed by `git add` in your projects |
| Flag preferences | You consistently pass `--dry-run` to `rsync` |
| Commit prefix style | Your commits use `fix:` / `feat:` conventional prefixes |
| Pre-command guards | You always `source venv/bin/activate` before `pip install` |

The profile is stored at `~/.config/shako/behavioral_profile.json`. No raw command content leaves your machine — only derived statistics are included in the AI context.

Disable with `[behavior] behavioral_fingerprinting = false` in `~/.config/shako/config.toml`.

## Danger Replay and Undo

Before executing a confirmed dangerous command (e.g. `rm -rf old_build/`), shako optionally snapshots the affected paths so you can recover them with a natural-language request:

```
$ rm -rf old_build/
shako: snapshot old_build/ before deleting? [y/N] y
→ snapshot sha3f7a9c saved

$ undo that rm
shako: restore old_build/ from snapshot sha3f7a9c? [y/N] y
→ restored
```

Natural-language undo phrases that trigger the undo flow:
- `undo that`, `undo that rm`, `undo the last command`
- `restore what I deleted`, `restore the last thing`
- `go back`, `revert that`, `undelete`

**Snapshot rules:**
- Git-tracked paths are skipped (use `git checkout` instead)
- Snapshots larger than 50 MB (configurable) are skipped
- Snapshots older than 7 days are garbage-collected automatically
- Snapshots live in `~/.local/share/shako/snapshots/<sha>/`
- The undo graph is stored in `~/.local/share/shako/undo_graph.json`

## LLM Configuration

shako works with any OpenAI-compatible API. See [Configuration](configuration.md) for provider setup.

The AI uses:
- **Temperature 0.1** by default (configurable) for deterministic translations
- **Max 512 tokens** per response
- **1 retry** with 2-second delay on network errors
- **Friendly error messages** when the LLM is unreachable
