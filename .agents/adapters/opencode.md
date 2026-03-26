# OpenCode Adapter

How this agentic framework integrates with [OpenCode](https://opencode.ai) — the terminal-based AI coding assistant.

## Entry Point

OpenCode reads `AGENTS.md` in the project root as part of its context. This activates the team framework.

Additionally, OpenCode supports an `agents/` directory with agent definitions. This framework's `.agents/` directory serves the same purpose with extended role definitions.

## OpenCode Agent Mapping

OpenCode has built-in agent concepts. Map this framework's roles to OpenCode agents:

### Configuration (`.opencode/agents.yaml` or project config)

```yaml
agents:
  lead:
    description: "Team orchestrator — reads .agents/leads/orchestrator.md"
    instructions_file: ".agents/leads/orchestrator.md"
    
  developer:
    description: "Implementation specialist"
    instructions_file: ".agents/roles/developer.md"
    
  tester:
    description: "QA and test writing"
    instructions_file: ".agents/roles/tester.md"
    
  reviewer:
    description: "Code review and security audit"
    instructions_file: ".agents/roles/reviewer.md"
    
  release:
    description: "Git workflow and releases"
    instructions_file: ".agents/roles/release-engineer.md"
    
  docs:
    description: "Documentation"
    instructions_file: ".agents/roles/doc-writer.md"
    
  architect:
    description: "Design and dependency analysis"
    instructions_file: ".agents/roles/architect.md"
```

## Role Activation in OpenCode

OpenCode supports agent switching via commands or context. When delegating:

1. Use `@agent-name` syntax if supported, or
2. Prefix instructions with the role context:
   ```
   Acting as: Developer (per .agents/roles/developer.md)
   Task: [task contract]
   ```
3. The agent follows the role's constraints and handoff format
4. Results are reviewed by switching back to the lead agent

## OpenCode-Specific Tool Mapping

| Framework Tool | OpenCode Tool |
|---------------|--------------|
| `edit` | Built-in file editing |
| `search` | Built-in search / grep |
| `lsp` | Built-in LSP integration |
| `bash` | Built-in terminal |
| `test_runner` | Terminal (project test command) |
| `git` | Terminal (git commands) |
| `gh_cli` | Terminal (gh commands) |

## Sub-Agent Pattern

If OpenCode supports spawning sub-agents:

1. Lead agent receives user request
2. Lead spawns specialist sub-agent with:
   - The role's persona file as system instructions
   - The task contract as the prompt
   - Constrained tool access per the role definition
3. Sub-agent completes work and returns handoff
4. Lead reviews and continues pipeline

If sub-agents are not supported, use the same prompt-based role switching as the Crush adapter.

## Session State

OpenCode sessions can persist context. Use this for multi-step pipelines:

1. Lead's delegation decisions persist in the conversation
2. Task state files in `.tasks/active/` provide cross-session continuity
3. Git commits provide permanent record of completed work

## Configuration Files

If OpenCode uses a project config file (e.g., `.opencode.yaml`), add:

```yaml
context:
  - AGENTS.md
  - .agents/team.yaml
  
instructions: |
  This project uses a team-based development framework.
  Read AGENTS.md for the team structure and protocols.
  Default to the Team Lead role unless explicitly assigned another role.
```
