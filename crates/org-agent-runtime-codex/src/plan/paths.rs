//! Whether one path is the same as or under another, as Codex's sandbox sees them: on macOS
//! `/var`, `/tmp` and `/etc` are symlinks into `/private`, so a write grant and a deny can
//! overlap through an alias that the literal paths don't show.

fn canonicalize_os_alias(path: &str, is_macos: bool) -> std::borrow::Cow<'_, str> {
    if !is_macos {
        return std::borrow::Cow::Borrowed(path);
    }
    for (alias_prefix, private_prefix) in [
        ("/var", "/private/var"),
        ("/tmp", "/private/tmp"),
        ("/etc", "/private/etc"),
    ] {
        if path == alias_prefix {
            return std::borrow::Cow::Borrowed(private_prefix);
        }
        if let Some(rest) = path.strip_prefix(alias_prefix) {
            if rest.starts_with('/') {
                return std::borrow::Cow::Owned(format!("{private_prefix}{rest}"));
            }
        }
    }
    std::borrow::Cow::Borrowed(path)
}

fn path_is_same_or_under_literal(path: &str, base: &str) -> bool {
    let p = path.trim_end_matches('/');
    let b = base.trim_end_matches('/');
    if b.is_empty() {
        return true;
    }
    p == b || p.starts_with(&format!("{b}/"))
}

fn path_is_same_or_under_for_os(path: &str, base: &str, is_macos: bool) -> bool {
    let p = canonicalize_os_alias(path.trim_end_matches('/'), is_macos);
    let b = canonicalize_os_alias(base.trim_end_matches('/'), is_macos);
    if b.is_empty() {
        return true;
    }
    p.as_ref() == b.as_ref() || p.starts_with(&format!("{b}/"))
}

pub(super) fn path_is_same_or_under(path: &str, base: &str) -> bool {
    path_is_same_or_under_for_os(
        path,
        base,
        org_platform::fs::has_macos_private_root_aliases(),
    )
}

fn paths_overlap_via_alias_only_for_os(path: &str, base: &str, is_macos: bool) -> bool {
    if !is_macos {
        return false;
    }
    let same_or_under = path_is_same_or_under_for_os(path, base, true)
        || path_is_same_or_under_for_os(base, path, true);
    let literal =
        path_is_same_or_under_literal(path, base) || path_is_same_or_under_literal(base, path);
    same_or_under && !literal
}

pub(super) fn paths_overlap_via_alias_only(path: &str, base: &str) -> bool {
    paths_overlap_via_alias_only_for_os(
        path,
        base,
        org_platform::fs::has_macos_private_root_aliases(),
    )
}

#[cfg(test)]
mod tests {
    use super::{path_is_same_or_under_for_os, paths_overlap_via_alias_only_for_os};

    #[test]
    fn linux_keeps_var_and_private_var_disjoint_while_macos_detects_alias_overlap() {
        // On Linux, `/var/lib/agent/work` and `/private/var/lib/agent` are distinct directories.
        assert!(!path_is_same_or_under_for_os(
            "/var/lib/agent/work",
            "/private/var/lib/agent",
            false
        ));
        assert!(!paths_overlap_via_alias_only_for_os(
            "/var/lib/agent/work",
            "/private/var/lib/agent",
            false
        ));
        assert!(!path_is_same_or_under_for_os(
            "/var/nested",
            "/private",
            false
        ));

        // On macOS, `/var` aliases `/private/var`, so both containment and alias overlap fire.
        assert!(path_is_same_or_under_for_os(
            "/var/lib/agent/work",
            "/private/var/lib/agent",
            true
        ));
        assert!(paths_overlap_via_alias_only_for_os(
            "/var/lib/agent/work",
            "/private/var/lib/agent",
            true
        ));
        assert!(path_is_same_or_under_for_os(
            "/var/nested",
            "/private",
            true
        ));
        assert!(path_is_same_or_under_for_os(
            "/private/var/tmp/run-1",
            "/private",
            true
        ));
    }
}
