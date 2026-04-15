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

### Anthropic Native API

To use Anthropic's native API (instead of OpenAI-compatible format), add `provider_type = "anthropic"`:

```toml
[providers.claude]
endpoint = "https://api.anthropic.com"
model = "claude-sonnet-4-5"
api_key_env = "ANTHROPIC_API_KEY"
provider_type = "anthropic"
```

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
edit_mode = "emacs"                      # "emacs" (default) or "vi"
behavioral_fingerprinting = true         # learn workflow patterns to personalise AI hints

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
| `provider_type` | string | (unset) | Set to `"anthropic"` to use Anthropic's native API format instead of OpenAI-compatible format |

## Behavior Config Fields

| Field | Type | Default | Description |
|---|---|---|---|
| `confirm_ai_commands` | boolean | `true` | Show `[Y/n/e]` before executing AI commands. When `false`, also auto-executes typo corrections without prompting |
| `auto_correct_typos` | boolean | `true` | Offer typo corrections for near-miss commands |
| `history_context_lines` | integer | `20` | Number of recent commands sent to AI for context |
| `safety_mode` | string | `"warn"` | `"warn"` shows warnings, `"block"` prevents execution, `"off"` disables |
| `edit_mode` | string | `"emacs"` | `"emacs"` (default) or `"vi"` for vi-style keybindings |
| `ai_enabled` | boolean | `true` | Global kill switch for all AI features |
| `behavioral_fingerprinting` | boolean | `true` | Learn command-sequence and flag-preference patterns; inject as AI context hint |

## Per-Project Config (`.shako.toml`)

Drop a `.shako.toml` in any project directory to provide AI context and per-project security scoping:

```toml
[ai]
context = "Rust project using actix-web. Tests: cargo nextest run. DB: PostgreSQL on port 5433."

# Optional: restrict which commands the AI is allowed to generate
[ai.scope]
allow_commands = ["python", "pip", "jupyter", "rg", "fd", "git", "ls", "cat"]
deny_commands  = ["sudo", "rm", "curl", "wget"]
allow_sudo     = false
allow_network  = true

# Optional: environment drift detection
[safety]
production_contexts = ["prod", "production", "prd"]  # kubectl / AWS profile substrings
context_warn_window_secs = 300                        # warn window after context switch (default: 5 min)

# Optional: incident runbook auto-save
[incident]
runbook_dir = "~/incidents"   # directory to save AI-generated runbooks
```

See [AI Features](ai-features.md#per-project-context-shakotoml) for details.

## Per-Project Config Fields

### `[ai.scope]` — Capability Scoping

Restricts which commands the AI is allowed to generate. If `[ai.scope]` is absent, no restrictions apply.

| Field | Type | Default | Description |
|---|---|---|---|
| `allow_commands` | list of strings | `[]` (all allowed) | Only these base command names are permitted. Empty list = allow all |
| `deny_commands` | list of strings | `[]` | Always denied, even if in `allow_commands` |
| `allow_sudo` | boolean | `false` | Whether `sudo`-prefixed commands are permitted |
| `allow_network` | boolean | `true` | Whether outbound network tools (`curl`, `wget`, `nc`, …) are permitted |

### `[safety]` — Environment Drift Detection

| Field | Type | Default | Description |
|---|---|---|---|
| `production_contexts` | list of strings | `[]` | Context name substrings (kubectl, AWS profile, etc.) treated as production |
| `context_warn_window_secs` | integer | `300` | Seconds after a context switch during which destructive commands trigger a warning |

### `[incident]` — Incident Mode

| Field | Type | Default | Description |
|---|---|---|---|
| `runbook_dir` | string | (unset) | Directory where `incident report` saves the AI-generated markdown runbook |

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
