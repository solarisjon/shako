//! Read builtin: `read`.

/// `read` — read a line from stdin into a variable.
///   -p prompt   print prompt before reading
///   -r          raw mode (accepted, currently default)
///   VAR         variable name to store result (default: REPLY)
pub fn builtin_read(args: &[&str]) -> i32 {
    let mut prompt = "";
    let mut var_name = "REPLY";
    let mut i = 0;

    while i < args.len() {
        match args[i] {
            "-p" => {
                i += 1;
                if i < args.len() {
                    prompt = args[i];
                }
            }
            "-r" => {}
            arg if !arg.starts_with('-') => {
                var_name = arg;
            }
            _ => {}
        }
        i += 1;
    }

    if !prompt.is_empty() {
        use std::io::Write;
        print!("{prompt}");
        std::io::stdout().flush().ok();
    }

    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) => return 1,
        Ok(_) => {}
        Err(e) => {
            eprintln!("shako: read: {e}");
            return 1;
        }
    }

    let value = line.trim_end_matches('\n').trim_end_matches('\r');
    unsafe { std::env::set_var(var_name, value) };
    0
}
