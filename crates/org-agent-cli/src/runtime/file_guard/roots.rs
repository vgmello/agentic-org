//! Allowed root discovery and confinement (`FileRoots`, forbidden-root screening, and
//! untrusted-env defense; `TASK-98.4`, `TASK-252`, ADR-0037).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use org_command::Refusal;

use crate::runtime::binding::OFFICE_ENV;

use super::denylist::{
    check_other_agent_for_id, has_denied_static_segment, push_unique, RootEntry,
};
use super::paths::{
    agent_id_from_path, denied_path, is_forbidden_root, is_under_host_user_home, lexical_normalize,
    lowercase_components, office_id_from_path, raw_normal_components, strip_root_prefix,
};
use super::worktrees::{discover_git_worktrees, find_git_root};

/// Allowed local roots for `file upload`, `--attach`, and `file fetch --output`.
#[derive(Clone, Debug, Default)]
pub(crate) struct FileRoots {
    pub cwd: PathBuf,
    pub workspace_roots: Vec<RootEntry>,
    pub private_home: Option<RootEntry>,
    pub worktrees: Vec<RootEntry>,
    pub session_tmpdir: Option<RootEntry>,
    pub self_agent_id: Option<String>,
}

impl FileRoots {
    /// Builds allowed roots purely from explicit directory and environment inputs without reading
    /// ambient process environment variables.
    ///
    /// Security invariants (`TASK-98.4`, `TASK-252`):
    /// - Never honors untrusted env wideners (`AGENT_OS_WORKTREES`, `AGENT_OS_PRIVATE_HOME`).
    /// - Derives `self_agent_id` from `cwd` first; when `cwd` has no agent ID, fails closed
    ///   (`self_agent_id = None`, `private_home = None`) if `HOME` and `AGENT_OS_AGENT_ID`
    ///   disagree so neither ambient env var can impersonate another agent.
    /// - Strictly allowlists `private_home` (own `.../agents/<id>/(home|private_home)`) and
    ///   `session_tmpdir` (own `.../agents/<id>/tmp`, own office runtime `.../t/<own-slug>`, or
    ///   dedicated owner-only `0700` subdirectory under OS temp roots).
    /// - Requires bidirectional `.git` back-pointers and managed-boundary confinement for any
    ///   linked Git worktree discovered from `.git/worktrees/*/gitdir`.
    #[cfg(test)]
    pub(crate) fn from_parts(
        cwd: PathBuf,
        self_agent_id: Option<String>,
        home: Option<PathBuf>,
        tmpdir: Option<PathBuf>,
    ) -> Self {
        Self::from_parts_with_office(cwd, self_agent_id, None, home, tmpdir)
    }

    /// Like [`Self::from_parts`], also binding the caller's office ID (e.g. `AGENT_OS_OFFICE`)
    /// so office runtime `t/<slug>` `TMPDIR` admission validates the caller's own slug.
    pub(crate) fn from_parts_with_office(
        cwd: PathBuf,
        self_agent_id: Option<String>,
        office_id: Option<String>,
        home: Option<PathBuf>,
        tmpdir: Option<PathBuf>,
    ) -> Self {
        let env_id = sanitize_id(self_agent_id);
        let env_office = sanitize_id(office_id);
        let cwd_entry = RootEntry::from_path(cwd.clone());
        let cwd_id = agent_id_from_cwd(&cwd, cwd_entry.as_ref());
        let cwd_office = office_id_from_cwd(&cwd, cwd_entry.as_ref());
        let home_opt = home.filter(|p| !p.as_os_str().is_empty());
        let home_id = home_opt.as_deref().and_then(agent_id_from_home);
        let home_office = home_opt.as_deref().and_then(office_id_from_home);
        let resolved_agent_id =
            resolve_self_id(cwd_id.as_deref(), home_id.as_deref(), env_id.as_deref());
        let resolved_office_id = resolve_self_id(
            cwd_office.as_deref(),
            home_office.as_deref(),
            env_office.as_deref(),
        );

        let mut workspace_roots = Vec::new();
        let mut worktrees = Vec::new();
        if let Some(cwd_entry) = cwd_entry {
            collect_git_roots(
                &cwd_entry.canonical,
                resolved_agent_id.as_deref(),
                &mut workspace_roots,
                &mut worktrees,
            );
            push_unique(&mut workspace_roots, cwd_entry);
        }

        let private_home = home_opt.and_then(|h| admit_agent_home(h, resolved_agent_id.as_deref()));

        let session_tmpdir = tmpdir.filter(|p| !p.as_os_str().is_empty()).and_then(|t| {
            admit_session_tmpdir(
                t,
                resolved_agent_id.as_deref(),
                resolved_office_id.as_deref(),
            )
        });

        Self {
            cwd,
            workspace_roots,
            private_home,
            worktrees,
            session_tmpdir,
            self_agent_id: resolved_agent_id,
        }
    }

    /// Discovers allowed roots from the current process environment and working directory.
    pub(crate) fn from_env() -> Self {
        #[cfg(test)]
        if let Some(overridden) = TEST_ROOTS.with(|cell| cell.borrow().clone()) {
            return overridden;
        }
        let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let self_agent_id = env::var("AGENT_OS_AGENT_ID").ok();
        let office_id = env::var(OFFICE_ENV).ok();
        let home = env::var_os("HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        let tmpdir = env::var_os("TMPDIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        Self::from_parts_with_office(cwd, self_agent_id, office_id, home, tmpdir)
    }

    pub(super) fn all_roots(&self) -> impl Iterator<Item = &RootEntry> {
        self.workspace_roots
            .iter()
            .chain(self.private_home.iter())
            .chain(self.worktrees.iter())
            .chain(self.session_tmpdir.iter())
    }
}

pub(super) fn match_beneath_root(
    lexical: &Path,
    roots: &FileRoots,
) -> Result<(PathBuf, PathBuf), Refusal> {
    // Match `lexical` directly against an allowed root's canonical or raw path so neither an
    // intermediate symlink inside the root nor an outside symlink pointing into the root is
    // dereferenced before `openat(O_NOFOLLOW)`.
    for entry in roots.all_roots() {
        if let Some(rel) = strip_root_prefix(lexical, &entry.canonical) {
            if !rel.as_os_str().is_empty() {
                return Ok((entry.canonical.clone(), rel));
            }
        }
        if let Some(rel) = strip_root_prefix(lexical, &entry.raw) {
            if !rel.as_os_str().is_empty() {
                return Ok((entry.canonical.clone(), rel));
            }
        }
    }
    Err(denied_path(
        "File path is outside allowed attachment roots or traverses a symlink outside them.",
        "Pass a regular file directly inside the workspace root, your private_home, active worktrees, or session-scoped TMPDIR.",
    ))
}

fn sanitize_id(raw: Option<String>) -> Option<String> {
    let trimmed = raw?.trim().to_owned();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains(['/', '\\', '\0'])
    {
        return None;
    }
    Some(trimmed)
}

fn collect_git_roots(
    cwd_canon: &Path,
    self_agent_id: Option<&str>,
    workspace_roots: &mut Vec<RootEntry>,
    worktrees: &mut Vec<RootEntry>,
) {
    let Some(repo_root) = find_git_root(cwd_canon) else {
        return;
    };
    let Some(repo_entry) = RootEntry::from_path(repo_root) else {
        return;
    };
    for wt in discover_git_worktrees(&repo_entry.canonical, self_agent_id) {
        push_unique(worktrees, wt);
    }
    push_unique(workspace_roots, repo_entry);
}

fn agent_id_from_cwd(cwd_raw: &Path, cwd_entry: Option<&RootEntry>) -> Option<String> {
    cwd_entry
        .and_then(|e| agent_id_from_path(&e.canonical).or_else(|| agent_id_from_path(&e.raw)))
        .or_else(|| agent_id_from_path(&lexical_normalize(cwd_raw)))
}

fn agent_id_from_home(home_path: &Path) -> Option<String> {
    fs::canonicalize(home_path)
        .ok()
        .and_then(|canon| agent_id_from_path(&canon))
        .or_else(|| agent_id_from_path(&lexical_normalize(home_path)))
}

fn office_id_from_cwd(cwd_raw: &Path, cwd_entry: Option<&RootEntry>) -> Option<String> {
    cwd_entry
        .and_then(|e| office_id_from_path(&e.canonical).or_else(|| office_id_from_path(&e.raw)))
        .or_else(|| office_id_from_path(&lexical_normalize(cwd_raw)))
}

fn office_id_from_home(home_path: &Path) -> Option<String> {
    fs::canonicalize(home_path)
        .ok()
        .and_then(|canon| office_id_from_path(&canon))
        .or_else(|| office_id_from_path(&lexical_normalize(home_path)))
}

fn resolve_self_id(
    cwd_id: Option<&str>,
    home_id: Option<&str>,
    env_id: Option<&str>,
) -> Option<String> {
    if let Some(id) = cwd_id {
        return Some(id.to_owned());
    }
    let e_id = env_id?;
    if let Some(h_id) = home_id {
        if !h_id.eq_ignore_ascii_case(e_id) {
            return None;
        }
    }
    Some(e_id.to_owned())
}

pub(super) fn is_office_scoped_agent_home(path: &Path, self_agent_id: &str) -> bool {
    let norm = lexical_normalize(path);
    if !norm.is_absolute() {
        return false;
    }
    let comps = raw_normal_components(&norm);
    comps.windows(3).any(|w| {
        w[0].eq_ignore_ascii_case("agents")
            && w[1].eq_ignore_ascii_case(self_agent_id)
            && (w[2].eq_ignore_ascii_case("home") || w[2].eq_ignore_ascii_case("private_home"))
    })
}

pub(super) fn is_allowed_agent_home_path(path: &Path, self_agent_id: &str) -> bool {
    let norm = lexical_normalize(path);
    if !norm.is_absolute() || is_forbidden_root(&norm) || has_denied_static_segment(&norm) {
        return false;
    }
    if check_other_agent_for_id(&norm, Some(self_agent_id), None).is_err() {
        return false;
    }
    is_office_scoped_agent_home(&norm, self_agent_id)
}

fn admit_agent_home(home: PathBuf, self_agent_id: Option<&str>) -> Option<RootEntry> {
    let self_id = self_agent_id?;
    let entry = RootEntry::from_path(home)?;
    if !is_allowed_agent_home_path(&entry.raw, self_id)
        || !is_allowed_agent_home_path(&entry.canonical, self_id)
    {
        return None;
    }
    Some(entry)
}

fn is_own_agent_tmpdir(comps: &[String], self_agent_id: Option<&str>) -> bool {
    let Some(self_id) = self_agent_id else {
        return false;
    };
    comps
        .windows(3)
        .any(|w| w[0] == "agents" && w[1].eq_ignore_ascii_case(self_id) && w[2] == "tmp")
}

/// Computes the deterministic 8-hex-char `t/<slug>` directory name matching
/// `org_office_sessions::host_auth::agent_tmpdir_slug(office_id, agent_id)`.
pub(super) fn expected_agent_tmpdir_slug(office_id: &str, agent_id: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let input = format!("{office_id}:{agent_id}");
    let digest = ring::digest::digest(&ring::digest::SHA256, input.as_bytes());
    let mut out = String::with_capacity(8);
    for &b in &digest.as_ref()[..4] {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn office_runtime_anchor_idx(comps: &[String]) -> Option<usize> {
    comps
        .iter()
        .position(|seg| seg == "agentic-org" || seg == "ao")
}

fn extract_office_runtime_tmpdir_slug(comps: &[String], anchor_idx: usize) -> Option<&str> {
    comps
        .iter()
        .enumerate()
        .skip(anchor_idx + 1)
        .find_map(|(t_idx, seg)| {
            (t_idx >= 2 && seg == "t")
                .then(|| comps.get(t_idx + 1).map(String::as_str))
                .flatten()
                .filter(|slug| !slug.is_empty())
        })
}

fn is_os_temp_subdir(norm: &Path, comps: &[String]) -> bool {
    if is_under_host_user_home(norm) {
        return false;
    }
    let refs: Vec<&str> = comps.iter().map(String::as_str).collect();
    match refs.as_slice() {
        ["tmp", _, ..]
        | ["private", "tmp", _, ..]
        | ["var", "tmp", _, ..]
        | ["private", "var", "tmp", _, ..]
        | ["var", "folders", _, _, "t", _, ..]
        | ["private", "var", "folders", _, _, "t", _, ..] => true,
        ["run", "user", uid, _, ..] => !uid.is_empty() && uid.bytes().all(|b| b.is_ascii_digit()),
        _ => false,
    }
}

pub(super) fn is_allowed_session_tmpdir_path(
    path: &Path,
    self_agent_id: Option<&str>,
    office_id: Option<&str>,
) -> bool {
    let norm = lexical_normalize(path);
    if !norm.is_absolute() || is_forbidden_root(&norm) || has_denied_static_segment(&norm) {
        return false;
    }
    if check_other_agent_for_id(&norm, self_agent_id, None).is_err() {
        return false;
    }
    let comps = lowercase_components(&norm);
    if is_own_agent_tmpdir(&comps, self_agent_id) {
        return true;
    }
    // Any path beneath an office runtime root (`.../agentic-org/...` or `.../ao/...`) must
    // match the caller's own `t/<slug>` — never fall through to generic OS temp admission,
    // which would admit sibling agents' `t/<other-slug>` or parent `ao/<handle>` dirs.
    if let Some(anchor_idx) = office_runtime_anchor_idx(&comps) {
        let Some(slug) = extract_office_runtime_tmpdir_slug(&comps, anchor_idx) else {
            return false;
        };
        let (Some(oid), Some(aid)) = (office_id, self_agent_id) else {
            return false;
        };
        return slug == expected_agent_tmpdir_slug(oid, aid);
    }
    is_os_temp_subdir(&norm, &comps)
}

fn admit_session_tmpdir(
    tmpdir: PathBuf,
    self_agent_id: Option<&str>,
    office_id: Option<&str>,
) -> Option<RootEntry> {
    let entry = RootEntry::from_path(tmpdir)?;
    if !is_allowed_session_tmpdir_path(&entry.raw, self_agent_id, office_id)
        || !is_allowed_session_tmpdir_path(&entry.canonical, self_agent_id, office_id)
    {
        return None;
    }
    let uid = org_platform::fs::effective_uid().ok()?;
    org_platform::fs::verify_owner_only_dir(&entry.raw, uid).ok()?;
    org_platform::fs::verify_owner_only_dir(&entry.canonical, uid).ok()?;
    Some(entry)
}

#[cfg(test)]
std::thread_local! {
    static TEST_ROOTS: std::cell::RefCell<Option<FileRoots>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn with_test_roots<T>(roots: FileRoots, f: impl FnOnce() -> T) -> T {
    struct Reset(Option<FileRoots>);
    impl Drop for Reset {
        fn drop(&mut self) {
            TEST_ROOTS.with(|cell| {
                *cell.borrow_mut() = self.0.take();
            });
        }
    }
    let prev = TEST_ROOTS.with(|cell| cell.borrow_mut().replace(roots));
    let _guard = Reset(prev);
    f()
}
