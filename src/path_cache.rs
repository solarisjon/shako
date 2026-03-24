use std::collections::HashSet;
use std::env;
use std::fs;
use std::sync::Arc;

/// Shared cache of all executable names found in $PATH.
///
/// Built once at startup and shared (via `Arc`) by the classifier,
/// completer, and highlighter so that none of them re-scan the
/// filesystem on every keystroke or tab press.
pub struct PathCache {
    /// Sorted, deduplicated list — used by completer (prefix filtering)
    /// and classifier (typo detection via edit distance).
    pub commands: Vec<String>,
    /// O(1) lookup set — used by the highlighter to colour-code the
    /// first token without calling `which()` on every keystroke.
    pub command_set: HashSet<String>,
}

impl PathCache {
    pub fn new() -> Arc<Self> {
        let commands = collect_path_commands();
        let command_set: HashSet<String> = commands.iter().cloned().collect();
        Arc::new(Self {
            commands,
            command_set,
        })
    }
}

/// Scan every directory in $PATH and collect executable names.
fn collect_path_commands() -> Vec<String> {
    let path_var = env::var("PATH").unwrap_or_default();
    let mut commands = Vec::new();

    for dir in path_var.split(':') {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Ok(name) = entry.file_name().into_string() {
                    commands.push(name);
                }
            }
        }
    }

    commands.sort();
    commands.dedup();
    commands
}
