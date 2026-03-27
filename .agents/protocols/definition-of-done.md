# Definition of Done

Exit criteria for each role. Work is not complete until ALL applicable criteria are met. The lead verifies these before accepting a handoff.

## Universal Criteria (All Roles)

- [ ] Task objective is fully addressed (not partially)
- [ ] Work stays within assigned scope
- [ ] Handoff document is complete and accurate
- [ ] No known issues are hidden or ignored
- [ ] All findings and decisions are documented

---

## Developer — Definition of Done

### Code Quality
- [ ] Code compiles/parses without errors
- [ ] No new warnings introduced (or warnings are justified)
- [ ] Error handling is present for all failure modes
- [ ] No hardcoded secrets, paths, or environment-specific values
- [ ] Code follows existing project patterns and style

### Testing
- [ ] All existing tests pass after changes
- [ ] Syntax/type checks pass
- [ ] Changes have been manually verified (if no automated tests exist)

### Scope
- [ ] Only files relevant to the task were modified
- [ ] No unrelated refactoring or cleanup
- [ ] If out-of-scope issues were found, they're noted (not fixed)

---

## Tester — Definition of Done

### Test Coverage
- [ ] Happy path tested
- [ ] At least 2 edge cases tested
- [ ] At least 1 error case tested
- [ ] Regression test written (if fixing a bug)

### Test Quality
- [ ] All tests pass
- [ ] Test names clearly describe what's being tested
- [ ] Assertions are meaningful (not just "no exception")
- [ ] Tests are deterministic (no flakiness)
- [ ] Tests are independent (no shared state / ordering dependency)

### Conventions
- [ ] Tests use project's test framework
- [ ] Tests are in the correct directory/file per project convention
- [ ] Test file naming matches project convention

---

## Reviewer — Definition of Done

### Review Completeness
- [ ] All changed files read in full
- [ ] Security checklist applied
- [ ] Error handling verified
- [ ] Test coverage assessed

### Review Quality
- [ ] All findings categorized by severity (blocking/warning/nit)
- [ ] Blocking issues have suggested fixes
- [ ] Overall verdict provided (approve/request-changes/blocking)
- [ ] Feedback is specific (file:line references, not vague)

---

## Release Engineer — Definition of Done

### Git Operations
- [ ] All changes committed with descriptive messages
- [ ] Commit messages follow project convention
- [ ] Working tree is clean
- [ ] No untracked files that should be committed

### Release (if applicable)
- [ ] Version number follows semver
- [ ] CHANGELOG.md updated
- [ ] Annotated tag created
- [ ] All tests pass on the final state

### History
- [ ] Git log reads clearly (no "WIP", "fix", "asdf" commits)
- [ ] Each commit is one logical change
- [ ] No merge conflicts left unresolved

---

## Doc Writer — Definition of Done

### Content
- [ ] Documentation is factually accurate (verified against code)
- [ ] Code examples are working and tested
- [ ] All new/changed features are documented
- [ ] No outdated information remains in changed docs

### Quality
- [ ] Writing is clear and concise
- [ ] Target audience can follow the documentation
- [ ] Consistent terminology throughout
- [ ] Proper markdown formatting

### Structure
- [ ] Documentation is in the correct location per project conventions
- [ ] Links and references are valid
- [ ] Follows existing doc structure/template

---

## Architect — Definition of Done

### Analysis
- [ ] All affected components identified
- [ ] Dependencies traced (direct and transitive)
- [ ] Impact assessment complete
- [ ] At least 2 alternatives considered

### Design
- [ ] Clear recommendation provided with rationale
- [ ] Trade-offs explicitly documented
- [ ] Interfaces/contracts defined (if applicable)
- [ ] Implementation guidance is specific enough for the developer

### Risk
- [ ] Risks identified and documented
- [ ] Backward compatibility assessed
- [ ] Performance implications considered

---

## Lead Verification Process

When reviewing a specialist's handoff:

1. **Read the handoff document** — Is it complete per the format?
2. **Check exit criteria** — Go through the role's DoD checklist above
3. **Verify evidence** — Are test results, file lists, and findings substantiated?
4. **Cross-check** — Does this output integrate cleanly with other specialists' work?
5. **Decide:**
   - **Approve** → Mark task as approved, proceed to next pipeline step
   - **Reject** → Provide specific feedback, return to specialist
   - **Partial approve** → Accept what's done, create follow-up task for gaps
