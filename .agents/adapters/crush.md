# Crush Adapter

How this agentic framework integrates with [Crush](https://charm.sh/crush) — Charm's CLI coding agent.

## Entry Point

Crush automatically reads `AGENTS.md` in the project root. This is the universal entry point for the team framework.

## Role Activation

In Crush, role switching happens through **context loading**. When the lead delegates a task:

1. The lead states which role to enter
2. Read the role's persona file from `.agents/roles/<role>.md`
3. Adopt the persona's identity, constraints, and working process
4. Complete the task following the role's guidelines
5. Produce the role-specific handoff format
6. Return to lead persona to review

### Example Flow

```
User: "Add input validation to the API endpoint"

Lead (reading orchestrator.md):
  → Analyzes: small feature, needs developer + tester
  → Pipeline: feature (simplified — skip architect, skip doc-writer)

Lead delegates to Developer:
  ## Entering Role: Developer
  [reads .agents/roles/developer.md]
  [creates task contract]
  [implements validation]
  [produces developer handoff]

Lead reviews developer output, then delegates to Tester:
  ## Entering Role: Tester
  [reads .agents/roles/tester.md]
  [creates task contract with developer's changes as context]
  [writes tests for the validation]
  [produces tester handoff]

Lead reviews, approves, delivers to user.
```

## Crush Skills Integration

This framework complements Crush skills. Skills are **capability packs** (how to do something), while roles are **behavioral personas** (how to think and work). They combine naturally:

| Crush Skill | Used By Role |
|-------------|-------------|
| `jons-dev-workflow` | Developer, Release Engineer |
| `cpe-engineer-workflow` | Developer, Release Engineer |
| `escalation-assistant` | Architect (for analysis) |
| `html-report-builder` | Doc Writer (for reports) |
| `confluence-publisher` | Doc Writer (for publishing) |
| `bug-hunter` | Developer (for investigation) |

## Crush-Specific Tool Mapping

| Framework Tool | Crush Tool |
|---------------|------------|
| `edit` | `edit` / `multiedit` |
| `search` | `grep` / `glob` / `agent` |
| `lsp` | `lsp_references` / `lsp_diagnostics` |
| `bash` | `bash` |
| `test_runner` | `bash` (with project's test command) |
| `git` | `bash` (git commands) |
| `gh_cli` | `bash` (gh commands) |

## Task Tracking in Crush

Tasks are tracked via files in `.tasks/`:
- **Backlog:** `.tasks/backlog.yaml`
- **Active:** `.tasks/active/TASK-XXX.md` (one file per active task)
- **Completed:** Tasks are removed from active/ when done (tracked in git history)

The Crush `todos` tool can also be used for real-time progress tracking within a session, complementing the file-based task system.

## Memory Integration

Store discovered project commands and patterns in Crush memory:
- Build commands
- Test commands
- Lint commands
- Deploy commands
- Project-specific conventions

These persist across sessions and benefit all roles.

## Multi-Session Workflow

For large tasks spanning multiple Crush sessions:
1. Lead creates task contracts in `.tasks/active/`
2. Each session picks up where the last left off by reading active tasks
3. Completed work is committed, task files updated
4. Next session reads the state and continues

## Configuration

No special Crush configuration needed. The framework is activated simply by having `AGENTS.md` in the project root. Crush reads it automatically and the team structure is available.
