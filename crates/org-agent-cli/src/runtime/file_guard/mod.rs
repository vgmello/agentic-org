//! Two-layer local filesystem exfiltration defense for `--attach`, `file upload`, and
//! `file fetch --output` (`TASK-98.4`, `TASK-252`, ADR-0037).
//!
//! `org-office-server` never opens caller-supplied filesystem paths: `agent-os` opens and reads
//! local files under the caller's own OS account, then streams `<= 512 KiB` chunks over
//! `attachment.upload_*` RPCs. Before a single upload RPC opens, this module enforces:
//!
//! 1. **Canonicalized allowed-root confinement** ([`roots`]): workspace root (`cwd` / git repo
//!    root), own office-scoped `private_home`, verified bidirectional own git worktrees, and
//!    dedicated session-scoped `TMPDIR` (NEVER `/`, OS/system trees, global temp roots, or direct
//!    host user home directories).
//! 2. **Unconditional denylist** ([`denylist`], checked both lexically and post-canonicalization):
//!    office internals (all 10 durable families + `durable/protected/` + `durable/local/` companion
//!    stores outside own `agents/<id>/`, `private/(task-notes.db*|user-space.db*)`,
//!    `attachment-staging/`, `blob-outbox/`, `restore-backups/`, `runtime/<office>/protected/`,
//!    another agent's home) and sensitive host credentials/state (`.ssh`, `.aws`, `.claude`,
//!    `.codex`, `.gemini`, `.docker`, `.kube`, `.gnupg`, `.config/gh`, `.cargo/credentials*`,
//!    `.netrc`, `.npmrc`, `.pypirc`, `.git-credentials`, `host-secrets`, `git-credentials`,
//!    `office.db*`, `host.db*`, `host-registry.*`, `git-state.json`, `hq-owner.v1.json`,
//!    `session-binding*.v1`, `.session-binding.*.tmp`).
//! 3. **POSIX safe open via `org_platform::fs`**: component-wise `openat` with
//!    `O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC` and `fstat` `S_ISREG` verification.
//! 4. **Bounded read + canonical `org_media` allowlist & magic-byte pre-check**: `<= 10 MiB` for
//!    images, `<= 25 MiB` for allowlisted documents, rejecting executables, HTML, SVG, and
//!    mismatched magic bytes BEFORE opening an upload stream.

mod denylist;
mod paths;
mod roots;
#[cfg(test)]
mod tests;
mod worktrees;

use std::fs;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use org_command::Refusal;
use org_proto::{ApiError, ErrorCode};

use self::denylist::check_denylisted;
use self::paths::{denied_path, lexical_normalize, path_is_within, resolve_abs};
use self::roots::match_beneath_root;

#[cfg(test)]
pub(crate) use self::denylist::RootEntry;
#[cfg(test)]
pub(crate) use self::paths::strip_root_prefix_with_case;
#[cfg(test)]
pub(crate) use self::roots::with_test_roots;
pub(crate) use self::roots::FileRoots;

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

/// Single security choke-point validating two-layer allowed-root confinement, explicit denylist,
/// no-symlink check, and `org_platform::fs::open_regular_beneath` (`O_NOFOLLOW | O_NONBLOCK |
/// O_CLOEXEC` + `fstat` `S_ISREG`) before reading any local file (`file upload`, `--attach`,
/// `file render --from`, `file render --css`).
pub(crate) fn open_confined_regular_file(
    raw_path: &str,
    roots: &FileRoots,
) -> Result<(PathBuf, fs::File), Refusal> {
    validate_raw_path(raw_path)?;
    let abs_raw = resolve_abs(&roots.cwd, raw_path);
    let lexical = lexical_normalize(&abs_raw);

    check_denylisted(
        &abs_raw,
        roots.self_agent_id.as_deref(),
        roots.private_home.as_ref(),
    )?;
    check_denylisted(
        &lexical,
        roots.self_agent_id.as_deref(),
        roots.private_home.as_ref(),
    )?;

    // Check symlink metadata before canonicalize so symlinks and non-regular files fail closed
    // without silently dereferencing outside or inside an allowed root.
    let lmeta = fs::symlink_metadata(&abs_raw).map_err(|err| map_open_err(raw_path, &err))?;
    if lmeta.file_type().is_symlink() {
        return Err(denied_path(
            "Symbolic links cannot be attached, uploaded, or rendered.",
            "Pass a regular file directly inside an allowed workspace, private home, worktree, or session TMPDIR.",
        ));
    }

    let canonical = fs::canonicalize(&abs_raw).map_err(|err| map_open_err(raw_path, &err))?;
    check_denylisted(
        &canonical,
        roots.self_agent_id.as_deref(),
        roots.private_home.as_ref(),
    )?;
    if !roots
        .all_roots()
        .any(|r| path_is_within(&canonical, &r.canonical))
    {
        return Err(denied_path(
            "File path is outside allowed attachment roots.",
            "Only regular files inside the workspace root, your own private_home, active worktrees, or session-scoped TMPDIR may be read.",
        ));
    }

    let (root, relative) = match_beneath_root(&lexical, roots)?;
    let file = org_platform::fs::open_regular_beneath(&root, &relative)
        .map_err(|err| map_open_err(raw_path, &err))?;
    Ok((canonical, file))
}

/// Validates and reads a local file for upload under `roots` BEFORE any upload RPC is opened.
pub(crate) fn read_upload_file(
    raw_path: &str,
    roots: &FileRoots,
) -> Result<LocalUploadFile, Refusal> {
    let (canonical, file) = open_confined_regular_file(raw_path, roots)?;
    let (filename, media) =
        org_media::classify_filename(&canonical).map_err(map_filename_media_err)?;
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
    org_media::sniff::check_matches(media, &bytes).map_err(map_sniff_media_err)?;

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

    check_denylisted(
        &abs_raw,
        roots.self_agent_id.as_deref(),
        roots.private_home.as_ref(),
    )?;
    check_denylisted(
        &lexical,
        roots.self_agent_id.as_deref(),
        roots.private_home.as_ref(),
    )?;

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
    check_denylisted(
        &canonical_parent,
        roots.self_agent_id.as_deref(),
        roots.private_home.as_ref(),
    )?;
    check_denylisted(
        &canonical_target,
        roots.self_agent_id.as_deref(),
        roots.private_home.as_ref(),
    )?;
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
    verify_real_parent_dirs(&root, &relative)?;
    Ok(PreparedOutputPath {
        root,
        relative,
        display_path: canonical_target,
    })
}

fn verify_real_parent_dirs(root: &Path, relative: &Path) -> Result<(), Refusal> {
    let Some(rel_parent) = relative.parent() else {
        return Ok(());
    };
    let mut cursor = root.to_path_buf();
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
    Ok(())
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

fn map_filename_media_err(err: ApiError) -> Refusal {
    let hint = match err.code {
        ErrorCode::Unsupported => {
            "Allowed types: png, jpg/jpeg, webp, gif, pdf, docx, xlsx, md, txt/log, csv, json."
        }
        _ => "Rename the file to a plain UTF-8 filename (1..=255 bytes, no leading dot) with an allowed extension.",
    };
    Refusal::new(err.code, err.message, hint)
}

fn map_sniff_media_err(err: ApiError) -> Refusal {
    Refusal::new(
        err.code,
        err.message,
        "Ensure the file contents match its extension and are not an executable, HTML, SVG, or mismatched binary.",
    )
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
