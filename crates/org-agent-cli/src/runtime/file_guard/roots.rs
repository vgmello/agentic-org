//! Allowed root discovery and confinement (`FileRoots`, forbidden-root screening, and
//! untrusted-env defense; `TASK-98.4`, `TASK-252`, ADR-0037).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use org_command::Refusal;

use super::denylist::{check_other_agent_for_id, has_denied_static_segment};
use super::paths::{
    agent_id_from_path, denied_path, is_under_host_user_home, lexical_normalize,
    lowercase_components, path_eq, strip_root_prefix,
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
    /// - Derives `self_agent_id` from `cwd` (or office-scoped `HOME`) before trusting ambient
    ///   `AGENT_OS_AGENT_ID`, so spoofing `AGENT_OS_AGENT_ID` cannot bypass `other_agent_home`.
    /// - Never admits direct host `HOME` (`/Users/<user>`, `/home/<user>`, `/root`) or arbitrary
    ///   subdirectories of host user homes as `private_home` or `session_tmpdir`.
    /// - Requires bidirectional `.git` back-pointers and managed-boundary confinement for any
    ///   linked Git worktree discovered from `.git/worktrees/*/gitdir`.
    pub(crate) fn from_parts(
        cwd: PathBuf,
        self_agent_id: Option<String>,
        home: Option<PathBuf>,
        tmpdir: Option<PathBuf>,
    ) -> Self {
        let env_agent_id = sanitize_agent_id(self_agent_id);
        let cwd_entry = RootEntry::from_path(cwd.clone());
        let resolved_agent_id = effective_self_agent_id(
            &cwd,
            cwd_entry.as_ref(),
            home.as_deref(),
            env_agent_id.clone(),
        );

        let mut workspace_roots = Vec::new();
        let mut worktrees = Vec::new();
        if let Some(cwd_entry) = cwd_entry {
            if let Some(repo_root) = find_git_root(&cwd_entry.canonical) {
                if let Some(repo_entry) = RootEntry::from_path(repo_root) {
                    for wt in
                        discover_git_worktrees(&repo_entry.canonical, resolved_agent_id.as_deref())
                    {
                        push_unique(&mut worktrees, wt);
                    }
                    push_unique(&mut workspace_roots, repo_entry);
                }
            }
            push_unique(&mut workspace_roots, cwd_entry);
        }

        let private_home = home.filter(|p| !p.as_os_str().is_empty()).and_then(|h| {
            admit_agent_home(h, resolved_agent_id.as_deref(), env_agent_id.as_deref())
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

/// Rejects `/`, global temp directories (including macOS `/var/folders/.../T` roots), direct host
/// user homes (`/Users/<user>`, `/home/<user>`, `/root`), top-level mount/container directories
/// (`/opt`, `/Volumes`, `/mnt`, `/Users`, `/home`), and OS/system trees so no root candidate can
/// widen confinement beyond a real project/session directory.
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
            | ["var", "folders"]
            | ["private", "var", "folders"]
            | ["var", "folders", _]
            | ["private", "var", "folders", _]
            | ["var", "folders", _, _]
            | ["private", "var", "folders", _, _]
            | ["var", "folders", _, _, "t"]
            | ["private", "var", "folders", _, _, "t"]
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

fn effective_self_agent_id(
    cwd_raw: &Path,
    cwd_entry: Option<&RootEntry>,
    home: Option<&Path>,
    env_agent_id: Option<String>,
) -> Option<String> {
    if let Some(id) = cwd_entry
        .and_then(|e| agent_id_from_path(&e.canonical).or_else(|| agent_id_from_path(&e.raw)))
        .or_else(|| agent_id_from_path(&lexical_normalize(cwd_raw)))
    {
        return Some(id);
    }
    if let Some(home_path) = home {
        let home_canon = fs::canonicalize(home_path).ok();
        if let Some(id) = home_canon
            .as_deref()
            .and_then(agent_id_from_path)
            .or_else(|| agent_id_from_path(&lexical_normalize(home_path)))
        {
            return Some(id);
        }
    }
    env_agent_id
}

pub(super) fn is_office_scoped_agent_home(path: &Path, self_agent_id: &str) -> bool {
    agent_id_from_path(path).is_some_and(|id| id.eq_ignore_ascii_case(self_agent_id))
}

pub(super) fn host_home_policy_allows_agent_home(path: &Path, self_agent_id: &str) -> bool {
    if is_forbidden_root(path) || has_denied_static_segment(path) {
        return false;
    }
    if is_under_host_user_home(path) {
        return is_office_scoped_agent_home(path, self_agent_id);
    }
    true
}

fn admit_agent_home(
    home: PathBuf,
    self_agent_id: Option<&str>,
    env_agent_id: Option<&str>,
) -> Option<RootEntry> {
    let self_id = self_agent_id?;
    let entry = RootEntry::from_path(home)?;
    check_other_agent_for_id(&entry.raw, Some(self_id), None).ok()?;
    check_other_agent_for_id(&entry.canonical, Some(self_id), None).ok()?;

    // If ambient `AGENT_OS_AGENT_ID` disagreed with the authoritative agent ID from `cwd`/`HOME`,
    // only admit `HOME` when `HOME` itself is demonstrably `self_id`'s office-scoped home.
    let env_mismatch = env_agent_id.is_some_and(|env_id| !env_id.eq_ignore_ascii_case(self_id));
    let office_scoped = is_office_scoped_agent_home(&entry.canonical, self_id)
        || is_office_scoped_agent_home(&entry.raw, self_id);
    if env_mismatch && !office_scoped {
        return None;
    }

    // Never admit arbitrary directories inside host user homes (`/Users/<user>/...`,
    // `/home/<user>/...`, `/root/...`) unless demonstrably an office-scoped agent home.
    if !host_home_policy_allows_agent_home(&entry.raw, self_id)
        || !host_home_policy_allows_agent_home(&entry.canonical, self_id)
    {
        return None;
    }
    Some(entry)
}

pub(super) fn host_home_policy_allows_tmpdir(path: &Path, self_agent_id: Option<&str>) -> bool {
    if is_forbidden_root(path) || has_denied_static_segment(path) {
        return false;
    }
    if is_under_host_user_home(path) {
        return is_dedicated_temp_subdir_in_user_home(path, self_agent_id);
    }
    true
}

fn is_dedicated_temp_subdir_in_user_home(path: &Path, self_agent_id: Option<&str>) -> bool {
    if self_agent_id.is_some_and(|id| is_office_scoped_agent_home(path, id)) {
        return true;
    }
    let comps = lowercase_components(path);
    comps.iter().any(|seg| {
        matches!(
            seg.as_str(),
            "agentic-org" | "session-tmp" | "tmp" | "temp" | "t"
        )
    })
}

fn admit_session_tmpdir(tmpdir: PathBuf, self_agent_id: Option<&str>) -> Option<RootEntry> {
    let entry = RootEntry::from_path(tmpdir)?;
    check_other_agent_for_id(&entry.raw, self_agent_id, None).ok()?;
    check_other_agent_for_id(&entry.canonical, self_agent_id, None).ok()?;

    if !host_home_policy_allows_tmpdir(&entry.raw, self_agent_id)
        || !host_home_policy_allows_tmpdir(&entry.canonical, self_agent_id)
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
