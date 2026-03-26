# Copilot CLI Adapter

How this agentic framework integrates with [GitHub Copilot CLI](https://docs.github.com/en/copilot/github-copilot-in-the-cli) and Copilot Workspace.

## Entry Point

Copilot CLI reads project context from repository files. Ensure `AGENTS.md` is committed to the repo root so Copilot can reference it.

For Copilot Workspace, the framework files in `.agents/` are automatically available as project context.

## Role Activation

Copilot CLI has a more constrained interaction model than Crush or OpenCode. Adapt the team framework as follows:

### Single-Turn Mode (Copilot CLI)

For `gh copilot suggest` and `gh copilot explain`:
- Role switching is not practical in single-turn mode
- Use the framework for **structured prompting** instead:

```bash
gh copilot suggest "Acting as the Developer role (per .agents/roles/developer.md), 
implement input validation for the /api/users endpoint. 
Follow the project's existing validation patterns."
```

### Multi-Turn Mode (Copilot Chat in IDE / Workspace)

For extended Copilot interactions:

1. Reference the team framework in your system prompt or workspace instructions
2. Use `@workspace` to include `.agents/` files as context
3. Switch roles by referencing persona files:
   ```
   @workspace Read .agents/roles/reviewer.md and review the changes 
   in src/api/users.ts using that role's checklist
   ```

## Copilot Workspace Integration

If using GitHub Copilot Workspace:

### Workspace Instructions (`.github/copilot-instructions.md`)

```markdown
## Development Framework

This project uses a team-based agentic development framework.

### Key Files
- `AGENTS.md` — Framework overview and entry point
- `.agents/team.yaml` — Team structure and configuration
- `.agents/leads/orchestrator.md` — Lead/orchestrator role
- `.agents/roles/` — Specialist role definitions
- `.agents/protocols/` — Handoff and escalation protocols

### Default Behavior
- Act as the Team Lead by default
- For implementation tasks, follow the Developer role constraints
- For review tasks, follow the Reviewer role checklist
- Always verify work against the Definition of Done
```

## GitHub Actions Integration

The framework can inform CI/CD pipelines:

```yaml
# .github/workflows/team-checks.yml
name: Team Quality Checks
on: [pull_request]
jobs:
  review-checklist:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Verify Definition of Done
        run: |
          # Check that PR description references task contract
          # Check that tests exist for changed code
          # Check that docs are updated if applicable
          echo "Checking team framework compliance..."
```

## Copilot-Specific Tool Mapping

| Framework Tool | Copilot Equivalent |
|---------------|-------------------|
| `edit` | Copilot inline suggestions / Workspace edits |
| `search` | `@workspace` search |
| `lsp` | IDE's built-in LSP (Copilot leverages it) |
| `bash` | `gh copilot suggest` for commands |
| `test_runner` | `gh copilot suggest` for test commands |
| `git` | Direct git commands or `gh` CLI |
| `gh_cli` | Native `gh` CLI integration |

## Limitations

Copilot CLI is more constrained than Crush or OpenCode:

1. **No persistent role switching** — Each interaction is relatively stateless
2. **Limited file editing** — Copilot CLI suggests, but doesn't directly edit
3. **No sub-agents** — Single agent model

### Workarounds

1. **Use task files** — Write task contracts to `.tasks/active/` so Copilot can read context
2. **Structured prompts** — Include role references in every prompt
3. **IDE integration** — Use Copilot Chat in VS Code/JetBrains for richer interaction
4. **Copilot Workspace** — Use for multi-file, multi-step tasks

## Best Practices for Copilot + This Framework

1. **Commit framework files** — Copilot needs them in the repo to reference
2. **Reference roles explicitly** — "As the Developer role, implement..."
3. **Use the review checklist** — Copy it into PR descriptions for Copilot-assisted review
4. **Task contracts as issues** — Create GitHub Issues from task contracts for Copilot to reference
5. **PR templates** — Base PR templates on the handoff formats
