//! Alias and abbreviation builtins: `alias`, `unalias`, `abbr`.

use super::state::ShellState;

/// `alias` — display or define aliases.
pub fn builtin_alias(args: &[&str], state: &mut ShellState) {
    if args.is_empty() {
        if state.aliases.is_empty() {
            return;
        }
        let mut sorted: Vec<_> = state.aliases.iter().collect();
        sorted.sort_by_key(|(k, _)| (*k).clone());
        for (name, value) in sorted {
            println!("alias {name}='{value}'");
        }
        return;
    }

    for arg in args {
        if let Some((name, value)) = arg.split_once('=') {
            let value = value.trim_matches('\'').trim_matches('"');
            state.aliases.insert(name.to_string(), value.to_string());
        } else {
            match state.aliases.get(*arg) {
                Some(value) => println!("alias {arg}='{value}'"),
                None => eprintln!("shako: alias: {arg}: not found"),
            }
        }
    }
}

/// `unalias` — remove aliases.
pub fn builtin_unalias(args: &[&str], state: &mut ShellState) {
    for arg in args {
        if *arg == "-a" {
            state.aliases.clear();
            return;
        }
        if state.aliases.remove(*arg).is_none() {
            eprintln!("shako: unalias: {arg}: not found");
        }
    }
}

/// Fish-compatible `abbr` builtin.
///   abbr --add name 'expansion'   (or -a)
///   abbr --erase name             (or -e)
///   abbr --list                   (or -l, or no args)
///   abbr name 'expansion'         (shorthand for --add)
pub fn builtin_abbr(args: &[&str], state: &mut ShellState) {
    if args.is_empty() {
        let mut sorted: Vec<_> = state.abbreviations.iter().collect();
        sorted.sort_by_key(|(k, _)| (*k).clone());
        for (name, value) in sorted {
            println!("abbr -a {name} '{value}'");
        }
        return;
    }

    let mut mode = "add";
    let mut positional = Vec::new();

    for arg in args {
        match *arg {
            "-a" | "--add" => mode = "add",
            "-e" | "--erase" => mode = "erase",
            "-l" | "--list" => mode = "list",
            _ if arg.starts_with('-') => {}
            _ => positional.push(*arg),
        }
    }

    match mode {
        "list" => {
            let mut sorted: Vec<_> = state.abbreviations.iter().collect();
            sorted.sort_by_key(|(k, _)| (*k).clone());
            for (name, value) in sorted {
                println!("abbr -a {name} '{value}'");
            }
        }
        "erase" => {
            for name in &positional {
                state.abbreviations.remove(*name);
            }
        }
        _ => {
            if positional.len() >= 2 {
                let name = positional[0].to_string();
                let value = positional[1..]
                    .join(" ")
                    .trim_matches('\'')
                    .trim_matches('"')
                    .to_string();
                state.abbreviations.insert(name, value);
            } else if positional.len() == 1 {
                if let Some(value) = state.abbreviations.get(positional[0]) {
                    println!("abbr -a {} '{}'", positional[0], value);
                }
            }
        }
    }
}
