# AI Features

shako's AI integration is designed to augment your shell workflow — never to get in the way. Real commands execute instantly; the AI only activates when you need it.

## Natural Language Translation

Type what you want in plain English. shako translates it to a shell command, shows you what it will run, and asks for confirmation:

```
$ show me the 10 largest files in this directory
❯ fd --type f -x stat -f '%z %N' {} | sort -rn | head -10
[Y]es / [n]o / [e]dit:
```

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

### Safety Layer

AI-generated commands are checked before confirmation:

- **Blocked** (`safety_mode = "block"`): `rm -rf /`, `mkfs`, `dd if=`, fork bombs
- **Warned** (`safety_mode = "warn"`): `sudo`, `rm`, `mv /`, `chmod`, `chown`
- **Off** (`safety_mode = "off"`): no safety checks

## Explain Mode

Append `?` to any command to get a plain-English explanation without executing it:

```
$ git rebase -i?
git rebase -i
Starts an interactive rebase. Opens your editor with a list of commits,
letting you reorder, squash, edit, or drop them. The -i flag means
"interactive" — without it, rebase runs automatically.

$ chmod 755?
chmod 755
Sets file permissions: owner can read/write/execute (7), group and
others can read/execute (5). Common for scripts and executables.

$ tar xzf?
tar xzf
Extracts (x) a gzip-compressed (z) tar archive (f = read from file).
Equivalent to: gunzip the file, then untar it.
```

You can also use the `?` prefix with a bare command name:

```
$ ? grep
grep
Searches for text patterns in files. Reads from stdin or files given
as arguments. Use -r for recursive, -n for line numbers, -i for
case-insensitive. Modern alternative: ripgrep (rg).
```

The rule: if `?` prefix + single known command → explain. If `?` prefix + multiple words → translate.

## Error Recovery

When a command fails with exit code ≥ 2, shako offers AI-powered diagnosis:

```
$ cargo build --featurse serde
error: unexpected argument '--featurse'
shako: command failed (exit 2). ask AI for help? [y/N] y
  cause: Typo in flag name — '--featurse' should be '--features'
  fix: cargo build --features serde
  [Y]es / [n]o / [e]dit:
```

The AI receives:
- The exact command that failed
- The exit code
- The **last 20 lines of stderr** (captured in real time)
- Your recent command history
- Current directory and git state

Exit code 1 is skipped (too common — grep no-match, test failures). Signals (≥ 128) are also skipped.

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

## LLM Configuration

shako works with any OpenAI-compatible API. See [Configuration](configuration.md) for provider setup.

The AI uses:
- **Temperature 0.1** by default (configurable) for deterministic translations
- **Max 512 tokens** per response
- **1 retry** with 2-second delay on network errors
- **Friendly error messages** when the LLM is unreachable
