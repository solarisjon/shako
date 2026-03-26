# Escalation Protocol

When and how to escalate issues up the chain. Escalation is not failure — it's a signal that a decision needs to be made at a higher level.

## Escalation Levels

```
Level 0: Specialist handles independently (within role constraints)
Level 1: Specialist → Lead (needs guidance or approval)
Level 2: Lead → User (needs business decision or external input)
```

## When Specialists Must Escalate to Lead

### Always Escalate:
1. **Scope creep** — The task requires changes beyond what was assigned
2. **Architectural decision** — Multiple valid approaches with significant trade-offs
3. **Breaking change** — The fix requires modifying a public API or contract
4. **Security finding** — Any vulnerability or exposure discovered during work
5. **Dependency issue** — Need to add, remove, or upgrade a dependency
6. **Conflict** — Task requirements contradict existing code or another task's output
7. **Stuck > 10 minutes** — Tried multiple approaches, none working
8. **Ambiguous requirement** — Acceptance criteria can be interpreted multiple ways

### Don't Escalate (Handle Yourself):
1. Minor style decisions within your role's authority
2. Implementation details that don't affect the interface
3. Test structure decisions (how to organize tests, what framework features to use)
4. Documentation formatting choices
5. Git commit message wording

## Escalation Format

```markdown
## Escalation

**From:** [role]
**Task:** [task ID]
**Severity:** blocking | needs-decision | informational

### Situation
[What's happening — be specific]

### What I Tried
1. [Approach 1 and why it didn't work]
2. [Approach 2 and why it didn't work]

### Options
1. **[Option A]** — [description, pros, cons]
2. **[Option B]** — [description, pros, cons]
3. **[Option C]** — [description, pros, cons]

### My Recommendation
[Which option and why — specialists should always have an opinion]

### Impact of Delay
[What happens if this isn't resolved quickly]
```

## When Lead Must Escalate to User

### Always Escalate:
1. **Ambiguous business requirement** — "Should this delete the record or soft-delete it?"
2. **Significant scope change** — Original request needs substantially more work than expected
3. **Risk disclosure** — A decision could cause data loss, security exposure, or breaking changes
4. **Multiple valid approaches** — Trade-offs that depend on user's priorities (speed vs. correctness, etc.)
5. **Missing access** — Need credentials, permissions, or resources that can't be obtained autonomously
6. **Can't reproduce** — Bug report can't be verified with available information

### Don't Escalate (Decide Yourself):
1. Implementation approach (when one is clearly better)
2. Which files to change
3. Test strategy
4. Commit granularity
5. Documentation structure
6. Role assignment

## Lead Escalation Format

When the lead needs user input, keep it concise and actionable:

```markdown
**Decision needed:** [one-sentence question]

**Context:** [minimum context for the user to decide]

**Options:**
1. [Option] — [one-sentence trade-off]
2. [Option] — [one-sentence trade-off]

**My recommendation:** [option] because [reason]

**If no preference:** I'll go with [default option]
```

## Escalation Response Protocol

### Lead responding to specialist:
- Respond with a clear decision, not another question
- If more investigation is needed, reassign as a new sub-task
- Update the task contract with any scope changes
- Unblock the specialist as fast as possible

### User responding to lead:
- Lead incorporates the decision and continues
- No need for the user to understand the internal team structure
- Lead translates user intent into specific task updates

## Escalation Anti-Patterns

1. **Escalation ping-pong** — Escalating back and forth without resolution. The lead must make a decision.
2. **Premature escalation** — Escalating before trying at least 2-3 approaches independently.
3. **Missing context** — Escalating without explaining what was tried and what options exist.
4. **Escalation avoidance** — Making decisions that should involve the lead (scope changes, security issues).
5. **Over-escalation to user** — Asking the user technical questions the team should resolve internally.
