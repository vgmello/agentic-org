//! Opening an existing organization from Git repository files (`TASK-39` / R1, `doc-91`
//! "Opening an existing organization", `doc-61` D25).
//!
//! Clones into a bare temporary repository with strict protocol/option/timeout bounds, inspects
//! the tree via `git ls-tree -r -l -z HEAD` and reads regular blobs via `git cat-file --batch`
//! without checking out a working tree (eliminating zip-bomb disk DoS and catching Git symlinks
//! `120000` and submodules `160000`), validates every file through the office's real production
//! projectors (`OrganizationFiles` and `ProjectFiles`) without guessing or repairing, reports any
//! invalid or unrecognized file by path and reason, and builds Entry 1 for a new HQ office.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use org_store::{
    Ack, Actor, Change, DurablePath, Id, OfficeIdentity, Operation, Refusal, Sha256, Stream,
    Timestamp, UnitOfWork,
};
use serde::{Deserialize, Serialize};

use crate::organization::{
    features::{create_organization::HQ_SLUG, operation, Creator, Op},
    ports::OrganizationRead,
    records::{
        self, By, Employee, EmployeeKind, EmployeeStatus, Kind, Meta, Office, OfficeCode,
        OfficeRole,
    },
};
use crate::store::open::validate_imported_files;

/// Re-exported canonical summary of local-only data that never lives in Git (`doc-91`).
pub use org_office_launch::LOCAL_ONLY_NOT_RESTORED;

/// Maximum size of any single imported state file (4 MiB).
const MAX_IMPORT_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Maximum total bytes across all imported state files (12 MiB, leaving safe headroom below
/// `org_store::MAX_ENTRY_BYTES` = 16 MiB for journal envelope, new HQ records, and CA escrow).
const MAX_IMPORT_TOTAL_BYTES: u64 = 12 * 1024 * 1024;
/// Maximum number of state files accepted in a single import.
const MAX_IMPORT_FILES: usize = 4_096;
/// Maximum directory depth inside `state/`.
const MAX_IMPORT_DEPTH: usize = 32;
/// Maximum bytes read from `git` stderr.
const MAX_GIT_STDERR_BYTES: u64 = 64 * 1024;
/// Maximum bytes read from `git ls-tree` / `rev-parse` stdout.
const MAX_GIT_META_STDOUT_BYTES: u64 = 4 * 1024 * 1024;
/// Wall-clock deadline for each `git` invocation.
const GIT_TIMEOUT: Duration = Duration::from_secs(45);

/// An active person record discovered in an imported organization so the administrator can pick
/// which person record belongs to them as organization owner.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ImportedPerson {
    pub id: String,
    pub name: String,
    pub handle: String,
    pub home_team_id: String,
}

/// Summary extracted by validating an imported organization through the real projectors.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ValidatedImport {
    pub organization_id: Id,
    pub name: String,
    pub slug: String,
    pub canonical_remote_url: Option<String>,
    pub hq_office_id: Id,
    pub hq_office_name: String,
    pub root_team_id: Option<Id>,
    pub has_active_root_team: bool,
    pub teams_count: usize,
    pub projects_count: usize,
    pub agents_count: usize,
    pub people: Vec<ImportedPerson>,
    pub reserved_handles: Vec<String>,
    pub reserved_entity_ids: Vec<String>,
}

/// Complete validated import ready for preview or Entry 1 HQ import.
#[derive(Clone, Debug)]
pub struct ImportedOrganization {
    pub summary: ValidatedImport,
    pub repository_remote_url: String,
    pub state_branch: String,
    pub commit_oid: Option<String>,
    pub local_only_not_restored: Vec<String>,
    pub files: BTreeMap<DurablePath, Vec<u8>>,
}

/// Failure while cloning, reading, or validating an existing organization from Git.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ImportError {
    InvalidInput(String),
    GitFailed(String),
    InvalidFile { path: String, reason: String },
    Storage(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::InvalidInput(msg) => f.write_str(msg),
            ImportError::GitFailed(msg) => write!(f, "git repository could not be read: {msg}"),
            ImportError::InvalidFile { path, reason } => write!(f, "{path}: {reason}"),
            ImportError::Storage(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for ImportError {}

/// RAII private temporary directory created atomically with mode `0700` and deleted on drop.
pub(crate) struct PrivateTempDir(PathBuf);

impl PrivateTempDir {
    pub(crate) fn create(prefix: &str) -> Result<Self, ImportError> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let pid = std::process::id();
        let base = std::env::temp_dir();
        for attempt in 0u32..32 {
            let path = base.join(format!("{prefix}-{pid}-{nonce}-{attempt}"));
            match org_platform::fs::create_owner_only_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(ImportError::Storage(format!("creating temp dir: {e}"))),
            }
        }
        Err(ImportError::Storage(
            "could not allocate temporary directory".into(),
        ))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for PrivateTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Validates a Git remote URL (`https://...`, `ssh://...`, or absolute `file:///...`) and branch
/// name before invoking `git`. Matches `PlanV1` strict remote/branch syntax and refuses unknown
/// schemes (`ext::`), credentials, queries/fragments, and leading `-` option injection.
pub fn check_git_source(remote_url: &str, state_branch: &str) -> Result<(), ImportError> {
    let valid_url = if remote_url.starts_with("https://") || remote_url.starts_with("ssh://") {
        org_proto::selected_bootstrap::valid_remote_url(remote_url)
    } else if let Some(path) = remote_url.strip_prefix("file://") {
        path.starts_with('/')
            && path.len() > 1
            && remote_url.len() <= 2048
            && !remote_url
                .chars()
                .any(|c| c.is_whitespace() || c.is_control())
            && !remote_url.contains(['?', '#', '\\'])
    } else {
        false
    };
    if !valid_url || remote_url.starts_with('-') {
        return Err(ImportError::InvalidInput(
            "repository_remote_url must be a valid https://, ssh://, or file:/// Git URL".into(),
        ));
    }
    if !org_proto::selected_bootstrap::valid_state_branch(state_branch) {
        return Err(ImportError::InvalidInput(
            "state_branch must be a valid Git branch name".into(),
        ));
    }
    Ok(())
}

fn base_git_command() -> Command {
    let mut cmd = Command::new("git");
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes")
        .args([
            "-c",
            "core.symlinks=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "submodule.recurse=false",
            "-c",
            "protocol.allow=never",
            "-c",
            "protocol.https.allow=always",
            "-c",
            "protocol.ssh.allow=always",
            "-c",
            "protocol.file.allow=always",
        ]);
    cmd
}

fn wait_with_timeout(
    child: &mut Child,
    timeout: Duration,
) -> Result<std::process::ExitStatus, bool> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(true);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(false);
            }
        }
    }
}

fn sanitize_git_stderr(stderr: &str, staging_dir: &Path, fallback: &str) -> String {
    let chosen = stderr
        .lines()
        .find(|l| {
            let t = l.trim_start();
            t.starts_with("fatal:") || t.starts_with("error:")
        })
        .unwrap_or(fallback)
        .trim();
    let redacted = chosen.replace(&staging_dir.display().to_string(), "<staging>");
    let sanitized = strip_url_credentials(&redacted);
    if sanitized.is_empty() {
        fallback.to_owned()
    } else {
        sanitized.chars().take(240).collect()
    }
}

fn strip_url_credentials(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find("://") {
        let after_scheme = &rest[pos + 3..];
        let seg_end = after_scheme
            .find(|c: char| c.is_whitespace() || c == '\'' || c == '"' || c == '/')
            .unwrap_or(after_scheme.len());
        let authority = &after_scheme[..seg_end];
        if let Some(at) = authority.rfind('@') {
            out.push_str(&rest[..pos + 3]);
            out.push_str(&authority[at + 1..]);
            rest = &after_scheme[seg_end..];
        } else {
            out.push_str(&rest[..pos + 3]);
            rest = after_scheme;
        }
    }
    out.push_str(rest);
    out
}

fn run_git_capture(
    args: &[&str],
    cwd: Option<&Path>,
    staging: &Path,
    fallback: &str,
) -> Result<Vec<u8>, ImportError> {
    let mut cmd = base_git_command();
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| ImportError::GitFailed(format!("failed to start git: {e}")))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stdout {
            let _ = s.take(MAX_GIT_META_STDOUT_BYTES).read_to_end(&mut buf);
        }
        buf
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stderr {
            let _ = s.take(MAX_GIT_STDERR_BYTES).read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    });
    let status = match wait_with_timeout(&mut child, GIT_TIMEOUT) {
        Ok(s) => s,
        Err(true) => return Err(ImportError::GitFailed("git operation timed out".into())),
        Err(false) => return Err(ImportError::GitFailed(fallback.into())),
    };
    let out = out_thread.join().unwrap_or_default();
    let err_text = err_thread.join().unwrap_or_default();
    if !status.success() {
        return Err(ImportError::GitFailed(sanitize_git_stderr(
            &err_text, staging, fallback,
        )));
    }
    Ok(out)
}

#[derive(Debug)]
struct TreeBlob {
    durable: DurablePath,
    oid: String,
    size: u64,
}

struct RawTreeEntry {
    mode: String,
    kind: String,
    oid: String,
    size: u64,
    path: String,
}

fn decode_ls_tree_raw(raw: &[u8]) -> Result<Vec<RawTreeEntry>, ImportError> {
    let mut entries = Vec::new();
    for record in raw.split(|&b| b == 0).filter(|r| !r.is_empty()) {
        let Some(tab) = record.iter().position(|&b| b == b'\t') else {
            return Err(ImportError::GitFailed("malformed git tree entry".into()));
        };
        let meta = std::str::from_utf8(&record[..tab])
            .map_err(|_| ImportError::GitFailed("non-utf8 git tree metadata".into()))?;
        let path_str =
            std::str::from_utf8(&record[tab + 1..]).map_err(|_| ImportError::InvalidFile {
                path: format!("state/{}", String::from_utf8_lossy(&record[tab + 1..])),
                reason: "file path is not valid UTF-8".into(),
            })?;
        let parts: Vec<&str> = meta.split_whitespace().collect();
        if parts.len() != 4 {
            return Err(ImportError::GitFailed("malformed git tree header".into()));
        }
        let size = if parts[3] == "-" {
            0
        } else {
            parts[3]
                .parse::<u64>()
                .map_err(|_| ImportError::GitFailed("invalid git blob size".into()))?
        };
        entries.push(RawTreeEntry {
            mode: parts[0].to_owned(),
            kind: parts[1].to_owned(),
            oid: parts[2].to_owned(),
            size,
            path: path_str.to_owned(),
        });
    }
    Ok(entries)
}

fn select_state_prefix(entries: &[RawTreeEntry]) -> Result<&'static str, ImportError> {
    if entries.iter().any(|e| e.path == "organization.yaml") {
        Ok("")
    } else if entries.iter().any(|e| e.path == "state/organization.yaml") {
        Ok("state/")
    } else {
        Err(ImportError::InvalidFile {
            path: "state/organization.yaml".into(),
            reason: "the organization record is missing".into(),
        })
    }
}

fn check_entry_mode(entry: &RawTreeEntry, durable_text: &str) -> Result<(), ImportError> {
    if entry.mode == "120000" {
        return Err(ImportError::InvalidFile {
            path: durable_text.to_owned(),
            reason: "symlinks are not permitted in organization state".into(),
        });
    }
    if entry.mode == "160000" {
        return Err(ImportError::InvalidFile {
            path: durable_text.to_owned(),
            reason: "submodules are not permitted in organization state".into(),
        });
    }
    if entry.kind != "blob" || (entry.mode != "100644" && entry.mode != "100755") {
        return Err(ImportError::InvalidFile {
            path: durable_text.to_owned(),
            reason: "only regular files are permitted in organization state".into(),
        });
    }
    Ok(())
}

fn check_entry_bounds(
    rel: &str,
    durable_text: &str,
    size: u64,
    count: usize,
    total_bytes: &mut u64,
) -> Result<(), ImportError> {
    if rel.split('/').count() > MAX_IMPORT_DEPTH {
        return Err(ImportError::InvalidFile {
            path: durable_text.to_owned(),
            reason: "directory nesting exceeds maximum allowed depth".into(),
        });
    }
    if count >= MAX_IMPORT_FILES {
        return Err(ImportError::InvalidFile {
            path: durable_text.to_owned(),
            reason: "organization state exceeds maximum file count limit".into(),
        });
    }
    if size > MAX_IMPORT_FILE_BYTES {
        return Err(ImportError::InvalidFile {
            path: durable_text.to_owned(),
            reason: "file exceeds the single-file import size limit".into(),
        });
    }
    *total_bytes = total_bytes.saturating_add(size);
    if *total_bytes > MAX_IMPORT_TOTAL_BYTES {
        return Err(ImportError::InvalidFile {
            path: durable_text.to_owned(),
            reason: "organization state exceeds total import size limit".into(),
        });
    }
    Ok(())
}

fn parse_ls_tree_entries(raw: &[u8]) -> Result<Vec<TreeBlob>, ImportError> {
    let entries = decode_ls_tree_raw(raw)?;
    let prefix = select_state_prefix(&entries)?;
    let mut blobs = Vec::new();
    let mut total_bytes: u64 = 0;
    for entry in entries {
        let rel = if prefix.is_empty() {
            entry.path.as_str()
        } else {
            entry
                .path
                .strip_prefix(prefix)
                .ok_or_else(|| ImportError::InvalidFile {
                    path: entry.path.clone(),
                    reason: "unrecognized file outside organization state/ directory".into(),
                })?
        };
        let durable_text = format!("state/{rel}");
        check_entry_mode(&entry, &durable_text)?;
        check_entry_bounds(
            rel,
            &durable_text,
            entry.size,
            blobs.len(),
            &mut total_bytes,
        )?;
        let durable = DurablePath::parse(&durable_text).map_err(|_| ImportError::InvalidFile {
            path: durable_text.clone(),
            reason: "invalid organization state file path".into(),
        })?;
        blobs.push(TreeBlob {
            durable,
            oid: entry.oid,
            size: entry.size,
        });
    }
    Ok(blobs)
}

fn read_git_blobs(
    repo_dir: &Path,
    blobs: &[TreeBlob],
    staging: &Path,
) -> Result<BTreeMap<DurablePath, Vec<u8>>, ImportError> {
    let mut cmd = base_git_command();
    cmd.current_dir(repo_dir)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| ImportError::GitFailed(format!("failed to start git cat-file: {e}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| ImportError::GitFailed("git cat-file stdin unavailable".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ImportError::GitFailed("git cat-file stdout unavailable".into()))?;
    let stderr = child.stderr.take();

    let oids: Vec<String> = blobs.iter().map(|b| b.oid.clone()).collect();
    let writer = std::thread::spawn(move || {
        for oid in oids {
            if writeln!(stdin, "{oid}").is_err() {
                break;
            }
        }
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stderr {
            let _ = s.take(MAX_GIT_STDERR_BYTES).read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    });

    let mut reader = BufReader::new(stdout);
    let mut files = BTreeMap::new();
    for blob in blobs {
        let mut header = String::new();
        let n = reader
            .read_line(&mut header)
            .map_err(|_| ImportError::GitFailed("failed reading git object header".into()))?;
        if n == 0 {
            return Err(ImportError::GitFailed(
                "unexpected end of git cat-file output".into(),
            ));
        }
        let parts: Vec<&str> = header.split_whitespace().collect();
        if parts.len() != 3 || parts[1] != "blob" {
            return Err(ImportError::InvalidFile {
                path: blob.durable.as_str().to_owned(),
                reason: "git object is not a readable blob".into(),
            });
        }
        let actual_size = parts[2]
            .parse::<u64>()
            .map_err(|_| ImportError::GitFailed("invalid git object length".into()))?;
        if actual_size != blob.size || actual_size > MAX_IMPORT_FILE_BYTES {
            return Err(ImportError::InvalidFile {
                path: blob.durable.as_str().to_owned(),
                reason: "git blob size mismatch or exceeds limit".into(),
            });
        }
        let mut content = vec![0u8; actual_size as usize];
        reader
            .read_exact(&mut content)
            .map_err(|_| ImportError::GitFailed("truncated git blob stream".into()))?;
        let mut lf = [0u8; 1];
        reader
            .read_exact(&mut lf)
            .map_err(|_| ImportError::GitFailed("missing git object delimiter".into()))?;
        files.insert(blob.durable.clone(), content);
    }

    let _ = writer.join();
    let status = match wait_with_timeout(&mut child, GIT_TIMEOUT) {
        Ok(s) => s,
        Err(true) => return Err(ImportError::GitFailed("git cat-file timed out".into())),
        Err(false) => return Err(ImportError::GitFailed("git cat-file failed".into())),
    };
    let err_text = err_thread.join().unwrap_or_default();
    if !status.success() {
        return Err(ImportError::GitFailed(sanitize_git_stderr(
            &err_text,
            staging,
            "failed to read repository objects",
        )));
    }
    Ok(files)
}

/// Walks a local `state_root` directory (for unit tests and local directory validation), enforcing
/// symlink, depth, file-count, and size limits, collecting every file under `state/<relative>`.
pub fn collect_state_files(
    state_root: &Path,
) -> Result<BTreeMap<DurablePath, Vec<u8>>, ImportError> {
    let mut files = BTreeMap::new();
    let mut total_bytes: u64 = 0;
    walk_dir(state_root, state_root, 0, &mut files, &mut total_bytes)?;
    Ok(files)
}

fn walk_dir(
    root: &Path,
    dir: &Path,
    depth: usize,
    out: &mut BTreeMap<DurablePath, Vec<u8>>,
    total_bytes: &mut u64,
) -> Result<(), ImportError> {
    if depth > MAX_IMPORT_DEPTH {
        return Err(ImportError::InvalidFile {
            path: "state/".into(),
            reason: "directory nesting exceeds maximum allowed depth".into(),
        });
    }
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| ImportError::Storage(format!("reading state directory: {e}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| ImportError::Storage(format!("listing state directory: {e}")))?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name();
        if dir == root && name == ".git" {
            continue;
        }
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)
            .map_err(|e| ImportError::Storage(format!("inspecting file: {e}")))?;
        let rel = path
            .strip_prefix(root)
            .map_err(|_| ImportError::Storage("path outside staging root".into()))?;
        let rel_str = rel.to_str().ok_or_else(|| ImportError::InvalidFile {
            path: format!("state/{}", rel.to_string_lossy()),
            reason: "file path is not valid UTF-8".into(),
        })?;
        let durable_text = format!("state/{rel_str}");
        if meta.file_type().is_symlink() {
            return Err(ImportError::InvalidFile {
                path: durable_text,
                reason: "symlinks are not permitted in organization state".into(),
            });
        }
        if meta.is_dir() {
            walk_dir(root, &path, depth + 1, out, total_bytes)?;
            continue;
        }
        if !meta.is_file() {
            return Err(ImportError::InvalidFile {
                path: durable_text,
                reason: "only regular files are permitted in organization state".into(),
            });
        }
        if out.len() >= MAX_IMPORT_FILES {
            return Err(ImportError::InvalidFile {
                path: durable_text,
                reason: "organization state exceeds maximum file count limit".into(),
            });
        }
        if meta.len() > MAX_IMPORT_FILE_BYTES {
            return Err(ImportError::InvalidFile {
                path: durable_text,
                reason: "file exceeds the single-file import size limit".into(),
            });
        }
        *total_bytes = total_bytes.saturating_add(meta.len());
        if *total_bytes > MAX_IMPORT_TOTAL_BYTES {
            return Err(ImportError::InvalidFile {
                path: durable_text,
                reason: "organization state exceeds total import size limit".into(),
            });
        }
        let durable = DurablePath::parse(&durable_text).map_err(|_| ImportError::InvalidFile {
            path: durable_text.clone(),
            reason: "invalid organization state file path".into(),
        })?;
        let bytes = fs::read(&path).map_err(|e| ImportError::InvalidFile {
            path: durable_text,
            reason: format!("could not read file: {e}"),
        })?;
        out.insert(durable, bytes);
    }
    Ok(())
}

fn validate_files_map(
    files: BTreeMap<DurablePath, Vec<u8>>,
    remote_url: &str,
    state_branch: &str,
    commit_oid: Option<String>,
) -> Result<ImportedOrganization, ImportError> {
    let org_path = records::organization_path();
    let org_bytes = files
        .get(&org_path)
        .ok_or_else(|| ImportError::InvalidFile {
            path: org_path.as_str().to_owned(),
            reason: "the organization record is missing".into(),
        })?;
    let org_record =
        records::decode_organization(org_bytes).map_err(|e| ImportError::InvalidFile {
            path: org_path.as_str().to_owned(),
            reason: e.to_string(),
        })?;
    let summary = validate_imported_files(&org_record.id, &files)?;
    Ok(ImportedOrganization {
        summary,
        repository_remote_url: remote_url.to_owned(),
        state_branch: state_branch.to_owned(),
        commit_oid,
        local_only_not_restored: LOCAL_ONLY_NOT_RESTORED
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
        files,
    })
}

/// Validates a local directory of organization state files against the full data contract.
pub fn validate_state_dir(
    state_root: &Path,
    remote_url: &str,
    state_branch: &str,
) -> Result<ImportedOrganization, ImportError> {
    let files = collect_state_files(state_root)?;
    validate_files_map(files, remote_url, state_branch, None)
}

/// Clones `state_branch` from `remote_url` into a bare temporary repository (no working-tree
/// checkout), inspects the Git tree for symlinks/submodules/size/count limits, reads regular
/// blobs via `git cat-file --batch`, and validates every file through the production projectors.
pub fn clone_and_validate_state(
    remote_url: &str,
    state_branch: &str,
) -> Result<ImportedOrganization, ImportError> {
    check_git_source(remote_url, state_branch)?;
    let staging = PrivateTempDir::create("org-import")?;
    let repo_dir = staging.path().join("repo.git");
    let repo_arg = repo_dir
        .to_str()
        .ok_or_else(|| ImportError::Storage("staging path is not valid UTF-8".into()))?;
    run_git_capture(
        &[
            "clone",
            "--bare",
            "--depth",
            "1",
            "--single-branch",
            "--branch",
            state_branch,
            "--",
            remote_url,
            repo_arg,
        ],
        None,
        staging.path(),
        "clone failed",
    )?;
    let rev_bytes = run_git_capture(
        &["rev-parse", "--verify", "HEAD"],
        Some(&repo_dir),
        staging.path(),
        "could not resolve branch head",
    )?;
    let commit_oid = String::from_utf8_lossy(&rev_bytes).trim().to_owned();
    let tree_bytes = run_git_capture(
        &["ls-tree", "-r", "-l", "-z", "HEAD"],
        Some(&repo_dir),
        staging.path(),
        "could not list repository files",
    )?;
    let blobs = parse_ls_tree_entries(&tree_bytes)?;
    let files = read_git_blobs(&repo_dir, &blobs, staging.path())?;
    validate_files_map(files, remote_url, state_branch, Some(commit_oid))
}

/// Allocates the next unused valid office code (`A`..`Z` excluding `I` and `O`, then `AA`..`ZZ`)
/// so a newly opened HQ that cannot prove its last task number takes a fresh office code
/// (`doc-91` "Opening an existing organization").
pub fn next_office_code(existing: &[OfficeCode]) -> Option<String> {
    const LETTERS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    let used: BTreeSet<&str> = existing.iter().map(|c| c.code.as_str()).collect();
    for &b in LETTERS {
        let code = (b as char).to_string();
        if !used.contains(code.as_str()) {
            return Some(code);
        }
    }
    for &a in LETTERS {
        for &b in LETTERS {
            let mut code = String::with_capacity(2);
            code.push(a as char);
            code.push(b as char);
            if !used.contains(code.as_str()) {
                return Some(code);
            }
        }
    }
    None
}

/// Command describing Entry 1 of an HQ opened from existing Git files (`R1`).
#[derive(Clone, Debug)]
pub struct ImportOrganization {
    pub organization_id: Id,
    pub office_id: Id,
    pub office_name: String,
    pub owner_member_id: Id,
    /// Optional new person to add into the root team when the administrator creates a fresh
    /// person record instead of selecting an existing person from Git.
    pub new_owner_person: Option<Creator>,
    pub imported: ImportedOrganization,
}

#[derive(Serialize)]
struct ImportCreatorFingerprint<'a> {
    employee_id: &'a Id,
    name: &'a str,
    handle: &'a str,
}

#[derive(Serialize)]
struct ImportFingerprint<'a> {
    organization_id: &'a Id,
    office_id: &'a Id,
    owner_member_id: &'a Id,
    repository_remote_url: &'a str,
    state_branch: &'a str,
    commit_oid: Option<&'a str>,
    content_sha256: String,
    new_owner_person: Option<ImportCreatorFingerprint<'a>>,
}

fn canonical_content_sha256(files: &BTreeMap<DurablePath, Vec<u8>>) -> String {
    let mut manifest = Vec::new();
    for (path, bytes) in files {
        let digest = Sha256::of(bytes);
        let _ = writeln!(manifest, "{}\0{}\0{}", path.as_str(), bytes.len(), digest);
    }
    Sha256::of(&manifest).to_string()
}

impl ImportOrganization {
    pub fn operation(&self, id: Id) -> Operation {
        let actor = Actor::Employee(self.owner_member_id.clone());
        let body = ImportFingerprint {
            organization_id: &self.organization_id,
            office_id: &self.office_id,
            owner_member_id: &self.owner_member_id,
            repository_remote_url: &self.imported.repository_remote_url,
            state_branch: &self.imported.state_branch,
            commit_oid: self.imported.commit_oid.as_deref(),
            content_sha256: canonical_content_sha256(&self.imported.files),
            new_owner_person: self
                .new_owner_person
                .as_ref()
                .map(|c| ImportCreatorFingerprint {
                    employee_id: &c.employee_id,
                    name: &c.name,
                    handle: &c.handle,
                }),
        };
        operation(Op::ImportOrganization, id, actor, &body)
    }
}

/// Builds Entry 1 for an HQ opened from Git files (`doc-91`, `doc-61` D25).
pub fn decide_import<E: org_store::Engine>(
    uow: &mut UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    command: &ImportOrganization,
) -> Result<Change, Refusal>
where
    E::Tx: crate::organization::ports::OrganizationRead,
{
    if command.organization_id != here.organization_id || command.office_id != here.office_id {
        return Err(Refusal::invalid(
            "this store is another organization's or office's",
        ));
    }
    if command.imported.summary.organization_id != here.organization_id {
        return Err(Refusal::invalid("imported organization_id does not match"));
    }
    if uow
        .view()
        .organization()
        .map_err(|_| Refusal::invalid("read failed"))?
        .is_some()
    {
        return Err(Refusal::invalid("the organization already exists"));
    }
    let generation = uow.next(Stream::Organization)?;
    if generation != 1 {
        return Err(Refusal::invalid(
            "import is the new HQ journal's first entry",
        ));
    }
    let now = uow.now();
    let by = By::Employee(command.owner_member_id.clone());
    let mut files = command.imported.files.clone();
    // Old HQ escrow is replaced by the new HQ's fresh CA escrow written by the caller.
    files.remove(&records::ca_escrow_path());

    ensure_owner_person(&mut files, command, &by, now)?;
    rewrite_org_and_repo(&mut files, command, &by, now)?;
    rewrite_offices_and_codes(&mut files, command, &by, now)?;

    let mut change = Change::new(Ack::Durable);
    for (path, bytes) in files {
        change = change.create(path, bytes);
    }
    let reply = serde_json::json!({
        "organization_id": command.organization_id.as_str(),
        "office_id": command.office_id.as_str(),
        "owner_member_id": command.owner_member_id.as_str(),
        "generation": generation,
    });
    Ok(change.reply(serde_json::to_vec(&reply).expect("reply JSON encodes")))
}

fn ensure_owner_person(
    files: &mut BTreeMap<DurablePath, Vec<u8>>,
    command: &ImportOrganization,
    by: &By,
    now: Timestamp,
) -> Result<(), Refusal> {
    let existing_active = command
        .imported
        .summary
        .people
        .iter()
        .any(|p| p.id == command.owner_member_id.as_str());
    if existing_active {
        return Ok(());
    }
    let Some(creator) = &command.new_owner_person else {
        return Err(Refusal::invalid(
            "owner_member_id must name an active person in the organization",
        ));
    };
    if creator.employee_id != command.owner_member_id {
        return Err(Refusal::invalid("new_owner_person employee_id mismatch"));
    }
    if !records::is_line(&creator.name) {
        return Err(Refusal::invalid("new_owner_person name is invalid"));
    }
    if !org_proto::selected_bootstrap::valid_handle(&creator.handle)
        || creator.handle == org_proto::selected_bootstrap::HR_HANDLE
    {
        return Err(Refusal::invalid("new_owner_person handle is invalid"));
    }
    if command
        .imported
        .summary
        .reserved_handles
        .iter()
        .any(|h| h == &creator.handle)
    {
        return Err(Refusal::invalid("new_owner_person handle already exists"));
    }
    if command
        .imported
        .summary
        .reserved_entity_ids
        .iter()
        .any(|id| id == creator.employee_id.as_str())
    {
        return Err(Refusal::invalid(
            "new_owner_person employee_id collides with an existing record",
        ));
    }
    let Some(root_team_id) = command
        .imported
        .summary
        .has_active_root_team
        .then_some(command.imported.summary.root_team_id.as_ref())
        .flatten()
    else {
        return Err(Refusal::invalid(
            "imported organization has no active root team",
        ));
    };
    let emp = Employee {
        id: creator.employee_id.clone(),
        handle: creator.handle.clone(),
        kind: EmployeeKind::Person,
        name: creator.name.clone(),
        home_team_id: root_team_id.clone(),
        role_id: None,
        reports_to_employee_id: None,
        status: EmployeeStatus::Active,
        contact: None,
        title: None,
        focus: None,
        role_text: None,
        title_in_team: None,
        offboarded_at: None,
        may_hire: true,
        builtin_key: None,
        meta: Meta::created(by, now),
        body: String::new(),
    };
    let path = records::employee_path(
        std::slice::from_ref(root_team_id),
        EmployeeKind::Person,
        &emp.id,
    )
    .map_err(|_| Refusal::invalid("invalid owner person path"))?;
    files.insert(path, records::encode_employee(&emp));
    Ok(())
}

fn rewrite_org_and_repo(
    files: &mut BTreeMap<DurablePath, Vec<u8>>,
    command: &ImportOrganization,
    by: &By,
    now: Timestamp,
) -> Result<(), Refusal> {
    let org_path = records::organization_path();
    let org_bytes = files
        .get(&org_path)
        .ok_or_else(|| Refusal::invalid("missing organization.yaml"))?;
    let mut org =
        records::decode_organization(org_bytes).map_err(|_| Refusal::invalid("invalid org"))?;
    let repo_id = org.organization_repository_id.clone();
    if org.state_branch != command.imported.state_branch {
        org.state_branch = command.imported.state_branch.clone();
        org.meta.revision = org.meta.revision.saturating_add(1);
        org.meta.updated_at = Some(records::time_text(now));
        org.meta.updated_by = by.clone();
        files.insert(org_path, records::encode_organization(&org));
    }
    let remote = command.imported.repository_remote_url.as_str();
    if !remote.starts_with("https://") && !remote.starts_with("ssh://") {
        return Ok(());
    }
    let repos_path = records::repositories_path();
    let Some(repo_bytes) = files.get(&repos_path) else {
        return Ok(());
    };
    let mut repos = records::decode_repositories(repo_bytes)
        .map_err(|_| Refusal::invalid("invalid repositories.yaml"))?;
    let Some(repo) = repos.iter_mut().find(|r| r.id == repo_id) else {
        return Ok(());
    };
    if repo.remote_url.as_deref() == Some(remote) {
        return Ok(());
    }
    repo.remote_url = Some(remote.to_owned());
    repo.meta.revision = repo.meta.revision.saturating_add(1);
    repo.meta.updated_at = Some(records::time_text(now));
    repo.meta.updated_by = by.clone();
    files.insert(repos_path, records::encode_repositories(&repos));
    Ok(())
}

fn rewrite_offices_and_codes(
    files: &mut BTreeMap<DurablePath, Vec<u8>>,
    command: &ImportOrganization,
    by: &By,
    now: Timestamp,
) -> Result<(), Refusal> {
    let now_text = records::time_text(now);
    let mut used_slugs = BTreeSet::new();
    let office_paths: Vec<(DurablePath, Id)> = files
        .keys()
        .filter_map(|p| match records::kind_of(p) {
            Some(Kind::Office(id)) => Some((p.clone(), id)),
            _ => None,
        })
        .collect();
    let mut old_hq_id: Option<Id> = None;
    for (path, id) in &office_paths {
        let office = records::decode_office(&files[path])
            .map_err(|_| Refusal::invalid("invalid office record"))?;
        if office.role == OfficeRole::Hq {
            old_hq_id = Some(id.clone());
        } else {
            used_slugs.insert(office.slug);
        }
    }
    if let Some(old_id) = &old_hq_id {
        let old_path = records::office_path(old_id);
        let mut old_office = records::decode_office(&files[&old_path])
            .map_err(|_| Refusal::invalid("invalid old HQ office"))?;
        old_office.role = OfficeRole::Satellite;
        if old_office.slug == HQ_SLUG {
            old_office.slug = unique_replaced_slug(&used_slugs, old_id);
        }
        used_slugs.insert(old_office.slug.clone());
        old_office.meta.revision = old_office.meta.revision.saturating_add(1);
        old_office.meta.updated_at = Some(now_text.clone());
        old_office.meta.updated_by = by.clone();
        files.insert(old_path, records::encode_office(&old_office));
    }
    let new_hq = Office {
        id: command.office_id.clone(),
        slug: HQ_SLUG.to_owned(),
        name: command.office_name.clone(),
        role: OfficeRole::Hq,
        meta: Meta::created(by, now),
    };
    files.insert(
        records::office_path(&command.office_id),
        records::encode_office(&new_hq),
    );

    let codes_path = records::office_codes_path();
    let mut codes = match files.get(&codes_path) {
        Some(bytes) => records::decode_office_codes(bytes)
            .map_err(|_| Refusal::invalid("invalid office-codes.yaml"))?,
        None => Vec::new(),
    };
    if let Some(old_id) = &old_hq_id {
        for entry in &mut codes {
            if entry.office_id == *old_id && entry.retired_at.is_none() {
                entry.retired_at = Some(now_text.clone());
            }
        }
    }
    let next_code = next_office_code(&codes)
        .ok_or_else(|| Refusal::invalid("no office codes left to assign"))?;
    codes.push(OfficeCode {
        code: next_code,
        office_id: command.office_id.clone(),
        issued_at: now_text,
        retired_at: None,
    });
    files.insert(codes_path, records::encode_office_codes(&codes));
    Ok(())
}

fn unique_replaced_slug(used: &BTreeSet<String>, old_id: &Id) -> String {
    let prefix: String = old_id
        .as_str()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    let base = format!("hq-previous-{prefix}");
    if !used.contains(&base) {
        return base;
    }
    for n in 2u32..1000 {
        let candidate = format!("{base}-{n}");
        if !used.contains(&candidate) {
            return candidate;
        }
    }
    format!("hq-prev-{prefix}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TestOffice;

    #[test]
    fn next_office_code_skips_used_and_ambiguous_letters() {
        let now = "2026-11-21T00:00:00.000Z".to_owned();
        let oid = Id::parse("01900000-0000-7000-8000-000000000001").unwrap();
        let mk = |code: &str| OfficeCode {
            code: code.into(),
            office_id: oid.clone(),
            issued_at: now.clone(),
            retired_at: None,
        };
        assert_eq!(next_office_code(&[mk("A")]).as_deref(), Some("B"));
        let all_single: Vec<OfficeCode> = b"ABCDEFGHJKLMNPQRSTUVWXYZ"
            .iter()
            .map(|&b| mk(&(b as char).to_string()))
            .collect();
        assert_eq!(next_office_code(&all_single).as_deref(), Some("AA"));
    }

    #[test]
    fn check_git_source_rejects_ext_credentials_and_option_injection() {
        for bad in [
            ("ext::sh -c id", "org/state"),
            ("-u", "org/state"),
            ("file://relative/path", "org/state"),
            ("https://user:pass@example.test/org.git", "org/state"),
            ("https://example.test/org.git?token=secret", "org/state"),
            ("ssh://-oProxyCommand=id@example.test/org.git", "org/state"),
            ("ssh://-host/org.git", "org/state"),
            ("https://example.test/org.git", "-bad"),
            ("https://example.test/org.git", "org..state"),
            ("https://example.test/org.git", "org/@{0}"),
            ("https://example.test/org.git", "org/state.lock"),
        ] {
            assert!(
                check_git_source(bad.0, bad.1).is_err(),
                "expected rejection for {bad:?}"
            );
        }
        assert!(check_git_source("https://example.test/org.git", "org/state").is_ok());
        assert!(check_git_source("ssh://git@example.test/org.git", "org/state").is_ok());
        assert!(check_git_source("file:///tmp/repo.git", "org/state").is_ok());
    }

    #[test]
    fn sanitize_git_stderr_prefers_fatal_and_redacts_staging_and_credentials() {
        let staging = Path::new("/tmp/org-import-12345");
        let stderr = "Cloning into '/tmp/org-import-12345/repo.git'...\nfatal: repository 'https://alice:topsecret@example.test/org.git' not found in /tmp/org-import-12345/repo.git\n";
        let msg = sanitize_git_stderr(stderr, staging, "clone failed");
        assert!(msg.starts_with("fatal:"), "{msg}");
        assert!(!msg.contains("/tmp/org-import-12345"), "{msg}");
        assert!(!msg.contains("topsecret"), "{msg}");
        assert!(msg.contains("https://example.test/org.git"), "{msg}");
    }

    #[test]
    fn parse_ls_tree_rejects_symlinks_and_submodules_before_blob_read() {
        let oid = "0123456789abcdef0123456789abcdef01234567";
        let symlink_raw =
            format!("100644 blob {oid} 10\torganization.yaml\0120000 blob {oid} 8\tlink.yaml\0");
        match parse_ls_tree_entries(symlink_raw.as_bytes()).unwrap_err() {
            ImportError::InvalidFile { path, reason } => {
                assert_eq!(path, "state/link.yaml");
                assert!(reason.contains("symlink"), "{reason}");
            }
            other => panic!("expected symlink refusal, got {other:?}"),
        }

        let submodule_raw =
            format!("100644 blob {oid} 10\torganization.yaml\0160000 commit {oid} -\tsubmod\0");
        match parse_ls_tree_entries(submodule_raw.as_bytes()).unwrap_err() {
            ImportError::InvalidFile { path, reason } => {
                assert_eq!(path, "state/submod");
                assert!(reason.contains("submodule"), "{reason}");
            }
            other => panic!("expected submodule refusal, got {other:?}"),
        }
    }

    #[test]
    fn validate_state_dir_accepts_valid_org_and_names_invalid_or_unknown_files() {
        let office = crate::Office::for_test(TestOffice::default());
        let state_dir = office.durable_root().join("state");
        let imported = validate_state_dir(&state_dir, "https://example.test/acme.git", "org/state")
            .expect("valid created state must pass import validation");
        assert_eq!(imported.summary.name, office.info().org_name);
        assert_eq!(imported.summary.teams_count, 1);
        assert_eq!(imported.summary.people.len(), 1);
        assert_eq!(imported.summary.agents_count, 1);
        assert!(imported.summary.has_active_root_team);
        assert!(imported.summary.reserved_handles.iter().any(|h| h == "hr"));
        assert!(!imported.local_only_not_restored.is_empty());

        // Copy to a scratch tree and inject an unrecognized file -> refused naming the exact path.
        let copy = tempfile::tempdir().unwrap();
        copy_tree(&state_dir, copy.path());
        fs::write(copy.path().join("rogue.txt"), b"not an org file").unwrap();
        let err = validate_state_dir(copy.path(), "https://example.test/acme.git", "org/state")
            .unwrap_err();
        match err {
            ImportError::InvalidFile { path, .. } => assert_eq!(path, "state/rogue.txt"),
            other => panic!("expected InvalidFile naming rogue.txt, got {other:?}"),
        }

        // Remove rogue file and corrupt organization.yaml -> refused naming state/organization.yaml.
        fs::remove_file(copy.path().join("rogue.txt")).unwrap();
        fs::write(copy.path().join("organization.yaml"), b"not: [valid").unwrap();
        let err = validate_state_dir(copy.path(), "https://example.test/acme.git", "org/state")
            .unwrap_err();
        match err {
            ImportError::InvalidFile { path, .. } => assert_eq!(path, "state/organization.yaml"),
            other => panic!("expected InvalidFile naming organization.yaml, got {other:?}"),
        }
    }

    fn copy_tree(src: &Path, dst: &Path) {
        for entry in fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let target = dst.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                fs::create_dir_all(&target).unwrap();
                copy_tree(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), &target).unwrap();
            }
        }
    }
}
