//! Deny-path and writable-root path resolution for escalation decisions (lexical and
//! canonical, symlink-aware).

use org_agent_runtime::SpecPath;

pub(super) fn push_unique_path(out: &mut Vec<String>, path: Option<std::path::PathBuf>) {
    let Some(s) = path.as_deref().and_then(std::path::Path::to_str) else {
        return;
    };
    if !out.iter().any(|existing| existing == s) {
        out.push(s.to_owned());
    }
}

pub(super) fn build_deny_strings(
    raw_strings: &[String],
    concrete_targets: &[String],
) -> Vec<String> {
    let mut deny_strings = raw_strings.to_vec();
    for target in concrete_targets {
        let p = std::path::Path::new(target);
        push_unique_path(&mut deny_strings, resolve_path_for_root_check(p));
        push_unique_path(&mut deny_strings, resolve_path_for_deny(p));
        if p.is_absolute() {
            push_unique_path(&mut deny_strings, p.canonicalize().ok());
            push_unique_path(&mut deny_strings, lexical_normalize_abs(p));
        }
    }
    deny_strings
}

pub(super) fn push_deny_components(
    path: &std::path::Path,
    queue: &mut std::collections::VecDeque<std::ffi::OsString>,
    prepend: bool,
) {
    let mut parts: Vec<std::ffi::OsString> = Vec::new();
    for comp in path.components() {
        match comp {
            std::path::Component::RootDir => parts.push(std::path::MAIN_SEPARATOR_STR.into()),
            std::path::Component::Prefix(pref) => parts.push(pref.as_os_str().to_os_string()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => parts.push("..".into()),
            std::path::Component::Normal(seg) => parts.push(seg.to_os_string()),
        }
    }
    if prepend {
        for part in parts.into_iter().rev() {
            queue.push_front(part);
        }
    } else {
        queue.extend(parts);
    }
}

/// Bounded component-by-component path resolver used strictly for deny matching (`matches_any_deny`).
/// Follows both live and dangling symlinks via `read_link` and collapses `..` while canonicalizing
/// existing prefixes so unsandboxed (`writable_roots = None`) sessions still block writes through
/// dangling symlinks or symlink+`..` traversals into denied targets.
pub(super) fn resolve_path_for_deny(path: &std::path::Path) -> Option<std::path::PathBuf> {
    const MAX_SYMLINK_HOPS: usize = 40;
    if !path.is_absolute() {
        return None;
    }
    let mut queue = std::collections::VecDeque::new();
    push_deny_components(path, &mut queue, false);
    let mut cur = std::path::PathBuf::new();
    let mut hops = 0usize;
    while let Some(seg) = queue.pop_front() {
        if seg == std::path::MAIN_SEPARATOR_STR {
            cur = std::path::PathBuf::from(std::path::MAIN_SEPARATOR_STR);
            continue;
        }
        if seg == ".." {
            if cur.parent().is_some() {
                cur.pop();
            }
            continue;
        }
        let next = cur.join(&seg);
        if let Ok(target) = std::fs::read_link(&next) {
            hops += 1;
            if hops > MAX_SYMLINK_HOPS {
                return None;
            }
            if target.is_absolute() {
                cur = std::path::PathBuf::from(std::path::MAIN_SEPARATOR_STR);
            }
            push_deny_components(&target, &mut queue, true);
            continue;
        }
        cur = next.canonicalize().unwrap_or(next);
    }
    cur.is_absolute().then_some(cur)
}

pub(super) fn lexical_normalize_abs(path: &std::path::Path) -> Option<std::path::PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut out = std::path::PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::RootDir => out.push(std::path::MAIN_SEPARATOR_STR),
            std::path::Component::Prefix(pref) => out.push(pref.as_os_str()),
            std::path::Component::Normal(seg) => out.push(seg),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if out.parent().is_some() {
                    out.pop();
                }
            }
        }
    }
    Some(out)
}

pub(super) fn all_targets_within_writable_roots(targets: &[String], roots: &[SpecPath]) -> bool {
    if targets.is_empty() || roots.is_empty() {
        return false;
    }
    let canon_roots: Vec<std::path::PathBuf> = roots
        .iter()
        .filter_map(|r| resolve_path_for_root_check(std::path::Path::new(r.as_str())))
        // Never accept filesystem root `/` as a writable root.
        .filter(|canon| canon.parent().is_some())
        .collect();
    if canon_roots.is_empty() {
        return false;
    }
    targets
        .iter()
        .all(|t| target_within_canonical_roots(t, &canon_roots))
}

pub(super) fn target_within_canonical_roots(raw: &str, canon_roots: &[std::path::PathBuf]) -> bool {
    let Some(resolved) = resolve_path_for_root_check(std::path::Path::new(raw)) else {
        return false;
    };
    canon_roots.iter().any(|root| resolved.starts_with(root))
}

pub(super) fn normalize_strict_abs_path(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let raw = path.to_str()?;
    if raw.is_empty() || raw.contains('\0') {
        return None;
    }
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return None;
    }
    // Rebuilding from `components()` strips trailing `/`, `/.`, and duplicate separators so
    // POSIX `lstat` (`symlink_metadata`) never dereferences a trailing-slash dangling symlink.
    let clean: std::path::PathBuf = path.components().collect();
    clean.is_absolute().then_some(clean)
}

pub(super) fn resolve_path_for_root_check(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let clean = normalize_strict_abs_path(path)?;
    if let Ok(canon) = clean.canonicalize() {
        return Some(canon);
    }
    let mut tail = Vec::new();
    let mut cur: &std::path::Path = &clean;
    while let Some(parent) = cur.parent() {
        // Fail closed on any symlink (leaf or intermediate) or non-NotFound metadata error
        // (ELOOP, EACCES, ENOTDIR, InvalidInput) so dangling symlinks cannot escape roots.
        match std::fs::symlink_metadata(cur) {
            Ok(meta) if meta.file_type().is_symlink() => return None,
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return None,
        }
        if let Some(name) = cur.file_name() {
            tail.push(name.to_os_string());
        }
        if let Ok(mut base) = parent.canonicalize() {
            for seg in tail.into_iter().rev() {
                base.push(seg);
            }
            return Some(base);
        }
        cur = parent;
    }
    None
}

pub(super) fn path_matches_glob_deny(candidate: &str, pattern: &str) -> bool {
    if org_platform::fs::paths_case_insensitive_by_default() {
        path_matches_glob(&candidate.to_lowercase(), &pattern.to_lowercase())
    } else {
        path_matches_glob(candidate, pattern)
    }
}

pub(super) fn path_matches_glob(candidate: &str, pattern: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix("/**") {
        let with_slash = format!("{prefix}/");
        return candidate == prefix
            || candidate.starts_with(&with_slash)
            || candidate.contains(&with_slash)
            || candidate.contains(prefix);
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return candidate.starts_with(prefix) || candidate.contains(prefix);
    }
    candidate == pattern || candidate.contains(pattern)
}
