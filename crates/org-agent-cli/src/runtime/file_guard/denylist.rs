//! Unconditional path denylist for office internals, host credential dotdirs/files, and other
//! agents' homes (`TASK-98.4`, `TASK-252`, ADR-0037).

use std::path::Path;

use org_command::Refusal;

use super::paths::{
    denied_path, lowercase_components, normalized_components, office_agent_segment, path_is_within,
};
use super::roots::RootEntry;

/// Host credential dotdirs/files and sensitive office/host state filenames that are ALWAYS
/// blocked anywhere in a path, even inside an allowed root or with an allowlisted extension
/// (for example `.codex/auth.json`, `.gemini/oauth_creds.json`, `.docker/config.json`).
const DENIED_HOST_SECRET_COMPONENTS: &[&str] = &[
    ".ssh",
    ".aws",
    ".claude",
    ".codex",
    ".gemini",
    ".docker",
    ".kube",
    ".gnupg",
    ".netrc",
    ".npmrc",
    ".pypirc",
    ".git-credentials",
    "host-secrets",
    "git-credentials",
    "git-state.json",
    "hq-owner.v1.json",
];

fn is_office_or_host_db(lower_seg: &str) -> bool {
    lower_seg.starts_with("office.db") || lower_seg.starts_with("host.db")
}

fn is_session_binding_filename(lower_seg: &str) -> bool {
    lower_seg == "session-binding.v1"
        || (lower_seg.starts_with("session-binding-") && lower_seg.ends_with(".v1"))
        || (lower_seg.starts_with(".session-binding.") && lower_seg.ends_with(".tmp"))
}

fn check_denied_host_secret_segment(
    comps: &[String],
    idx: usize,
    seg: &str,
) -> Result<(), Refusal> {
    let lower = seg.to_ascii_lowercase();
    if DENIED_HOST_SECRET_COMPONENTS.contains(&lower.as_str())
        || is_office_or_host_db(&lower)
        || is_session_binding_filename(&lower)
    {
        return Err(denied_path(
            "Access to sensitive host credentials, provider auth state, or office/host databases is forbidden.",
            "Attach a regular workspace or non-sensitive file instead.",
        ));
    }
    if lower == ".config"
        && comps
            .get(idx + 1)
            .is_some_and(|next| next.eq_ignore_ascii_case("gh"))
    {
        return Err(denied_path(
            "Access to GitHub CLI credentials (`.config/gh`) is forbidden.",
            "Attach a regular workspace or non-sensitive file instead.",
        ));
    }
    Ok(())
}

fn check_denied_office_segment(comps: &[String], idx: usize, seg: &str) -> Result<(), Refusal> {
    let eq = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    if eq(seg, "attachment-staging") || eq(seg, "blob-outbox") {
        return Err(denied_path(
            "Access to office attachment staging or outbox paths is forbidden.",
            "Attach a regular user or workspace file instead.",
        ));
    }
    if eq(seg, "durable")
        && comps
            .get(idx + 1)
            .is_some_and(|n| eq(n, "blobs") || eq(n, "private") || eq(n, "messaging"))
    {
        return Err(denied_path(
            "Direct access to durable office storage (`durable/blobs`, `durable/private`, `durable/messaging`) is forbidden.",
            "Reference existing attachments by --attachment-id or attach a workspace/private_home file instead.",
        ));
    }
    if eq(seg, "private")
        && comps.get(idx + 1).is_some_and(|leaf| {
            let lower = leaf.to_ascii_lowercase();
            lower.starts_with("task-notes.db") || lower.starts_with("user-space.db")
        })
    {
        return Err(denied_path(
            "Direct access to private SQLite databases (`task-notes.db*`, `user-space.db*`) is forbidden.",
            "Attach a workspace or own private_home file instead.",
        ));
    }
    if eq(seg, "runtime") && comps.get(idx + 2).is_some_and(|c| eq(c, "protected")) {
        return Err(denied_path(
            "Direct access to runtime/<office>/protected/ is forbidden.",
            "Attach a workspace or own private_home file instead.",
        ));
    }
    Ok(())
}

/// Returns `true` when `path` contains any statically forbidden host credential or office-internal
/// path segment (independent of caller agent identity).
pub(super) fn has_denied_static_segment(path: &Path) -> bool {
    let comps = lowercase_components(path);
    for (idx, seg) in comps.iter().enumerate() {
        if check_denied_host_secret_segment(&comps, idx, seg).is_err()
            || check_denied_office_segment(&comps, idx, seg).is_err()
        {
            return true;
        }
    }
    false
}

fn is_own_agent_home_prefix(
    path: &Path,
    comps: &[String],
    prefix_len: usize,
    private_home: Option<&RootEntry>,
) -> bool {
    let Some(home) = private_home else {
        return false;
    };
    let matches_root = |root_path: &Path| {
        if !path_is_within(path, root_path) {
            return false;
        }
        let h_comps = normalized_components(root_path);
        h_comps.len() >= prefix_len
            && comps[..prefix_len]
                .iter()
                .zip(&h_comps[..prefix_len])
                .all(|(a, b)| a.eq_ignore_ascii_case(b))
    };
    matches_root(&home.canonical) || matches_root(&home.raw)
}

/// Refuses `path` if it targets another agent's home, worktrees, workspace, or tmp directory
/// for `self_agent_id`.
pub(super) fn check_other_agent_for_id(
    path: &Path,
    self_agent_id: Option<&str>,
    private_home: Option<&RootEntry>,
) -> Result<(), Refusal> {
    let comps = normalized_components(path);
    for (idx, seg) in comps.iter().enumerate() {
        if let Some((agent_seg, prefix_len)) = office_agent_segment(&comps, idx, seg) {
            let is_own_id = self_agent_id.is_some_and(|id| agent_seg.eq_ignore_ascii_case(id));
            if !is_own_id && !is_own_agent_home_prefix(path, &comps, prefix_len, private_home) {
                return Err(denied_path(
                    "Access to another agent's private_home or workspace is forbidden.",
                    "Attach files only from your own workspace, worktrees, private_home, or session TMPDIR.",
                ));
            }
        }
    }
    Ok(())
}

/// Checks `path` against all unconditional office, host-credential, and other-agent denylists.
pub(super) fn check_denylisted(
    path: &Path,
    self_agent_id: Option<&str>,
    private_home: Option<&RootEntry>,
) -> Result<(), Refusal> {
    let comps = normalized_components(path);
    for (idx, seg) in comps.iter().enumerate() {
        check_denied_host_secret_segment(&comps, idx, seg)?;
        check_denied_office_segment(&comps, idx, seg)?;
    }
    check_other_agent_for_id(path, self_agent_id, private_home)
}
