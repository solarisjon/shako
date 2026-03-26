# Agentic Development Framework

This project uses a **team-based multi-agent workflow** for software development. When working in this repository, you are part of a development team with specialized roles coordinated by a lead orchestrator.

## Quick Start

1. **Read your role** — Check `.agents/team.yaml` for the team structure
2. **Follow protocols** — All work follows the handoff and escalation protocols in `.agents/protocols/`
3. **Track tasks** — Active work lives in `.tasks/active/`, backlog in `.tasks/backlog.yaml`

## How It Works

A **Team Lead** (orchestrator) receives tasks, decomposes them, and delegates to specialist agents. Each specialist operates under a defined persona with specific constraints, tools, and exit criteria.

```
User Request
     │
     ▼
┌─────────────────────────────────────────────────────────────────┐
│                          TEAM LEAD                              │
│  • Decomposes task into sub-tasks                               │
│  • Assigns to appropriate specialist(s)                         │
│  • Reviews outputs against Definition of Done                   │
│  • Resolves conflicts and makes architectural calls             │
└──┬──────┬──────┬──────┬──────┬───────┬──────────┬──────────────┘
   │      │      │      │      │       │          │
┌──▼──┐┌──▼──┐┌──▼──┐┌──▼──┐┌─▼───┐┌──▼────┐┌───▼────┐
│ Dev ││Test ││Rev  ││Rel  ││ Doc ││Purify ││ Arch   │
│     ││     ││     ││ Eng ││Write││       ││        │
└─────┘└─────┘└─────┘└─────┘└──┬──┘└──▲────┘└────────┘
                                │      │
                                └──────┘
                           docs flow to purifier
```

## Entering a Role

When the lead delegates a task, adopt the persona defined in `.agents/roles/<role>.md`. Each role file contains:

- **Identity** — Who you are and your mindset
- **Constraints** — What you must and must not do
- **Tools** — Which tools you should use
- **Exit Criteria** — When your work is complete
- **Handoff Format** — How to return results to the lead

## Agent Runtime Compatibility

This framework is **runtime-agnostic**. It works with any coding agent that can read markdown files:

| Runtime | Adapter | Entry Point |
|---------|---------|-------------|
| **Crush** | `.agents/adapters/crush.md` | This file (AGENTS.md) + Crush skills |
| **OpenCode** | `.agents/adapters/opencode.md` | This file (AGENTS.md) + OpenCode agents |
| **Copilot CLI** | `.agents/adapters/copilot-cli.md` | This file (AGENTS.md) + Copilot instructions |

Each adapter maps the universal role definitions to the specific capabilities of that runtime.

## File Structure

```
.agents/
├── team.yaml              # Team roster, config, defaults
├── leads/
│   └── orchestrator.md    # Lead persona + delegation logic
├── roles/
│   ├── developer.md       # Implementation specialist
│   ├── tester.md          # Quality assurance specialist
│   ├── reviewer.md        # Code review + security audit
│   ├── release-engineer.md # Git, versioning, CI/CD
│   ├── doc-writer.md      # Documentation specialist
│   ├── purifier.md        # Doc purification via Purify/AISP
│   └── architect.md       # Design + technical strategy
├── protocols/
│   ├── handoff.md         # Task delegation format
│   ├── escalation.md      # When/how to escalate
│   └── definition-of-done.md # Acceptance criteria per role
├── templates/
│   ├── task-contract.md   # Standard task assignment
│   └── review-checklist.md # Code review template
└── adapters/
    ├── crush.md           # Crush-specific mappings
    ├── opencode.md        # OpenCode-specific mappings
    └── copilot-cli.md     # Copilot CLI mappings
.tasks/
├── backlog.yaml           # Queued work
└── active/                # Currently delegated tasks
```

## Principles

1. **Separation of concerns** — Each role has a single responsibility
2. **Explicit contracts** — No ambiguity in what's expected
3. **Verifiable output** — Every role has measurable exit criteria
4. **Progressive complexity** — Use only the roles you need (lead + developer is the minimum team)
5. **Runtime agnostic** — Works with any AI coding agent that reads files
