//! Test builtin: `test` / `[`.

/// `test`/`[` — evaluate a conditional expression. Returns 0 (true) or 1 (false).
pub fn builtin_test(args: &[&str]) -> i32 {
    if test_eval(args) {
        0
    } else {
        1
    }
}

pub fn test_eval(args: &[&str]) -> bool {
    match args {
        [] => false,
        ["!", rest @ ..] => !test_eval(rest),
        [op, operand] => test_unary(op, operand),
        [lhs, op, rhs] => test_binary(lhs, op, rhs),
        _ => {
            if let Some(pos) = args.iter().position(|a| *a == "-o") {
                return test_eval(&args[..pos]) || test_eval(&args[pos + 1..]);
            }
            if let Some(pos) = args.iter().position(|a| *a == "-a") {
                return test_eval(&args[..pos]) && test_eval(&args[pos + 1..]);
            }
            args.len() == 1 && !args[0].is_empty()
        }
    }
}

fn test_unary(op: &str, operand: &str) -> bool {
    use std::path::Path;
    let path = Path::new(operand);
    match op {
        "-e" => path.exists(),
        "-f" => path.is_file(),
        "-d" => path.is_dir(),
        "-r" => {
            use std::os::unix::fs::PermissionsExt;
            path.metadata()
                .map(|m| m.permissions().mode() & 0o444 != 0)
                .unwrap_or(false)
        }
        "-w" => {
            use std::os::unix::fs::PermissionsExt;
            path.metadata()
                .map(|m| m.permissions().mode() & 0o222 != 0)
                .unwrap_or(false)
        }
        "-x" => {
            use std::os::unix::fs::PermissionsExt;
            path.metadata()
                .map(|m| m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        }
        "-s" => path.metadata().map(|m| m.len() > 0).unwrap_or(false),
        "-L" | "-h" => path
            .symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false),
        "-z" => operand.is_empty(),
        "-n" => !operand.is_empty(),
        _ => !op.is_empty(),
    }
}

fn test_binary(lhs: &str, op: &str, rhs: &str) -> bool {
    match op {
        "=" | "==" => lhs == rhs,
        "!=" => lhs != rhs,
        "-eq" => parse_int(lhs) == parse_int(rhs),
        "-ne" => parse_int(lhs) != parse_int(rhs),
        "-lt" => parse_int(lhs) < parse_int(rhs),
        "-le" => parse_int(lhs) <= parse_int(rhs),
        "-gt" => parse_int(lhs) > parse_int(rhs),
        "-ge" => parse_int(lhs) >= parse_int(rhs),
        _ => false,
    }
}

fn parse_int(s: &str) -> i64 {
    s.trim().parse().unwrap_or(0)
}
