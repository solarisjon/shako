use std::cell::RefCell;
use std::collections::HashMap;
use std::env;

/// Snapshot of shell session context used to enrich command substitution.
///
/// Populated by the main loop before each command dispatch via
/// [`set_subst_context`] so that `$( )` expansions respect the current
/// session's aliases and function definitions.
#[derive(Clone, Default)]
pub struct SubstContext {
    /// Alias name → expanded value (e.g. `"ll" → "ls -la"`).
    pub aliases: HashMap<String, String>,
    /// Function name → body text (verbatim source passed to `function … end`).
    pub functions: HashMap<String, String>,
}

thread_local! {
    /// Thread-local substitution context set by the interactive loop.
    static SUBST_CTX: RefCell<Option<SubstContext>> = const { RefCell::new(None) };
}

/// Install a substitution context for the current thread.
///
/// Call this once per command dispatch (before `parse_args` / `execute_command`)
/// so that `$(…)` expansions run under shako with the current session's aliases
/// and function definitions rather than under plain `/bin/sh`.
pub fn set_subst_context(ctx: SubstContext) {
    SUBST_CTX.with(|c| *c.borrow_mut() = Some(ctx));
}

/// Clear the substitution context (e.g. at the end of a dispatch cycle).
#[allow(dead_code)]
pub fn clear_subst_context() {
    SUBST_CTX.with(|c| *c.borrow_mut() = None);
}

/// Build the preamble script (alias + function definitions) from the current
/// substitution context.  Returns an empty string when no context is set.
fn subst_preamble() -> String {
    SUBST_CTX.with(|c| {
        let borrow = c.borrow();
        let ctx = match borrow.as_ref() {
            Some(ctx) => ctx,
            None => return String::new(),
        };

        let mut out = String::new();

        // Re-define aliases so the substituted command sees them.
        for (name, value) in &ctx.aliases {
            // Escape single quotes inside the alias value.
            let escaped = value.replace('\'', "'\\''");
            out.push_str(&format!("alias {name}='{escaped}'\n"));
        }

        // Re-define functions so the substituted command sees them.
        for (name, body) in &ctx.functions {
            out.push_str(&format!("function {name}\n{body}\nend\n"));
        }

        out
    })
}

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
                '$' if i + 2 < chars.len() && chars[i + 1] == '(' && chars[i + 2] == '(' => {
                    i += 3; // skip '$(('
                    let expr = extract_arithmetic_expr(&chars, &mut i);
                    match eval_arithmetic(&expr) {
                        Ok(v) => current.push_str(&v.to_string()),
                        Err(e) => {
                            eprintln!("shako: arithmetic: {e}");
                            current.push('0');
                        }
                    }
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
                '$' if i + 2 < chars.len() && chars[i + 1] == '(' && chars[i + 2] == '(' => {
                    i += 3; // skip '$(('
                    let expr = extract_arithmetic_expr(&chars, &mut i);
                    match eval_arithmetic(&expr) {
                        Ok(v) => current.push_str(&v.to_string()),
                        Err(e) => {
                            eprintln!("shako: arithmetic: {e}");
                            current.push('0');
                        }
                    }
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

        if !token.quoted {
            for brace_word in expand_braces(&expanded) {
                if contains_glob_chars(&brace_word) {
                    match expand_glob(&brace_word) {
                        Some(paths) => result.extend(paths),
                        None => result.push(brace_word),
                    }
                } else {
                    result.push(brace_word);
                }
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

/// Expand brace expressions in a single unquoted token.
///
/// Handles list form `{a,b,c}` and range form `{1..5}` / `{a..e}`.
/// Returns a single-element vec if no valid brace expression is found.
fn expand_braces(token: &str) -> Vec<String> {
    // Find the first '{'
    let open_pos = match token.find('{') {
        Some(p) => p,
        None => return vec![token.to_string()],
    };

    // Find the matching '}'
    let bytes = token.as_bytes();
    let mut depth = 0usize;
    let mut close_pos = None;
    for (i, &byte) in bytes.iter().enumerate().skip(open_pos) {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    close_pos = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }

    let close_pos = match close_pos {
        Some(p) => p,
        None => return vec![token.to_string()],
    };

    let prefix = &token[..open_pos];
    let inner = &token[open_pos + 1..close_pos];
    let suffix = &token[close_pos + 1..];

    // Empty brace expression is literal
    if inner.is_empty() {
        return vec![token.to_string()];
    }

    // Try range form first, then list form
    let alternatives: Vec<String> = if let Some(alts) = try_brace_range(inner) {
        alts
    } else {
        brace_list_split(inner)
    };

    // Single element means no expansion — return as literal
    if alternatives.len() <= 1 {
        return vec![token.to_string()];
    }

    alternatives
        .iter()
        .map(|alt| format!("{prefix}{alt}{suffix}"))
        .collect()
}

/// Try to parse inner brace content as a range (`start..end`).
/// Returns `None` if not a valid range expression.
fn try_brace_range(inner: &str) -> Option<Vec<String>> {
    let sep = inner.find("..")?;
    let start_str = &inner[..sep];
    let end_str = &inner[sep + 2..];

    if start_str.is_empty() || end_str.is_empty() {
        return None;
    }

    // Numeric range
    if let (Ok(start), Ok(end)) = (start_str.parse::<i64>(), end_str.parse::<i64>()) {
        let zero_pad = (start_str.len() > 1 && start_str.starts_with('0'))
            || (end_str.len() > 1 && end_str.starts_with('0'));
        let pad_width = if zero_pad {
            start_str.len().max(end_str.len())
        } else {
            0
        };

        let nums: Vec<i64> = if start <= end {
            (start..=end).collect()
        } else {
            (end..=start).rev().collect()
        };

        return Some(
            nums.iter()
                .map(|&n| {
                    if pad_width > 0 {
                        format!("{n:0>pad_width$}")
                    } else {
                        n.to_string()
                    }
                })
                .collect(),
        );
    }

    // Character range (single chars only)
    let mut sc = start_str.chars();
    let mut ec = end_str.chars();
    if let (Some(s), None, Some(e), None) = (sc.next(), sc.next(), ec.next(), ec.next()) {
        let (su, eu) = (s as u32, e as u32);
        let chars: Vec<String> = if su <= eu {
            (su..=eu)
                .filter_map(char::from_u32)
                .map(|c| c.to_string())
                .collect()
        } else {
            let mut r: Vec<String> = (eu..=su)
                .filter_map(char::from_u32)
                .map(|c| c.to_string())
                .collect();
            r.reverse();
            r
        };
        return Some(chars);
    }

    None
}

/// Split brace list content by commas, respecting nested brace depth.
fn brace_list_split(inner: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;

    for c in inner.chars() {
        match c {
            '{' => {
                depth += 1;
                current.push(c);
            }
            '}' if depth > 0 => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => {
                // `std::mem::take` moves `current` into `parts` and replaces it
                // with an empty String, avoiding the clone + clear pair.
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(c),
        }
    }
    parts.push(current);
    parts
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

    // $((expr)) — arithmetic expansion
    if chars[*i] == '(' && *i + 1 < chars.len() && chars[*i + 1] == '(' {
        *i += 2; // skip '(('
        let expr = extract_arithmetic_expr(chars, i);
        match eval_arithmetic(&expr) {
            Ok(v) => return v.to_string(),
            Err(e) => {
                eprintln!("shako: arithmetic: {e}");
                return "0".to_string();
            }
        }
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
        if chars[*i] == '{' {
            depth += 1;
        }
        if chars[*i] == '}' {
            depth -= 1;
            if depth == 0 {
                break;
            }
        }
        *i += 1;
    }
    let inner: String = chars[start..*i].iter().collect();
    if *i < chars.len() {
        *i += 1;
    } // skip '}'

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
        return if value.is_empty() {
            word.to_string()
        } else {
            value
        };
    }
    if let Some(word) = rest.strip_prefix(":+") {
        return if value.is_empty() {
            String::new()
        } else {
            word.to_string()
        };
    }
    if let Some(word) = rest.strip_prefix(":?") {
        if value.is_empty() {
            eprintln!(
                "shako: {varname}: {}",
                if word.is_empty() {
                    "parameter null or not set"
                } else {
                    word
                }
            );
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
                for i in 0..=s.len() {
                    if m(rest_p, &s[i..]) {
                        return true;
                    }
                }
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

/// Extract an arithmetic expression up to the closing `))` sequence.
/// `i` starts after the `$((` prefix; advances past the `))` on return.
fn extract_arithmetic_expr(chars: &[char], i: &mut usize) -> String {
    let start = *i;
    while *i + 1 < chars.len() {
        if chars[*i] == ')' && chars[*i + 1] == ')' {
            let content: String = chars[start..*i].iter().collect();
            *i += 2; // consume '))'
            return content;
        }
        *i += 1;
    }
    // unclosed — consume the rest
    let content: String = chars[start..].iter().collect();
    *i = chars.len();
    content
}

/// Evaluate a POSIX arithmetic expression string like `"2 + 3 * 4"` or `"$x ** 2"`.
/// Variable references (`$VAR` or bare `VAR`) are expanded via the environment.
/// Returns `Err` on division by zero, integer overflow, or unparseable numbers.
fn eval_arithmetic(expr: &str) -> Result<i64, String> {
    let expr = expand_arith_vars(expr.trim());
    let chars: Vec<char> = expr.chars().collect();
    let mut pos = 0;
    arith_parse_expr(&chars, &mut pos)
}

/// Replace `$VAR` references inside an arithmetic expression with their values.
fn expand_arith_vars(expr: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' {
            i += 1;
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect();
            let val = std::env::var(&name).unwrap_or_else(|_| "0".to_string());
            result.push_str(&val);
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

fn arith_skip_ws(chars: &[char], pos: &mut usize) {
    while *pos < chars.len() && chars[*pos] == ' ' {
        *pos += 1;
    }
}

fn arith_parse_expr(chars: &[char], pos: &mut usize) -> Result<i64, String> {
    arith_parse_or(chars, pos)
}

fn arith_parse_or(chars: &[char], pos: &mut usize) -> Result<i64, String> {
    let mut left = arith_parse_and(chars, pos)?;
    loop {
        arith_skip_ws(chars, pos);
        if *pos + 1 < chars.len() && chars[*pos] == '|' && chars[*pos + 1] == '|' {
            *pos += 2;
            let right = arith_parse_and(chars, pos)?;
            left = if left != 0 || right != 0 { 1 } else { 0 };
        } else {
            break;
        }
    }
    Ok(left)
}

fn arith_parse_and(chars: &[char], pos: &mut usize) -> Result<i64, String> {
    let mut left = arith_parse_cmp(chars, pos)?;
    loop {
        arith_skip_ws(chars, pos);
        if *pos + 1 < chars.len() && chars[*pos] == '&' && chars[*pos + 1] == '&' {
            *pos += 2;
            let right = arith_parse_cmp(chars, pos)?;
            left = if left != 0 && right != 0 { 1 } else { 0 };
        } else {
            break;
        }
    }
    Ok(left)
}

fn arith_parse_cmp(chars: &[char], pos: &mut usize) -> Result<i64, String> {
    let left = arith_parse_add(chars, pos)?;
    arith_skip_ws(chars, pos);
    if *pos >= chars.len() {
        return Ok(left);
    }
    let c = chars[*pos];
    let n = if *pos + 1 < chars.len() {
        chars[*pos + 1]
    } else {
        '\0'
    };
    if c == '=' && n == '=' {
        *pos += 2;
        let right = arith_parse_add(chars, pos)?;
        Ok(if left == right { 1 } else { 0 })
    } else if c == '!' && n == '=' {
        *pos += 2;
        let right = arith_parse_add(chars, pos)?;
        Ok(if left != right { 1 } else { 0 })
    } else if c == '<' && n == '=' {
        *pos += 2;
        let right = arith_parse_add(chars, pos)?;
        Ok(if left <= right { 1 } else { 0 })
    } else if c == '>' && n == '=' {
        *pos += 2;
        let right = arith_parse_add(chars, pos)?;
        Ok(if left >= right { 1 } else { 0 })
    } else if c == '<' && n != '<' {
        *pos += 1;
        let right = arith_parse_add(chars, pos)?;
        Ok(if left < right { 1 } else { 0 })
    } else if c == '>' && n != '>' {
        *pos += 1;
        let right = arith_parse_add(chars, pos)?;
        Ok(if left > right { 1 } else { 0 })
    } else {
        Ok(left)
    }
}

fn arith_parse_add(chars: &[char], pos: &mut usize) -> Result<i64, String> {
    let mut left = arith_parse_mul(chars, pos)?;
    loop {
        arith_skip_ws(chars, pos);
        if *pos < chars.len() && chars[*pos] == '+' {
            *pos += 1;
            let r = arith_parse_mul(chars, pos)?;
            left = left
                .checked_add(r)
                .ok_or_else(|| "arithmetic overflow".to_string())?;
        } else if *pos < chars.len() && chars[*pos] == '-' {
            *pos += 1;
            let r = arith_parse_mul(chars, pos)?;
            left = left
                .checked_sub(r)
                .ok_or_else(|| "arithmetic overflow".to_string())?;
        } else {
            break;
        }
    }
    Ok(left)
}

fn arith_parse_mul(chars: &[char], pos: &mut usize) -> Result<i64, String> {
    let mut left = arith_parse_pow(chars, pos)?;
    loop {
        arith_skip_ws(chars, pos);
        if *pos < chars.len()
            && chars[*pos] == '*'
            && (*pos + 1 >= chars.len() || chars[*pos + 1] != '*')
        {
            *pos += 1;
            let r = arith_parse_pow(chars, pos)?;
            left = left
                .checked_mul(r)
                .ok_or_else(|| "arithmetic overflow".to_string())?;
        } else if *pos < chars.len() && chars[*pos] == '/' {
            *pos += 1;
            let r = arith_parse_pow(chars, pos)?;
            if r == 0 {
                return Err("division by zero".to_string());
            }
            left = left
                .checked_div(r)
                .ok_or_else(|| "arithmetic overflow".to_string())?;
        } else if *pos < chars.len() && chars[*pos] == '%' {
            *pos += 1;
            let r = arith_parse_pow(chars, pos)?;
            if r == 0 {
                return Err("division by zero".to_string());
            }
            left = left
                .checked_rem(r)
                .ok_or_else(|| "arithmetic overflow".to_string())?;
        } else {
            break;
        }
    }
    Ok(left)
}

fn arith_parse_pow(chars: &[char], pos: &mut usize) -> Result<i64, String> {
    let base = arith_parse_unary(chars, pos)?;
    arith_skip_ws(chars, pos);
    if *pos + 1 < chars.len() && chars[*pos] == '*' && chars[*pos + 1] == '*' {
        *pos += 2;
        let exp = arith_parse_unary(chars, pos)?;
        if exp < 0 {
            return Ok(0);
        }
        base.checked_pow(exp as u32)
            .ok_or_else(|| "arithmetic overflow".to_string())
    } else {
        Ok(base)
    }
}

fn arith_parse_unary(chars: &[char], pos: &mut usize) -> Result<i64, String> {
    arith_skip_ws(chars, pos);
    if *pos < chars.len() && chars[*pos] == '-' {
        *pos += 1;
        let v = arith_parse_unary(chars, pos)?;
        v.checked_neg()
            .ok_or_else(|| "arithmetic overflow".to_string())
    } else if *pos < chars.len() && chars[*pos] == '+' {
        *pos += 1;
        arith_parse_unary(chars, pos)
    } else if *pos < chars.len() && chars[*pos] == '!' {
        *pos += 1;
        let v = arith_parse_unary(chars, pos)?;
        Ok(if v == 0 { 1 } else { 0 })
    } else {
        arith_parse_atom(chars, pos)
    }
}

fn arith_parse_atom(chars: &[char], pos: &mut usize) -> Result<i64, String> {
    arith_skip_ws(chars, pos);
    if *pos >= chars.len() {
        return Ok(0);
    }
    if chars[*pos] == '(' {
        *pos += 1;
        let val = arith_parse_expr(chars, pos)?;
        arith_skip_ws(chars, pos);
        if *pos < chars.len() && chars[*pos] == ')' {
            *pos += 1;
        }
        return Ok(val);
    }
    let start = *pos;
    if chars[*pos].is_ascii_digit() {
        while *pos < chars.len() && chars[*pos].is_ascii_digit() {
            *pos += 1;
        }
        let s: String = chars[start..*pos].iter().collect();
        return s
            .parse::<i64>()
            .map_err(|_| format!("number too large: {s}"));
    }
    if chars[*pos].is_alphabetic() || chars[*pos] == '_' {
        while *pos < chars.len() && (chars[*pos].is_alphanumeric() || chars[*pos] == '_') {
            *pos += 1;
        }
        let name: String = chars[start..*pos].iter().collect();
        let val_str = std::env::var(&name).unwrap_or_else(|_| "0".to_string());
        return val_str
            .trim()
            .parse::<i64>()
            .map_err(|_| format!("${name}: not an integer: {val_str:?}"));
    }
    Ok(0)
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

/// Run a command and capture its stdout, trimming the trailing newlines.
///
/// Uses the current shako binary (via `std::env::current_exe`) when possible
/// so that session aliases and functions (injected via [`set_subst_context`])
/// are visible inside `$(…)`.  Falls back to `sh -c` only when the shako
/// binary cannot be located.
fn run_command_substitution(cmd: &str) -> String {
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return String::new();
    }

    let preamble = subst_preamble();
    let full_cmd = if preamble.is_empty() {
        cmd.to_string()
    } else {
        format!("{preamble}{cmd}")
    };

    // Prefer re-entering shako so session aliases / functions are visible.
    // We look for the shako binary on PATH first; if not found we fall back to
    // `current_exe()` (which works in installed/release builds but not in test
    // harnesses where the binary is the test runner, not a real shako shell).
    let shako_exe: Option<std::path::PathBuf> = which::which("shako").ok().or_else(|| {
        std::env::current_exe().ok().and_then(|p| {
            // Only use current_exe when the binary looks like a shako binary.
            // Test runner binaries live under target/…/deps/ and have names like
            // `shako-<hash>` — avoid re-entering those.
            let name = p.file_name()?.to_string_lossy().to_string();
            if name == "shako" {
                Some(p)
            } else {
                None
            }
        })
    });

    let output = if let Some(ref exe) = shako_exe {
        std::process::Command::new(exe)
            .args(["-c", &full_cmd])
            .output()
            .ok()
    } else {
        None
    };

    // Fall back to /bin/sh when shako is not on PATH (tests, etc.).
    let output = output.or_else(|| {
        std::process::Command::new("sh")
            .args(["-c", cmd])
            .output()
            .ok()
    });

    match output {
        Some(output) => {
            let mut result = String::from_utf8_lossy(&output.stdout).to_string();
            // Shells strip trailing newlines from command substitution.
            while result.ends_with('\n') || result.ends_with('\r') {
                result.pop();
            }
            result
        }
        None => {
            eprintln!("shako: command substitution: failed to run command");
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

    #[test]
    fn test_arithmetic_basic() {
        let args = parse_args("echo $((2 + 3))");
        assert_eq!(args, vec!["echo", "5"]);
    }

    #[test]
    fn test_arithmetic_mul() {
        let args = parse_args("echo $((6 * 7))");
        assert_eq!(args, vec!["echo", "42"]);
    }

    #[test]
    fn test_arithmetic_precedence() {
        let args = parse_args("echo $((2 + 3 * 4))");
        assert_eq!(args, vec!["echo", "14"]);
    }

    #[test]
    fn test_arithmetic_parens() {
        let args = parse_args("echo $(((2 + 3) * 4))");
        assert_eq!(args, vec!["echo", "20"]);
    }

    #[test]
    fn test_arithmetic_power() {
        let args = parse_args("echo $((2 ** 10))");
        assert_eq!(args, vec!["echo", "1024"]);
    }

    #[test]
    fn test_arithmetic_div_mod() {
        let args = parse_args("echo $((17 / 5)) $((17 % 5))");
        assert_eq!(args, vec!["echo", "3", "2"]);
    }

    #[test]
    fn test_arithmetic_unary_minus() {
        let args = parse_args("echo $((-3 + 5))");
        assert_eq!(args, vec!["echo", "2"]);
    }

    #[test]
    fn test_arithmetic_var_expansion() {
        unsafe { env::set_var("SHAKO_ARITH_X", "7") };
        let args = parse_args("echo $(($SHAKO_ARITH_X * 6))");
        assert_eq!(args, vec!["echo", "42"]);
        unsafe { env::remove_var("SHAKO_ARITH_X") };
    }

    #[test]
    fn test_arithmetic_in_double_quotes() {
        let args = parse_args(r#"echo "result=$((3 + 4))""#);
        assert_eq!(args, vec!["echo", "result=7"]);
    }

    #[test]
    fn test_arithmetic_div_by_zero_errors() {
        // Division by zero must produce "0" (after printing an error) rather than
        // silently wrapping or panicking.
        let args = parse_args("echo $((1 / 0))");
        assert_eq!(args, vec!["echo", "0"]);
    }

    #[test]
    fn test_arithmetic_mod_by_zero_errors() {
        let args = parse_args("echo $((5 % 0))");
        assert_eq!(args, vec!["echo", "0"]);
    }

    #[test]
    fn test_eval_arithmetic_div_by_zero() {
        assert!(eval_arithmetic("1 / 0").is_err());
        assert!(eval_arithmetic("9 % 0").is_err());
    }

    #[test]
    fn test_eval_arithmetic_overflow() {
        assert!(eval_arithmetic("9223372036854775807 + 1").is_err());
        assert!(eval_arithmetic("-9223372036854775808 - 1").is_err());
    }

    // ── Brace expansion ────────────────────────────────────────────

    #[test]
    fn test_brace_expansion_list() {
        let result = expand_braces("{a,b,c}");
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_brace_expansion_prefix_suffix() {
        let result = expand_braces("foo{1,2,3}bar");
        assert_eq!(result, vec!["foo1bar", "foo2bar", "foo3bar"]);
    }

    #[test]
    fn test_brace_expansion_with_empty_element() {
        let result = expand_braces("file{,.bak}");
        assert_eq!(result, vec!["file", "file.bak"]);
    }

    #[test]
    fn test_brace_expansion_numeric_range() {
        let result = expand_braces("{1..5}");
        assert_eq!(result, vec!["1", "2", "3", "4", "5"]);
    }

    #[test]
    fn test_brace_expansion_char_range() {
        let result = expand_braces("{a..e}");
        assert_eq!(result, vec!["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn test_brace_expansion_range_reverse_numeric() {
        let result = expand_braces("{5..1}");
        assert_eq!(result, vec!["5", "4", "3", "2", "1"]);
    }

    #[test]
    fn test_brace_expansion_range_reverse_char() {
        let result = expand_braces("{e..a}");
        assert_eq!(result, vec!["e", "d", "c", "b", "a"]);
    }

    #[test]
    fn test_brace_expansion_zero_padded() {
        let result = expand_braces("{01..05}");
        assert_eq!(result, vec!["01", "02", "03", "04", "05"]);
    }

    #[test]
    fn test_brace_expansion_empty_literal() {
        let result = expand_braces("{}");
        assert_eq!(result, vec!["{}"]);
    }

    #[test]
    fn test_brace_expansion_single_literal() {
        let result = expand_braces("{foo}");
        assert_eq!(result, vec!["{foo}"]);
    }

    #[test]
    fn test_brace_expansion_via_parse_args() {
        let args = parse_args("echo {a,b,c}");
        assert_eq!(args, vec!["echo", "a", "b", "c"]);
    }

    #[test]
    fn test_brace_expansion_quoted_not_expanded() {
        let args = parse_args(r#"echo "{a,b,c}""#);
        assert_eq!(args, vec!["echo", "{a,b,c}"]);
    }

    // ── Edge cases: empty / whitespace-only input ───────────────

    #[test]
    fn test_tokenize_empty_input() {
        let tokens = tokenize("");
        assert!(tokens.is_empty(), "empty input should produce no tokens");
    }

    #[test]
    fn test_tokenize_whitespace_only() {
        let tokens = tokenize("   \t  ");
        assert!(
            tokens.is_empty(),
            "whitespace-only input should produce no tokens"
        );
    }

    #[test]
    fn test_parse_args_empty_input() {
        let args = parse_args("");
        assert!(args.is_empty(), "empty input should produce no args");
    }

    #[test]
    fn test_parse_args_whitespace_only() {
        let args = parse_args("   ");
        assert!(
            args.is_empty(),
            "whitespace-only input should produce no args"
        );
    }

    // ── Edge case: $() command substitution inside double quotes ─

    #[test]
    fn test_command_substitution_dollar_in_double_quotes() {
        // $() inside double quotes should still be expanded
        let args = parse_args(r#"echo "value=$(echo 42)""#);
        assert_eq!(args, vec!["echo", "value=42"]);
    }

    #[test]
    fn test_command_substitution_dollar_space_in_double_quotes() {
        // Result of $() with spaces inside double quotes stays as a single token
        let args = parse_args(r#"echo "$(echo hello world)""#);
        assert_eq!(args, vec!["echo", "hello world"]);
    }
}
