# Code Review Checklist Template

Use this checklist for formal code reviews. Not every item applies to every review — skip items that are clearly not relevant, but consciously decide (don't accidentally skip).

---

## Review: [Task ID / PR / Change Description]

**Reviewer:** [role: reviewer]
**Date:** [timestamp]
**Files reviewed:** [count]

---

### Correctness

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | Code does what the task requires | | |
| 2 | Logic is sound (no off-by-one, wrong comparisons) | | |
| 3 | Edge cases handled (null, empty, boundary values) | | |
| 4 | Return values and error codes are correct | | |
| 5 | State mutations are safe (no race conditions) | | |
| 6 | Resource cleanup on all paths (files, connections) | | |

### Security

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | No hardcoded secrets/tokens/passwords | | |
| 2 | Input validated at trust boundaries | | |
| 3 | No injection vectors (SQL, XSS, command) | | |
| 4 | Auth/authz checks present where needed | | |
| 5 | Sensitive data not in logs or error messages | | |
| 6 | Dependencies from trusted sources | | |

### Error Handling

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All error paths handled (no silent failures) | | |
| 2 | Error messages informative but not leaking | | |
| 3 | Errors propagated correctly (not swallowed) | | |
| 4 | Graceful degradation where appropriate | | |

### Maintainability

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | Code readable without extensive comments | | |
| 2 | Functions have single responsibility | | |
| 3 | No unnecessary complexity or abstraction | | |
| 4 | Names clear and consistent with codebase | | |
| 5 | No dead code or commented-out code | | |
| 6 | DRY — no unnecessary duplication | | |

### Performance

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | No obvious N+1 or unbounded operations | | |
| 2 | Resource usage bounded (memory, handles) | | |
| 3 | Appropriate caching if relevant | | |

### Testing

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | Tests exist for new/changed code | | |
| 2 | Error cases tested (not just happy path) | | |
| 3 | Tests are deterministic | | |
| 4 | Assertions are meaningful | | |

### Style & Conventions

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | Follows project's established style | | |
| 2 | Consistent with surrounding code | | |
| 3 | No gratuitous formatting changes | | |

---

### Findings Summary

**Blocking:**
1. [file:line — issue — suggested fix]

**Warning:**
1. [file:line — issue — suggestion]

**Nit:**
1. [file:line — minor issue]

**Praise:**
1. [file:line — good pattern/approach]

---

**Verdict:** [ ] Approve  [ ] Request Changes  [ ] Blocking Issues

**Summary:** [2-3 sentence overall assessment]
