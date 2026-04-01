/// Safety layer — detect dangerous commands before execution.
///
/// Detection is done via **parse-tree analysis** rather than raw string
/// matching so that trivial whitespace or flag variations cannot bypass it.
///
/// # Bypass examples that are now caught
/// | Intent | Was previously missed |
/// |---|---|
/// | `rm  -rf /` | double space |
/// | `rm -r -f /` | flags split across tokens |
/// | `rm --recursive --force /` | long flag aliases |
/// | `: () {` | spaces in fork-bomb pattern |
use crate::parser;

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Expand a short-option bundle into individual flags.
///
/// `-rf` → `{r, f}`.  `-r` → `{r}`.  A long option (`--foo`) passes through
/// as-is.  A non-option token returns an empty set.
fn expand_flags(token: &str) -> std::collections::HashSet<char> {
    if token.starts_with("--") {
        return std::collections::HashSet::new(); // handled separately
    }
    if let Some(rest) = token.strip_prefix('-') {
        return rest.chars().collect();
    }
    std::collections::HashSet::new()
}

/// Return all long-flag names (without `--`) present in `args`.
fn long_flags(args: &[String]) -> Vec<String> {
    args.iter()
        .filter_map(|a| a.strip_prefix("--").map(|s| s.to_ascii_lowercase()))
        .collect()
}

/// Return all short-flag characters from all flag tokens in `args`.
fn all_short_flags(args: &[String]) -> std::collections::HashSet<char> {
    args.iter().flat_map(|a| expand_flags(a)).collect()
}

// ─── rm analysis ────────────────────────────────────────────────────────────

/// Dangerous root paths targeted by `rm` (post-expansion, i.e. after tilde/glob).
const DANGEROUS_RM_TARGETS_EXPANDED: &[&str] = &["/", "/*"];

/// Detect `rm` invocations that are recursive + force-ful + target a dangerous root.
///
/// `raw_command` is the original pre-parse string; it is used to catch `~` and
/// `*` before they are expanded by the parser to actual filesystem paths.
fn is_dangerous_rm(args: &[String], raw_command: &str) -> bool {
    // args[0] should be "rm" (already verified by callers)
    let rest = if args.len() > 1 {
        &args[1..]
    } else {
        return false;
    };

    let short = all_short_flags(rest);
    let longs = long_flags(rest);

    let is_recursive =
        short.contains(&'r') || short.contains(&'R') || longs.iter().any(|f| f == "recursive");

    let is_force = short.contains(&'f') || longs.iter().any(|f| f == "force");

    if !is_recursive || !is_force {
        return false;
    }

    // Check post-expansion targets (e.g. "/" or "/*").
    let hits_expanded = rest
        .iter()
        .filter(|a| !a.starts_with('-'))
        .any(|target| DANGEROUS_RM_TARGETS_EXPANDED.contains(&target.as_str()));

    if hits_expanded {
        return true;
    }

    // Also check the raw (pre-parse) string for `~`, `*`, and `/*` because the
    // parser expands globs before we see the tokens.
    let raw_tokens: Vec<&str> = raw_command.split_ascii_whitespace().collect();
    raw_tokens
        .iter()
        .any(|t| *t == "~" || *t == "*" || *t == "/*")
}

// ─── Fork-bomb analysis ─────────────────────────────────────────────────────

/// Strip all ASCII whitespace from a string and return the condensed form.
/// Used to normalise fork-bomb patterns that insert spaces to evade detection.
fn condensed(s: &str) -> String {
    s.chars().filter(|c| !c.is_ascii_whitespace()).collect()
}

/// True when the input looks like a fork bomb regardless of internal spacing.
fn is_fork_bomb(command: &str) -> bool {
    // Canonical form: `:(){:|:&};:`
    // Common variants insert spaces: `: () { : | : & }; :`
    let c = condensed(command);
    // Must start with a function definition that recurses through a pipe.
    c.contains("(){") && c.contains(":|:") && c.contains("&}") || c.contains(":(){")
    // already condensed version
}

// ─── mkfs / dd analysis ─────────────────────────────────────────────────────

/// True when the command writes to a block device via `dd`.
fn is_dangerous_dd(args: &[String]) -> bool {
    // dd if=<file> of=<device>  — `of=` pointing to /dev/ is dangerous.
    args.iter().any(|a| {
        a.starts_with("of=")
            && (a.contains("/dev/sd")
                || a.contains("/dev/hd")
                || a.contains("/dev/nvme")
                || a.contains("/dev/disk")
                || a.contains("/dev/mapper/"))
    })
}

// ─── chmod analysis ──────────────────────────────────────────────────────────

/// True for `chmod 777 /` or `chmod -R 777 /` style destructive ops.
fn is_dangerous_chmod(args: &[String]) -> bool {
    if args.len() < 2 {
        return false;
    }
    let rest = &args[1..];

    // Collect non-flag args (mode and path).
    let positional: Vec<&str> = rest
        .iter()
        .filter(|a| !a.starts_with('-'))
        .map(String::as_str)
        .collect();

    if positional.len() < 2 {
        return false;
    }

    // Check for world-writable mode applied to root.
    let mode = positional[0];
    let path = positional[positional.len() - 1];

    let is_world_write = mode == "777" || mode == "a+w" || mode == "o+w";
    let targets_root = path == "/" || path == "/*";

    is_world_write && targets_root
}

// ─── Public API ─────────────────────────────────────────────────────────────

/// Check if a command looks dangerous.
///
/// Uses parse-tree analysis to resist common whitespace/flag bypass attempts.
pub fn is_dangerous(command: &str) -> bool {
    if is_fork_bomb(command) {
        return true;
    }

    // Tokenise for structured analysis.
    let args = parser::parse_args(command);
    if args.is_empty() {
        return false;
    }

    let cmd = args[0].to_ascii_lowercase();

    // If the command is prefixed with `sudo`, strip it and re-evaluate the
    // remainder so that `sudo rm -rf /` is caught the same as `rm -rf /`.
    if cmd == "sudo" {
        if args.len() < 2 {
            return false;
        }
        // Re-evaluate from the first non-sudo token.
        let rest_command = command.trim_start().trim_start_matches("sudo").trim_start();
        return is_dangerous(rest_command);
    }

    match cmd.as_str() {
        "rm" => is_dangerous_rm(&args, command),
        "mkfs" | "mkfs.ext4" | "mkfs.xfs" | "mkfs.btrfs" => true,
        "dd" => is_dangerous_dd(&args),
        "chmod" => is_dangerous_chmod(&args),
        // Redirection to a raw disk device: `> /dev/sda`
        _ => {
            // Detect `anything > /dev/sd*` via raw string (redirects are
            // parsed separately from args; simple substring check is OK here
            // because the target is a fixed device path prefix, not a pattern).
            let lower = command.to_ascii_lowercase();
            lower.contains("> /dev/sd")
                || lower.contains("> /dev/hd")
                || lower.contains("> /dev/nvme")
        }
    }
}

/// Check if an AI-generated command should get extra confirmation.
pub fn needs_extra_confirmation(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();

    // Use word-boundary checks for "rm" to avoid false positives on commands
    // like `arm` whose lowercase form contains the substring "rm ".
    let has_rm = lower == "rm"
        || lower.starts_with("rm ")
        || lower.contains(" rm ")
        || lower.contains("\trm ");

    lower.contains("sudo")
        || has_rm
        || lower.contains("mv /")
        || lower.contains("chmod")
        || lower.contains("chown")
        || is_dangerous(command)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── rm ───────────────────────────────────────────────────────────────────

    #[test]
    fn test_rm_rf_root_caught() {
        assert!(is_dangerous("rm -rf /"));
    }

    #[test]
    fn test_rm_double_space_caught() {
        // Bypass attempt: double space between flags and target.
        assert!(is_dangerous("rm  -rf /"));
    }

    #[test]
    fn test_rm_split_flags_caught() {
        // Bypass attempt: -r and -f as separate tokens.
        assert!(is_dangerous("rm -r -f /"));
    }

    #[test]
    fn test_rm_long_flags_caught() {
        assert!(is_dangerous("rm --recursive --force /"));
    }

    #[test]
    fn test_rm_rf_tilde_caught() {
        assert!(is_dangerous("rm -rf ~"));
    }

    #[test]
    fn test_rm_rf_star_caught() {
        assert!(is_dangerous("rm -rf *"));
    }

    #[test]
    fn test_rm_r_only_not_dangerous() {
        // Without -f it's annoying but not catastrophic — do not block.
        assert!(!is_dangerous("rm -r /tmp/foo"));
    }

    #[test]
    fn test_rm_rf_subdir_not_dangerous() {
        // Legitimate: rm -rf /tmp/build
        assert!(!is_dangerous("rm -rf /tmp/build"));
    }

    // ── fork bomb ────────────────────────────────────────────────────────────

    #[test]
    fn test_fork_bomb_compact_caught() {
        assert!(is_dangerous(":(){:|:&};:"));
    }

    #[test]
    fn test_fork_bomb_spaced_caught() {
        // Bypass attempt: spaces inserted.
        assert!(is_dangerous(": () { : | : & }; :"));
    }

    // ── mkfs ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_mkfs_caught() {
        assert!(is_dangerous("mkfs /dev/sda"));
    }

    #[test]
    fn test_mkfs_ext4_caught() {
        assert!(is_dangerous("mkfs.ext4 /dev/sdb1"));
    }

    // ── dd ───────────────────────────────────────────────────────────────────

    #[test]
    fn test_dd_to_disk_caught() {
        assert!(is_dangerous("dd if=/dev/zero of=/dev/sda"));
    }

    #[test]
    fn test_dd_to_file_safe() {
        assert!(!is_dangerous(
            "dd if=/dev/urandom of=/tmp/rand.bin bs=1M count=10"
        ));
    }

    // ── chmod ────────────────────────────────────────────────────────────────

    #[test]
    fn test_chmod_777_root_caught() {
        assert!(is_dangerous("chmod 777 /"));
    }

    #[test]
    fn test_chmod_r_777_root_caught() {
        assert!(is_dangerous("chmod -R 777 /"));
    }

    #[test]
    fn test_chmod_777_subdir_safe() {
        assert!(!is_dangerous("chmod 777 /tmp/mydir"));
    }

    // ── needs_extra_confirmation ─────────────────────────────────────────────

    #[test]
    fn test_sudo_needs_extra_confirmation() {
        assert!(needs_extra_confirmation("sudo rm -rf /tmp/foo"));
    }

    #[test]
    fn test_rm_needs_extra_confirmation() {
        assert!(needs_extra_confirmation("rm -f myfile.txt"));
    }

    // ── additional rm edge cases ─────────────────────────────────────────────

    #[test]
    fn test_rm_rf_slash_star_caught() {
        // rm -rf /* is just as dangerous as rm -rf /
        assert!(is_dangerous("rm -rf /*"));
    }

    #[test]
    fn test_rm_rf_home_root_not_dangerous() {
        // rm -rf ~/project is legitimate; home subdirs are not root targets.
        assert!(!is_dangerous("rm -rf ~/project"));
    }

    #[test]
    fn test_rm_rf_with_leading_sudo() {
        // sudo rm -rf / must still be caught even though the first token is sudo.
        assert!(is_dangerous("sudo rm -rf /"));
    }

    #[test]
    fn test_rm_rf_tmp_subdir_safe() {
        // A specific directory under /tmp is never a dangerous root target.
        assert!(!is_dangerous("rm -rf /tmp/my-test-build-dir"));
    }

    // ── dd edge cases ────────────────────────────────────────────────────────

    #[test]
    fn test_dd_to_nvme_caught() {
        assert!(is_dangerous("dd if=/dev/urandom of=/dev/nvme0n1"));
    }

    #[test]
    fn test_dd_to_mapper_caught() {
        assert!(is_dangerous("dd if=/dev/zero of=/dev/mapper/root"));
    }

    // ── fork bomb edge cases ─────────────────────────────────────────────────

    #[test]
    fn test_fork_bomb_no_semicolon_caught() {
        // Without the trailing semicolon it's still recognisable.
        assert!(is_dangerous(":(){:|:&}"));
    }

    // ── chmod edge cases ─────────────────────────────────────────────────────

    #[test]
    fn test_chmod_777_empty_path_not_dangerous() {
        // chmod 777 with no path arguments is not dangerous (it will error).
        assert!(!is_dangerous("chmod 777"));
    }

    // ── needs_extra_confirmation edge cases ───────────────────────────────────

    #[test]
    fn test_chmod_recursive_needs_extra() {
        assert!(needs_extra_confirmation("chmod -R 755 /srv/www"));
    }

    #[test]
    fn test_chown_recursive_needs_extra() {
        assert!(needs_extra_confirmation("chown -R root /etc"));
    }

    #[test]
    fn test_plain_ls_no_extra() {
        assert!(!needs_extra_confirmation("ls -la /tmp"));
    }
}
