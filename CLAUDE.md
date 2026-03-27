# shako — Claude Code Guidelines

## Project identity

shako is a **fish-inspired, AI-augmented interactive shell** written in Rust.
The design goal is fish shell compatibility first. When in doubt about syntax,
behaviour, or UX, fish shell is the reference implementation.

## Shell syntax — fish compliance is mandatory

shako control-flow syntax **must** follow fish, not bash:

| Construct | fish (correct) | bash (do NOT use) |
|---|---|---|
| for loop | `for VAR in LIST` … `end` | `for VAR in LIST; do` … `done` |
| while loop | `while CONDITION` … `end` | `while CONDITION; do` … `done` |
| if / else | `if CONDITION` … `else if` … `else` … `end` | `if …; then` … `elif` … `else` … `fi` |
| function | `function name` … `end` | `function name() { … }` |

**`end` closes every block.** There is no `do`, `done`, `fi`, or `then`.

Any documentation, examples, or code you add must use fish syntax.
Never add bash-style examples to the docs.

## Backward compatibility

The control engine may silently accept `done`/`fi` as aliases for `end` so
users migrating from bash are not immediately broken, but the **canonical**
and **documented** syntax is always fish-style `end`.

## Key modules

- `src/control.rs` — control-flow parser + executor (for/while/if/break/continue)
- `src/parser.rs` — word splitting, glob, tilde, variable, command substitution
- `src/ai/client.rs` — LLM HTTP client
- `src/builtins.rs` — builtin commands and ShellState (large, avoid unnecessary edits)
- `src/classifier.rs` — routes input to shell / AI / builtin

## Code style

- Rust 2024 edition, `cargo clippy` must pass clean before any PR
- No `unwrap()` in non-test code without a preceding comment justifying it
- End-to-end flow: `main.rs` REPL → `classifier.rs` → `control.rs` / `executor.rs` / `ai/`

## Workflow

Before opening a PR: `cargo fmt && cargo clippy && cargo test`
