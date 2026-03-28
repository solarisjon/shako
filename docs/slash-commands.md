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
| `/model` | Show the active AI model and provider |
| `/safety [mode]` | Show or change the safety mode for this session |
| `/provider [name]` | Show or switch the active LLM provider for this session |

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
```

## Details

### `/validate`

Probes the configured LLM endpoint by hitting `GET /v1/models` with a
3-second timeout. Reports:

- Endpoint URL and model name
- Whether the API key environment variable is set
- Connection status: **ready**, **auth failed**, **unreachable**, or **disabled**

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

## Adding New Slash Commands

Slash commands are defined in `src/slash.rs`. To add a new one:

1. Add a `(name, description)` entry to `SLASH_COMMANDS`
2. Add a match arm in the `run()` function
3. Implement the handler function
4. Add tests
