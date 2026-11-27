//! Bidirectional Git worktree discovery and managed-boundary confinement (`TASK-98.4`,
//! `TASK-252`, ADR-0037).
//!
//! Never admits a forged `.git/worktrees/<name>/gitdir` pointer:
//! 1. `<wt>/.git` must be a real regular file (not a symlink) whose `gitdir:` back-pointer
//!    canonicalizes to the exact `<common_dir>/worktrees/<name>` directory.
//! 2. The candidate worktree root must pass `is_forbidden_root`, static credential/office
//!    denylists, and other-agent-home checks.
//! 3. The candidate worktree root must stay within the managed workspace/worktree boundary
//!    (inside `repo_root`, under a non-forbidden parent of `repo_root`, or inside the agent's
//!    own `.../agents/<self_id>/` subtree).

use std::ffi::OsStr;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use super::denylist::{check_other_agent_for_id, has_denied_static_segment};
use super::paths::{agent_managed_ancestor, lexical_normalize, path_eq, path_is_within};
use super::roots::{is_forbidden_root, push_unique, RootEntry};

const MAX_GIT_POINTER_BYTES: u64 = 8 * 1024;

pub(super) fn find_git_root(start: &Path) -> Option<PathBuf> {
    for ancestor in start.ancestors() {
        if is_forbidden_root(ancestor) {
            break;
        }
        if ancestor.join(".git").exists() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

pub(super) fn discover_git_worktrees(
    repo_root: &Path,
    self_agent_id: Option<&str>,
) -> Vec<RootEntry> {
    let mut out = Vec::new();
    let Some(common_dir) = resolve_git_common_dir(repo_root) else {
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
        if let Some(root) = verify_worktree_entry(&entry.path(), repo_root, self_agent_id) {
            push_unique(&mut out, root);
        }
    }
    out
}

fn read_small_git_text(path: &Path) -> Option<String> {
    let file = org_platform::fs::open_nofollow_ro(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() {
        return None;
    }
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

fn resolve_git_common_dir(repo_root: &Path) -> Option<PathBuf> {
    let dot_git = repo_root.join(".git");
    let meta = fs::symlink_metadata(&dot_git).ok()?;
    if meta.file_type().is_dir() {
        return fs::canonicalize(&dot_git).ok();
    }
    if !meta.file_type().is_file() {
        return None;
    }
    let text = read_small_git_text(&dot_git)?;
    let raw_gitdir = parse_gitdir_pointer(&text)?;
    let gitdir_path = Path::new(raw_gitdir);
    let gitdir = if gitdir_path.is_absolute() {
        gitdir_path.to_path_buf()
    } else {
        repo_root.join(gitdir_path)
    };
    let gitdir = fs::canonicalize(gitdir).ok()?;
    if !gitdir.is_dir() || is_forbidden_root(&gitdir) {
        return None;
    }
    let commondir_file = gitdir.join("commondir");
    if let Some(common_text) = read_small_git_text(&commondir_file) {
        let raw_common = common_text.trim();
        if !raw_common.is_empty() {
            let common_path = Path::new(raw_common);
            let common = if common_path.is_absolute() {
                common_path.to_path_buf()
            } else {
                gitdir.join(common_path)
            };
            if let Ok(canonical_common) = fs::canonicalize(common) {
                if canonical_common.is_dir() && !is_forbidden_root(&canonical_common) {
                    return Some(canonical_common);
                }
            }
        }
    }
    Some(gitdir)
}

fn verify_worktree_back_pointer(wt_canon: &Path, expected_admin_canon: &Path) -> bool {
    let dot_git = wt_canon.join(".git");
    let Ok(meta) = fs::symlink_metadata(&dot_git) else {
        return false;
    };
    if !meta.file_type().is_file() {
        return false;
    }
    let Some(text) = read_small_git_text(&dot_git) else {
        return false;
    };
    let Some(raw_back) = parse_gitdir_pointer(&text) else {
        return false;
    };
    let back_path = Path::new(raw_back);
    let resolved = if back_path.is_absolute() {
        back_path.to_path_buf()
    } else {
        wt_canon.join(back_path)
    };
    let Ok(back_canon) = fs::canonicalize(resolved) else {
        return false;
    };
    back_canon.is_dir() && path_eq(&back_canon, expected_admin_canon)
}

fn worktree_within_managed_boundary(
    wt_canon: &Path,
    repo_canon: &Path,
    self_agent_id: Option<&str>,
) -> bool {
    if path_is_within(wt_canon, repo_canon) {
        return true;
    }
    if let Some(repo_parent) = repo_canon.parent() {
        if !is_forbidden_root(repo_parent)
            && !has_denied_static_segment(repo_parent)
            && path_is_within(wt_canon, repo_parent)
        {
            return true;
        }
    }
    if let Some(self_id) = self_agent_id {
        if let Some(agent_root) = agent_managed_ancestor(repo_canon, self_id) {
            if !is_forbidden_root(&agent_root)
                && !has_denied_static_segment(&agent_root)
                && path_is_within(wt_canon, &agent_root)
            {
                return true;
            }
        }
    }
    false
}

fn verify_worktree_entry(
    admin_entry: &Path,
    repo_root: &Path,
    self_agent_id: Option<&str>,
) -> Option<RootEntry> {
    let admin_meta = fs::symlink_metadata(admin_entry).ok()?;
    if !admin_meta.file_type().is_dir() {
        return None;
    }
    let admin_canon = fs::canonicalize(admin_entry).ok()?;
    let gitdir_file = admin_entry.join("gitdir");
    let text = read_small_git_text(&gitdir_file)?;
    let raw_target = text.trim();
    if raw_target.is_empty() {
        return None;
    }
    let target_path = Path::new(raw_target);
    let target = lexical_normalize(&if target_path.is_absolute() {
        target_path.to_path_buf()
    } else {
        admin_entry.join(target_path)
    });
    if target.file_name() != Some(OsStr::new(".git")) {
        return None;
    }
    let wt_root = target.parent()?;
    let root = RootEntry::from_path(wt_root.to_path_buf())?;
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
