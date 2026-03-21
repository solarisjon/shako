# jbosh — Jon's Brilliant Operating Shell

## Vision

A fish-inspired interactive shell where natural language and shell commands coexist seamlessly. Type `ls` and it runs immediately. Type `list all python files modified today` and it routes through an AI backend, translates to a shell command, confirms, and executes.

**Shell-first, AI-augmented** — not an AI agent that happens to have shell access.

## Core Design Principles

1. **Zero-latency for real commands** — if the input resolves to an executable, run it instantly with no AI roundtrip
2. **Transparent AI fallback** — natural language inputs get routed to LLM, translated to commands, confirmed, then executed
3. **Configurable backend** — works with any OpenAI-compatible LLM API (corporate proxies, local models, cloud providers)
4. **Fish-grade UX** — syntax highlighting, autosuggestions, smart completions, sane defaults
5. **Learn from the user** — AI context includes shell history, current directory, OS, and user corrections

## Input Classification (The Core Problem)

### Heuristic Approach (Phase 1)

```
Input → Tokenize first word
  → Resolves to binary in $PATH?          → Execute as command
  → Is a shell builtin (cd, export, etc)?  → Execute as builtin
  → Starts with known operator (|, >, <)?  → Parse as pipeline
  → Starts with sigil `?` or `ai:`?        → Force AI mode
  → None of the above?                     → Route to AI
```

### Edge Cases

| Input | Resolution |
|---|---|
| `ls` | Command (in $PATH) |
| `lss` | Not in $PATH → AI → "Did you mean `ls`?" or translates intent |
| `git stash pop` | Command (git in $PATH) |
| `undo my last git commit` | AI → `git reset HEAD~1` |
| `list all python files modified today` | AI → `find . -name "*.py" -mtime 0` |
| `? ls` | Force AI — explain what ls does |
| `rm -rf /` | Command (but safety layer intercepts) |

### Typo vs Intent Ambiguity

When a first-token doesn't resolve to a binary:
1. Check Levenshtein distance to known commands (< 2 edits = likely typo → suggest correction)
2. If not a typo → route to AI

## Architecture

```
┌──────────────────────────────────────────────────────┐
│                     jbosh                            │
│                                                      │
│  ┌─────────┐    ┌──────────────┐    ┌─────────────┐ │
│  │ Reedline │───→│   Classifier │───→│  Executor   │ │
│  │ (input)  │    │              │    │  (fork/exec)│ │
│  └─────────┘    │  command?────────→│             │ │
│       │          │  builtin?───────→│  Builtins   │ │
│       │          │  NL?────────┐   └─────────────┘ │
│       │          └─────────────│───────────────────┘ │
│       │                        │                      │
│       │                        ▼                      │
│       │          ┌─────────────────────┐              │
│       │          │    AI Bridge        │              │
│       │          │                     │              │
│       │          │  ┌───────────────┐  │              │
│       │          │  │ Context       │  │              │
│       │          │  │ - cwd         │  │              │
│       │          │  │ - OS/arch     │  │              │
│       │          │  │ - history     │  │              │
│       │          │  │ - ls output   │  │              │
│       │          │  └───────────────┘  │              │
│       │          │         │           │              │
│       │          │         ▼           │              │
│       │          │  ┌───────────────┐  │              │
│       │          │  │ LLM Client    │──│──→ LLM Proxy │
│       │          │  │ (configurable)│  │   (any       │
│       │          │  └───────────────┘  │    OpenAI-   │
│       │          │         │           │    compat)   │
│       │          │         ▼           │              │
│       │          │  ┌───────────────┐  │              │
│       │          │  │ Confirmation  │  │              │
│       │          │  │ Prompt        │  │              │
│       │          │  └───────────────┘  │              │
│       │          └─────────────────────┘              │
│       │                                               │
│  ┌────▼────┐                                          │
│  │ Highlighter, Completer, Hinter (fish-like UX)     │
│  └──────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────┘
```

## Module Layout

```
src/
├── main.rs              # Entry point, REPL loop
├── classifier.rs        # Input classification (command vs NL)
├── executor.rs          # Process execution, pipes, redirects
├── builtins.rs          # cd, export, source, exit, etc.
├── ai/
│   ├── mod.rs           # AI bridge orchestrator
│   ├── client.rs        # HTTP client for LLM API
│   ├── context.rs       # Build context (cwd, history, OS)
│   ├── prompt.rs        # System prompt templates
│   └── confirm.rs       # User confirmation before execution
├── shell/
│   ├── mod.rs           # Shell state (env, aliases, history)
│   ├── highlighter.rs   # Syntax highlighting (fish-like)
│   ├── completer.rs     # Tab completion
│   ├── hinter.rs        # Autosuggestions from history
│   └── prompt.rs        # Prompt rendering (git, path, etc.)
├── config/
│   ├── mod.rs           # Config loading
│   └── schema.rs        # Config types
└── safety.rs            # Dangerous command detection
```

## Configuration

```toml
# ~/.config/jbosh/config.toml

[llm]
endpoint = "https://llm-proxy.internal.company.com/v1/chat/completions"
model = "gpt-4"
api_key_env = "JBOSH_LLM_KEY"        # env var name holding the key
timeout_secs = 30
max_tokens = 512
verify_ssl = false                     # for internal CAs

[behavior]
confirm_ai_commands = true             # show translated command before executing
auto_correct_typos = true              # suggest corrections for near-miss commands
history_context_lines = 20             # how many history lines to send as AI context
safety_mode = "warn"                   # "warn" | "block" | "off" for dangerous commands

[appearance]
theme = "fish"                         # syntax highlighting theme
prompt_style = "starship"              # "minimal" | "fish" | "starship"
show_ai_thinking = false               # show "thinking..." while waiting for LLM

[aliases]
ll = "ls -la"
".." = "cd .."
```

## AI System Prompt (Draft)

```
You are a shell command translator. The user is working in an interactive shell.

Environment:
- OS: {os} ({arch})
- Shell: jbosh
- Current directory: {cwd}
- Recent history: {history}

The user typed natural language instead of a shell command. Translate their
intent into one or more shell commands.

Rules:
1. Return ONLY the command(s), one per line. No explanation.
2. Use standard POSIX utilities when possible.
3. Prefer simple, readable commands over clever one-liners.
4. If the intent is ambiguous, return the safest interpretation.
5. Never generate destructive commands (rm -rf, mkfs, etc.) without
   the user explicitly describing destruction.
6. If you cannot translate the intent, respond with: JBOSH_CANNOT_TRANSLATE
```

## Safety Layer

Commands matching these patterns trigger confirmation regardless of source:

- `rm -rf` with `/`, `~`, or `*`
- `mkfs`, `dd`, `:(){ :|:& };:`
- `chmod 777`, `chmod -R`
- `> /dev/sda`, `> /dev/null` (for important files)
- Any `sudo` command generated by AI

## Phased Roadmap

### Phase 1: Walking (MVP)
- [ ] Basic REPL with reedline
- [ ] Input classification (command vs NL)
- [ ] Command execution (fork/exec, basic pipes)
- [ ] LLM client with configurable endpoint
- [ ] AI translation → confirm → execute flow
- [ ] Config file loading
- [ ] Basic builtins (cd, exit, export)

### Phase 2: Running (Fish Parity)
- [ ] Syntax highlighting
- [ ] History-based autosuggestions
- [ ] Tab completion (path, command, git)
- [ ] Prompt customization (git branch, exit code, etc.)
- [ ] Job control (bg, fg, Ctrl-Z)
- [ ] Pipes and redirects
- [ ] Aliases and abbreviations

### Phase 3: Flying (AI-Native Features)
- [ ] AI-powered completions ("show me files ..." → suggests contextual commands)
- [ ] `explain` mode — `? ls -la` explains the command
- [ ] Error recovery — command fails → AI suggests fix
- [ ] Multi-step AI workflows ("set up a python project here")
- [ ] Context-aware suggestions based on project type (Makefile, package.json, etc.)
- [ ] Learning from corrections (user edits AI suggestion → fine-tune context)

### Phase 4: Orbit (Ecosystem)
- [ ] Plugin system
- [ ] Shareable prompt templates
- [ ] Shell scripting language (fish-like, not POSIX)
- [ ] Remote session support
- [ ] Team/org shared configs

## Technology Stack

| Component | Choice | Rationale |
|---|---|---|
| Language | Rust | Performance, safety, nushell/fish precedent |
| Line editor | reedline | Nushell's battle-tested editor, built for shells |
| Terminal | crossterm | Cross-platform terminal manipulation |
| TUI (future) | ratatui | Rich terminal UI when needed |
| HTTP | reqwest | Async HTTP for LLM API calls |
| Async | tokio | Runtime for async LLM calls |
| Config | toml | Standard Rust config format |
| Serialization | serde + serde_json | LLM API request/response handling |

## Key Decisions Log

| Decision | Choice | Why |
|---|---|---|
| Shell vs wrapper | Full shell | Wrappers can't do highlighting, completion, job control |
| AI routing | Heuristic-first | Zero latency for real commands, no AI dependency for basic use |
| LLM protocol | OpenAI-compatible | De facto standard, all proxies support it |
| Confirmation UX | Inline prompt | Show translated command, [Y/n/e(dit)] before execution |
| Config format | TOML | Rust ecosystem standard, readable |

## Name

**jbosh** — Jon's Brilliant Operating Shell (or just "bosh" in casual use).
