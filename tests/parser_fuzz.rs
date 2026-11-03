//! Dependency-free fuzz harness for the text-handling entry points.
//!
//! `cargo-fuzz` needs nightly, so this drives the same targets from stable with
//! a seeded PRNG: random strings biased toward shell metacharacters plus
//! mutations of known-tricky seeds.  The property under test is simply "never
//! panics, and never runs a command substitution while analysing text".
//!
//! Tune with `SHAKO_FUZZ_ITERS` (default 20000) and `SHAKO_FUZZ_SEED`.
//! Only inert entry points are exercised: nothing here executes input.

use std::panic::{AssertUnwindSafe, catch_unwind};

use shako::ai::exfil_guard;
use shako::{control, parser, safety};

/// xorshift64* — small, fast, and reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Fragments that exercise quoting, chaining, redirects, heredocs and control flow.
const FRAGMENTS: &[&str] = &[
    "'",
    "\"",
    "\\",
    "`",
    "$(",
    ")",
    "${",
    "}",
    "$((",
    "))",
    "$",
    "~",
    "*",
    "?",
    "[",
    "]",
    "{",
    ",",
    "..",
    ";",
    "&&",
    "||",
    "|",
    "&",
    ">",
    ">>",
    "<",
    "<<",
    "<<<",
    "2>",
    "2>&1",
    "&>",
    " ",
    "  ",
    "\t",
    "\n",
    "=",
    "#",
    "!",
    "@",
    "%",
    "-",
    "--",
    "-rf",
    "/",
    "/dev/sda",
    "~/.ssh/id_rsa",
    "rm",
    "echo",
    "cat",
    "curl",
    "sudo",
    "env",
    "for",
    "while",
    "if",
    "else",
    "end",
    "function",
    "in",
    "do",
    "done",
    "fi",
    "break",
    "continue",
    "return",
    "a",
    "b",
    "x",
    "0",
    "1",
    "9",
    "é",
    "日",
    "\u{0}",
    "\u{7f}",
    "\r",
];

/// Inputs that have historically been, or look like they could be, awkward.
const SEEDS: &[&str] = &[
    "echo 'unterminated",
    "echo \"unterminated",
    "echo $(",
    "echo ${",
    "echo ${é",
    "echo ${日:-x}",
    "echo $((1+",
    "echo {1..3",
    "echo }{",
    "a && || b",
    "| | |",
    "cat <<EOF\nbody",
    "cat <<<",
    "for i in 1 2 3\n echo $i",
    "if true\n else\n end\n end",
    "while\nend",
    "function\nend",
    "rm -rf /; echo ok",
    "echo `echo `",
    "echo \\",
    "2>&1 > >",
    "{,,,}{,,}",
    "$((((((((1))))))))",
];

fn random_input(rng: &mut Rng) -> String {
    if rng.below(4) == 0 {
        // Mutate a seed: splice a random fragment in, or truncate it.
        let mut s = SEEDS[rng.below(SEEDS.len())].to_string();
        match rng.below(3) {
            0 => {
                let at = char_boundary(&s, rng.below(s.len() + 1));
                s.insert_str(at, FRAGMENTS[rng.below(FRAGMENTS.len())]);
            }
            1 => {
                let at = char_boundary(&s, rng.below(s.len() + 1));
                s.truncate(at);
            }
            _ => s.push_str(FRAGMENTS[rng.below(FRAGMENTS.len())]),
        }
        return s;
    }
    let parts = 1 + rng.below(14);
    (0..parts)
        .map(|_| FRAGMENTS[rng.below(FRAGMENTS.len())])
        .collect()
}

/// Largest char boundary at or below `idx`.
fn char_boundary(s: &str, mut idx: usize) -> usize {
    while !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn run_targets(input: &str) {
    // Pure text splitting.
    let _ = parser::split_chains(input);
    let _ = parser::split_pipes(input);
    // Inert parsing: never executes `$(...)`.
    let _ = parser::parse_args_inert(input);
    // Control-flow parser (parse only, never exec).
    let _ = control::is_control_flow(input);
    let _ = control::has_control_flow(input);
    let _ = control::parse_body(input);
    // Analysis layers that sit in front of AI-generated commands.
    let _ = safety::is_dangerous(input);
    let _ = safety::needs_extra_confirmation(input);
    let _ = safety::is_snapshotable(input);
    let _ = exfil_guard::scan(input);
}

#[test]
fn text_entry_points_never_panic() {
    let iters = env_u64("SHAKO_FUZZ_ITERS", 20_000);
    let seed = env_u64("SHAKO_FUZZ_SEED", 0x5eed_5eed_5eed_5eed) | 1;
    let mut rng = Rng(seed);

    // Keep panic messages out of the output; failures are reported below.
    std::panic::set_hook(Box::new(|_| {}));

    let mut failures: Vec<String> = Vec::new();
    for s in SEEDS {
        if catch_unwind(AssertUnwindSafe(|| run_targets(s))).is_err() {
            failures.push((*s).to_string());
        }
    }
    for _ in 0..iters {
        let input = random_input(&mut rng);
        if catch_unwind(AssertUnwindSafe(|| run_targets(&input))).is_err() {
            failures.push(input);
            if failures.len() >= 10 {
                break;
            }
        }
    }
    let _ = std::panic::take_hook();

    assert!(
        failures.is_empty(),
        "panics (seed {seed:#x}) on inputs: {failures:#?}"
    );
}

#[test]
fn safety_analysis_never_runs_command_substitution() {
    let marker = std::env::temp_dir().join(format!("shako_fuzz_marker_{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let m = marker.display();

    for cmd in [
        format!("echo $(touch {m})"),
        format!("echo `touch {m}`"),
        format!("rm -rf $(touch {m})"),
        format!("sudo env X=$(touch {m}) ls"),
        format!("cat \"$(touch {m})\" | sh"),
    ] {
        let _ = safety::is_dangerous(&cmd);
        let _ = safety::needs_extra_confirmation(&cmd);
        let _ = safety::is_snapshotable(&cmd);
        let _ = parser::parse_args_inert(&cmd);
        assert!(
            !marker.exists(),
            "substitution executed while analysing {cmd:?}"
        );
    }
}
