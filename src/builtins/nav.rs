//! Navigation builtins: `cd`, `z`, `zi`.

use crate::smart_defaults;

/// `cd` — change the current directory.
pub fn builtin_cd(args: &[&str]) -> i32 {
    let target = if args.is_empty() {
        match dirs::home_dir() {
            Some(home) => home,
            None => {
                eprintln!("shako: cd: HOME not set");
                return 1;
            }
        }
    } else if args[0] == "-" {
        match std::env::var("OLDPWD") {
            Ok(old) => {
                println!("{old}");
                std::path::PathBuf::from(old)
            }
            Err(_) => {
                eprintln!("shako: cd: OLDPWD not set");
                return 1;
            }
        }
    } else {
        let path = args[0];
        // If the path still contains glob metacharacters, it means the glob
        // expansion found no matches and returned the pattern literally.
        if path.chars().any(|c| c == '*' || c == '?' || c == '[') {
            eprintln!("shako: cd: {path}: no matches found");
            return 1;
        }
        if path.starts_with('~') {
            match dirs::home_dir() {
                Some(home) => home.join(path.trim_start_matches('~').trim_start_matches('/')),
                None => {
                    eprintln!("shako: cd: HOME not set");
                    return 1;
                }
            }
        } else {
            std::path::PathBuf::from(path)
        }
    };

    if let Ok(current) = std::env::current_dir() {
        unsafe { std::env::set_var("OLDPWD", current) };
    }

    if let Err(e) = std::env::set_current_dir(&target) {
        eprintln!("shako: cd: {}: {e}", target.display());
        1
    } else if let Ok(cwd) = std::env::current_dir() {
        // Keep PWD in sync — Starship and most Unix tools read PWD rather than
        // resolving the kernel cwd, so without this the prompt stays stale.
        unsafe { std::env::set_var("PWD", &cwd) };
        if smart_defaults::has_zoxide() {
            smart_defaults::zoxide_add(&cwd.display().to_string());
        }
        0
    } else {
        0
    }
}

/// `z` — zoxide-powered smart cd. Falls back to regular cd if zoxide is not installed.
pub fn builtin_z(args: &[&str]) {
    if args.is_empty() {
        builtin_cd(args);
        return;
    }

    if !smart_defaults::has_zoxide() {
        builtin_cd(args);
        return;
    }

    match smart_defaults::zoxide_query(args) {
        Some(path) => {
            builtin_cd(&[path.as_str()]);
        }
        None => {
            eprintln!("shako: z: no match for {:?}", args.join(" "));
        }
    }
}

/// `zi` — interactive zoxide selection via fzf.
pub fn builtin_zi() {
    if !smart_defaults::has_zoxide() {
        eprintln!("shako: zi: zoxide not installed");
        return;
    }

    let output = std::process::Command::new("zoxide")
        .args(["query", "--list", "--score"])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let list = String::from_utf8_lossy(&out.stdout).to_string();
            if list.trim().is_empty() {
                eprintln!("shako: zi: no directories tracked yet");
                return;
            }

            if smart_defaults::has_fzf() {
                if let Some(selected) = smart_defaults::fzf_select(&list, "z>") {
                    // zoxide output format: "  score /path/to/dir"
                    let path = selected.split_whitespace().last().unwrap_or("");
                    if !path.is_empty() {
                        builtin_cd(&[path]);
                    }
                }
            } else {
                // No fzf — just print the list
                print!("{list}");
            }
        }
        _ => eprintln!("shako: zi: failed to query zoxide"),
    }
}
