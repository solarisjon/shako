//! Directory stack builtins: `pushd`, `popd`, `dirs`.

use super::nav::builtin_cd;
use super::state::ShellState;

/// `pushd` — push cwd onto the directory stack and cd to the new dir.
pub fn builtin_pushd(args: &[&str], state: &mut ShellState) -> i32 {
    if args.is_empty() {
        eprintln!("shako: pushd: too few arguments");
        return 1;
    }
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("shako: pushd: {e}");
            return 1;
        }
    };
    let code = builtin_cd(args);
    if code == 0 {
        state.dir_stack.push(cwd);
        builtin_dirs(state);
    }
    code
}

/// `popd` — pop the top directory off the stack and cd there.
pub fn builtin_popd(_args: &[&str], state: &mut ShellState) -> i32 {
    match state.dir_stack.pop() {
        Some(dir) => {
            let dir_str = dir.display().to_string();
            let code = builtin_cd(&[dir_str.as_str()]);
            if code == 0 {
                builtin_dirs(state);
            }
            code
        }
        None => {
            eprintln!("shako: popd: directory stack empty");
            1
        }
    }
}

/// `dirs` — print the directory stack (cwd first, then stack).
pub fn builtin_dirs(state: &ShellState) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut parts = vec![cwd.display().to_string()];
    for dir in state.dir_stack.iter().rev() {
        parts.push(dir.display().to_string());
    }
    println!("{}", parts.join("  "));
}
