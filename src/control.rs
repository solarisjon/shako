// Control flow engine for shako.
//
// Handles `if/elif/else/end`, `for/end`, `while/end`, and the
// associated `break`, `continue`, and `local` builtins.  Input is a
// semicolon-separated string (the body of a function or a multi-statement
// REPL line joined into one string).
//
// Entry points
// ─────────────────────────────────────────────────────────────────────────
//   parse_body(s)                         → Vec<Statement>
//   exec_statements(&stmts, &mut locals)  → ExecSignal
//   is_control_flow(s) / has_control_flow(s) → bool

// ─── Public types ─────────────────────────────────────────────────────────

/// Propagation result for a statement list.
#[derive(Debug)]
pub enum ExecSignal {
    Normal(i32),
    Break,
    Continue,
    Return(i32),
}

/// A parsed shell statement.
#[derive(Debug, Clone)]
pub enum Statement {
    Simple(String),
    If {
        condition: String,
        then_body: Vec<Statement>,
        elif_branches: Vec<(String, Vec<Statement>)>,
        else_body: Vec<Statement>,
    },
    For {
        var: String,
        items_expr: String,
        body: Vec<Statement>,
    },
    While {
        condition: String,
        body: Vec<Statement>,
    },
    Break,
    Continue,
    /// `local VAR` or `local VAR=value`
    Local(String),
}

// ─── Internal keyword/token types ─────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Kw {
    If,
    Then,
    Elif,
    Else,
    Fi,
    For,
    Do,
    Done,
    End,
    While,
    Break,
    Continue,
    Local,
}

#[derive(Debug, Clone)]
enum BodyToken {
    Kw(Kw),
    Cmd(String),
}

// ─── Public helper ────────────────────────────────────────────────────────

/// Quick check: does the input begin a control-flow construct?
pub fn is_control_flow(input: &str) -> bool {
    matches!(
        input.split_whitespace().next().unwrap_or(""),
        "if" | "for" | "while"
    )
}

/// Returns true if ANY semicolon-separated segment of `input` starts with a
/// control-flow keyword.  Use this for routing in `-c` mode and the REPL so
/// that `export N=0; while ...` is handled correctly by the control engine.
pub fn has_control_flow(input: &str) -> bool {
    split_semicolons(input)
        .iter()
        .any(|seg| is_control_flow(seg.trim()))
}

// ─── Tokenization ─────────────────────────────────────────────────────────

/// Split on unquoted semicolons only (not `&&` or `||`).
fn split_semicolons(s: &str) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut prev_bs = false;

    for c in s.chars() {
        if prev_bs {
            prev_bs = false;
            current.push(c);
            continue;
        }
        match c {
            '\\' if !in_single => { prev_bs = true; current.push(c); }
            '\'' if !in_double => { in_single = !in_single; current.push(c); }
            '"' if !in_single => { in_double = !in_double; current.push(c); }
            ';' if !in_single && !in_double => {
                let t = current.trim().to_string();
                if !t.is_empty() { result.push(t); }
                current.clear();
            }
            _ => { current.push(c); }
        }
    }
    let tail = current.trim().to_string();
    if !tail.is_empty() {
        result.push(tail);
    }
    result
}

/// Check whether a string begins with a complete keyword word.
/// Returns `(Kw, rest_after_keyword)` or `None`.
fn leading_keyword(seg: &str) -> Option<(Kw, &str)> {
    // Order matters: try longer keywords first to avoid "el" prefix matching.
    static KWS: &[(&str, Kw)] = &[
        ("elif", Kw::Elif),
        ("else", Kw::Else),
        ("then", Kw::Then),
        ("done", Kw::Done),   // bash compat alias for end
        ("end", Kw::End),     // fish canonical block closer
        ("while", Kw::While),
        ("local", Kw::Local),
        ("continue", Kw::Continue),
        ("break", Kw::Break),
        ("fi", Kw::Fi),       // bash compat alias for end
        ("for", Kw::For),
        ("do", Kw::Do),       // bash compat, optional in fish
        ("if", Kw::If),
    ];
    for (kw_str, kw) in KWS {
        if seg == *kw_str {
            return Some((kw.clone(), ""));
        }
        if let Some(rest) = seg.strip_prefix(kw_str) {
            // Whole-word check: character after keyword must be whitespace
            if rest.starts_with(|c: char| c.is_whitespace()) {
                return Some((kw.clone(), rest.trim_start()));
            }
        }
    }
    None
}

/// Recursively emit tokens from a segment string, re-checking for keywords
/// in the remainder after each keyword is extracted.
fn emit_segment(seg: &str, tokens: &mut Vec<BodyToken>) {
    let seg = seg.trim();
    if seg.is_empty() { return; }
    if let Some((kw, rest)) = leading_keyword(seg) {
        tokens.push(BodyToken::Kw(kw));
        emit_segment(rest, tokens);
    } else {
        tokens.push(BodyToken::Cmd(seg.to_string()));
    }
}

/// Convert a body string into a flat token list.
fn tokenize_body(input: &str) -> Vec<BodyToken> {
    let mut tokens = Vec::new();
    for seg in split_semicolons(input) {
        emit_segment(&seg, &mut tokens);
    }
    tokens
}

// ─── Recursive-descent parser ─────────────────────────────────────────────

struct Parser {
    tokens: Vec<BodyToken>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<BodyToken>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek_kw(&self) -> Option<&Kw> {
        match self.tokens.get(self.pos) {
            Some(BodyToken::Kw(kw)) => Some(kw),
            _ => None,
        }
    }

    fn peek_is_stop(&self, stops: &[Kw]) -> bool {
        self.peek_kw().map(|k| stops.contains(k)).unwrap_or(false)
    }

    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn skip_kw(&mut self, expected: &Kw) {
        if self.peek_kw() == Some(expected) {
            self.pos += 1;
        }
    }

    fn take_cmd(&mut self) -> String {
        match self.tokens.get(self.pos) {
            Some(BodyToken::Cmd(c)) => { let s = c.clone(); self.pos += 1; s }
            _ => String::new(),
        }
    }

    fn take_kw(&mut self) -> Option<Kw> {
        match self.tokens.get(self.pos) {
            Some(BodyToken::Kw(k)) => { let k = k.clone(); self.pos += 1; Some(k) }
            _ => None,
        }
    }

    /// Skip the next token if it is any recognised block-closer:
    /// `end` (fish canonical; preferred) or `done` / `fi` (accepted for compat).
    fn skip_end_kw(&mut self) {
        if matches!(
            self.tokens.get(self.pos),
            Some(BodyToken::Kw(Kw::End | Kw::Done | Kw::Fi))
        ) {
            self.pos += 1;
        }
    }

    /// Parse all statements until EOF.
    fn parse_all(&mut self) -> Vec<Statement> {
        self.parse_until(&[])
    }

    /// Parse statements until one of `stops` is the next token (stop kw not consumed).
    fn parse_until(&mut self, stops: &[Kw]) -> Vec<Statement> {
        let mut stmts = Vec::new();
        while !self.at_end() && !self.peek_is_stop(stops) {
            if let Some(stmt) = self.parse_one() {
                stmts.push(stmt);
            }
        }
        stmts
    }

    /// Parse a single statement and return it, or `None` to skip.
    fn parse_one(&mut self) -> Option<Statement> {
        match self.tokens.get(self.pos)?.clone() {
            BodyToken::Kw(Kw::If) => {
                self.pos += 1;
                let condition = self.take_cmd();
                self.skip_kw(&Kw::Then); // optional in fish syntax
                let then_body = self.parse_until(&[Kw::Elif, Kw::Else, Kw::End, Kw::Fi]);

                let mut elif_branches = Vec::new();
                let mut else_body = Vec::new();

                loop {
                    match self.peek_kw() {
                        Some(Kw::Elif) => {
                            self.pos += 1;
                            let elif_cond = self.take_cmd();
                            self.skip_kw(&Kw::Then); // optional in fish syntax
                            let body = self.parse_until(&[Kw::Elif, Kw::Else, Kw::End, Kw::Fi]);
                            elif_branches.push((elif_cond, body));
                        }
                        Some(Kw::Else) => {
                            self.pos += 1;
                            else_body = self.parse_until(&[Kw::End, Kw::Fi]);
                            self.skip_end_kw(); // accept end / fi
                            break;
                        }
                        Some(Kw::End) | Some(Kw::Fi) => { self.skip_end_kw(); break; }
                        _ => break,
                    }
                }

                Some(Statement::If { condition, then_body, elif_branches, else_body })
            }

            BodyToken::Kw(Kw::For) => {
                self.pos += 1;
                let spec = self.take_cmd();
                let (var, items_expr) = parse_for_spec(&spec);
                self.skip_kw(&Kw::Do); // optional in fish syntax
                let body = self.parse_until(&[Kw::End, Kw::Done]);
                self.skip_end_kw(); // accept end / done
                Some(Statement::For { var, items_expr, body })
            }

            BodyToken::Kw(Kw::While) => {
                self.pos += 1;
                let condition = self.take_cmd();
                self.skip_kw(&Kw::Do); // optional in fish syntax
                let body = self.parse_until(&[Kw::End, Kw::Done]);
                self.skip_end_kw(); // accept end / done
                Some(Statement::While { condition, body })
            }

            BodyToken::Kw(Kw::Break) => { self.pos += 1; Some(Statement::Break) }
            BodyToken::Kw(Kw::Continue) => { self.pos += 1; Some(Statement::Continue) }
            BodyToken::Kw(Kw::Local) => {
                self.pos += 1;
                let spec = self.take_cmd();
                if spec.is_empty() { None } else { Some(Statement::Local(spec)) }
            }

            // Stray structural keywords at statement level — skip silently
            BodyToken::Kw(_) => { self.take_kw(); None }

            BodyToken::Cmd(cmd) => {
                self.pos += 1;
                // `local VAR=val` can arrive as Cmd when the semicolon-split
                // didn't separate `local` from its argument in the original body.
                if let Some(rest) = cmd.strip_prefix("local ") {
                    let rest = rest.trim().to_string();
                    if rest.is_empty() { None } else { Some(Statement::Local(rest)) }
                } else {
                    Some(Statement::Simple(cmd))
                }
            }
        }
    }
}

fn parse_for_spec(spec: &str) -> (String, String) {
    // "VAR in ITEMS..."
    let mut parts = spec.splitn(3, ' ');
    let var = parts.next().unwrap_or("").to_string();
    match parts.next() {
        Some("in") => {
            let items = parts.next().unwrap_or("").to_string();
            (var, items)
        }
        Some(items) => (var, items.to_string()),
        None => (var, String::new()),
    }
}

/// Parse a semicolon-separated body string into a statement list.
pub fn parse_body(input: &str) -> Vec<Statement> {
    let tokens = tokenize_body(input);
    let mut parser = Parser::new(tokens);
    parser.parse_all()
}

// ─── Execution ────────────────────────────────────────────────────────────

/// Execute a list of statements.
///
/// `locals` accumulates `(VAR_NAME, previous_value)` for `local` declarations.
/// The **caller** must restore them after the enclosing function returns.
pub fn exec_statements(
    stmts: &[Statement],
    locals: &mut Vec<(String, Option<String>)>,
) -> ExecSignal {
    let mut last = 0i32;
    for stmt in stmts {
        match exec_one(stmt, locals) {
            ExecSignal::Normal(c) => last = c,
            other => return other,
        }
    }
    ExecSignal::Normal(last)
}

fn exec_one(stmt: &Statement, locals: &mut Vec<(String, Option<String>)>) -> ExecSignal {
    match stmt {
        Statement::Simple(cmd) => exec_simple(cmd, locals),

        Statement::If { condition, then_body, elif_branches, else_body } => {
            if run_condition(condition) == 0 {
                exec_statements(then_body, locals)
            } else {
                for (cond, body) in elif_branches {
                    if run_condition(cond) == 0 {
                        return exec_statements(body, locals);
                    }
                }
                exec_statements(else_body, locals)
            }
        }

        Statement::For { var, items_expr, body } => {
            let items = crate::parser::parse_args(items_expr);
            let mut last = 0i32;
            'for_loop: for item in &items {
                declare_local(var, locals);
                unsafe { std::env::set_var(var, item) };
                match exec_statements(body, locals) {
                    ExecSignal::Normal(c) => last = c,
                    ExecSignal::Continue => continue 'for_loop,
                    ExecSignal::Break => break 'for_loop,
                    ret @ ExecSignal::Return(_) => return ret,
                }
            }
            ExecSignal::Normal(last)
        }

        Statement::While { condition, body } => {
            let mut last = 0i32;
            'while_loop: loop {
                if run_condition(condition) != 0 { break; }
                match exec_statements(body, locals) {
                    ExecSignal::Normal(c) => last = c,
                    ExecSignal::Continue => continue 'while_loop,
                    ExecSignal::Break => break 'while_loop,
                    ret @ ExecSignal::Return(_) => return ret,
                }
            }
            ExecSignal::Normal(last)
        }

        Statement::Break => ExecSignal::Break,
        Statement::Continue => ExecSignal::Continue,

        Statement::Local(spec) => {
            if let Some((var, val)) = spec.split_once('=') {
                let var = var.trim();
                declare_local(var, locals);
                let expanded = crate::parser::parse_args(val).join(" ");
                unsafe { std::env::set_var(var, &expanded) };
            } else {
                let var = spec.trim();
                if !var.is_empty() {
                    declare_local(var, locals);
                    unsafe { std::env::remove_var(var) };
                }
            }
            ExecSignal::Normal(0)
        }
    }
}

/// Save the current env value of `var` into `locals` (once per var).
fn declare_local(var: &str, locals: &mut Vec<(String, Option<String>)>) {
    if !locals.iter().any(|(v, _)| v == var) {
        locals.push((var.to_string(), std::env::var(var).ok()));
    }
}

/// Execute a simple command, checking the FUNCTION_RETURN thread-local after.
fn exec_simple(cmd: &str, _locals: &mut Vec<(String, Option<String>)>) -> ExecSignal {
    let cmd = cmd.trim();
    if cmd.is_empty() { return ExecSignal::Normal(0); }

    let first = cmd.split_whitespace().next().unwrap_or("");
    let code = if crate::builtins::is_builtin(first) {
        crate::builtins::run_builtin_stateless(cmd)
    } else {
        crate::executor::execute_command(cmd)
            .and_then(|s| s.code())
            .unwrap_or(0)
    };

    // `return` builtin signals via thread-local
    if let Some(ret) = crate::builtins::take_function_return() {
        return ExecSignal::Return(ret);
    }

    ExecSignal::Normal(code)
}

/// Run a condition and return its exit code.
fn run_condition(condition: &str) -> i32 {
    let condition = condition.trim();
    if condition.is_empty() { return 0; }

    let first = condition.split_whitespace().next().unwrap_or("");
    if crate::builtins::is_builtin(first) {
        crate::builtins::run_builtin_stateless(condition)
    } else {
        crate::executor::execute_command(condition)
            .and_then(|s| s.code())
            .unwrap_or(1)
    }
}

// ─── Unit tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_control_flow() {
        assert!(is_control_flow("if [ x = x ]; then echo yes; fi"));
        assert!(is_control_flow("for i in 1 2 3; do echo $i; done"));
        assert!(is_control_flow("while false; do echo hi; done"));
        assert!(!is_control_flow("echo hello"));
        assert!(!is_control_flow("ls -la"));
        assert!(!is_control_flow(""));
    }

    #[test]
    fn test_split_semicolons_basic() {
        assert_eq!(split_semicolons("a; b; c"), vec!["a", "b", "c"]);
    }

    #[test]
    fn test_split_semicolons_quoted() {
        assert_eq!(
            split_semicolons("echo 'a;b'; echo c"),
            vec!["echo 'a;b'", "echo c"]
        );
    }

    #[test]
    fn test_split_semicolons_empty_segments() {
        assert_eq!(split_semicolons("a;;b"), vec!["a", "b"]);
    }

    #[test]
    fn test_leading_keyword_if() {
        let (kw, rest) = leading_keyword("if [ x = y ]").unwrap();
        assert_eq!(kw, Kw::If);
        assert_eq!(rest, "[ x = y ]");
    }

    #[test]
    fn test_leading_keyword_then_with_cmd() {
        let (kw, rest) = leading_keyword("then echo yes").unwrap();
        assert_eq!(kw, Kw::Then);
        assert_eq!(rest, "echo yes");
    }

    #[test]
    fn test_leading_keyword_fi_alone() {
        let (kw, rest) = leading_keyword("fi").unwrap();
        assert_eq!(kw, Kw::Fi);
        assert_eq!(rest, "");
    }

    #[test]
    fn test_leading_keyword_no_match() {
        assert!(leading_keyword("echo fi").is_none());
        assert!(leading_keyword("finder").is_none());
    }

    #[test]
    fn test_parse_for_spec_in() {
        let (v, i) = parse_for_spec("i in 1 2 3");
        assert_eq!(v, "i");
        assert_eq!(i, "1 2 3");
    }

    #[test]
    fn test_parse_body_if_simple() {
        let stmts = parse_body("if true; then echo yes; fi");
        assert_eq!(stmts.len(), 1);
        let Statement::If { condition, then_body, else_body, elif_branches } = &stmts[0] else {
            panic!("expected If");
        };
        assert_eq!(condition, "true");
        assert_eq!(then_body.len(), 1);
        assert!(else_body.is_empty());
        assert!(elif_branches.is_empty());
    }

    #[test]
    fn test_parse_body_if_else() {
        let stmts = parse_body("if false; then echo yes; else echo no; fi");
        assert_eq!(stmts.len(), 1);
        let Statement::If { then_body, else_body, .. } = &stmts[0] else { panic!() };
        assert_eq!(then_body.len(), 1);
        assert_eq!(else_body.len(), 1);
    }

    #[test]
    fn test_parse_body_if_elif_else() {
        let stmts = parse_body("if false; then echo a; elif true; then echo b; else echo c; fi");
        let Statement::If { elif_branches, else_body, .. } = &stmts[0] else { panic!() };
        assert_eq!(elif_branches.len(), 1);
        assert_eq!(else_body.len(), 1);
    }

    #[test]
    fn test_parse_body_for() {
        let stmts = parse_body("for i in 1 2 3; do echo $i; done");
        assert_eq!(stmts.len(), 1);
        let Statement::For { var, items_expr, body } = &stmts[0] else { panic!() };
        assert_eq!(var, "i");
        assert_eq!(items_expr, "1 2 3");
        assert_eq!(body.len(), 1);
    }

    #[test]
    fn test_parse_body_while() {
        let stmts = parse_body("while false; do echo hi; done");
        assert_eq!(stmts.len(), 1);
        let Statement::While { condition, .. } = &stmts[0] else { panic!() };
        assert_eq!(condition, "false");
    }

    #[test]
    fn test_parse_body_break_in_for() {
        let stmts = parse_body("for i in 1; do break; done");
        let Statement::For { body, .. } = &stmts[0] else { panic!() };
        assert!(matches!(body[0], Statement::Break));
    }

    #[test]
    fn test_parse_body_continue_in_for() {
        let stmts = parse_body("for i in 1 2; do continue; echo skip; done");
        let Statement::For { body, .. } = &stmts[0] else { panic!() };
        assert!(matches!(body[0], Statement::Continue));
    }

    #[test]
    fn test_parse_body_nested_if() {
        let stmts = parse_body("if true; then if true; then echo deep; fi; fi");
        let Statement::If { then_body, .. } = &stmts[0] else { panic!() };
        assert!(matches!(then_body[0], Statement::If { .. }));
    }

    #[test]
    fn test_parse_body_local() {
        let stmts = parse_body("local x=42");
        assert_eq!(stmts.len(), 1);
        assert!(matches!(&stmts[0], Statement::Local(s) if s == "x=42"));
    }

    #[test]
    fn test_parse_body_multiple_stmts() {
        let stmts = parse_body("echo a; echo b; echo c");
        assert_eq!(stmts.len(), 3);
    }
}
