/// shako library entry point.
///
/// Re-exports core modules so they are accessible from integration tests,
/// benchmarks, and external tooling without going through the binary entry
/// point in `main.rs`.
///
/// Only modules with meaningful public APIs are exposed here.  Internal
/// implementation details that have no stable public surface (e.g. `spinner`,
/// `fish_import`) are intentionally left out.

pub mod classifier;
pub mod config;
pub mod parser;
pub mod path_cache;
pub mod shell;

// The `safety` and `slash` modules expose types referenced by shell/repl.rs.
pub mod safety;
pub mod slash;

// Needed by benchmarks for AI config types.
pub mod ai;
pub mod builtins;
pub mod executor;
pub mod proactive;
pub mod smart_defaults;

// Conditionally compiled modules.
#[cfg(feature = "fish-import")]
pub mod fish_import;

pub mod behavioral_profile;
pub mod control;
pub mod env_context;
pub mod incident;
pub mod journal;
pub mod learned_prefs;
pub mod setup;
pub mod spinner;
pub mod undo;
