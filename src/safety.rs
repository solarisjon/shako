/// Safety layer — detect dangerous commands before execution.
const DANGEROUS_PATTERNS: &[&str] = &[
    "rm -rf /",
    "rm -rf ~",
    "rm -rf *",
    "mkfs",
    "dd if=",
    ":(){",
    "chmod 777 /",
    "chmod -R 777",
    "> /dev/sda",
    "fork bomb",
];

/// Check if a command looks dangerous.
pub fn is_dangerous(command: &str) -> bool {
    let lower = command.to_lowercase();
    DANGEROUS_PATTERNS.iter().any(|p| lower.contains(p))
}

/// Check if an AI-generated command should get extra confirmation.
pub fn needs_extra_confirmation(command: &str) -> bool {
    let lower = command.to_lowercase();
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
