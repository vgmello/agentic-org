//! Allowed root discovery and confinement (`FileRoots`, forbidden-root screening, and
//! untrusted-env defense; `TASK-98.4`, `TASK-252`, ADR-0037).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use org_command::Refusal;

use super::denylist::{check_other_agent_for_id, has_denied_static_segment};
use super::paths::{
    agent_id_from_path, denied_path, is_under_host_user_home, lexical_normalize,
    lowercase_components, path_eq, raw_normal_components, strip_root_prefix,
};
use super::worktrees::{discover_git_worktrees, find_git_root};

/// One allowed root directory, carrying both its pre-canonicalized path (for matching paths
/// containing OS ancestor symlinks such as macOS `/var -> /private/var` without dereferencing
/// symlinks inside the root) and its canonical path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RootEntry {
    pub raw: PathBuf,
    pub canonical: PathBuf,
}

impl RootEntry {
    pub(crate) fn from_path(path: PathBuf) -> Option<Self> {
        let raw = lexical_normalize(&path);
        if is_forbidden_root(&raw) || has_denied_static_segment(&raw) {
            return None;
        }
        let canonical = fs::canonicalize(&path).ok()?;
        if !canonical.is_dir()
            || is_forbidden_root(&canonical)
            || has_denied_static_segment(&canonical)
        {
            return None;
        }
        Some(Self { raw, canonical })
    }
}

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
    ///   `session_tmpdir` (own `.../agents/<id>/tmp`, office runtime `.../t/<slug>`, or
    ///   dedicated subdirectory under OS temp roots).
    /// - Requires bidirectional `.git` back-pointers and managed-boundary confinement for any
    ///   linked Git worktree discovered from `.git/worktrees/*/gitdir`.
    pub(crate) fn from_parts(
        cwd: PathBuf,
        self_agent_id: Option<String>,
        home: Option<PathBuf>,
        tmpdir: Option<PathBuf>,
    ) -> Self {
        let env_id = sanitize_agent_id(self_agent_id);
        let cwd_entry = RootEntry::from_path(cwd.clone());
        let cwd_id = agent_id_from_cwd(&cwd, cwd_entry.as_ref());
        let home_opt = home.filter(|p| !p.as_os_str().is_empty());
        let home_id = home_opt.as_deref().and_then(agent_id_from_home);
        let resolved_agent_id =
            resolve_self_agent_id(cwd_id.as_deref(), home_id.as_deref(), env_id.as_deref());

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

        let has_authoritative_identity = cwd_id.is_some() || env_id.is_some();
        let private_home = home_opt.and_then(|h| {
            admit_agent_home(h, resolved_agent_id.as_deref(), has_authoritative_identity)
        });

        let session_tmpdir = tmpdir
            .filter(|p| !p.as_os_str().is_empty())
            .and_then(|t| admit_session_tmpdir(t, resolved_agent_id.as_deref()));

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
        let home = env::var_os("HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        let tmpdir = env::var_os("TMPDIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        Self::from_parts(cwd, self_agent_id, home, tmpdir)
    }

    pub(super) fn all_roots(&self) -> impl Iterator<Item = &RootEntry> {
        self.workspace_roots
            .iter()
            .chain(self.private_home.iter())
            .chain(self.worktrees.iter())
            .chain(self.session_tmpdir.iter())
    }
}

/// Rejects `/`, global temp directories (including macOS `/var/folders/...` buckets and `C`/`0`
/// trees), direct host user homes (`/Users/<user>`, `/home/<user>`, `/root`, `/var/root`,
/// `/private/var/root`), top-level mount/container directories (`/opt`, `/Volumes`, `/mnt`,
/// `/Users`, `/home`), and OS/system trees so no root candidate can widen confinement beyond a
/// real project/session directory.
pub(crate) fn is_forbidden_root(path: &Path) -> bool {
    let norm = lexical_normalize(path);
    if !norm.is_absolute() {
        return false;
    }
    let comps = lowercase_components(&norm);
    let refs: Vec<&str> = comps.iter().map(String::as_str).collect();
    matches!(
        refs.as_slice(),
        [] | ["tmp"]
            | ["private"]
            | ["private", "tmp"]
            | ["var"]
            | ["private", "var"]
            | ["var", "tmp"]
            | ["private", "var", "tmp"]
            | ["var", "root", ..]
            | ["private", "var", "root", ..]
            | ["var", "folders"]
            | ["private", "var", "folders"]
            | ["var", "folders", _]
            | ["private", "var", "folders", _]
            | ["var", "folders", _, _]
            | ["private", "var", "folders", _, _]
            | ["var", "folders", _, _, _]
            | ["private", "var", "folders", _, _, _]
            | ["var", "folders", _, _, "c" | "0", ..]
            | ["private", "var", "folders", _, _, "c" | "0", ..]
            | ["run"]
            | ["run", "user"]
            | ["run", "user", _]
            | ["opt"]
            | ["volumes"]
            | ["mnt"]
            | ["root"]
            | ["home"]
            | ["home", _]
            | ["users"]
            | ["users", _]
            | ["etc", ..]
            | ["private", "etc", ..]
            | ["usr", ..]
            | ["bin", ..]
            | ["sbin", ..]
            | ["lib", ..]
            | ["lib64", ..]
            | ["system", ..]
            | ["library", ..]
            | ["applications", ..]
            | ["proc", ..]
            | ["sys", ..]
            | ["dev", ..]
    )
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

pub(super) fn push_unique(list: &mut Vec<RootEntry>, entry: RootEntry) {
    if !list.iter().any(|e| path_eq(&e.canonical, &entry.canonical)) {
        list.push(entry);
    }
}

fn sanitize_agent_id(raw: Option<String>) -> Option<String> {
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

fn resolve_self_agent_id(
    cwd_id: Option<&str>,
    home_id: Option<&str>,
    env_id: Option<&str>,
) -> Option<String> {
    if let Some(id) = cwd_id {
        return Some(id.to_owned());
    }
    if let (Some(h_id), Some(e_id)) = (home_id, env_id) {
        if !h_id.eq_ignore_ascii_case(e_id) {
            return None;
        }
    }
    env_id.or(home_id).map(str::to_owned)
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

fn admit_agent_home(
    home: PathBuf,
    self_agent_id: Option<&str>,
    has_authoritative_identity: bool,
) -> Option<RootEntry> {
    if !has_authoritative_identity {
        return None;
    }
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

fn is_office_runtime_session_tmpdir(comps: &[String]) -> bool {
    let Some(anchor_idx) = comps
        .iter()
        .position(|seg| seg == "agentic-org" || seg == "ao")
    else {
        return false;
    };
    comps
        .iter()
        .enumerate()
        .skip(anchor_idx + 1)
        .any(|(t_idx, seg)| {
            t_idx >= 2 && seg == "t" && comps.get(t_idx + 1).is_some_and(|slug| !slug.is_empty())
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

pub(super) fn is_allowed_session_tmpdir_path(path: &Path, self_agent_id: Option<&str>) -> bool {
    let norm = lexical_normalize(path);
    if !norm.is_absolute() || is_forbidden_root(&norm) || has_denied_static_segment(&norm) {
        return false;
    }
    if check_other_agent_for_id(&norm, self_agent_id, None).is_err() {
        return false;
    }
    let comps = lowercase_components(&norm);
    is_own_agent_tmpdir(&comps, self_agent_id)
        || is_office_runtime_session_tmpdir(&comps)
        || is_os_temp_subdir(&norm, &comps)
}

fn admit_session_tmpdir(tmpdir: PathBuf, self_agent_id: Option<&str>) -> Option<RootEntry> {
    let entry = RootEntry::from_path(tmpdir)?;
    if !is_allowed_session_tmpdir_path(&entry.raw, self_agent_id)
        || !is_allowed_session_tmpdir_path(&entry.canonical, self_agent_id)
    {
        return None;
    }
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
