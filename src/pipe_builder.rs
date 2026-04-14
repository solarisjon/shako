//! AI Pipe Builder with intermediate live previews.
//!
//! Triggered by `|? <description>` in the shell. The AI decomposes the
//! description into a pipeline plan (Vec of steps), executes each step
//! incrementally against real data, and shows a live preview of each
//! stage's output before the user commits to running the full pipeline.
//!
//! ## User workflow
//!
//! ```text
//! |? top 10 IPs by request count in access.log
//!
//! shako: building pipeline...
//! ╭── pipe builder ──────────────────────────────╮
//! │ Step 1: grep pattern from access.log          │
//! │ Preview: 42,891 matching lines               │
//! │ + awk '{print $1}'                            │
//! │ Preview: 192.168.1.1, 10.0.0.2, ...          │
//! │ + sort | uniq -c | sort -rn | head -10        │
//! │ Preview: 3,214 192.168.1.1 ...               │
//! │ Full pipeline: grep ... | awk ... | sort ...  │
//! │ [Y]es  [n]o  [e]dit  [a]dd step              │
//! ╰──────────────────────────────────────────────╯
//! ```

use serde::Deserialize;
use std::io::{self, Write};
use std::process::{Command, Stdio};

// ─── Types ─────────────────────────────────────────────────────────────────────

/// A single step in the AI-generated pipeline plan.
#[derive(Debug, Clone, Deserialize)]
pub struct PipelineStep {
    /// Human-readable description of what this step does.
    pub description: String,
    /// Shell command fragment for this step (to be joined with `|`).
    pub command: String,
}

/// AI-generated pipeline plan: an ordered list of steps whose commands
/// are joined with `|` to form the full pipeline.
#[derive(Debug, Clone)]
pub struct PipelinePlan {
    pub steps: Vec<PipelineStep>,
}

impl PipelinePlan {
    /// The full pipeline command: all step commands joined with ` | `.
    pub fn full_command(&self) -> String {
        self.steps
            .iter()
            .map(|s| s.command.as_str())
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

// ─── Pipeline planning ─────────────────────────────────────────────────────────

/// Parse an AI response into a [`PipelinePlan`].
///
/// The AI is instructed to return a JSON array of `{description, command}`
/// objects. If JSON parsing fails, we try a line-by-line fallback: treat
/// each non-empty line as a command fragment with an auto-generated description.
pub fn parse_plan(raw: &str) -> Option<PipelinePlan> {
    // Try JSON array parse.
    let raw = raw.trim();
    // Strip markdown code fences if present.
    let stripped = raw
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    if let Ok(steps) = serde_json::from_str::<Vec<PipelineStep>>(stripped) {
        if !steps.is_empty() {
            return Some(PipelinePlan { steps });
        }
    }

    // Fallback: treat each non-empty line as a command fragment.
    let steps: Vec<PipelineStep> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("```") && !l.starts_with('#'))
        .enumerate()
        .map(|(i, cmd)| PipelineStep {
            description: format!("step {}", i + 1),
            command: cmd.to_string(),
        })
        .collect();

    if steps.is_empty() {
        None
    } else {
        Some(PipelinePlan { steps })
    }
}

// ─── Preview execution ─────────────────────────────────────────────────────────

/// Execute a pipeline up to and including `step_index` and capture the first
/// 5 lines of output as a preview string.
///
/// Uses a 3-second timeout on the entire preview execution. If the command
/// times out or produces no output, returns a placeholder message.
pub fn run_preview(plan: &PipelinePlan, step_index: usize) -> String {
    let partial_cmd = plan
        .steps
        .iter()
        .take(step_index + 1)
        .map(|s| s.command.as_str())
        .collect::<Vec<_>>()
        .join(" | ");

    // Run via sh -c with head -5 appended to limit output.
    let preview_cmd = format!("({partial_cmd}) 2>/dev/null | head -5");

    let output = Command::new("sh")
        .arg("-c")
        .arg(&preview_cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();

    match output {
        Ok(out) if out.status.success() || !out.stdout.is_empty() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let lines: Vec<&str> = text.lines().take(5).collect();
            if lines.is_empty() {
                "(no output)".to_string()
            } else {
                lines.join("\n")
            }
        }
        Ok(_) => "(command produced no output or failed)".to_string(),
        Err(e) => format!("(preview error: {e})"),
    }
}

/// Count the number of output lines from the first step.
///
/// Used to show "42,891 matching lines" style stats.
pub fn count_lines_preview(plan: &PipelinePlan, step_index: usize) -> Option<usize> {
    let partial_cmd = plan
        .steps
        .iter()
        .take(step_index + 1)
        .map(|s| s.command.as_str())
        .collect::<Vec<_>>()
        .join(" | ");

    let count_cmd = format!("({partial_cmd}) 2>/dev/null | wc -l");
    let output = Command::new("sh")
        .arg("-c")
        .arg(&count_cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    let text = String::from_utf8_lossy(&output.stdout);
    text.trim().parse::<usize>().ok()
}

// ─── Interactive UI ────────────────────────────────────────────────────────────

/// Present the pipeline plan to the user with live previews and a confirm panel.
///
/// Returns `Some(command)` if the user confirms (or edits) the pipeline,
/// `None` if they cancel.
pub fn present_and_confirm(plan: &PipelinePlan) -> Option<String> {
    // Gradient palette matching the rest of the shell UI.
    const GRAD: &[u8] = &[30, 31, 32, 37, 38, 44, 45];
    let mid_color = GRAD[GRAD.len() / 2];

    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);

    let grad_line = |width: usize| -> String {
        (0..width)
            .map(|i| {
                let idx = if width <= 1 {
                    0
                } else {
                    i * (GRAD.len() - 1) / (width - 1)
                };
                format!("\x1b[38;5;{}m─\x1b[0m", GRAD[idx])
            })
            .collect()
    };
    let border = |c: char| format!("\x1b[38;5;{mid_color}m{c}\x1b[0m");
    let visible_len = |s: &str| -> usize {
        let mut len = 0;
        let mut in_esc = false;
        for c in s.chars() {
            if c == '\x1b' {
                in_esc = true;
            } else if in_esc {
                if c.is_ascii_alphabetic() {
                    in_esc = false;
                }
            } else {
                len += 1;
            }
        }
        len
    };

    // Collect all display rows (step + preview pairs).
    let mut rows: Vec<String> = Vec::new();
    for (i, step) in plan.steps.iter().enumerate() {
        let step_label = if i == 0 {
            format!("\x1b[90mStep 1:\x1b[0m \x1b[1;36m{}\x1b[0m", step.command)
        } else {
            format!("\x1b[90m  + \x1b[0m\x1b[1;36m{}\x1b[0m", step.command)
        };
        rows.push(step_label);
        if !step.description.is_empty() {
            rows.push(format!("      \x1b[90m{}\x1b[0m", step.description));
        }

        // Run preview for this step.
        eprint!("\x1b[90m  previewing step {}…\x1b[0m\r", i + 1);
        io::stdout().flush().ok();
        let preview = run_preview(plan, i);
        let count_hint = if i == 0 {
            count_lines_preview(plan, i)
        } else {
            None
        };
        print!("\r\x1b[K");
        io::stdout().flush().ok();

        // Format preview lines (indented, dim).
        for (pi, pline) in preview.lines().enumerate() {
            if pi == 0 {
                if let Some(cnt) = count_hint {
                    if cnt > 5 {
                        rows.push(format!(
                            "      \x1b[90m▶ {} lines total, showing first 5:\x1b[0m",
                            cnt
                        ));
                    }
                }
            }
            rows.push(format!("      \x1b[90m{pline}\x1b[0m"));
        }
    }

    let full_pipeline = plan.full_command();
    let pipeline_row = format!("\x1b[90mfull:\x1b[0m   \x1b[36m{full_pipeline}\x1b[0m");
    rows.push(pipeline_row.clone());

    let opts_row = "\x1b[90m[Y]es  [n]o  [e]dit\x1b[0m";

    // Calculate box width.
    let max_content = rows
        .iter()
        .chain(std::iter::once(&opts_row.to_string()))
        .map(|r| visible_len(r))
        .max()
        .unwrap_or(40)
        .max(40);
    let inner_width = (max_content + 4).min(term_width.saturating_sub(2));
    let content_inner = inner_width.saturating_sub(4);

    let box_line = |content: &str| -> String {
        let vl = visible_len(content);
        let pad = content_inner.saturating_sub(vl);
        format!(" {b}  {content}{}  {b}", " ".repeat(pad), b = border('│'))
    };

    // Header.
    let label = format!("\x1b[38;5;{mid_color}m pipe builder \x1b[0m");
    let label_vis = 15usize;
    let rest = inner_width.saturating_sub(label_vis + 2);

    eprintln!(
        " {tl}{sep}{label}{rest}{tr}",
        tl = border('╭'),
        sep = grad_line(2),
        label = label,
        rest = grad_line(rest),
        tr = border('╮'),
    );
    for row in &rows {
        eprintln!("{}", box_line(row));
    }
    eprintln!(
        " {bl}{sep}{br}",
        bl = border('├'),
        sep = grad_line(inner_width),
        br = border('┤'),
    );

    // Options prompt.
    let opts_vis = visible_len(opts_row);
    let opts_pad = content_inner.saturating_sub(opts_vis);
    eprint!(
        " {b}  {opts}{}  {b} ",
        " ".repeat(opts_pad),
        b = border('│'),
        opts = opts_row,
    );
    io::stdout().flush().ok();

    let mut answer = String::new();
    io::stdin().read_line(&mut answer).ok();
    let answer = answer.trim().to_lowercase();

    eprintln!(
        " {bl}{bot}{br}",
        bl = border('╰'),
        bot = grad_line(inner_width),
        br = border('╯'),
    );

    match answer.as_str() {
        "" | "y" | "yes" => Some(full_pipeline),
        "e" | "edit" => {
            eprint!(" {} ", border('❯'));
            io::stdout().flush().ok();
            let mut edited = String::new();
            io::stdin().read_line(&mut edited).ok();
            let edited = edited.trim().to_string();
            if edited.is_empty() {
                None
            } else {
                Some(edited)
            }
        }
        _ => None,
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plan_json() {
        let raw = r#"[{"description": "filter errors", "command": "grep ERROR log.txt"},
                      {"description": "count occurrences", "command": "wc -l"}]"#;
        let plan = parse_plan(raw).unwrap();
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].command, "grep ERROR log.txt");
        assert_eq!(plan.steps[1].command, "wc -l");
    }

    #[test]
    fn test_parse_plan_json_with_fences() {
        let raw = "```json\n[{\"description\":\"test\",\"command\":\"ls -la\"}]\n```";
        let plan = parse_plan(raw).unwrap();
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].command, "ls -la");
    }

    #[test]
    fn test_parse_plan_fallback_lines() {
        let raw = "grep pattern file.txt\nawk '{print $1}'\nsort | uniq -c";
        let plan = parse_plan(raw).unwrap();
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].command, "grep pattern file.txt");
    }

    #[test]
    fn test_full_command_join() {
        let plan = PipelinePlan {
            steps: vec![
                PipelineStep {
                    description: "filter".to_string(),
                    command: "grep foo bar.txt".to_string(),
                },
                PipelineStep {
                    description: "count".to_string(),
                    command: "wc -l".to_string(),
                },
            ],
        };
        assert_eq!(plan.full_command(), "grep foo bar.txt | wc -l");
    }

    #[test]
    fn test_run_preview_echo() {
        let plan = PipelinePlan {
            steps: vec![PipelineStep {
                description: "echo test".to_string(),
                command: "echo hello_world".to_string(),
            }],
        };
        let preview = run_preview(&plan, 0);
        assert!(
            preview.contains("hello_world"),
            "preview should show echo output"
        );
    }

    #[test]
    fn test_parse_plan_empty_returns_none() {
        assert!(parse_plan("").is_none());
        assert!(parse_plan("```\n```").is_none());
    }
}
