# Task Contract Template

Copy this template when creating a new task assignment. Fill in all sections. Remove any sections marked (optional) if not needed.

---

## Task Assignment

**ID:** TASK-XXX
**Role:** [developer | tester | reviewer | release-engineer | doc-writer | architect]
**Priority:** [blocking | high | normal | low]
**Pipeline:** [feature | bugfix | refactor | docs-only]
**Created:** [timestamp]
**Deadline:** [if applicable]

### Objective

[One clear sentence: what needs to be done and why]

### Context

**Codebase area:**
- `path/to/relevant/dir/` — [why this area]

**Background:**
[What the user asked for, what decisions have been made, what prior work exists]

**Constraints:**
- [Must maintain backward compatibility with X]
- [Must not exceed Y ms response time]
- [Must work with Z version of dependency]

**Related tasks:** [TASK-YYY, TASK-ZZZ] (optional)

### Acceptance Criteria

1. [ ] [Specific, testable condition]
2. [ ] [Specific, testable condition]
3. [ ] [Specific, testable condition]
4. [ ] All existing tests pass
5. [ ] Changes follow project conventions

### Input

**Files to read:**
- `path/to/file.ext` — [what to look for]
- `path/to/other.ext` — [what to look for]

**Prior specialist output:** (optional)
[Reference handoff from another specialist if this task depends on it]

### Expected Output

**Deliverables:**
- [Code changes in `path/to/file.ext`]
- [New test file at `path/to/test.ext`]
- [Review document]
- [Updated CHANGELOG.md]

**Format:** [Use your role's standard handoff format]

### Notes

[Gotchas, edge cases, decisions the lead has already made, things to watch out for]

### Scope Boundaries

**In scope:**
- [Explicitly included]

**Out of scope:**
- [Explicitly excluded — note for later if needed]

---

## Task Lifecycle

```
1. Lead creates this contract
2. Specialist reads contract + persona file
3. Specialist works autonomously within constraints
4. Specialist produces handoff in role-specific format
5. Lead reviews against acceptance criteria + Definition of Done
6. Lead approves, rejects (with feedback), or escalates
```
