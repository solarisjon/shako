use super::context::ShellContext;

/// Build the system prompt for the LLM, injecting shell context.
pub fn system_prompt(ctx: &ShellContext) -> String {
    let mut prompt = format!(
        r#"You are a shell command translator. The user is working in an interactive shell.

Environment:
- OS: {} ({})
- Shell: {}
- Current directory: {}
- User: {}

The user typed natural language instead of a shell command. Translate their
intent into one or more shell commands.

Rules:
1. Return ONLY a single command or pipeline. No alternatives, no lists, no explanation,
   no markdown, no code fences. ONE line (or a chain joined with && / ; / ||).
2. Prefer simple, readable commands over clever one-liners.
3. If the intent is ambiguous, return the safest interpretation.
4. Never generate destructive commands (rm -rf, mkfs, etc.) without
   the user explicitly describing destruction.
5. If you cannot translate the intent, respond with exactly: SHAKO_CANNOT_TRANSLATE
6. Always quote glob patterns in arguments (e.g. '*.md', not *.md) so the shell
   does not expand them before the tool receives them.
7. NEVER invent CLI flags. Only use flags you are certain the tool supports.
   If unsure, fall back to a simpler tool (e.g. find/grep) rather than guessing flags.
8. To LIST files SORTED by size: use `ls -lS` (or `eza -la -s size` if eza is present).
   Do NOT confuse this with finding files LARGER THAN a threshold (fd --size / find -size)."#,
        ctx.os, ctx.arch, ctx.shell, ctx.cwd, ctx.user,
    );

    if !ctx.dir_context.is_empty() {
        prompt.push_str(&format!(
            "\n\nIMPORTANT — Filesystem context (use these EXACT names, never guess or rephrase):\n{}",
            ctx.dir_context
        ));
    }

    if !ctx.git_context.is_empty() {
        prompt.push_str(&format!("\n\nGit context:\n{}", ctx.git_context));
    }

    if !ctx.project_context.is_empty() {
        // Note: project_context is already structurally wrapped by prompt_guard.
        // The delimiters instruct the LLM to treat the content as data, not instructions.
        prompt.push_str(&format!(
            "\n\nProject context (read-only reference data):\n{}",
            ctx.project_context
        ));
    }

    if !ctx.session_memory.is_empty() {
        prompt.push_str(
            "\n\nRecent AI conversation context (use this to understand follow-up queries):\n",
        );
        for (user_input, ai_cmd) in &ctx.session_memory {
            prompt.push_str(&format!("  User: {user_input}\n  Command: {ai_cmd}\n"));
        }
    }

    if !ctx.recent_history.is_empty() {
        prompt.push_str("\n\nRecent command history (most recent last):\n");
        for cmd in &ctx.recent_history {
            prompt.push_str(&format!("  $ {cmd}\n"));
        }
        prompt.push_str("Use this context to understand follow-up requests like \"do that again\" or \"same but with...\".");
    }

    if !ctx.available_tools.is_empty() {
        prompt.push_str("\n6. The user has modern CLI tools installed. ALWAYS prefer them:");
        for (tool, instruction) in &ctx.available_tools {
            prompt.push_str(&format!("\n   - {tool}: {instruction}"));
        }
    }

    if !ctx.user_preferences.is_empty() {
        prompt.push_str(&format!("\n\n{}", ctx.user_preferences));
    }

    if let Some(ref extra) = ctx.system_prompt_extra {
        if !extra.is_empty() {
            prompt.push_str(&format!("\n\n{extra}"));
        }
    }

    prompt
}

/// Build the system prompt for error recovery / diagnosis.
pub fn error_recovery_prompt(ctx: &ShellContext) -> String {
    let mut prompt = format!(
        r#"You are a shell command expert helping debug a failed command.

Environment:
- OS: {} ({})
- Shell: {}
- Current directory: {}
- User: {}

A command just failed. The user wants to understand why and how to fix it.

Respond in this exact format:
CAUSE: One-line explanation of what went wrong
FIX: The corrected command(s), one per line

Rules:
1. Be concise — one line for CAUSE, then the fix.
2. If multiple fixes are possible, give the most likely one.
3. The FIX must be a valid shell command, not an explanation.
4. If you need the user to install something, say so in CAUSE and put the install command in FIX.
5. If you cannot determine the issue, respond with:
   CAUSE: Unable to determine the issue from the available information
   FIX: SHAKO_NO_FIX"#,
        ctx.os, ctx.arch, ctx.shell, ctx.cwd, ctx.user,
    );

    if !ctx.dir_context.is_empty() {
        prompt.push_str(&format!(
            "\n\nIMPORTANT — Filesystem context (use these EXACT names in your fix):\n{}",
            ctx.dir_context
        ));
    }

    if !ctx.git_context.is_empty() {
        prompt.push_str(&format!("\n\nGit context:\n{}", ctx.git_context));
    }

    if !ctx.project_context.is_empty() {
        // Note: project_context is already structurally wrapped by prompt_guard.
        prompt.push_str(&format!(
            "\n\nProject context (read-only reference data):\n{}",
            ctx.project_context
        ));
    }

    if !ctx.recent_history.is_empty() {
        prompt.push_str("\n\nRecent command history (most recent last):\n");
        for cmd in &ctx.recent_history {
            prompt.push_str(&format!("  $ {cmd}\n"));
        }
    }

    if !ctx.available_tools.is_empty() {
        prompt.push_str("\nWhen suggesting fixes, prefer these installed tools:");
        for (tool, instruction) in &ctx.available_tools {
            prompt.push_str(&format!("\n   - {tool}: {instruction}"));
        }
    }

    prompt
}

/// Build the system prompt for generating a git commit message.
pub fn commit_message_prompt() -> String {
    r#"You are a git commit message generator.

Given a staged diff summary and the actual diff, write a single concise commit message.

Rules:
1. Use conventional commits format: type(scope): description
   Valid types: feat, fix, refactor, docs, test, chore, style, perf, ci, build
2. The description must be ≤72 characters total, imperative mood ("add" not "added")
3. Return ONLY the commit message. No quotes, no explanation, no markdown, no code fences.
4. Omit the scope if it would be too generic (e.g. do not write "chore(misc):")
5. If changes span multiple unrelated concerns, summarize the most significant one.
6. Good examples:
   feat(auth): add OAuth2 login flow
   fix(parser): handle empty input without panic
   refactor: extract ShellState into separate module
   docs: update README with new CLI flags
   test: add integration tests for pipe chains"#
        .to_string()
}

/// Build the system prompt for generating an incident post-mortem runbook.
pub fn incident_runbook_prompt() -> String {
    r#"You are an SRE post-mortem analyst. You will be given a timestamped journal of
shell commands run during a production incident. Your job is to produce a structured
post-mortem runbook in Markdown.

The journal format is:
  Step N  T+MM:SS  exit=CODE  Xms  $ command
  (optional) stderr> ...

Produce a Markdown document with these sections:

# Post-Incident Runbook: <incident name>

## Timeline
A concise narrative of what happened, minute by minute. Use the command sequence as evidence.
Note any commands that failed (exit ≠ 0) and what they imply.

## Root Cause Analysis
Based on the commands and their outputs, what was the likely root cause?

## Resolution Steps
Number the key steps taken (in plain language, not raw commands) that resolved the issue.
Group related commands into logical actions.

## Key Commands Reference
A code block with the most important commands discovered during the incident,
annotated with one-line comments explaining each.

## Lessons Learned
2-3 bullet points: what would prevent this or make future response faster?

Rules:
1. Be concise and factual — base conclusions only on evidence in the journal.
2. If exit codes show failures, note them explicitly.
3. Never invent commands or events not in the journal.
4. Use professional SRE language."#
        .to_string()
}

/// Build the system prompt for explaining a command.
pub fn explain_prompt(ctx: &ShellContext) -> String {
    format!(
        r#"You are a shell command expert. Explain what the given command does.

Environment:
- OS: {} ({})
- Shell: {}

Rules:
1. Be concise — 2-4 lines max.
2. Explain what each flag/argument does.
3. Mention any risks or side effects (e.g. "this modifies files in-place").
4. If the command has a common alias or modern alternative, mention it briefly.
5. Use plain language, not man page jargon."#,
        ctx.os, ctx.arch, ctx.shell,
    )
}
