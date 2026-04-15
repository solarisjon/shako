# Slash Commands

Slash commands are shako meta-commands for inspecting and configuring the shell
at runtime. They use a `/name [args]` syntax and are classified before builtins
or `$PATH` lookup, so they never conflict with external programs.

Absolute filesystem paths (e.g. `/usr/bin/ls`) are **not** affected — only
single alphabetic words after `/` are treated as slash commands.

## Available Commands

| Command | Description |
|---|---|
| `/help` | List all available slash commands |
| `/validate` | Validate the AI endpoint (connectivity, auth, model) |
| `/config` | Show the full current configuration |
| `/model` | Show the active AI model and provider (read-only; edit config to change) |
| `/safety [mode]` | Show or change the safety mode for this session |
| `/provider [name]` | Show or switch the active LLM provider for this session |
| `/history` | Fuzzy-browse shell history and select a command to re-run |
| `/audit verify\|search <q>` | Verify the AI audit log chain or search AI interaction history |
| `/shortcuts [tool]` | List smart default shortcuts, optionally filtered by tool name |

## Usage Examples

```
$ /help                   # list all slash commands
$ /validate               # check AI endpoint health
$ /config                 # dump current config to terminal
$ /model                  # show active model
$ /safety                 # show current safety mode
$ /safety off             # disable safety checks (session only)
$ /safety warn            # re-enable safety warnings
$ /provider lm_studio     # switch to the lm_studio provider
$ /provider               # show available providers
$ /history                # browse history interactively (fzf if available)
$ /audit verify           # check audit log hash chain integrity
$ /audit search rsync     # find past AI interactions mentioning rsync
$ /shortcuts              # list all smart default shortcuts
$ /shortcuts podman       # show only podman shortcuts
$ /shortcuts git          # show only git shortcuts
```

## Details

### `/validate`

Probes the configured LLM endpoint by hitting `GET /v1/models` with a
3-second timeout. Reports:

- Endpoint URL and model name
- Whether the API key environment variable is set
- Connection status: **ready**, **auth failed**, **unreachable**, or **disabled**

### `/model`

Shows the active provider name and model. Runtime model switching is not yet supported — to change models, edit `~/.config/shako/config.toml` and restart.

### `/safety [mode]`

Controls how shako handles dangerous AI-generated commands (`rm -rf`,
`sudo`, `chmod`, etc.):

| Mode | Behavior |
|---|---|
| `warn` | Show a warning, still allow execution (default) |
| `block` | Refuse to execute dangerous commands |
| `off` | No safety checks |

Changes are **session-only** — they revert when you exit. Edit
`~/.config/shako/config.toml` to change the default.

### `/provider [name]`

Switch between named LLM providers defined in your config:

```toml
# ~/.config/shako/config.toml
active_provider = "work_proxy"

[providers.lm_studio]
endpoint = "http://localhost:1234"
model = "llama3"

[providers.work_proxy]
endpoint = "https://llm-proxy.corp.example.com"
model = "claude-haiku-4.5"
api_key_env = "LLMPROXY_KEY"
```

```
$ /provider lm_studio     # switch to local model
$ /provider work_proxy    # switch back to work proxy
```

Changes are **session-only**.

### `/history`

Browse your shell history interactively and select a command to pre-fill in the readline buffer:

- **With `fzf`**: history is piped through `fzf` with `--height=40% --reverse`. Press Enter to select; the chosen command appears in the prompt ready to edit or run.
- **Without `fzf`**: a built-in paginated picker is used.

The selected command is placed in the readline input buffer rather than executed immediately, so you can review or edit it first.

### `/audit`

Manage the immutable AI audit log at `~/.local/share/shako/audit.jsonl`.

#### `/audit verify`

Walks the entire JSONL file and verifies the hash chain. Reports:

- Total number of entries
- Whether the chain is intact
- On failure: the line number and nature of the break (prev_hash mismatch or hash mismatch)

```
$ /audit verify
✓ audit log intact — 1,247 entries
```

#### `/audit search <query>`

Case-insensitive substring search across `nl_input`, `generated`, and `executed` fields. Returns the 20 most-recent matches:

```
$ /audit search rsync
[2026-04-11T14:23:01Z] ai_query
  input:    sync build artifacts to staging
  generated: rsync -avz --progress ./build/ deploy@prod:/var/www/
  executed:  rsync -avz --progress ./build/ deploy@prod:/var/www/
  decision:  execute  exit: 0
```

### `/shortcuts [tool]`

List all smart default shortcuts shako has registered, grouped by the tool they require. Each entry shows whether the tool is currently installed (✓) or missing (✗).

```
$ /shortcuts
Smart default shortcuts  (✓ active  ✗ tool not installed)

  git
    ✓  ga        git add
    ✓  gaa       git add -A
    ✓  gs        git status
    ...

  podman
    ✓  pps       podman ps
    ✗  ppod      podman pod ps
    ...
```

Filtering by tool name shows only matching shortcuts:

```
$ /shortcuts kubectl
Shortcuts for 'kubectl'  (✓ active  ✗ tool not installed)

  kubectl
    ✓  k         kubectl
    ✓  kgp       kubectl get pods
    ...
```

See [Smart Defaults](smart-defaults.md) for the full shortcut reference.

## Adding New Slash Commands

Slash commands are defined in `src/slash.rs`. To add a new one:

1. Add a `(name, description)` entry to `SLASH_COMMANDS`
2. Add a match arm in the `run()` function
3. Implement the handler function
4. Add tests
