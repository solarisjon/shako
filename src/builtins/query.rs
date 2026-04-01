//! Query builtins: `type`, `functions`.

use super::state::ShellState;
use super::BUILTINS;

/// `type` — describe how each NAME would be interpreted.
pub fn builtin_type(args: &[&str], state: &ShellState) -> i32 {
    let mut found = true;
    for arg in args {
        if BUILTINS.contains(arg) {
            println!("{arg} is a shell builtin");
        } else if let Some(func) = state.functions.get(*arg) {
            println!("{arg} is a function: {{ {} }}", func.body);
        } else if let Some(value) = state.aliases.get(*arg) {
            println!("{arg} is aliased to '{value}'");
        } else if let Some(value) = state.abbreviations.get(*arg) {
            println!("{arg} is an abbreviation for '{value}'");
        } else if let Ok(path) = which::which(arg) {
            println!("{arg} is {}", path.display());
        } else {
            eprintln!("shako: type: {arg}: not found");
            found = false;
        }
    }
    if found {
        0
    } else {
        1
    }
}

/// `functions` — list all defined shell functions.
pub fn builtin_functions(state: &ShellState) {
    if state.functions.is_empty() {
        return;
    }
    let mut sorted: Vec<_> = state.functions.iter().collect();
    sorted.sort_by_key(|(k, _)| (*k).clone());
    for (name, func) in sorted {
        println!("function {name}() {{ {} }}", func.body);
    }
}
