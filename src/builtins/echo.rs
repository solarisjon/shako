//! Echo builtin: `echo`.

/// `echo` — print arguments to stdout.
///   -n   no trailing newline
///   -e   interpret backslash escapes (\n \t \\ \a \b \r)
pub fn builtin_echo(args: &[&str]) -> i32 {
    let mut newline = true;
    let mut interpret = false;
    let mut arg_start = 0;

    for (i, arg) in args.iter().enumerate() {
        match *arg {
            "-n" => {
                newline = false;
                arg_start = i + 1;
            }
            "-e" => {
                interpret = true;
                arg_start = i + 1;
            }
            "-ne" | "-en" => {
                newline = false;
                interpret = true;
                arg_start = i + 1;
            }
            _ => break,
        }
    }

    let output = args[arg_start..].join(" ");
    let output = if interpret {
        unescape_echo(&output)
    } else {
        output
    };

    if newline {
        println!("{output}");
    } else {
        print!("{output}");
        use std::io::Write;
        std::io::stdout().flush().ok();
    }
    0
}

fn unescape_echo(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some('a') => result.push('\x07'),
                Some('b') => result.push('\x08'),
                Some('\\') => result.push('\\'),
                Some('0') => result.push('\0'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}
