//! Secret-free sandbox and policy denial classification for session telemetry (`TASK-136`).
//!
//! Only returns a boolean classification so raw runtime stderr, notices, prompts, or tool
//! arguments are never retained inside telemetry structures.

/// Substrings indicating a sandbox, confinement, or office policy denial in runtime notices
/// or turn error summaries. Checked case-insensitively; avoids matching benign status text
/// like `"sandbox enabled"`.
const SANDBOX_DENIAL_PATTERNS: &[&str] = &[
    "permission denied",
    "operation not permitted",
    "read-only file system",
    "denied by policy",
    "denied by office",
    "outside writable",
    "blocked by sandbox",
    "sandbox denied",
    "sandbox violation",
    "sandbox blocked",
    "sandbox_bypass",
    "policy violation",
    "bwrap:",
    "landlock",
    "seccomp",
    "seatbelt",
    "sandbox-exec:",
];

/// Returns `true` when `text` signals a sandbox, OS confinement, or policy denial.
/// Never stores or echoes `text`.
pub(crate) fn is_sandbox_denial_text(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    SANDBOX_DENIAL_PATTERNS
        .iter()
        .any(|needle| lower.contains(needle))
}
