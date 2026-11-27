//! Two-layer local filesystem exfiltration defense for `--attach`, `file upload`, and
//! `file fetch --output` (`TASK-98.4`, ADR-0037).
//!
//! `org-office-server` never opens caller-supplied filesystem paths: `agent-os` opens and reads
//! local files under the caller's own OS account, then streams `<= 512 KiB` chunks over
//! `attachment.upload_*` RPCs. Before a single upload RPC opens, this module enforces:
//!
//! 1. **Canonicalized allowed-root confinement**: workspace root (`cwd` / git repo root), own
//!    `private_home` (derived strictly from `HOME` when `AGENT_OS_AGENT_ID` is set), own active
//!    git worktrees, and session-scoped `TMPDIR` (NEVER global `/tmp`, `/private/tmp`,
//!    `/var/tmp`, `/`, or OS/system roots).
//! 2. **Explicit denylist** (checked both lexically and post-canonicalization, even if nested or
//!    symlinked): `durable/blobs/`, `durable/private/`, `durable/messaging/`,
//!    `private/(task-notes.db*|user-space.db*)`, `attachment-staging/`, `blob-outbox/`, another
//!    agent's home, and `runtime/<office>/protected/`.
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
        let raw = lexical_normalize(&path);
        if is_forbidden_root(&raw) {
            return None;
        }
        let canonical = fs::canonicalize(&path).ok()?;
        if !canonical.is_dir() || is_forbidden_root(&canonical) {
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
    /// Never honors untrusted env wideners (`AGENT_OS_WORKTREES`, `AGENT_OS_PRIVATE_HOME`): own
    /// `private_home` is derived strictly from `home` when `self_agent_id` is set, worktrees are
    /// discovered strictly from Git metadata, and every root candidate is screened against global
    /// temp, root, and OS/system directories (both raw and canonical).
    pub(crate) fn from_parts(
        cwd: PathBuf,
        self_agent_id: Option<String>,
        home: Option<PathBuf>,
        tmpdir: Option<PathBuf>,
    ) -> Self {
        let self_agent_id = self_agent_id
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());

        let mut workspace_roots = Vec::new();
        let mut worktrees = Vec::new();
        if let Some(cwd_entry) = RootEntry::from_path(cwd.clone()) {
            if let Some(repo_root) = find_git_root(&cwd_entry.canonical) {
                if let Some(repo_entry) = RootEntry::from_path(repo_root) {
                    for wt in discover_git_worktrees(&repo_entry.canonical) {
                        push_unique(&mut worktrees, wt);
                    }
                    push_unique(&mut workspace_roots, repo_entry);
                }
            }
            push_unique(&mut workspace_roots, cwd_entry);
        }

        let private_home = self_agent_id
            .as_ref()
            .and(home)
            .filter(|p| !p.as_os_str().is_empty())
            .and_then(RootEntry::from_path);

        let session_tmpdir = tmpdir
            .filter(|p| !p.as_os_str().is_empty())
            .and_then(RootEntry::from_path);

        Self {
            cwd,
            workspace_roots,
            private_home,
            worktrees,
            session_tmpdir,
            self_agent_id,
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
    if raw.ends_with('/') || raw == "." || raw.ends_with("/.") {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "File path must not end with `/`, `.`, or `/.`.",
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

const MAX_GIT_POINTER_BYTES: u64 = 8 * 1024;

/// Rejects global temp directories, `/`, top-level shared/user containers, and OS/system trees
/// so no root candidate (`cwd`, repo root, `private_home`, worktree, or `TMPDIR`) can widen
/// confinement beyond a real project/session directory.
fn is_forbidden_root(path: &Path) -> bool {
    let norm = lexical_normalize(path);
    if !norm.is_absolute() {
        return false;
    }
    let comps: Vec<String> = norm
        .components()
        .filter_map(|c| match c {
            Component::Normal(seg) => Some(seg.to_string_lossy().to_ascii_lowercase()),
            _ => None,
        })
        .collect();
    let refs: Vec<&str> = comps.iter().map(String::as_str).collect();
    matches!(
        refs.as_slice(),
        [] | ["tmp"]
            | ["private"]
            | ["private", "tmp"]
            | ["var", "tmp"]
            | ["private", "var", "tmp"]
            | ["var"]
            | ["private", "var"]
            | ["home"]
            | ["users"]
            | ["etc", ..]
            | ["private", "etc", ..]
            | ["usr", ..]
            | ["system", ..]
            | ["proc", ..]
            | ["sys", ..]
            | ["dev", ..]
    )
}

fn find_git_root(start: &Path) -> Option<PathBuf> {
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

fn read_small_git_text(path: &Path) -> Option<String> {
    let file = org_platform::fs::open_nofollow_ro(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_GIT_POINTER_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if (bytes.len() as u64) > MAX_GIT_POINTER_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn resolve_git_common_dir(repo_root: &Path) -> Option<PathBuf> {
    let dot_git = repo_root.join(".git");
    let meta = fs::symlink_metadata(&dot_git).ok()?;
    if meta.file_type().is_dir() {
        return Some(dot_git);
    }
    if !meta.file_type().is_file() {
        return None;
    }
    let text = read_small_git_text(&dot_git)?;
    let raw_gitdir = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("gitdir:"))
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let gitdir_path = Path::new(raw_gitdir);
    let gitdir = if gitdir_path.is_absolute() {
        gitdir_path.to_path_buf()
    } else {
        repo_root.join(gitdir_path)
    };
    let gitdir = fs::canonicalize(gitdir).ok()?;
    if !gitdir.is_dir() {
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
                if canonical_common.is_dir() {
                    return Some(canonical_common);
                }
            }
        }
    }
    Some(gitdir)
}

fn discover_git_worktrees(repo_root: &Path) -> Vec<RootEntry> {
    let mut out = Vec::new();
    let Some(common_dir) = resolve_git_common_dir(repo_root) else {
        return out;
    };
    let worktrees_dir = common_dir.join("worktrees");
    let Ok(entries) = fs::read_dir(worktrees_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let gitdir_file = entry.path().join("gitdir");
        let Some(text) = read_small_git_text(&gitdir_file) else {
            continue;
        };
        let raw_target = text.trim();
        if raw_target.is_empty() {
            continue;
        }
        let target_path = Path::new(raw_target);
        let target = lexical_normalize(&if target_path.is_absolute() {
            target_path.to_path_buf()
        } else {
            entry.path().join(target_path)
        });
        let Some(wt_root) = target.parent() else {
            continue;
        };
        if let Some(root) = RootEntry::from_path(wt_root.to_path_buf()) {
            push_unique(&mut out, root);
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

fn strip_root_prefix(path: &Path, root: &Path) -> Option<PathBuf> {
    strip_root_prefix_with_case(
        path,
        root,
        org_platform::fs::paths_case_insensitive_by_default(),
    )
}

fn match_beneath_root(lexical: &Path, roots: &FileRoots) -> Result<(PathBuf, PathBuf), Refusal> {
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

fn is_own_agent_home_prefix(
    path: &Path,
    comps: &[String],
    prefix_len: usize,
    roots: &FileRoots,
) -> bool {
    let Some(home) = roots.private_home.as_ref() else {
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
                if eq(next, "blobs") || eq(next, "private") || eq(next, "messaging") {
                    return Err(denied_path(
                        "Direct access to durable office storage (`durable/blobs`, `durable/private`, `durable/messaging`) is forbidden.",
                        "Reference existing attachments by --attachment-id or attach a workspace/private_home file instead.",
                    ));
                }
            }
        }
        if eq(seg, "private") {
            if let Some(leaf) = comps.get(idx + 1) {
                let lower = leaf.to_ascii_lowercase();
                if lower.starts_with("task-notes.db") || lower.starts_with("user-space.db") {
                    return Err(denied_path(
                        "Direct access to private SQLite databases (`task-notes.db*`, `user-space.db*`) is forbidden.",
                        "Attach a workspace or own private_home file instead.",
                    ));
                }
            }
        }
        if eq(seg, "runtime") && comps.get(idx + 2).is_some_and(|c| eq(c, "protected")) {
            return Err(denied_path(
                "Direct access to runtime/<office>/protected/ is forbidden.",
                "Attach a workspace or own private_home file instead.",
            ));
        }
        // Another agent's home strictly under `local/offices/<office>/agents/<id>/...` or
        // `agents/<id>/(home|private_home)/...`.
        let other_agent = if eq(seg, "local")
            && comps.get(idx + 1).is_some_and(|c| eq(c, "offices"))
            && comps.get(idx + 3).is_some_and(|c| eq(c, "agents"))
        {
            comps.get(idx + 4).map(|id| (id, idx + 5))
        } else if eq(seg, "agents")
            && comps
                .get(idx + 2)
                .is_some_and(|c| eq(c, "home") || eq(c, "private_home"))
        {
            comps.get(idx + 1).map(|id| (id, idx + 3))
        } else {
            None
        };
        if let Some((agent_seg, prefix_len)) = other_agent {
            let is_own_id = roots
                .self_agent_id
                .as_deref()
                .is_some_and(|id| eq(agent_seg, id));
            let is_own_home = is_own_agent_home_prefix(path, &comps, prefix_len, roots);
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
