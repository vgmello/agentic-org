//! Bidirectional Git worktree discovery and managed-boundary confinement (`TASK-98.4`,
//! `TASK-252`, ADR-0037).
//!
//! Never admits a forged `.git` file or `.git/worktrees/<name>/gitdir` pointer:
//! 1. When `repo_root/.git` is a regular file (`gitdir: <target>`), `<target>/gitdir` must be a
//!    real regular file pointing back to `<repo_root>/.git` before following `commondir` or
//!    scanning sibling worktrees.
//! 2. `<common_dir>/worktrees/<name>/gitdir` is opened via `org_platform::fs::open_regular_beneath`
//!    to eliminate parent-directory TOCTOU, and `<wt>/.git` must be a real regular file whose
//!    `gitdir:` back-pointer canonicalizes to `<common_dir>/worktrees/<name>`.
//! 3. Every Git admin directory and candidate worktree root must pass `is_forbidden_root`, static
//!    credential/office denylists, and other-agent isolation checks, and stay within the managed
//!    workspace/worktree boundary.

use std::ffi::OsStr;
use std::fs;
use std::io::{ErrorKind, Read as _};
use std::path::{Path, PathBuf};

use super::denylist::{check_other_agent_for_id, has_denied_static_segment};
use super::paths::{
    agent_id_from_path, agent_managed_ancestor, lexical_normalize, path_eq, path_is_within,
};
use super::roots::{is_forbidden_root, push_unique, RootEntry};

const MAX_GIT_POINTER_BYTES: u64 = 8 * 1024;

pub(super) fn find_git_root(start: &Path) -> Option<PathBuf> {
    let agent_boundary =
        agent_id_from_path(start).and_then(|id| agent_managed_ancestor(start, &id));
    for ancestor in start.ancestors() {
        if is_forbidden_root(ancestor) || has_denied_static_segment(ancestor) {
            break;
        }
        if exceeds_agent_boundary(ancestor, agent_boundary.as_deref()) {
            break;
        }
        if has_real_dot_git(ancestor) {
            return Some(ancestor.to_path_buf());
        }
        if is_agents_child_dir(ancestor) {
            break;
        }
    }
    None
}

fn exceeds_agent_boundary(ancestor: &Path, boundary: Option<&Path>) -> bool {
    boundary.is_some_and(|b| path_eq(ancestor, b) || !path_is_within(ancestor, b))
}

fn is_agents_child_dir(dir: &Path) -> bool {
    dir.parent()
        .and_then(Path::file_name)
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.eq_ignore_ascii_case("agents"))
}

fn has_real_dot_git(dir: &Path) -> bool {
    let Ok(meta) = fs::symlink_metadata(dir.join(".git")) else {
        return false;
    };
    let ft = meta.file_type();
    ft.is_dir() || ft.is_file()
}

pub(super) fn discover_git_worktrees(
    repo_root: &Path,
    self_agent_id: Option<&str>,
) -> Vec<RootEntry> {
    let mut out = Vec::new();
    let Some(common_dir) = resolve_git_common_dir(repo_root, self_agent_id) else {
        return out;
    };
    let worktrees_dir = common_dir.join("worktrees");
    let Ok(wt_meta) = fs::symlink_metadata(&worktrees_dir) else {
        return out;
    };
    if !wt_meta.file_type().is_dir() {
        return out;
    }
    let Ok(entries) = fs::read_dir(&worktrees_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        if let Some(root) =
            verify_worktree_entry(&common_dir, &entry.file_name(), repo_root, self_agent_id)
        {
            push_unique(&mut out, root);
        }
    }
    out
}

fn read_small_git_text_beneath(root: &Path, relative: &Path) -> Option<String> {
    let file = org_platform::fs::open_regular_beneath(root, relative).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_GIT_POINTER_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if (bytes.len() as u64) > MAX_GIT_POINTER_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn parse_gitdir_pointer(text: &str) -> Option<&str> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("gitdir:"))
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn resolve_git_target(base: &Path, raw: &str) -> PathBuf {
    let target = Path::new(raw);
    let joined = if target.is_absolute() {
        target.to_path_buf()
    } else {
        base.join(target)
    };
    lexical_normalize(&joined)
}

fn validate_git_admin_dir(raw_path: &Path, self_agent_id: Option<&str>) -> Option<PathBuf> {
    let lex = lexical_normalize(raw_path);
    if is_forbidden_root(&lex)
        || has_denied_static_segment(&lex)
        || check_other_agent_for_id(&lex, self_agent_id, None).is_err()
    {
        return None;
    }
    let canon = fs::canonicalize(raw_path).ok()?;
    if !canon.is_dir()
        || is_forbidden_root(&canon)
        || has_denied_static_segment(&canon)
        || check_other_agent_for_id(&canon, self_agent_id, None).is_err()
    {
        return None;
    }
    Some(canon)
}

fn verify_admin_points_back_to_repo(gitdir: &Path, repo_root: &Path) -> bool {
    let Some(back_text) = read_small_git_text_beneath(gitdir, Path::new("gitdir")) else {
        return false;
    };
    let raw_back = back_text.trim();
    if raw_back.is_empty() {
        return false;
    }
    let target = resolve_git_target(gitdir, raw_back);
    if target.file_name() != Some(OsStr::new(".git")) {
        return false;
    }
    target
        .parent()
        .and_then(|parent| fs::canonicalize(parent).ok())
        .is_some_and(|parent_canon| path_eq(&parent_canon, repo_root))
}

fn resolve_commondir(gitdir: &Path, self_agent_id: Option<&str>) -> Option<PathBuf> {
    let commondir_path = gitdir.join("commondir");
    match fs::symlink_metadata(&commondir_path) {
        Err(err) if err.kind() == ErrorKind::NotFound => return Some(gitdir.to_path_buf()),
        Err(_) => return None,
        Ok(meta) if !meta.file_type().is_file() => return None,
        Ok(_) => {}
    }
    let common_text = read_small_git_text_beneath(gitdir, Path::new("commondir"))?;
    let raw_common = common_text.trim();
    if raw_common.is_empty() {
        return None;
    }
    let common = resolve_git_target(gitdir, raw_common);
    validate_git_admin_dir(&common, self_agent_id)
}

fn resolve_git_common_dir(repo_root: &Path, self_agent_id: Option<&str>) -> Option<PathBuf> {
    let dot_git = repo_root.join(".git");
    let meta = fs::symlink_metadata(&dot_git).ok()?;
    if meta.file_type().is_dir() {
        return validate_git_admin_dir(&dot_git, self_agent_id);
    }
    if !meta.file_type().is_file() {
        return None;
    }
    let text = read_small_git_text_beneath(repo_root, Path::new(".git"))?;
    let raw_gitdir = parse_gitdir_pointer(&text)?;
    let gitdir_raw = resolve_git_target(repo_root, raw_gitdir);
    let gitdir = validate_git_admin_dir(&gitdir_raw, self_agent_id)?;
    if !verify_admin_points_back_to_repo(&gitdir, repo_root) {
        return None;
    }
    resolve_commondir(&gitdir, self_agent_id)
}

fn verify_worktree_back_pointer(wt_canon: &Path, expected_admin_canon: &Path) -> bool {
    let Some(text) = read_small_git_text_beneath(wt_canon, Path::new(".git")) else {
        return false;
    };
    let Some(raw_back) = parse_gitdir_pointer(&text) else {
        return false;
    };
    let resolved = resolve_git_target(wt_canon, raw_back);
    fs::canonicalize(resolved)
        .ok()
        .is_some_and(|back_canon| back_canon.is_dir() && path_eq(&back_canon, expected_admin_canon))
}

fn is_valid_boundary_root(path: &Path) -> bool {
    !is_forbidden_root(path) && !has_denied_static_segment(path)
}

fn worktree_within_managed_boundary(
    wt_canon: &Path,
    repo_canon: &Path,
    self_agent_id: Option<&str>,
) -> bool {
    if path_is_within(wt_canon, repo_canon) {
        return true;
    }
    if repo_canon
        .parent()
        .is_some_and(|p| is_valid_boundary_root(p) && path_is_within(wt_canon, p))
    {
        return true;
    }
    self_agent_id
        .and_then(|id| agent_managed_ancestor(repo_canon, id))
        .is_some_and(|root| is_valid_boundary_root(&root) && path_is_within(wt_canon, &root))
}

fn read_worktree_target(common_dir: &Path, entry_name: &OsStr) -> Option<(PathBuf, PathBuf)> {
    let rel_gitdir = Path::new("worktrees").join(entry_name).join("gitdir");
    let text = read_small_git_text_beneath(common_dir, &rel_gitdir)?;
    let raw_target = text.trim();
    if raw_target.is_empty() {
        return None;
    }
    let admin_entry = common_dir.join("worktrees").join(entry_name);
    let admin_meta = fs::symlink_metadata(&admin_entry).ok()?;
    if !admin_meta.file_type().is_dir() {
        return None;
    }
    let admin_canon = fs::canonicalize(&admin_entry).ok()?;
    if !path_is_within(&admin_canon, common_dir) {
        return None;
    }
    let target = resolve_git_target(&admin_canon, raw_target);
    if target.file_name() != Some(OsStr::new(".git")) {
        return None;
    }
    Some((admin_canon, target.parent()?.to_path_buf()))
}

fn verify_worktree_entry(
    common_dir: &Path,
    entry_name: &OsStr,
    repo_root: &Path,
    self_agent_id: Option<&str>,
) -> Option<RootEntry> {
    let (admin_canon, wt_root) = read_worktree_target(common_dir, entry_name)?;
    let root = RootEntry::from_path(wt_root)?;
    check_other_agent_for_id(&root.raw, self_agent_id, None).ok()?;
    check_other_agent_for_id(&root.canonical, self_agent_id, None).ok()?;
    if !verify_worktree_back_pointer(&root.canonical, &admin_canon) {
        return None;
    }
    if !worktree_within_managed_boundary(&root.canonical, repo_root, self_agent_id) {
        return None;
    }
    Some(root)
}
