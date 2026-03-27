# Handoff Protocol

How tasks are passed between the lead and specialist roles. This protocol ensures no context is lost and expectations are clear at every transition.

## Lead → Specialist Handoff

When the lead delegates a task, the following structure is used:

```markdown
## Task Assignment

**ID:** [unique identifier, e.g., TASK-001]
**Role:** [developer | tester | reviewer | release-engineer | doc-writer | purifier | architect]
**Priority:** [blocking | high | normal | low]
**Pipeline:** [feature | bugfix | refactor | docs-only]

### Objective
[One clear sentence describing what needs to be done]

### Context
- **Codebase area:** [which files/directories are relevant]
- **Related tasks:** [any prior work this builds on]
- **Constraints:** [performance requirements, backward compatibility, etc.]
- **User's exact words:** [quote the original request if helpful]

### Acceptance Criteria
1. [Specific, verifiable condition]
2. [Specific, verifiable condition]
3. [Specific, verifiable condition]

### Input Files
- `path/to/relevant/file.ext` — [why this file matters]
- `path/to/another/file.ext` — [what to look at]

### Expected Output
[What the specialist should produce — code changes, test file, review document, etc.]

### Notes
[Any additional context, gotchas, or decisions already made]
```

## Specialist → Lead Handoff

When a specialist completes work, they report using their role-specific handoff format (defined in each role's persona file). All handoffs share these common elements:

```markdown
## [Role] Handoff

**Task:** [task ID from assignment]
**Status:** complete | partial | blocked

### Work Summary
[Brief description of what was done]

### Changes / Deliverables
[List of files changed, tests written, findings reported, etc.]

### Verification
[Evidence that exit criteria are met — test results, lint output, etc.]

### Issues / Blockers
[Anything preventing completion or requiring lead attention]

### Recommendations
[Optional — suggestions for next steps or follow-up work]
```

## Handoff Rules

### For the Lead:
1. **Be specific** — Vague tasks produce vague results
2. **Include "why"** — Context helps specialists make better decisions
3. **Set boundaries** — Explicitly state what's in and out of scope
4. **One task per handoff** — Don't bundle unrelated work
5. **Provide files** — List the starting files so the specialist doesn't waste time searching

### For Specialists:
1. **Stay in scope** — Only do what was assigned. Note anything else for the lead.
2. **Report honestly** — If something is partial or blocked, say so clearly
3. **Include evidence** — Don't just say "tests pass" — show the output
4. **Flag surprises** — If you found something unexpected, report it even if you fixed it
5. **Use your role's format** — Consistent structure helps the lead review quickly

## Inter-Specialist Handoff

Sometimes one specialist's output is input for another (e.g., developer → tester). In these cases:

1. The lead reviews the first specialist's output
2. The lead creates a new task contract for the second specialist
3. The lead includes relevant output from the first specialist as context
4. Specialists never delegate directly to each other — everything goes through the lead

## Handoff State Transitions

```
pending → assigned → in_progress → review → approved → completed
                         │                      │
                         ▼                      ▼
                      blocked              rejected
                         │                      │
                         ▼                      ▼
                    (escalation)          (back to specialist
                                          with feedback)
```

## Blocked Tasks

When a specialist is blocked:

```markdown
## Blocked Report

**Task:** [task ID]
**Blocked by:** [specific issue]
**Attempted:** [what the specialist tried]
**Needs:** [what the lead or another specialist must provide]
**Can continue with:** [any partial work that can proceed independently]
```

The lead then either:
- Resolves the blocker directly
- Assigns a different specialist to unblock
- Escalates to the user for clarification
- Descopes the blocked portion
