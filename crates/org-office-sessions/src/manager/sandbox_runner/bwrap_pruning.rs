//! Linux Claude `bwrap` absent-path deny pruning and concrete directory normalization (`TASK-144`,
//! `TASK-163`, `TASK-190`), co-located inside the `sandbox_runner` local confinement adapter.

use org_agent_runtime::{
    PathPattern, PermissionMode, PermissionPolicy, RuleScope, RuntimeHostProfile, SandboxSpec,
    SessionSpec,
};

pub(super) fn is_linux_bwrap_sandbox(sandbox_enabled: bool, profile: RuntimeHostProfile) -> bool {
    sandbox_enabled
        && profile.uses_mount_mask_sandbox
        && !org_platform::confinement::unix_socket_path_filter_supported()
}

pub(crate) fn apply_linux_claude_bwrap_pruning(spec: &mut SessionSpec) {
    // Keep `preset_cfg.policy.deny` 100% complete in `SessionManager` (enforced by
    // `decide_escalation_with_roots` over `--permission-mode manual` on every tool call),
    // while pruning absent concrete paths from Linux Claude's `spec.sandbox` and
    // `spec.policy.deny` so Claude 2.1.x `@anthropic-ai/sandbox-runtime` (`bwrap`), which
    // merges `permissions.deny` into `bwrap` mounts, never materializes permanent 0-byte
    // `0444` files on the host (`TASK-144`).
    if prune_nonexistent_bwrap_denies(&mut spec.sandbox, &mut spec.policy) {
        // Pin approval-bridge mediation explicitly whenever any policy deny rule is pruned:
        // never rely on `capabilities().enforces_deny_rules` or surviving deny count to clear
        // `permissions.allow` or downgrade `AcceptEdits -> Ask`.
        spec.policy.allow.clear();
        if spec.permission_mode == PermissionMode::AcceptEdits {
            spec.permission_mode = PermissionMode::Ask;
        }
    }
}

/// `raw` without its trailing directory glob: any run of `/**` (a malformed `dir/**/**`
/// included, `TASK-190`), or else one `/*`.
fn strip_dir_glob_suffix(raw: &str) -> &str {
    let mut stripped = raw;
    while let Some(shorter) = stripped.strip_suffix("/**") {
        stripped = shorter;
    }
    if stripped.len() == raw.len() {
        return raw.strip_suffix("/*").unwrap_or(raw);
    }
    stripped
}

pub(crate) fn pattern_exists_for_bwrap(pat: &PathPattern) -> bool {
    let raw = pat.as_str();
    let stripped = strip_dir_glob_suffix(raw);
    let prefix = if stripped.is_empty() { "/" } else { stripped };
    if prefix.contains('*') || prefix.contains('?') || prefix.contains('[') {
        return true;
    }
    std::fs::symlink_metadata(prefix).is_ok()
}

/// Normalizes a concrete existing directory glob (`<dir>/**` or `<dir>/*` with no internal
/// wildcards) into `<dir>` for Linux Claude `bwrap` so `@anthropic-ai/sandbox-runtime` mounts
/// `--tmpfs <dir>` atomically at the directory boundary instead of traversing inside `<dir>` and
/// following symlinks (such as a sibling agent's `.cargo/registry` or `.rustup`, `TASK-163`).
fn normalize_bwrap_concrete_dir_pattern(pat: &PathPattern) -> (PathPattern, bool) {
    let raw = pat.as_str();
    let stripped = strip_dir_glob_suffix(raw);
    if stripped.len() == raw.len()
        || stripped.is_empty()
        || stripped.contains('*')
        || stripped.contains('?')
        || stripped.contains('[')
    {
        return (pat.clone(), false);
    }
    if std::fs::symlink_metadata(stripped).is_ok_and(|m| m.file_type().is_dir()) {
        if let Ok(normalized) = PathPattern::try_from(stripped) {
            return (normalized, true);
        }
    }
    (pat.clone(), false)
}

/// Drops non-existent concrete paths from Linux Claude `bwrap` `sandbox.deny_read`,
/// `sandbox.deny_write`, and `policy.deny` `RuleScope::Path` entries (`TASK-144`), and normalizes
/// concrete existing directory globs (`dir/**` -> `dir`) on `deny_read`, `deny_write` and
/// `policy.deny` so `@anthropic-ai/sandbox-runtime` never traverses symlinks inside denied
/// sibling agent homes (`TASK-163`, `TASK-190`). Returns `true` if any `policy.deny` rule was
/// pruned or normalized. Callers must leave `preset_cfg.policy.deny` untouched so
/// `SessionManager`'s approval bridge (`decide_escalation_with_roots`) retains 100% coverage of
/// absent/future and glob paths.
pub(crate) fn prune_nonexistent_bwrap_denies(
    sandbox: &mut SandboxSpec,
    policy: &mut PermissionPolicy,
) -> bool {
    sandbox.deny_read.retain(pattern_exists_for_bwrap);
    for pat in &mut sandbox.deny_read {
        let (norm, _) = normalize_bwrap_concrete_dir_pattern(pat);
        *pat = norm;
    }
    sandbox.deny_write.retain(pattern_exists_for_bwrap);
    for pat in &mut sandbox.deny_write {
        let (norm, _) = normalize_bwrap_concrete_dir_pattern(pat);
        *pat = norm;
    }
    let before = policy.deny.len();
    policy.deny.retain(|rule| match &rule.scope {
        RuleScope::Path(pat) => pattern_exists_for_bwrap(pat),
        _ => true,
    });
    let mut modified = policy.deny.len() < before;
    for rule in &mut policy.deny {
        if let RuleScope::Path(pat) = &mut rule.scope {
            let (norm, changed) = normalize_bwrap_concrete_dir_pattern(pat);
            if changed {
                *pat = norm;
                modified = true;
            }
        }
    }
    modified
}
