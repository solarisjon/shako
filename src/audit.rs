//! Immutable AI Audit Log — tamper-evident hash-chained journal.
//!
//! Every AI query, generated command, and user decision is appended to
//! `~/.local/share/shako/audit.jsonl` as a JSONL record. Each record
//! includes a `hash` field computed over the record body concatenated with
//! the `prev_hash` of the previous entry. Any retroactive modification of an
//! entry breaks the hash chain, detectable by `/audit verify`.
//!
//! # Format
//!
//! Each line is a JSON object. Fields:
//! - `ts`        — RFC 3339 timestamp
//! - `kind`      — entry kind (see [`AuditKind`])
//! - `nl_input`  — natural-language input (for AI queries)
//! - `generated` — AI-generated command (for AI queries)
//! - `executed`  — command actually executed after any edit
//! - `decision`  — user decision: `"execute"`, `"edit"`, `"cancel"`, `"block"`
//! - `exit_code` — exit code of executed command (optional)
//! - `prev_hash` — hash of the previous entry (`""` for the first)
//! - `hash`      — hash of this entry (body + prev_hash)
//!
//! # Tamper evidence
//!
//! The hash is a simple but deterministic FNV-1a 64-bit hash encoded as a
//! 16-character hex string.  This is not cryptographic — it deters accidental
//! corruption and naive editing, not a sophisticated adversary.  For
//! cryptographic assurance, layer on top of a content-addressed store or sign
//! the file with GPG.

use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ─── Types ─────────────────────────────────────────────────────────────────────

/// The kind of event being logged.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditKind {
    /// User typed natural language → AI translated → user confirmed/edited/cancelled.
    AiQuery,
    /// User typed a direct shell command (not via AI).
    DirectCommand,
    /// Safety layer blocked a command before it was shown to the user.
    SafetyBlock,
    /// Secret Canary flagged a command as credential exfiltration risk.
    ExfilBlock,
}

/// A single audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// RFC 3339 timestamp.
    pub ts: String,
    /// Entry kind.
    pub kind: AuditKind,
    /// Natural-language input (empty for non-AI entries).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub nl_input: String,
    /// AI-generated command (empty for non-AI entries).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub generated: String,
    /// Command actually executed (may differ from `generated` after an edit).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub executed: String,
    /// User decision: `"execute"`, `"edit"`, `"cancel"`, or `"block"`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub decision: String,
    /// Exit code of the executed command (`-1` if not executed).
    #[serde(default = "minus_one", skip_serializing_if = "is_minus_one")]
    pub exit_code: i32,
    /// Hash of the previous entry (`""` for the first entry).
    pub prev_hash: String,
    /// Hash of this entry (body fields + prev_hash).
    pub hash: String,
}

fn minus_one() -> i32 {
    -1
}

fn is_minus_one(v: &i32) -> bool {
    *v == -1
}

// ─── Hash chain ────────────────────────────────────────────────────────────────

/// FNV-1a 64-bit hash, encoded as 16 hex chars.
///
/// Chosen for simplicity (no external deps) and determinism across platforms.
/// Not cryptographic — sufficient for tamper detection of accidental or naive edits.
fn fnv1a_hex(input: &str) -> String {
    const FNV_OFFSET: u64 = 14_695_981_039_346_656_037;
    const FNV_PRIME: u64 = 1_099_511_628_211;

    let mut hash: u64 = FNV_OFFSET;
    for byte in input.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

/// Compute the hash for an entry.
///
/// The hash covers: `ts|kind|nl_input|generated|executed|decision|exit_code|prev_hash`
fn compute_hash(entry: &AuditEntry) -> String {
    let body = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}",
        entry.ts,
        serde_json::to_string(&entry.kind).unwrap_or_default(),
        entry.nl_input,
        entry.generated,
        entry.executed,
        entry.decision,
        entry.exit_code,
        entry.prev_hash,
    );
    fnv1a_hex(&body)
}

// ─── Persistence ───────────────────────────────────────────────────────────────

/// Return the path to the audit log file.
pub fn audit_path() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local").join("share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("shako").join("audit.jsonl")
}

/// Read the hash of the last entry in the audit log.
///
/// Returns `""` if the log is empty or cannot be read.
fn last_hash() -> String {
    let path = audit_path();
    if !path.exists() {
        return String::new();
    }
    let Ok(content) = std::fs::read_to_string(&path) else {
        return String::new();
    };
    // Read the last non-empty line.
    let last_line = content
        .lines().rfind(|l| !l.trim().is_empty())
        .unwrap_or("");
    if let Ok(entry) = serde_json::from_str::<AuditEntry>(last_line) {
        entry.hash
    } else {
        String::new()
    }
}

/// Append an audit entry to the log (fire-and-forget; errors are silently dropped).
///
/// Uses O_APPEND for POSIX-safe concurrent writes without locking.
fn append_entry(entry: &AuditEntry) {
    let path = audit_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(line) = serde_json::to_string(entry) else {
        return;
    };
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    if let Ok(mut f) = options.open(&path) {
        let _ = writeln!(f, "{line}");
    }
}

// ─── Public API ────────────────────────────────────────────────────────────────

/// Record an AI query event (NL input + generated command + user decision).
pub fn record_ai_query(
    nl_input: &str,
    generated: &str,
    executed: &str,
    decision: &str,
    exit_code: i32,
) {
    let prev_hash = last_hash();
    let mut entry = AuditEntry {
        ts: now_rfc3339(),
        kind: AuditKind::AiQuery,
        nl_input: nl_input.to_string(),
        generated: generated.to_string(),
        executed: executed.to_string(),
        decision: decision.to_string(),
        exit_code,
        prev_hash,
        hash: String::new(),
    };
    entry.hash = compute_hash(&entry);
    append_entry(&entry);
}

/// Record a direct shell command typed by the user (not via AI).
#[allow(dead_code)]
pub fn record_direct_command(command: &str, exit_code: i32) {
    let prev_hash = last_hash();
    let mut entry = AuditEntry {
        ts: now_rfc3339(),
        kind: AuditKind::DirectCommand,
        nl_input: String::new(),
        generated: String::new(),
        executed: command.to_string(),
        decision: "execute".to_string(),
        exit_code,
        prev_hash,
        hash: String::new(),
    };
    entry.hash = compute_hash(&entry);
    append_entry(&entry);
}

/// Record a safety block event (dangerous command blocked before showing to user).
pub fn record_safety_block(command: &str, reason: &str) {
    let prev_hash = last_hash();
    let mut entry = AuditEntry {
        ts: now_rfc3339(),
        kind: AuditKind::SafetyBlock,
        nl_input: String::new(),
        generated: command.to_string(),
        executed: String::new(),
        decision: format!("block: {reason}"),
        exit_code: -1,
        prev_hash,
        hash: String::new(),
    };
    entry.hash = compute_hash(&entry);
    append_entry(&entry);
}

/// Record an exfiltration block (Secret Canary critical risk, blocked).
pub fn record_exfil_block(command: &str, risk_summary: &str) {
    let prev_hash = last_hash();
    let mut entry = AuditEntry {
        ts: now_rfc3339(),
        kind: AuditKind::ExfilBlock,
        nl_input: String::new(),
        generated: command.to_string(),
        executed: String::new(),
        decision: format!("exfil_block: {risk_summary}"),
        exit_code: -1,
        prev_hash,
        hash: String::new(),
    };
    entry.hash = compute_hash(&entry);
    append_entry(&entry);
}

// ─── Verification ──────────────────────────────────────────────────────────────

/// Verify the audit log hash chain.
///
/// Returns `Ok(count)` if the chain is intact, `Err(report)` if any breaks
/// are detected (with a human-readable description of the first break found).
pub fn verify_chain() -> Result<usize, String> {
    let path = audit_path();
    if !path.exists() {
        return Ok(0);
    }
    let content =
        std::fs::read_to_string(&path).map_err(|e| format!("cannot read audit log: {e}"))?;

    let mut prev_hash = String::new();
    let mut count = 0usize;

    for (line_no, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let entry: AuditEntry = serde_json::from_str(line)
            .map_err(|e| format!("line {}: JSON parse error: {e}", line_no + 1))?;

        if entry.prev_hash != prev_hash {
            return Err(format!(
                "line {}: chain break — prev_hash mismatch (expected '{}', got '{}')",
                line_no + 1,
                prev_hash,
                entry.prev_hash
            ));
        }

        let expected_hash = compute_hash(&entry);
        if entry.hash != expected_hash {
            return Err(format!(
                "line {}: hash mismatch — entry was tampered (stored '{}', computed '{}')",
                line_no + 1,
                entry.hash,
                expected_hash
            ));
        }

        prev_hash = entry.hash.clone();
        count += 1;
    }

    Ok(count)
}

/// Search audit log entries matching a query string.
///
/// Returns entries whose `nl_input`, `generated`, or `executed` fields contain
/// the query (case-insensitive).  Returns at most `limit` most-recent matches.
pub fn search_entries(query: &str, limit: usize) -> Vec<AuditEntry> {
    let path = audit_path();
    if !path.exists() {
        return vec![];
    }
    let Ok(content) = std::fs::read_to_string(&path) else {
        return vec![];
    };
    let query_lower = query.to_lowercase();

    let mut matches: Vec<AuditEntry> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<AuditEntry>(l).ok())
        .filter(|e| {
            e.nl_input.to_lowercase().contains(&query_lower)
                || e.generated.to_lowercase().contains(&query_lower)
                || e.executed.to_lowercase().contains(&query_lower)
        })
        .collect();

    // Return most-recent first.
    matches.reverse();
    matches.truncate(limit);
    matches
}

// ─── Helpers ───────────────────────────────────────────────────────────────────

fn now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Format as a minimal RFC 3339 UTC string: YYYY-MM-DDTHH:MM:SSZ
    let s = secs;
    let sec = s % 60;
    let min = (s / 60) % 60;
    let hour = (s / 3600) % 24;
    let days = s / 86400;
    // Days since Unix epoch (1970-01-01)
    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    let mut year = 1970u64;
    loop {
        let leap = is_leap(year);
        let yd = if leap { 366 } else { 365 };
        if days < yd {
            break;
        }
        days -= yd;
        year += 1;
    }
    let months = [
        31u64,
        28 + is_leap(year) as u64,
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u64;
    for m in months {
        if days < m {
            break;
        }
        days -= m;
        month += 1;
    }
    (year, month, days + 1)
}

fn is_leap(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fnv1a_hex_deterministic() {
        let h1 = fnv1a_hex("hello world");
        let h2 = fnv1a_hex("hello world");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16);
    }

    #[test]
    fn test_fnv1a_hex_different_inputs() {
        let h1 = fnv1a_hex("cargo build");
        let h2 = fnv1a_hex("cargo clean");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_now_rfc3339_format() {
        let ts = now_rfc3339();
        // Basic format check: YYYY-MM-DDTHH:MM:SSZ
        assert_eq!(ts.len(), 20);
        assert!(ts.ends_with('Z'));
        assert!(ts.contains('T'));
    }

    #[test]
    fn test_compute_hash_changes_with_content() {
        let mut e1 = AuditEntry {
            ts: "2026-04-11T12:00:00Z".to_string(),
            kind: AuditKind::AiQuery,
            nl_input: "list files".to_string(),
            generated: "ls -la".to_string(),
            executed: "ls -la".to_string(),
            decision: "execute".to_string(),
            exit_code: 0,
            prev_hash: String::new(),
            hash: String::new(),
        };
        let mut e2 = e1.clone();
        e2.nl_input = "delete files".to_string();

        e1.hash = compute_hash(&e1);
        e2.hash = compute_hash(&e2);
        assert_ne!(e1.hash, e2.hash);
    }

    #[test]
    fn test_audit_entry_roundtrip_json() {
        let mut entry = AuditEntry {
            ts: "2026-04-11T12:00:00Z".to_string(),
            kind: AuditKind::DirectCommand,
            nl_input: String::new(),
            generated: String::new(),
            executed: "git status".to_string(),
            decision: "execute".to_string(),
            exit_code: 0,
            prev_hash: String::new(),
            hash: String::new(),
        };
        entry.hash = compute_hash(&entry);

        let json = serde_json::to_string(&entry).unwrap();
        let parsed: AuditEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.executed, "git status");
        assert_eq!(parsed.hash, entry.hash);
    }

    #[test]
    fn test_verify_chain_empty_log() {
        // verify_chain on a non-existent path: need to override path for test
        // We test the pure-logic path using a temp file.
        let result = verify_chain_from_str("");
        assert_eq!(result, Ok(0));
    }

    #[test]
    fn test_verify_chain_single_entry() {
        let mut e = AuditEntry {
            ts: "2026-04-11T12:00:00Z".to_string(),
            kind: AuditKind::AiQuery,
            nl_input: "build project".to_string(),
            generated: "cargo build".to_string(),
            executed: "cargo build".to_string(),
            decision: "execute".to_string(),
            exit_code: 0,
            prev_hash: String::new(),
            hash: String::new(),
        };
        e.hash = compute_hash(&e);
        let line = serde_json::to_string(&e).unwrap();
        assert_eq!(verify_chain_from_str(&line), Ok(1));
    }

    #[test]
    fn test_verify_chain_detects_tamper() {
        let mut e = AuditEntry {
            ts: "2026-04-11T12:00:00Z".to_string(),
            kind: AuditKind::AiQuery,
            nl_input: "build".to_string(),
            generated: "cargo build".to_string(),
            executed: "cargo build".to_string(),
            decision: "execute".to_string(),
            exit_code: 0,
            prev_hash: String::new(),
            hash: String::new(),
        };
        e.hash = compute_hash(&e);
        let mut bad = e.clone();
        bad.executed = "rm -rf /".to_string(); // tamper
                                               // hash unchanged — should fail
        let line = serde_json::to_string(&bad).unwrap();
        let result = verify_chain_from_str(&line);
        assert!(result.is_err(), "tampered entry should fail verification");
    }

    /// Pure-logic verification that doesn't touch the filesystem.
    fn verify_chain_from_str(content: &str) -> Result<usize, String> {
        let mut prev_hash = String::new();
        let mut count = 0usize;
        for (line_no, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let entry: AuditEntry =
                serde_json::from_str(line).map_err(|e| format!("line {}: {e}", line_no + 1))?;
            if entry.prev_hash != prev_hash {
                return Err(format!("line {}: chain break", line_no + 1));
            }
            let expected = compute_hash(&entry);
            if entry.hash != expected {
                return Err(format!("line {}: hash mismatch", line_no + 1));
            }
            prev_hash = entry.hash.clone();
            count += 1;
        }
        Ok(count)
    }
}
