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
    lower.contains("sudo")
        || lower.contains("rm ")
        || lower.contains("mv /")
        || lower.contains("chmod")
        || lower.contains("chown")
        || is_dangerous(command)
}
