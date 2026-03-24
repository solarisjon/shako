# Configuration

## Config File Location

`~/.config/shako/config.toml` — created by the setup wizard on first run, or manually.

Re-run the wizard anytime with `shako --init`.

## LLM Providers

shako supports multiple named providers. Set `active_provider` to switch between them:

```toml
active_provider = "work_proxy"

[providers.lm_studio]
endpoint = "http://localhost:1234"
model = "your-local-model"

[providers.work_proxy]
endpoint = "https://llm-proxy.company.com"
model = "claude-sonnet-4.5"
api_key_env = "LLMPROXY_KEY"
verify_ssl = false

[providers.openai]
endpoint = "https://api.openai.com"
model = "gpt-4o"
api_key_env = "OPENAI_API_KEY"
```

Any OpenAI-compatible endpoint works. The endpoint is auto-normalized:
- Bare hostnames get `https://` and `/v1/chat/completions` added
- `http://localhost:*` keeps `http://`

### Legacy Single Provider

If you don't need multiple providers, use the `[llm]` block (used when `active_provider` is unset):

```toml
[llm]
endpoint = "http://localhost:11434/v1/chat/completions"
model = "claude-haiku-4.5"
```

## Full Config Reference

```toml
# Which named provider to use (omit to use [llm] block)
active_provider = "work_proxy"

# --- Named providers ---

[providers.lm_studio]
endpoint = "http://localhost:1234"       # auto-appends /v1/chat/completions
model = "your-model-name"
# api_key_env not needed for local models
timeout_secs = 30                        # request timeout (default: 30)
max_tokens = 512                         # max response tokens (default: 512)
verify_ssl = true                        # set false for self-signed certs
temperature = 0.1                        # LLM temperature (default: 0.1)

[providers.work_proxy]
endpoint = "https://llm-proxy.company.com"
model = "claude-sonnet-4.5"
api_key_env = "LLMPROXY_KEY"             # env var name (not the key itself!)
verify_ssl = false

# --- Legacy single provider (used when active_provider is unset) ---

[llm]
endpoint = "http://localhost:11434/v1/chat/completions"
model = "claude-haiku-4.5"
api_key_env = "LLMPROXY_KEY"
timeout_secs = 30
max_tokens = 512
verify_ssl = true
temperature = 0.1

# --- Behavior settings ---

[behavior]
confirm_ai_commands = true               # show command before executing (default: true)
auto_correct_typos = true                # suggest corrections for near-miss commands
history_context_lines = 20               # recent commands included in AI context
safety_mode = "warn"                     # "warn" | "block" | "off"

# --- Fish compatibility ---

[fish]
source_config = false                    # source ~/.config/fish/ at startup

# --- User aliases (override smart defaults) ---

[aliases]
ll = "ls -la"
".." = "cd .."
"..." = "cd ../.."
gs = "git status -sb"
```

## Provider Config Fields

| Field | Type | Default | Description |
|---|---|---|---|
| `endpoint` | string | `http://localhost:11434/v1/chat/completions` | LLM API endpoint (auto-normalized) |
| `model` | string | `claude-haiku-4.5` | Model name sent to the API |
| `api_key_env` | string | `LLMPROXY_KEY` | Name of env var holding the API key |
| `timeout_secs` | integer | `30` | HTTP request timeout in seconds |
| `max_tokens` | integer | `512` | Maximum tokens in LLM response |
| `verify_ssl` | boolean | `true` | Verify TLS certificates (false for self-signed) |
| `temperature` | float | `0.1` | LLM temperature (lower = more deterministic) |

## Behavior Config Fields

| Field | Type | Default | Description |
|---|---|---|---|
| `confirm_ai_commands` | boolean | `true` | Show `[Y/n/e]` before executing AI commands |
| `auto_correct_typos` | boolean | `true` | Offer typo corrections for near-miss commands |
| `history_context_lines` | integer | `20` | Number of recent commands sent to AI for context |
| `safety_mode` | string | `"warn"` | `"warn"` shows warnings, `"block"` prevents execution, `"off"` disables |

## Per-Project Config (`.shako.toml`)

Drop a `.shako.toml` in any project directory to provide AI context specific to that project:

```toml
[ai]
context = "Rust project using actix-web. Tests: cargo nextest run. DB: PostgreSQL on port 5433."
```

See [AI Features](ai-features.md#per-project-context-shakotoml) for details.

## Startup Script

`~/.config/shako/config.shako` (or `init.sh` for backward compatibility) is sourced at startup. Supports:

```bash
alias k='kubectl'
export EDITOR=nvim
set -x GOPATH ~/go              # fish-style
set -gx DOCKER_HOST unix:///var/run/docker.sock

function mkcd() { mkdir -p "$1" && cd "$1" }
```

## Config Snippets (`conf.d/`)

Files in `~/.config/shako/conf.d/` are sourced alphabetically at startup. Use numeric prefixes for ordering:

```
conf.d/
├── 00-env.sh         # environment variables
├── 10-aliases.sh     # aliases
└── 20-functions.sh   # function definitions
```

## Autoloaded Functions

Files in `~/.config/shako/functions/` define functions that are lazily loaded on first call:

```bash
# ~/.config/shako/functions/mkcd.sh
function mkcd() { mkdir -p "$1" && cd "$1" }
```

The function name must match the filename (without `.sh`).
