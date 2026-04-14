//! Danger Replay and Undo Graph — filesystem snapshots before risky commands.
//!
//! # Overview
//!
//! When shako intercepts a dangerous or extra-confirmation command the user
//! confirms, this module optionally takes a **targeted snapshot** of the
//! affected paths before execution. Those snapshots are recorded in an undo
//! graph (a DAG stored in `~/.local/share/shako/undo_graph.json`) so the user
//! can later say "undo that rm" or "restore what I deleted" and shako resolves
//! it to the right snapshot.
//!
//! # Lifecycle
//!
//! ```text
//!  user types: rm -rf old_build/
//!  → safety.rs: is_dangerous → true → offer snapshot
//!  → undo::take_snapshot("rm -rf old_build/", ["old_build/"])
//!  → sha3f7a9c written to undo_graph.json
//!  → command executes
//!  [later]
//!  user types: undo that rm
//!  → classifier: UndoRequest
//!  → undo::find_latest_snapshot()
//!  → confirm + restore
//! ```
//!
//! # On-disk layout
//!
//! Snapshots live in `~/.local/share/shako/snapshots/<sha>/` (created with
//! `cp -a`). Each entry in the undo graph references:
//!   - `sha`       — 7-char prefix of sha256(path + timestamp)
//!   - `paths`     — list of paths that were captured
//!   - `timestamp` — RFC3339 UTC capture time
//!   - `command`   — the command that triggered the snapshot
//!   - `restored`  — whether this snapshot was already used
//!
//! # Limits
//!
//! - Max snapshot size: configurable, default 50 MB.  Skip if larger.
//! - Skip if all paths are git-tracked (git can undo for you).
//! - Garbage-collect snapshots older than N days (default: 7).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ─── Data model ──────────────────────────────────────────────────────────────

/// A single node in the undo graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEntry {
    /// Short identifier (7 hex chars).
    pub sha: String,
    /// Absolute paths that were snapshotted.
    pub paths: Vec<String>,
    /// ISO 8601 / RFC 3339 timestamp.
    pub timestamp: String,
    /// The command that prompted the snapshot.
    pub command: String,
    /// True once this snapshot has been used to restore.
    pub restored: bool,
}

/// The full undo graph persisted to disk.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct UndoGraph {
    pub entries: Vec<SnapshotEntry>,
}

// ─── Path helpers ─────────────────────────────────────────────────────────────

/// `~/.local/share/shako/`
fn shako_data_dir() -> PathBuf {
    dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("shako")
}

/// Path to the undo graph JSON file.
pub fn undo_graph_path() -> PathBuf {
    shako_data_dir().join("undo_graph.json")
}

/// Directory that holds all snapshot archives.
fn snapshots_dir() -> PathBuf {
    shako_data_dir().join("snapshots")
}

/// Directory for a specific snapshot by sha.
fn snapshot_dir(sha: &str) -> PathBuf {
    snapshots_dir().join(sha)
}

// ─── Graph I/O ────────────────────────────────────────────────────────────────

/// Load the undo graph from disk.  Returns an empty graph on missing file.
pub fn load_graph() -> UndoGraph {
    let path = undo_graph_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => UndoGraph::default(),
    }
}

/// Persist the undo graph to disk.
fn save_graph(graph: &UndoGraph) -> Result<()> {
    let path = undo_graph_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create dir {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(graph)?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

// ─── Snapshot creation ────────────────────────────────────────────────────────

/// Generate a 7-char SHA hex identifier from path + timestamp.
fn make_sha(paths: &[&str], timestamp: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    for p in paths {
        p.hash(&mut h);
    }
    timestamp.hash(&mut h);
    format!("{:07x}", h.finish() & 0x0FFFFFFF)
}

/// Current UTC timestamp as RFC 3339.
fn now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Minimal RFC 3339 without external crate: YYYY-MM-DDTHH:MM:SSZ
    let s = secs;
    let secs_in_day = s % 86400;
    let days = s / 86400;
    // Days since epoch to Gregorian (simplified, valid 1970-2099)
    let (year, month, day) = days_to_ymd(days);
    let h = secs_in_day / 3600;
    let m = (secs_in_day % 3600) / 60;
    let sc = secs_in_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{sc:02}Z")
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Gregorian calendar computation, valid 1970-2199.
    let mut d = days;
    let mut y: u64 = 1970;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let days_in_year = if leap { 366 } else { 365 };
        if d < days_in_year {
            break;
        }
        d -= days_in_year;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let month_days: [u64; 12] = [
        31,
        if leap { 29 } else { 28 },
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
    for &md in &month_days {
        if d < md {
            break;
        }
        d -= md;
        month += 1;
    }
    (y, month, d + 1)
}

/// Compute the total size (bytes) of a path (file or directory).
fn path_size(p: &Path) -> u64 {
    if p.is_file() {
        return p.metadata().map(|m| m.len()).unwrap_or(0);
    }
    if p.is_dir() {
        let mut total = 0u64;
        if let Ok(rd) = std::fs::read_dir(p) {
            for entry in rd.flatten() {
                total += path_size(&entry.path());
            }
        }
        return total;
    }
    0
}

/// Returns true if the path is tracked by git (i.e. git can already undo it).
fn is_git_tracked(p: &Path) -> bool {
    // Quick check: run git ls-files --error-unmatch on the path.
    std::process::Command::new("git")
        .args(["ls-files", "--error-unmatch", "--"])
        .arg(p)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Attempt to extract affected filesystem paths from a command string.
///
/// This is best-effort: parse positional arguments from known destructive
/// commands. For `rm`, `mv`, `chmod`, `chown` we extract the non-flag args.
pub fn extract_paths(command: &str) -> Vec<String> {
    let args = crate::parser::parse_args(command);
    if args.is_empty() {
        return vec![];
    }
    let cmd = args[0].to_ascii_lowercase();
    // Strip leading "sudo" to get actual command.
    let (cmd, args) = if cmd == "sudo" && args.len() > 1 {
        (args[1].to_ascii_lowercase(), args[1..].to_vec())
    } else {
        (cmd, args)
    };

    match cmd.as_str() {
        "rm" | "mv" => {
            // Collect non-flag positional args after the command itself.
            args.iter()
                .skip(1)
                .filter(|a| !a.starts_with('-'))
                .cloned()
                .collect()
        }
        "chmod" | "chown" => {
            // Skip flags AND the mode/owner argument (first non-flag positional).
            // e.g. chmod 777 /tmp/mydir — skip "777", keep "/tmp/mydir"
            //      chown root:root /etc  — skip "root:root", keep "/etc"
            let positionals: Vec<&String> = args
                .iter()
                .skip(1)
                .filter(|a| !a.starts_with('-'))
                .collect();
            // The first positional is the mode/owner spec, the rest are paths.
            positionals.iter().skip(1).map(|s| s.to_string()).collect()
        }
        "dd" => {
            // For dd, snapshot the of= target directory if it's a real path.
            args.iter()
                .filter_map(|a| {
                    if let Some(p) = a.strip_prefix("of=") {
                        let path = Path::new(p);
                        if path.exists() && !p.starts_with("/dev/") {
                            return Some(p.to_string());
                        }
                    }
                    None
                })
                .collect()
        }
        _ => vec![],
    }
}

/// Outcome of a snapshot attempt.
pub enum SnapshotResult {
    /// Snapshot taken successfully. Contains the SHA.
    Taken(String),
    /// Skipped because paths are git-tracked.
    GitTracked,
    /// Skipped because the total size exceeds the limit.
    TooLarge(u64),
    /// Skipped because no actionable paths were found.
    NoPaths,
    /// An error occurred.
    Error(anyhow::Error),
}

/// Take a filesystem snapshot before executing a dangerous command.
///
/// - `command`       — the raw command string (used for the graph record)
/// - `max_bytes`     — size limit in bytes (0 = use default 50 MB)
///
/// Returns a [`SnapshotResult`] indicating outcome.
pub fn take_snapshot(command: &str, max_bytes: u64) -> SnapshotResult {
    let limit = if max_bytes == 0 {
        50 * 1024 * 1024
    } else {
        max_bytes
    };

    let raw_paths = extract_paths(command);
    if raw_paths.is_empty() {
        return SnapshotResult::NoPaths;
    }

    // Filter to paths that actually exist.
    let existing: Vec<PathBuf> = raw_paths
        .iter()
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .collect();

    if existing.is_empty() {
        return SnapshotResult::NoPaths;
    }

    // Skip if all paths are git-tracked.
    if existing.iter().all(|p| is_git_tracked(p)) {
        return SnapshotResult::GitTracked;
    }

    // Size check.
    let total_size: u64 = existing.iter().map(|p| path_size(p)).sum();
    if total_size > limit {
        return SnapshotResult::TooLarge(total_size);
    }

    // Create snapshot.
    let timestamp = now_rfc3339();
    let path_strs: Vec<&str> = raw_paths.iter().map(String::as_str).collect();
    let sha = make_sha(&path_strs, &timestamp);
    let dest = snapshot_dir(&sha);

    if let Err(e) = std::fs::create_dir_all(&dest) {
        return SnapshotResult::Error(e.into());
    }

    // Copy each path.
    for path in &existing {
        let status = std::process::Command::new("cp")
            .arg("-a")
            .arg(path)
            .arg(&dest)
            .status();
        if let Err(e) = status {
            return SnapshotResult::Error(anyhow::anyhow!(
                "cp -a {} failed: {}",
                path.display(),
                e
            ));
        }
    }

    // Record in graph.
    let entry = SnapshotEntry {
        sha: sha.clone(),
        paths: raw_paths,
        timestamp,
        command: command.to_string(),
        restored: false,
    };

    let mut graph = load_graph();
    graph.entries.push(entry);
    if let Err(e) = save_graph(&graph) {
        return SnapshotResult::Error(e);
    }

    SnapshotResult::Taken(sha)
}

// ─── Snapshot lookup ──────────────────────────────────────────────────────────

/// Find the most recent unrestored snapshot entry.
pub fn find_latest_snapshot() -> Option<SnapshotEntry> {
    let graph = load_graph();
    // Entries appended newest-last; iterate in reverse.
    graph.entries.into_iter().rev().find(|e| !e.restored)
}

/// Find the most recent snapshot that matches a keyword (path or command fragment).
pub fn find_snapshot_matching(keyword: &str) -> Option<SnapshotEntry> {
    let graph = load_graph();
    let kw_lower = keyword.to_ascii_lowercase();
    graph.entries.into_iter().rev().find(|e| {
        !e.restored
            && (e.command.to_ascii_lowercase().contains(&kw_lower)
                || e.paths
                    .iter()
                    .any(|p| p.to_ascii_lowercase().contains(&kw_lower)))
    })
}

// ─── Snapshot restore ─────────────────────────────────────────────────────────

/// Restore paths from a snapshot.
///
/// Copies snapshot contents back to their original locations.
/// Marks the snapshot as restored in the graph.
pub fn restore_snapshot(sha: &str) -> Result<()> {
    let src = snapshot_dir(sha);
    if !src.exists() {
        anyhow::bail!("snapshot {} not found at {}", sha, src.display());
    }

    // Each item in the snapshot dir is named after the original basename.
    // We restore it to cwd-relative (or absolute if originally absolute).
    let rd =
        std::fs::read_dir(&src).with_context(|| format!("read snapshot dir {}", src.display()))?;

    for entry in rd.flatten() {
        let item_name = entry.file_name();
        let dest = PathBuf::from(".").join(&item_name);
        // If the original path was absolute, use the graph entry to find it.
        let graph = load_graph();
        let abs_dest: PathBuf = graph
            .entries
            .iter()
            .find(|e| e.sha == sha)
            .and_then(|e| {
                e.paths
                    .iter()
                    .find(|p| {
                        Path::new(p)
                            .file_name()
                            .map(|n| n == item_name)
                            .unwrap_or(false)
                    })
                    .map(PathBuf::from)
            })
            .unwrap_or(dest);

        // Parent must exist.
        if let Some(parent) = abs_dest.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).ok();
            }
        }

        let status = std::process::Command::new("cp")
            .arg("-a")
            .arg(entry.path())
            .arg(&abs_dest)
            .status()
            .with_context(|| {
                format!("cp -a {} -> {}", entry.path().display(), abs_dest.display())
            })?;

        if !status.success() {
            anyhow::bail!(
                "cp -a returned non-zero restoring {}",
                item_name.to_string_lossy()
            );
        }
    }

    // Mark restored.
    mark_restored(sha)?;
    Ok(())
}

/// Mark a snapshot entry as restored in the graph.
fn mark_restored(sha: &str) -> Result<()> {
    let mut graph = load_graph();
    if let Some(e) = graph.entries.iter_mut().find(|e| e.sha == sha) {
        e.restored = true;
    }
    save_graph(&graph)
}

// ─── Garbage collection ───────────────────────────────────────────────────────

/// Remove snapshots older than `max_age_days` days.
///
/// Purges both the on-disk directories and their graph entries.
pub fn gc_old_snapshots(max_age_days: u64) {
    let mut graph = load_graph();
    let cutoff_secs = max_age_days * 86400;

    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut keep = Vec::new();
    for entry in &graph.entries {
        let age_secs = entry_age_secs(&entry.timestamp, now);
        if age_secs > cutoff_secs {
            // Remove from disk.
            let dir = snapshot_dir(&entry.sha);
            if dir.exists() {
                let _ = std::fs::remove_dir_all(&dir);
            }
        } else {
            keep.push(entry.clone());
        }
    }

    graph.entries = keep;
    let _ = save_graph(&graph);
}

/// Parse a minimal RFC 3339 timestamp into seconds since UNIX epoch.
fn rfc3339_to_secs(ts: &str) -> u64 {
    // Format: YYYY-MM-DDTHH:MM:SSZ (as produced by now_rfc3339)
    let parts: Vec<&str> = ts.split(['T', 'Z', '-', ':']).collect();
    if parts.len() < 6 {
        return 0;
    }
    let year: u64 = parts[0].parse().unwrap_or(1970);
    let month: u64 = parts[1].parse().unwrap_or(1);
    let day: u64 = parts[2].parse().unwrap_or(1);
    let h: u64 = parts[3].parse().unwrap_or(0);
    let m: u64 = parts[4].parse().unwrap_or(0);
    let s: u64 = parts[5].parse().unwrap_or(0);

    // Days since epoch (rough but sufficient for GC purposes).
    let years_since_1970 = year.saturating_sub(1970);
    let leap_years = years_since_1970 / 4;
    let days_from_year = years_since_1970 * 365 + leap_years;
    let month_offsets: [u64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let days_from_month = month_offsets[((month as usize).saturating_sub(1)).min(11)];
    let total_days = days_from_year + days_from_month + day.saturating_sub(1);
    total_days * 86400 + h * 3600 + m * 60 + s
}

fn entry_age_secs(timestamp: &str, now: u64) -> u64 {
    let entry_secs = rfc3339_to_secs(timestamp);
    now.saturating_sub(entry_secs)
}

// ─── Human-readable snapshot display ─────────────────────────────────────────

/// Format a snapshot entry for display in confirmation panels.
pub fn format_snapshot(entry: &SnapshotEntry) -> String {
    let paths = entry.paths.join(", ");
    let size_hint = entry
        .paths
        .iter()
        .map(|p| path_size(Path::new(p)))
        .sum::<u64>();
    let size_display = if size_hint == 0 {
        String::new()
    } else {
        format!(" ({:.1} MB)", size_hint as f64 / 1_048_576.0)
    };
    format!(
        "snapshot {} — {}{} from {}",
        entry.sha, paths, size_display, entry.timestamp
    )
}

/// List all unrestored snapshots for the /undo command.
#[allow(dead_code)]
pub fn list_snapshots() -> Vec<SnapshotEntry> {
    let graph = load_graph();
    graph.entries.into_iter().filter(|e| !e.restored).collect()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_paths_rm() {
        let paths = extract_paths("rm -rf old_build");
        assert_eq!(paths, vec!["old_build"]);
    }

    #[test]
    fn test_extract_paths_rm_with_sudo() {
        let paths = extract_paths("sudo rm -rf /tmp/test");
        assert_eq!(paths, vec!["/tmp/test"]);
    }

    #[test]
    fn test_extract_paths_mv() {
        let paths = extract_paths("mv src dst");
        assert_eq!(paths, vec!["src", "dst"]);
    }

    #[test]
    fn test_extract_paths_chmod() {
        let paths = extract_paths("chmod 777 /tmp/mydir");
        assert_eq!(paths, vec!["/tmp/mydir"]);
    }

    #[test]
    fn test_extract_paths_unknown() {
        let paths = extract_paths("ls -la");
        assert!(paths.is_empty());
    }

    #[test]
    fn test_make_sha_deterministic() {
        let a = make_sha(&["foo", "bar"], "2024-01-01T00:00:00Z");
        let b = make_sha(&["foo", "bar"], "2024-01-01T00:00:00Z");
        assert_eq!(a, b);
        assert_eq!(a.len(), 7);
    }

    #[test]
    fn test_now_rfc3339_format() {
        let ts = now_rfc3339();
        assert!(
            ts.len() >= 19,
            "timestamp should be at least 19 chars: {ts}"
        );
        assert!(ts.contains('T'), "timestamp should contain T: {ts}");
        assert!(ts.ends_with('Z'), "timestamp should end with Z: {ts}");
    }

    #[test]
    fn test_format_snapshot() {
        let entry = SnapshotEntry {
            sha: "abc1234".to_string(),
            paths: vec!["old_build".to_string()],
            timestamp: "2024-01-15T10:00:00Z".to_string(),
            command: "rm -rf old_build".to_string(),
            restored: false,
        };
        let display = format_snapshot(&entry);
        assert!(display.contains("abc1234"));
        assert!(display.contains("old_build"));
    }

    #[test]
    fn test_rfc3339_to_secs_epoch() {
        // 1970-01-01T00:00:00Z should be 0.
        assert_eq!(rfc3339_to_secs("1970-01-01T00:00:00Z"), 0);
    }

    #[test]
    fn test_rfc3339_to_secs_one_day() {
        // 1970-01-02T00:00:00Z should be 86400.
        assert_eq!(rfc3339_to_secs("1970-01-02T00:00:00Z"), 86400);
    }

    #[test]
    fn test_entry_age_recent() {
        let ts = now_rfc3339();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let age = entry_age_secs(&ts, now);
        // Should be at most a few seconds.
        assert!(age < 60, "age should be < 60 seconds: {age}");
    }

    #[test]
    fn test_undo_graph_roundtrip() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let graph_path = tmp.path().join("undo_graph.json");

        let entry = SnapshotEntry {
            sha: "abc1234".to_string(),
            paths: vec!["foo".to_string()],
            timestamp: now_rfc3339(),
            command: "rm -rf foo".to_string(),
            restored: false,
        };
        let graph = UndoGraph {
            entries: vec![entry],
        };
        let json = serde_json::to_string_pretty(&graph).unwrap();
        std::fs::write(&graph_path, &json).unwrap();

        let loaded: UndoGraph =
            serde_json::from_str(&std::fs::read_to_string(&graph_path).unwrap()).unwrap();
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].sha, "abc1234");
    }
}
