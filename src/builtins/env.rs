//! Environment builtins: `export`, `unset`.

/// `export` — display or set environment variables.
///   export KEY=VALUE   sets the variable
///   export KEY         displays the current value
pub fn builtin_export(args: &[&str]) {
    for arg in args {
        if let Some((key, value)) = arg.split_once('=') {
            unsafe { std::env::set_var(key, value) };
        } else {
            match std::env::var(arg) {
                Ok(val) => println!("{arg}={val}"),
                Err(_) => eprintln!("shako: export: {arg}: not set"),
            }
        }
    }
}

/// `unset` — remove environment variables.
pub fn builtin_unset(args: &[&str]) {
    for arg in args {
        unsafe { std::env::remove_var(arg) };
    }
}
