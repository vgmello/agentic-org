//! Pure path normalization, case-aware prefix matching, and agent-path extraction helpers
//! shared across `file_guard` submodules.

use std::path::{Component, Path, PathBuf};

use org_command::Refusal;
use org_proto::ErrorCode;

pub(super) fn resolve_abs(cwd: &Path, raw: &str) -> PathBuf {
    let p = Path::new(raw);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    }
}

/// Resolves `.` and `..` lexically without touching the filesystem or dereferencing symlinks.
pub(super) fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Normal path segments preserving original case.
pub(super) fn raw_normal_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(seg) => Some(seg.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// Normal path segments ASCII-lowercased regardless of host OS case sensitivity.
pub(super) fn lowercase_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(seg) => Some(seg.to_string_lossy().to_ascii_lowercase()),
            _ => None,
        })
        .collect()
}

/// Normal path segments folded only when the host filesystem is case-insensitive by default.
pub(super) fn normalized_components(path: &Path) -> Vec<String> {
    if org_platform::fs::paths_case_insensitive_by_default() {
        lowercase_components(path)
    } else {
        raw_normal_components(path)
    }
}

pub(super) fn path_is_within(path: &Path, root: &Path) -> bool {
    if org_platform::fs::paths_case_insensitive_by_default() {
        let p = normalized_components(path);
        let r = normalized_components(root);
        p.len() >= r.len() && p[..r.len()] == r[..]
    } else {
        path.starts_with(root)
    }
}

pub(super) fn path_eq(a: &Path, b: &Path) -> bool {
    if org_platform::fs::paths_case_insensitive_by_default() {
        a.is_absolute() == b.is_absolute() && normalized_components(a) == normalized_components(b)
    } else {
        a == b
    }
}

pub(crate) fn strip_root_prefix_with_case(
    path: &Path,
    root: &Path,
    case_insensitive: bool,
) -> Option<PathBuf> {
    if !case_insensitive {
        return path.strip_prefix(root).ok().map(Path::to_path_buf);
    }
    let mut path_comps = path.components();
    for root_comp in root.components() {
        let path_comp = path_comps.next()?;
        match (path_comp, root_comp) {
            (Component::Normal(p), Component::Normal(r)) => {
                if !p
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&r.to_string_lossy())
                {
                    return None;
                }
            }
            (a, b) if a == b => {}
            _ => return None,
        }
    }
    Some(path_comps.collect())
}

pub(super) fn strip_root_prefix(path: &Path, root: &Path) -> Option<PathBuf> {
    strip_root_prefix_with_case(
        path,
        root,
        org_platform::fs::paths_case_insensitive_by_default(),
    )
}

/// Whether `path` lies at or beneath a standard host user home tree (`/Users/<user>`,
/// `/home/<user>`, or `/root`).
pub(super) fn is_under_host_user_home(path: &Path) -> bool {
    let norm = lexical_normalize(path);
    if !norm.is_absolute() {
        return false;
    }
    let comps = lowercase_components(&norm);
    let refs: Vec<&str> = comps.iter().map(String::as_str).collect();
    matches!(
        refs.as_slice(),
        ["users", _, ..] | ["home", _, ..] | ["root", ..]
    )
}

/// Extracts an office-scoped agent ID if `path` lies inside an office agent directory:
/// - `.../local/offices/<office>/agents/<id>/...`
/// - `.../teams/<team>/agents/<id>/...`
/// - `.../agents/<id>/(home|private_home|worktrees|workspace|tmp)/...`
pub(super) fn agent_id_from_path(path: &Path) -> Option<String> {
    let comps = raw_normal_components(path);
    for (idx, seg) in comps.iter().enumerate() {
        if let Some((id, _)) = office_agent_segment(&comps, idx, seg) {
            return Some(id.to_owned());
        }
    }
    None
}

/// Matches an office-managed `.../agents/<id>` layout at `idx`, returning `(agent_id, end_idx)`
/// where `end_idx` is the component index immediately after `<id>`.
pub(super) fn office_agent_segment<'a>(
    comps: &'a [String],
    idx: usize,
    seg: &str,
) -> Option<(&'a str, usize)> {
    let eq = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    if eq(seg, "local")
        && comps.get(idx + 1).is_some_and(|c| eq(c, "offices"))
        && comps.get(idx + 3).is_some_and(|c| eq(c, "agents"))
    {
        return comps.get(idx + 4).map(|id| (id.as_str(), idx + 5));
    }
    if eq(seg, "teams") && comps.get(idx + 2).is_some_and(|c| eq(c, "agents")) {
        return comps.get(idx + 3).map(|id| (id.as_str(), idx + 4));
    }
    if eq(seg, "agents")
        && comps.get(idx + 2).is_some_and(|c| {
            eq(c, "home")
                || eq(c, "private_home")
                || eq(c, "worktrees")
                || eq(c, "workspace")
                || eq(c, "tmp")
        })
    {
        return comps.get(idx + 1).map(|id| (id.as_str(), idx + 2));
    }
    None
}

/// Returns the ancestor directory ending at `.../agents/<agent_id>` when `path` resides under
/// that agent's office-managed tree.
pub(super) fn agent_managed_ancestor(path: &Path, agent_id: &str) -> Option<PathBuf> {
    let comps = raw_normal_components(path);
    let mut target_end = None;
    for (idx, seg) in comps.iter().enumerate() {
        if let Some((found_id, end_idx)) = office_agent_segment(&comps, idx, seg) {
            if found_id.eq_ignore_ascii_case(agent_id) {
                target_end = Some(end_idx);
                break;
            }
        }
    }
    let end_idx = target_end?;
    let mut out = PathBuf::new();
    let mut normal_seen = 0usize;
    for comp in path.components() {
        out.push(comp.as_os_str());
        if matches!(comp, Component::Normal(_)) {
            normal_seen += 1;
            if normal_seen == end_idx {
                return Some(out);
            }
        }
    }
    None
}

pub(super) fn denied_path(message: &str, hint: &str) -> Refusal {
    Refusal::new(ErrorCode::NotAuthorized, message, hint)
}
