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
1. Return ONLY the command(s), one per line. No explanation, no markdown, no code fences.
2. Prefer simple, readable commands over clever one-liners.
3. If the intent is ambiguous, return the safest interpretation.
4. Never generate destructive commands (rm -rf, mkfs, etc.) without
   the user explicitly describing destruction.
5. If you cannot translate the intent, respond with exactly: JBOSH_CANNOT_TRANSLATE"#,
        ctx.os, ctx.arch, ctx.shell, ctx.cwd, ctx.user,
    );

    if !ctx.dir_context.is_empty() {
        prompt.push_str(&format!(
            "\n\nIMPORTANT — Filesystem context (use these EXACT names, never guess or rephrase):\n{}",
            ctx.dir_context
        ));
    }

    if !ctx.available_tools.is_empty() {
        prompt.push_str("\n6. The user has modern CLI tools installed. ALWAYS prefer them:");
        for (tool, instruction) in &ctx.available_tools {
            prompt.push_str(&format!("\n   - {tool}: {instruction}"));
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
   FIX: JBOSH_NO_FIX"#,
        ctx.os, ctx.arch, ctx.shell, ctx.cwd, ctx.user,
    );

    if !ctx.dir_context.is_empty() {
        prompt.push_str(&format!(
            "\n\nIMPORTANT — Filesystem context (use these EXACT names in your fix):\n{}",
            ctx.dir_context
        ));
    }

    if !ctx.available_tools.is_empty() {
        prompt.push_str("\nWhen suggesting fixes, prefer these installed tools:");
        for (tool, instruction) in &ctx.available_tools {
            prompt.push_str(&format!("\n   - {tool}: {instruction}"));
        }
    }

    prompt
}
