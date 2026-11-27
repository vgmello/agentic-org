//! Two-layer local filesystem exfiltration defense for `--attach`, `file upload`, and
//! `file fetch --output` (`TASK-98.4`, ADR-0037).
//!
//! `org-office-server` never opens caller-supplied filesystem paths: `agent-os` opens and reads
//! local files under the caller's own OS account, then streams `<= 512 KiB` chunks over
//! `attachment.upload_*` RPCs. Before a single upload RPC opens, this module enforces:
//!
//! 1. **Canonicalized allowed-root confinement**: workspace root (`cwd` / git repo root), own
//!    `private_home`, own active worktrees, and session-scoped `TMPDIR` (NEVER global `/tmp`,
//!    `/private/tmp`, `/var/tmp`, or `/`).
//! 2. **Explicit denylist** (checked both lexically and post-canonicalization, even if nested or
//!    symlinked): `durable/blobs/`, `durable/private/task-notes.db*`,
//!    `durable/private/user-space.db*`, `attachment-staging/`, another agent's `private_home`,
//!    and `runtime/<office>/protected/`.
//! 3. **POSIX safe open via `org_platform::fs`**: component-wise `openat` with
//!    `O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC` and `fstat` `S_ISREG` verification (rejecting
//!    symlinks, FIFOs, devices, sockets, and directories with zero parent-symlink TOCTOU).
//! 4. **Bounded read + extension allowlist + magic-byte pre-check**: `<= 10 MiB` for images,
//!    `<= 25 MiB` for allowlisted documents, rejecting executables, HTML, SVG, and mismatched
//!    magic bytes BEFORE opening an upload stream.

use std::env;
use std::fs;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use org_command::Refusal;
use org_proto::ErrorCode;

use super::file_magic::{check_magic_matches, classify_filename};

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
        let canonical = fs::canonicalize(&path).ok()?;
        if !canonical.is_dir() {
            return None;
        }
        let raw = lexical_normalize(&path);
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
    /// Discovers allowed roots from the current process environment and working directory.
    pub(crate) fn from_env() -> Self {
        #[cfg(test)]
        if let Some(overridden) = TEST_ROOTS.with(|cell| cell.borrow().clone()) {
            return overridden;
        }
        let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let self_agent_id = env::var("AGENT_OS_AGENT_ID")
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());

        let mut workspace_roots = Vec::new();
        let mut worktrees = Vec::new();
        if let Some(cwd_entry) = RootEntry::from_path(cwd.clone()) {
            if let Some(repo_root) = find_git_root(&cwd_entry.canonical) {
                for wt in discover_git_worktrees(&repo_root) {
                    push_unique(&mut worktrees, wt);
                }
                if let Some(entry) = RootEntry::from_path(repo_root) {
                    push_unique(&mut workspace_roots, entry);
                }
            }
            push_unique(&mut workspace_roots, cwd_entry);
        }

        let private_home_env = env::var_os("AGENT_OS_PRIVATE_HOME")
            .filter(|v| !v.is_empty())
            .or_else(|| {
                self_agent_id
                    .as_ref()
                    .and_then(|_| env::var_os("HOME"))
                    .filter(|v| !v.is_empty())
            });
        let private_home = private_home_env
            .map(PathBuf::from)
            .and_then(RootEntry::from_path)
            .filter(|e| !is_global_root(&e.raw) && !is_global_root(&e.canonical));

        if let Some(raw_list) = env::var_os("AGENT_OS_WORKTREES") {
            for item in env::split_paths(&raw_list) {
                if let Some(entry) = RootEntry::from_path(item) {
                    if !is_global_root(&entry.canonical) {
                        push_unique(&mut worktrees, entry);
                    }
                }
            }
        }

        let session_tmpdir = env::var_os("TMPDIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .filter(|p| !is_global_temp_or_root(p))
            .and_then(RootEntry::from_path)
            .filter(|e| !is_global_temp_or_root(&e.raw) && !is_global_temp_or_root(&e.canonical));

        Self {
            cwd,
            workspace_roots,
            private_home,
            worktrees,
            session_tmpdir,
            self_agent_id,
        }
    }

    fn all_roots(&self) -> impl Iterator<Item = &RootEntry> {
        self.workspace_roots
            .iter()
            .chain(self.private_home.iter())
            .chain(self.worktrees.iter())
            .chain(self.session_tmpdir.iter())
    }
}

/// Validated local file ready to stream over `attachment.upload_*`.
#[derive(Clone, Debug)]
pub(crate) struct LocalUploadFile {
    pub filename: String,
    pub mime_type: &'static str,
    pub bytes: Vec<u8>,
}

/// Validated output path beneath an allowed root for `file fetch --output`.
#[derive(Clone, Debug)]
pub(crate) struct PreparedOutputPath {
    pub root: PathBuf,
    pub relative: PathBuf,
    pub display_path: PathBuf,
}

/// Validates and reads a local file for upload under `roots` BEFORE any upload RPC is opened.
pub(crate) fn read_upload_file(
    raw_path: &str,
    roots: &FileRoots,
) -> Result<LocalUploadFile, Refusal> {
    validate_raw_path(raw_path)?;
    let abs_raw = resolve_abs(&roots.cwd, raw_path);
    let lexical = lexical_normalize(&abs_raw);

    check_denylisted(&abs_raw, roots)?;
    check_denylisted(&lexical, roots)?;

    // Check symlink metadata before canonicalize so symlinks and non-regular files fail closed
    // without silently dereferencing outside or inside an allowed root.
    let lmeta = fs::symlink_metadata(&abs_raw).map_err(|err| map_open_err(raw_path, &err))?;
    if lmeta.file_type().is_symlink() {
        return Err(denied_path(
            "Symbolic links cannot be attached or uploaded.",
            "Pass a regular file directly inside an allowed workspace, private home, worktree, or session TMPDIR.",
        ));
    }

    let canonical = fs::canonicalize(&abs_raw).map_err(|err| map_open_err(raw_path, &err))?;
    check_denylisted(&canonical, roots)?;
    if !roots
        .all_roots()
        .any(|r| path_is_within(&canonical, &r.canonical))
    {
        return Err(denied_path(
            "File path is outside allowed attachment roots.",
            "Only regular files inside the workspace root, your own private_home, active worktrees, or session-scoped TMPDIR may be attached.",
        ));
    }

    let (root, relative) = match_beneath_root(&lexical, roots)?;
    let (filename, media) = classify_filename(&canonical)?;

    let file = org_platform::fs::open_regular_beneath(&root, &relative)
        .map_err(|err| map_open_err(raw_path, &err))?;
    let max_bytes = media.max_bytes();
    let mut bytes = Vec::new();
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| map_open_err(raw_path, &err))?;
    if bytes.is_empty() {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "Cannot upload an empty file.",
            "Pass a non-empty regular file.",
        ));
    }
    if (bytes.len() as u64) > max_bytes {
        return Err(Refusal::new(
            ErrorCode::TooLarge,
            format!(
                "File exceeds the {} MiB limit for `{}`.",
                max_bytes / (1024 * 1024),
                media.mime()
            ),
            "Choose a smaller file within the image (10 MiB) or document (25 MiB) limit.",
        ));
    }
    check_magic_matches(media, &bytes)?;

    Ok(LocalUploadFile {
        filename,
        mime_type: media.mime(),
        bytes,
    })
}

/// Validates `--output <path>` for `file fetch` BEFORE downloading chunks from the office.
pub(crate) fn prepare_output_path(
    raw_path: &str,
    roots: &FileRoots,
) -> Result<PreparedOutputPath, Refusal> {
    validate_raw_path(raw_path)?;
    let abs_raw = resolve_abs(&roots.cwd, raw_path);
    let lexical = lexical_normalize(&abs_raw);

    check_denylisted(&abs_raw, roots)?;
    check_denylisted(&lexical, roots)?;

    let Some(file_name) = lexical.file_name() else {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "Output path must name a file.",
            "Pass a file path such as `./downloaded.pdf`.",
        ));
    };
    let Some(parent_raw) = lexical.parent() else {
        return Err(denied_path(
            "Output path has no parent directory.",
            "Choose an output file inside an allowed writable directory.",
        ));
    };
    let canonical_parent = fs::canonicalize(parent_raw).map_err(|_| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            "Output parent directory does not exist.",
            "Create the parent directory inside an allowed root first, then retry.",
        )
    })?;
    if !canonical_parent.is_dir() {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "Output parent path is not a directory.",
            "Pass a file path inside an existing directory.",
        ));
    }
    let canonical_target = canonical_parent.join(file_name);
    check_denylisted(&canonical_parent, roots)?;
    check_denylisted(&canonical_target, roots)?;
    if !roots
        .all_roots()
        .any(|r| path_is_within(&canonical_parent, &r.canonical))
    {
        return Err(denied_path(
            "Output path is outside allowed writable roots.",
            "Write fetched files only inside the workspace root, your private_home, active worktrees, or session-scoped TMPDIR.",
        ));
    }
    if let Ok(meta) = fs::symlink_metadata(&lexical) {
        if meta.file_type().is_symlink() || !meta.file_type().is_file() {
            return Err(denied_path(
                "Output path must not be a symbolic link, directory, FIFO, or special file.",
                "Choose a regular file path inside an allowed writable root.",
            ));
        }
    }
    let (root, relative) = match_beneath_root(&lexical, roots)?;
    if let Some(rel_parent) = relative.parent() {
        let mut cursor = root.clone();
        for comp in rel_parent.components() {
            let Component::Normal(seg) = comp else {
                return Err(denied_path(
                    "Output path contains invalid directory traversal components.",
                    "Pass a plain file path inside an allowed writable root.",
                ));
            };
            cursor.push(seg);
            let meta = fs::symlink_metadata(&cursor).map_err(|_| {
                Refusal::new(
                    ErrorCode::InvalidRequest,
                    "Output parent directory does not exist.",
                    "Create the parent directory inside an allowed root first, then retry.",
                )
            })?;
            if meta.file_type().is_symlink() || !meta.file_type().is_dir() {
                return Err(denied_path(
                    "Output path traverses a symbolic link or non-directory component.",
                    "Write fetched files only through real directories inside an allowed writable root.",
                ));
            }
        }
    }
    Ok(PreparedOutputPath {
        root,
        relative,
        display_path: canonical_target,
    })
}

/// Writes verified bytes to a prepared output path via `org_platform::fs::write_regular_beneath`.
pub(crate) fn write_prepared_output(
    target: &PreparedOutputPath,
    bytes: &[u8],
) -> Result<(), Refusal> {
    org_platform::fs::write_regular_beneath(&target.root, &target.relative, bytes).map_err(|err| {
        Refusal::new(
            ErrorCode::NotAuthorized,
            format!("Could not safely write output file: {err}"),
            "Ensure the output path is a regular file (never a symlink, FIFO, or directory) inside an allowed writable root.",
        )
    })
}

fn validate_raw_path(raw: &str) -> Result<(), Refusal> {
    if raw.is_empty() || raw.contains('\0') {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "File path must be non-empty and contain no NUL bytes.",
            "Pass a valid path to a regular file.",
        ));
    }
    if raw.ends_with('/') {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "File path must not end with `/`.",
            "Pass a regular file path, not a directory.",
        ));
    }
    Ok(())
}

fn resolve_abs(cwd: &Path, raw: &str) -> PathBuf {
    let p = Path::new(raw);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    }
}

/// Resolves `.` and `..` lexically without touching the filesystem or dereferencing symlinks.
fn lexical_normalize(path: &Path) -> PathBuf {
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

fn push_unique(list: &mut Vec<RootEntry>, entry: RootEntry) {
    if !list.iter().any(|e| e.canonical == entry.canonical) {
        list.push(entry);
    }
}

fn is_global_root(path: &Path) -> bool {
    path == Path::new("/")
}

fn is_global_temp_or_root(path: &Path) -> bool {
    let norm = lexical_normalize(path);
    let raw = norm.to_string_lossy();
    let cmp = if org_platform::fs::paths_case_insensitive_by_default() {
        raw.to_ascii_lowercase()
    } else {
        raw.into_owned()
    };
    matches!(
        cmp.as_str(),
        "/" | "/tmp" | "/private/tmp" | "/var/tmp" | "/private/var/tmp"
    )
}

fn find_git_root(start: &Path) -> Option<PathBuf> {
    for ancestor in start.ancestors() {
        if ancestor.join(".git").exists() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

fn discover_git_worktrees(repo_root: &Path) -> Vec<RootEntry> {
    let mut out = Vec::new();
    let git_dir = repo_root.join(".git");
    let worktrees_dir = if git_dir.is_dir() {
        git_dir.join("worktrees")
    } else {
        return out;
    };
    let Ok(entries) = fs::read_dir(worktrees_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let gitdir_file = entry.path().join("gitdir");
        let Ok(text) = fs::read_to_string(gitdir_file) else {
            continue;
        };
        let target = PathBuf::from(text.trim());
        let Some(wt_root) = target.parent() else {
            continue;
        };
        if let Some(root) = RootEntry::from_path(wt_root.to_path_buf()) {
            if !is_global_root(&root.canonical) {
                push_unique(&mut out, root);
            }
        }
    }
    out
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    if org_platform::fs::paths_case_insensitive_by_default() {
        let p = normalized_components(path);
        let r = normalized_components(root);
        p.len() >= r.len() && p[..r.len()] == r[..]
    } else {
        path.starts_with(root)
    }
}

fn normalized_components(path: &Path) -> Vec<String> {
    let fold = org_platform::fs::paths_case_insensitive_by_default();
    path.components()
        .filter_map(|c| match c {
            Component::Normal(seg) => {
                let s = seg.to_string_lossy();
                Some(if fold {
                    s.to_ascii_lowercase()
                } else {
                    s.into_owned()
                })
            }
            _ => None,
        })
        .collect()
}

fn match_beneath_root(lexical: &Path, roots: &FileRoots) -> Result<(PathBuf, PathBuf), Refusal> {
    // Match `lexical` directly against an allowed root's canonical or raw path so neither an
    // intermediate symlink inside the root nor an outside symlink pointing into the root is
    // dereferenced before `openat(O_NOFOLLOW)`.
    for entry in roots.all_roots() {
        if let Ok(rel) = lexical.strip_prefix(&entry.canonical) {
            if !rel.as_os_str().is_empty() {
                return Ok((entry.canonical.clone(), rel.to_path_buf()));
            }
        }
        if let Ok(rel) = lexical.strip_prefix(&entry.raw) {
            if !rel.as_os_str().is_empty() {
                return Ok((entry.canonical.clone(), rel.to_path_buf()));
            }
        }
    }
    Err(denied_path(
        "File path is outside allowed attachment roots or traverses a symlink outside them.",
        "Pass a regular file directly inside the workspace root, your private_home, active worktrees, or session-scoped TMPDIR.",
    ))
}

fn check_denylisted(path: &Path, roots: &FileRoots) -> Result<(), Refusal> {
    let comps = normalized_components(path);
    let eq = |a: &str, b: &str| a.eq_ignore_ascii_case(b);

    for (idx, seg) in comps.iter().enumerate() {
        if eq(seg, "attachment-staging") || eq(seg, "blob-outbox") {
            return Err(denied_path(
                "Access to office attachment staging or outbox paths is forbidden.",
                "Attach a regular user or workspace file instead.",
            ));
        }
        if eq(seg, "durable") {
            if let Some(next) = comps.get(idx + 1) {
                if eq(next, "blobs") {
                    return Err(denied_path(
                        "Direct access to durable/blobs/ is forbidden.",
                        "Reference existing attachments by --attachment-id instead of reading durable/blobs/.",
                    ));
                }
                if eq(next, "private") {
                    if let Some(leaf) = comps.get(idx + 2) {
                        let lower = leaf.to_ascii_lowercase();
                        if lower.starts_with("task-notes.db")
                            || lower.starts_with("user-space.db")
                            || lower == "blobs"
                        {
                            return Err(denied_path(
                                "Direct access to durable/private/ databases or blobs is forbidden.",
                                "Attach a workspace or own private_home file instead.",
                            ));
                        }
                    }
                }
            }
        }
        if eq(seg, "runtime") && comps.get(idx + 2).is_some_and(|c| eq(c, "protected")) {
            return Err(denied_path(
                "Direct access to runtime/<office>/protected/ is forbidden.",
                "Attach a workspace or own private_home file instead.",
            ));
        }
        // Another agent's `private_home` under `local/offices/<office>/agents/<id>/...`,
        // `agents/<id>/private_home/...`, or `private_home/<id>/...`.
        let other_agent = if eq(seg, "local")
            && comps.get(idx + 1).is_some_and(|c| eq(c, "offices"))
            && comps.get(idx + 3).is_some_and(|c| eq(c, "agents"))
        {
            comps.get(idx + 4)
        } else if (eq(seg, "agents") && comps.get(idx + 2).is_some_and(|c| eq(c, "private_home")))
            || eq(seg, "private_home")
        {
            comps.get(idx + 1)
        } else {
            None
        };
        if let Some(agent_seg) = other_agent {
            let is_own_id = roots
                .self_agent_id
                .as_deref()
                .is_some_and(|id| eq(agent_seg, id));
            let is_own_home = roots.private_home.as_ref().is_some_and(|h| {
                path_is_within(path, &h.canonical) || path_is_within(path, &h.raw)
            });
            if !is_own_id && !is_own_home {
                return Err(denied_path(
                    "Access to another agent's private_home is forbidden.",
                    "Attach files only from your own workspace, worktrees, private_home, or session TMPDIR.",
                ));
            }
        }
    }
    Ok(())
}

fn denied_path(message: &str, hint: &str) -> Refusal {
    Refusal::new(ErrorCode::NotAuthorized, message, hint)
}

fn map_open_err(raw_path: &str, err: &std::io::Error) -> Refusal {
    use std::io::ErrorKind;
    if err.raw_os_error() == Some(org_platform::fs::ELOOP) {
        return denied_path(
            "Symbolic links are not followed when reading attachments.",
            "Pass a regular file directly inside an allowed root.",
        );
    }
    match err.kind() {
        ErrorKind::NotFound => Refusal::new(
            ErrorCode::NotFound,
            format!("Local file `{raw_path}` was not found."),
            "Check the file path and retry.",
        ),
        ErrorKind::PermissionDenied => denied_path(
            "Permission denied reading local file.",
            "Ensure the file is readable and inside an allowed root.",
        ),
        ErrorKind::InvalidInput => Refusal::new(
            ErrorCode::InvalidRequest,
            "Path must name a regular file (never a directory, FIFO, device, or socket).",
            "Pass a regular file inside an allowed root.",
        ),
        _ => Refusal::new(
            ErrorCode::InvalidRequest,
            format!("Could not read local file `{raw_path}`: {err}"),
            "Verify the file is a readable regular file inside an allowed root.",
        ),
    }
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
