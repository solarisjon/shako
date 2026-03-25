use std::env;

/// A parsed token from shell input, after quote handling.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub value: String,
    /// Whether this token was quoted (suppresses glob expansion).
    pub quoted: bool,
}

/// Tokenize a shell input string, respecting single and double quotes,
/// backslash escapes, and tilde/env-var expansion.
///
/// - Single quotes: literal, no expansion, no escapes
/// - Double quotes: env var expansion, backslash escapes, no glob
/// - Unquoted: env var expansion, glob expansion, tilde expansion
pub fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    enum QuoteState {
        None,
        Single,
        Double,
    }

    let mut state = QuoteState::None;

    while i < chars.len() {
        let c = chars[i];

        match state {
            QuoteState::None => match c {
                '\'' => {
                    state = QuoteState::Single;
                    quoted = true;
                    i += 1;
                }
                '"' => {
                    state = QuoteState::Double;
                    quoted = true;
                    i += 1;
                }
                '`' => {
                    i += 1; // skip opening backtick
                    let start = i;
                    while i < chars.len() && chars[i] != '`' {
                        i += 1;
                    }
                    let cmd: String = chars[start..i].iter().collect();
                    if i < chars.len() {
                        i += 1; // skip closing backtick
                    }
                    current.push_str(&run_command_substitution(&cmd));
                }
                '$' if i + 1 < chars.len() && chars[i + 1] == '(' => {
                    i += 2; // skip '$('
                    let cmd = extract_balanced(&chars, &mut i, '(', ')');
                    current.push_str(&run_command_substitution(&cmd));
                }
                '\\' if i + 1 < chars.len() => {
                    current.push(chars[i + 1]);
                    i += 2;
                }
                ' ' | '\t' => {
                    if !current.is_empty() {
                        tokens.push(Token {
                            value: current.clone(),
                            quoted,
                        });
                        current.clear();
                        quoted = false;
                    }
                    i += 1;
                }
                _ => {
                    current.push(c);
                    i += 1;
                }
            },
            QuoteState::Single => {
                if c == '\'' {
                    state = QuoteState::None;
                } else {
                    current.push(c);
                }
                i += 1;
            }
            QuoteState::Double => match c {
                '"' => {
                    state = QuoteState::None;
                    i += 1;
                }
                '`' => {
                    i += 1;
                    let start = i;
                    while i < chars.len() && chars[i] != '`' {
                        i += 1;
                    }
                    let cmd: String = chars[start..i].iter().collect();
                    if i < chars.len() {
                        i += 1;
                    }
                    current.push_str(&run_command_substitution(&cmd));
                }
                '$' if i + 1 < chars.len() && chars[i + 1] == '(' => {
                    i += 2; // skip '$('
                    let cmd = extract_balanced(&chars, &mut i, '(', ')');
                    current.push_str(&run_command_substitution(&cmd));
                }
                '\\' if i + 1 < chars.len() => {
                    let next = chars[i + 1];
                    match next {
                        '"' | '\\' | '$' | '`' => {
                            current.push(next);
                            i += 2;
                        }
                        _ => {
                            current.push('\\');
                            current.push(next);
                            i += 2;
                        }
                    }
                }
                '$' => {
                    let expanded = expand_env_at(&chars, &mut i);
                    current.push_str(&expanded);
                }
                _ => {
                    current.push(c);
                    i += 1;
                }
            },
        }
    }

    if !current.is_empty() {
        tokens.push(Token {
            value: current,
            quoted,
        });
    }

    tokens
}

/// Expand environment variables and globs in a list of tokens.
pub fn expand(tokens: Vec<Token>) -> Vec<String> {
    let mut result = Vec::new();

    for token in tokens {
        let expanded = expand_tilde(&token.value);
        let expanded = expand_env_vars(&expanded);

        if !token.quoted && contains_glob_chars(&expanded) {
            match expand_glob(&expanded) {
                Some(paths) => result.extend(paths),
                None => result.push(expanded),
            }
        } else {
            result.push(expanded);
        }
    }

    result
}

/// Tokenize and expand a shell input string into ready-to-execute args.
pub fn parse_args(input: &str) -> Vec<String> {
    let tokens = tokenize(input);
    expand(tokens)
}

/// Expand `~` at the start of a token to `$HOME`.
fn expand_tilde(input: &str) -> String {
    if input == "~" {
        return env::var("HOME").unwrap_or_else(|_| "~".to_string());
    }
    if let Some(rest) = input.strip_prefix("~/") {
        let home = env::var("HOME").unwrap_or_else(|_| "~".to_string());
        return format!("{home}/{rest}");
    }
    input.to_string()
}

/// Expand `$VAR` and `${VAR}` in a string.
fn expand_env_vars(input: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '$' {
            let expanded = expand_env_at(&chars, &mut i);
            result.push_str(&expanded);
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

/// Parse and expand an env var starting at `$` in the char array.
/// Advances `i` past the variable reference.
fn expand_env_at(chars: &[char], i: &mut usize) -> String {
    *i += 1; // skip '$'

    if *i >= chars.len() {
        return "$".to_string();
    }

    // ${VAR} form — may include operators like ${VAR:-default}, ${#VAR}, etc.
    if chars[*i] == '{' {
        *i += 1;
        return expand_brace_param(chars, i);
    }

    // $(...) — command substitution
    if chars[*i] == '(' {
        *i += 1; // skip '('
        let cmd = extract_balanced(chars, i, '(', ')');
        return run_command_substitution(&cmd);
    }

    // $? — last exit code
    if chars[*i] == '?' {
        *i += 1;
        return crate::shell::prompt::last_status().to_string();
    }

    // $VAR form
    let start = *i;
    while *i < chars.len() && (chars[*i].is_alphanumeric() || chars[*i] == '_') {
        *i += 1;
    }

    if *i == start {
        return "$".to_string();
    }

    let name: String = chars[start..*i].iter().collect();
    env::var(&name).unwrap_or_default()
}

fn contains_glob_chars(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[')
}

/// Expand `${...}` parameter expressions. Called with `i` pointing just past
/// the opening `{`; advances `i` past the closing `}`.
///
/// Supported operators:
///   `${#VAR}`          — string length
///   `${VAR:-word}`     — value or default
///   `${VAR:+word}`     — alt if set and non-empty
///   `${VAR:?word}`     — error if unset
///   `${VAR:=word}`     — assign default if unset
///   `${VAR##pat}`      — remove longest matching prefix
///   `${VAR#pat}`       — remove shortest matching prefix
///   `${VAR%%pat}`      — remove longest matching suffix
///   `${VAR%pat}`       — remove shortest matching suffix
///   `${VAR//old/new}`  — replace all occurrences
///   `${VAR/old/new}`   — replace first occurrence
fn expand_brace_param(chars: &[char], i: &mut usize) -> String {
    let start = *i;
    // Collect everything up to the matching '}'
    let mut depth = 1usize;
    while *i < chars.len() {
        if chars[*i] == '{' { depth += 1; }
        if chars[*i] == '}' {
            depth -= 1;
            if depth == 0 { break; }
        }
        *i += 1;
    }
    let inner: String = chars[start..*i].iter().collect();
    if *i < chars.len() { *i += 1; } // skip '}'

    // ${#VAR} — string length
    if let Some(varname) = inner.strip_prefix('#') {
        return env::var(varname).unwrap_or_default().len().to_string();
    }

    // Detect operator: :-, :+, :?, :=, ##, #, %%, %, //, /
    // We scan for the first operator character that is not part of the var name.
    let var_chars: Vec<char> = inner.chars().collect();
    let mut vi = 0;
    while vi < var_chars.len() && (var_chars[vi].is_alphanumeric() || var_chars[vi] == '_') {
        vi += 1;
    }

    let varname = &inner[..vi];
    let rest = &inner[vi..];

    if rest.is_empty() {
        return env::var(varname).unwrap_or_default();
    }

    let value = env::var(varname).unwrap_or_default();

    if let Some(word) = rest.strip_prefix(":-") {
        return if value.is_empty() { word.to_string() } else { value };
    }
    if let Some(word) = rest.strip_prefix(":+") {
        return if value.is_empty() { String::new() } else { word.to_string() };
    }
    if let Some(word) = rest.strip_prefix(":?") {
        if value.is_empty() {
            eprintln!("shako: {varname}: {}", if word.is_empty() { "parameter null or not set" } else { word });
            return String::new();
        }
        return value;
    }
    if let Some(word) = rest.strip_prefix(":=") {
        if value.is_empty() {
            unsafe { env::set_var(varname, word) };
            return word.to_string();
        }
        return value;
    }
    if let Some(pat) = rest.strip_prefix("##") {
        return glob_strip_prefix_longest(&value, pat);
    }
    if let Some(pat) = rest.strip_prefix('#') {
        return glob_strip_prefix_shortest(&value, pat);
    }
    if let Some(pat) = rest.strip_prefix("%%") {
        return glob_strip_suffix_longest(&value, pat);
    }
    if let Some(pat) = rest.strip_prefix('%') {
        return glob_strip_suffix_shortest(&value, pat);
    }
    if let Some(replacement_expr) = rest.strip_prefix("//") {
        if let Some(slash) = replacement_expr.find('/') {
            let pat = &replacement_expr[..slash];
            let rep = &replacement_expr[slash + 1..];
            return value.replace(pat, rep);
        }
        return value.replace(replacement_expr, "");
    }
    if let Some(replacement_expr) = rest.strip_prefix('/') {
        if let Some(slash) = replacement_expr.find('/') {
            let pat = &replacement_expr[..slash];
            let rep = &replacement_expr[slash + 1..];
            if let Some(pos) = value.find(pat) {
                return format!("{}{}{}", &value[..pos], rep, &value[pos + pat.len()..]);
            }
            return value;
        }
        if let Some(pos) = value.find(replacement_expr) {
            return value[..pos].to_string() + &value[pos + replacement_expr.len()..];
        }
        return value;
    }

    // Fallback: treat the whole inner as a variable name
    env::var(&inner).unwrap_or_default()
}

fn glob_strip_prefix_shortest(s: &str, pat: &str) -> String {
    for end in 0..=s.len() {
        if s.is_char_boundary(end) && fnmatch(pat, &s[..end]) {
            return s[end..].to_string();
        }
    }
    s.to_string()
}

fn glob_strip_prefix_longest(s: &str, pat: &str) -> String {
    for end in (0..=s.len()).rev() {
        if s.is_char_boundary(end) && fnmatch(pat, &s[..end]) {
            return s[end..].to_string();
        }
    }
    s.to_string()
}

fn glob_strip_suffix_shortest(s: &str, pat: &str) -> String {
    for start in (0..=s.len()).rev() {
        if s.is_char_boundary(start) && fnmatch(pat, &s[start..]) {
            return s[..start].to_string();
        }
    }
    s.to_string()
}

fn glob_strip_suffix_longest(s: &str, pat: &str) -> String {
    for start in 0..=s.len() {
        if s.is_char_boundary(start) && fnmatch(pat, &s[start..]) {
            return s[..start].to_string();
        }
    }
    s.to_string()
}

/// Simple glob match supporting `*` (any sequence) and `?` (any one char).
fn fnmatch(pat: &str, s: &str) -> bool {
    let pat: Vec<char> = pat.chars().collect();
    let s: Vec<char> = s.chars().collect();
    fn m(pat: &[char], s: &[char]) -> bool {
        match (pat, s) {
            ([], []) => true,
            (['*', rest_p @ ..], _) => {
                // * matches 0 or more chars
                for i in 0..=s.len() { if m(rest_p, &s[i..]) { return true; } }
                false
            }
            (['?', rest_p @ ..], [_, rest_s @ ..]) => m(rest_p, rest_s),
            ([p, rest_p @ ..], [c, rest_s @ ..]) if p == c => m(rest_p, rest_s),
            _ => false,
        }
    }
    m(&pat, &s)
}



/// Expand a glob pattern into matching file paths.
fn expand_glob(pattern: &str) -> Option<Vec<String>> {
    let matches: Vec<String> = glob::glob(pattern)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|path| path.display().to_string())
        .collect();

    if matches.is_empty() {
        None // no matches — return pattern as-is
    } else {
        let mut sorted = matches;
        sorted.sort();
        Some(sorted)
    }
}

/// Extract content between balanced delimiters, handling nesting.
/// `i` starts after the opening delimiter and is advanced past the closing one.
fn extract_balanced(chars: &[char], i: &mut usize, open: char, close: char) -> String {
    let mut depth = 1;
    let start = *i;

    while *i < chars.len() && depth > 0 {
        if chars[*i] == open {
            depth += 1;
        } else if chars[*i] == close {
            depth -= 1;
            if depth == 0 {
                break;
            }
        }
        *i += 1;
    }

    let content: String = chars[start..*i].iter().collect();
    if *i < chars.len() {
        *i += 1; // skip closing delimiter
    }
    content
}

/// Run a command and capture its stdout, trimming the trailing newline.
fn run_command_substitution(cmd: &str) -> String {
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return String::new();
    }

    match std::process::Command::new("sh").args(["-c", cmd]).output() {
        Ok(output) => {
            let mut result = String::from_utf8_lossy(&output.stdout).to_string();
            // Shells strip trailing newlines from command substitution
            while result.ends_with('\n') || result.ends_with('\r') {
                result.pop();
            }
            result
        }
        Err(e) => {
            eprintln!("shako: command substitution: {e}");
            String::new()
        }
    }
}

/// Split input on command chain operators (`;`, `&&`, `||`), respecting quotes.
/// Returns a list of (command, operator_after) pairs.
pub fn split_chains(input: &str) -> Vec<(String, ChainOp)> {
    let mut chains = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;

    while i < chars.len() {
        let c = chars[i];

        // Track quote state
        if c == '\'' && !in_double {
            in_single = !in_single;
            current.push(c);
            i += 1;
            continue;
        }
        if c == '"' && !in_single {
            in_double = !in_double;
            current.push(c);
            i += 1;
            continue;
        }
        if in_single || in_double {
            current.push(c);
            i += 1;
            continue;
        }

        // Backslash escape
        if c == '\\' && i + 1 < chars.len() {
            current.push(c);
            current.push(chars[i + 1]);
            i += 2;
            continue;
        }

        // Check for operators
        if c == '&' && i + 1 < chars.len() && chars[i + 1] == '&' {
            let cmd = current.trim().to_string();
            if !cmd.is_empty() {
                chains.push((cmd, ChainOp::And));
            }
            current.clear();
            i += 2;
            continue;
        }
        if c == '|' && i + 1 < chars.len() && chars[i + 1] == '|' {
            let cmd = current.trim().to_string();
            if !cmd.is_empty() {
                chains.push((cmd, ChainOp::Or));
            }
            current.clear();
            i += 2;
            continue;
        }
        if c == ';' {
            let cmd = current.trim().to_string();
            if !cmd.is_empty() {
                chains.push((cmd, ChainOp::Semi));
            }
            current.clear();
            i += 1;
            continue;
        }

        current.push(c);
        i += 1;
    }

    let cmd = current.trim().to_string();
    if !cmd.is_empty() {
        chains.push((cmd, ChainOp::None));
    }

    chains
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChainOp {
    /// `&&` — run next only if this succeeds
    And,
    /// `||` — run next only if this fails
    Or,
    /// `;` — run next regardless
    Semi,
    /// Last command in the chain
    None,
}

/// Split input on pipe `|` operators, respecting quotes.
/// Does NOT split on `||`.
pub fn split_pipes(input: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c == '\'' && !in_double {
            in_single = !in_single;
            current.push(c);
            i += 1;
            continue;
        }
        if c == '"' && !in_single {
            in_double = !in_double;
            current.push(c);
            i += 1;
            continue;
        }
        if !in_single && !in_double && c == '|' {
            // Check it's not ||
            if i + 1 < chars.len() && chars[i + 1] == '|' {
                current.push('|');
                current.push('|');
                i += 2;
                continue;
            }
            segments.push(current.trim().to_string());
            current.clear();
            i += 1;
            continue;
        }
        current.push(c);
        i += 1;
    }

    let last = current.trim().to_string();
    if !last.is_empty() {
        segments.push(last);
    }

    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_tokenize() {
        let tokens = tokenize("ls -la");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].value, "ls");
        assert_eq!(tokens[1].value, "-la");
    }

    #[test]
    fn test_double_quotes() {
        let tokens = tokenize(r#"echo "hello world""#);
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].value, "echo");
        assert_eq!(tokens[1].value, "hello world");
        assert!(tokens[1].quoted);
    }

    #[test]
    fn test_single_quotes() {
        let tokens = tokenize("echo 'hello world'");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[1].value, "hello world");
        assert!(tokens[1].quoted);
    }

    #[test]
    fn test_backslash_escape() {
        let tokens = tokenize(r"echo hello\ world");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[1].value, "hello world");
    }

    #[test]
    fn test_env_var_expansion() {
        unsafe { env::set_var("SHAKO_TEST_VAR", "expanded") };
        let result = expand_env_vars("$SHAKO_TEST_VAR");
        assert_eq!(result, "expanded");
        unsafe { env::remove_var("SHAKO_TEST_VAR") };
    }

    #[test]
    fn test_env_var_braces() {
        unsafe { env::set_var("SHAKO_TEST_VAR2", "braced") };
        let result = expand_env_vars("${SHAKO_TEST_VAR2}");
        assert_eq!(result, "braced");
        unsafe { env::remove_var("SHAKO_TEST_VAR2") };
    }

    #[test]
    fn test_tilde_expansion() {
        let home = env::var("HOME").unwrap();
        assert_eq!(expand_tilde("~"), home);
        assert_eq!(expand_tilde("~/foo"), format!("{home}/foo"));
        assert_eq!(expand_tilde("/absolute"), "/absolute");
    }

    #[test]
    fn test_split_chains() {
        let chains = split_chains("echo a && echo b || echo c; echo d");
        assert_eq!(chains.len(), 4);
        assert_eq!(chains[0], ("echo a".to_string(), ChainOp::And));
        assert_eq!(chains[1], ("echo b".to_string(), ChainOp::Or));
        assert_eq!(chains[2], ("echo c".to_string(), ChainOp::Semi));
        assert_eq!(chains[3], ("echo d".to_string(), ChainOp::None));
    }

    #[test]
    fn test_split_chains_respects_quotes() {
        let chains = split_chains(r#"echo "a && b"; echo c"#);
        assert_eq!(chains.len(), 2);
        assert_eq!(chains[0].0, r#"echo "a && b""#);
    }

    #[test]
    fn test_split_pipes() {
        let pipes = split_pipes("ls | grep foo | wc -l");
        assert_eq!(pipes, vec!["ls", "grep foo", "wc -l"]);
    }

    #[test]
    fn test_split_pipes_not_or() {
        let pipes = split_pipes("cmd1 || cmd2");
        assert_eq!(pipes.len(), 1);
        assert_eq!(pipes[0], "cmd1 || cmd2");
    }

    #[test]
    fn test_mixed_quotes_in_args() {
        let tokens = tokenize(r#"grep -r "hello" 'src/*.rs'"#);
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[2].value, "hello");
        assert!(tokens[2].quoted);
        assert_eq!(tokens[3].value, "src/*.rs");
        assert!(tokens[3].quoted); // glob suppressed
    }

    #[test]
    fn test_parse_args_full() {
        unsafe { env::set_var("SHAKO_PARSE_TEST", "works") };
        let args = parse_args("echo $SHAKO_PARSE_TEST");
        assert_eq!(args, vec!["echo", "works"]);
        unsafe { env::remove_var("SHAKO_PARSE_TEST") };
    }

    #[test]
    fn test_command_substitution_dollar() {
        let args = parse_args("echo $(echo hello)");
        assert_eq!(args, vec!["echo", "hello"]);
    }

    #[test]
    fn test_command_substitution_backtick() {
        let args = parse_args("echo `echo world`");
        assert_eq!(args, vec!["echo", "world"]);
    }

    #[test]
    fn test_command_substitution_nested() {
        let args = parse_args("echo $(echo $(echo nested))");
        assert_eq!(args, vec!["echo", "nested"]);
    }

    #[test]
    fn test_command_substitution_in_double_quotes() {
        let args = parse_args(r#"echo "$(echo "hello world")""#);
        assert_eq!(args, vec!["echo", "hello world"]);
    }

    #[test]
    fn test_command_substitution_not_in_single_quotes() {
        let tokens = tokenize("echo '$(echo hello)'");
        assert_eq!(tokens[1].value, "$(echo hello)");
    }
}
