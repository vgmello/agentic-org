//! A workspace's external sign-in state, kept outside Git (`TASK-137`):
//! `<application base>/agent-os/workspaces/<sha256-hex(canonical workspace root)>/` (`0700`) with
//! `binding.v1.json`, `key.ed25519.pkcs8` and `token.v1`, each `0600` and never opened through a
//! symbolic link. A development build may move the root with `AGENT_OS_STATE_DIR` or
//! `ORG_DEV_ACCOUNT_BASE`; a release build refuses both.

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use org_command::Refusal;
use org_proto::ErrorCode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// Moves the state root (development builds only).
pub const STATE_DIR_ENV: &str = "AGENT_OS_STATE_DIR";
/// A development build's account base; the state root is `<base>/agent-os`.
pub const DEV_ACCOUNT_BASE_ENV: &str = "ORG_DEV_ACCOUNT_BASE";

const BINDING_FILE: &str = "binding.v1.json";
const KEY_FILE: &str = "key.ed25519.pkcs8";
const TOKEN_FILE: &str = "token.v1";
const MAX_STATE_FILE_BYTES: u64 = 16 * 1024;

/// Where this workspace signs in: the office and the agent, never a secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceBinding {
    pub version: u32,
    pub workspace_root: String,
    pub office_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socket: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_dir: Option<String>,
    pub member_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    pub session_id: String,
    pub lease_expires_at: String,
}

/// The disposable fast token (refreshed by challenge-response; deleted on logout), wiped from
/// memory when dropped.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenFile {
    pub version: u32,
    pub session_id: String,
    #[serde(with = "org_proto::external_auth::secret_string")]
    pub token: Zeroizing<String>,
    pub expires_at: String,
}

impl std::fmt::Debug for TokenFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenFile")
            .field("session_id", &self.session_id)
            .field("token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl TokenFile {
    /// Whether the token is past (or within `margin_ms` of) its expiry at `now_ms`.
    pub fn is_stale(&self, now_ms: u64, margin_ms: u64) -> bool {
        org_proto::parse_rfc3339_ms(&self.expires_at)
            .is_none_or(|expires| now_ms.saturating_add(margin_ms) >= expires)
    }
}

fn refusal(message: impl Into<String>) -> Refusal {
    Refusal::new(
        ErrorCode::Unavailable,
        message,
        "Check the agent-os workspace state folder (owner-only, no symbolic links) and retry.",
    )
}

/// The state root for this account, from `env` and `HOME`.
pub fn state_root_from(
    env: &dyn Fn(&str) -> Option<OsString>,
    development: bool,
) -> Result<PathBuf, Refusal> {
    let set = |name: &str| env(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    for (name, suffix) in [
        (STATE_DIR_ENV, None),
        (DEV_ACCOUNT_BASE_ENV, Some("agent-os")),
    ] {
        if let Some(dir) = set(name) {
            if !development {
                return Err(refusal(format!(
                    "{name} is a development setting; this release build refuses it"
                )));
            }
            if !dir.is_absolute() {
                return Err(refusal(format!("{name} must be an absolute path")));
            }
            return Ok(match suffix {
                Some(suffix) => dir.join(suffix),
                None => dir,
            });
        }
    }
    let home = set("HOME")
        .filter(|h| h.is_absolute())
        .ok_or_else(|| refusal("HOME is not set to an absolute path"))?;
    org_platform::dirs::application_base(&home)
        .map(|base| base.join("agent-os"))
        .map_err(|_| refusal("this system has no per-user application folder"))
}

/// The state root of this process.
pub fn state_root() -> Result<PathBuf, Refusal> {
    state_root_from(&|name| std::env::var_os(name), cfg!(debug_assertions))
}

/// The folder name of a workspace: SHA-256 hex of its canonical path.
pub fn workspace_key(canonical_root: &Path) -> String {
    let digest = Sha256::digest(canonical_root.as_os_str().as_encoded_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether `dir` holds a `.git` entry (a repository or a worktree).
fn is_git_root(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir.join(".git")).is_ok()
}

/// The default workspace root for `cwd`: its Git root, else `cwd` itself (canonical).
pub fn default_workspace_root(cwd: &Path) -> Result<PathBuf, Refusal> {
    let canonical = cwd
        .canonicalize()
        .map_err(|e| refusal(format!("cannot resolve the working folder: {e}")))?;
    Ok(canonical
        .ancestors()
        .find(|dir| is_git_root(dir))
        .unwrap_or(&canonical)
        .to_path_buf())
}

/// The candidate workspace roots for `cwd`: the canonical folder and each parent up to and
/// including the Git root (or the filesystem root).
pub fn candidate_roots(cwd: &Path) -> Vec<PathBuf> {
    let Ok(canonical) = cwd.canonicalize() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for dir in canonical.ancestors() {
        out.push(dir.to_path_buf());
        if is_git_root(dir) {
            break;
        }
    }
    out
}

/// One workspace's state folder.
#[derive(Debug, Clone)]
pub struct WorkspaceState {
    state_root: Option<PathBuf>,
    dir: PathBuf,
}

fn check_real_owned_dir(path: &Path, uid: u32, require_exists: bool) -> Result<bool, Refusal> {
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(refusal(format!(
            "the workspace state folder {} must not contain `..`",
            path.display()
        )));
    }
    let norm: PathBuf = path.components().collect();
    match std::fs::symlink_metadata(&norm) {
        Ok(meta) => {
            if !meta.file_type().is_dir() {
                return Err(refusal(format!(
                    "{} must be a real directory (never a symbolic link)",
                    norm.display()
                )));
            }
            if meta.uid() != uid {
                return Err(refusal(format!(
                    "{} is not owned by the current account",
                    norm.display()
                )));
            }
            Ok(true)
        }
        Err(e) if !require_exists && e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(refusal(format!(
            "cannot inspect the workspace state folder {}: {e}",
            norm.display()
        ))),
    }
}

fn check_owner_only_dir(path: &Path, uid: u32, require_exists: bool) -> Result<bool, Refusal> {
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(refusal(format!(
            "the workspace state folder {} must not contain `..`",
            path.display()
        )));
    }
    let norm: PathBuf = path.components().collect();
    match std::fs::symlink_metadata(&norm) {
        Ok(_) => {
            org_platform::fs::verify_owner_only_dir(&norm, uid)
                .map_err(|e| refusal(format!("the workspace state folder is not safe: {e}")))?;
            Ok(true)
        }
        Err(e) if !require_exists && e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(refusal(format!(
            "cannot inspect the workspace state folder {}: {e}",
            norm.display()
        ))),
    }
}

impl WorkspaceState {
    /// The state folder of `canonical_root` under `root` (not created).
    pub fn for_root(root: &Path, canonical_root: &Path) -> Self {
        let state_root: PathBuf = root.components().collect();
        Self {
            dir: state_root
                .join("workspaces")
                .join(workspace_key(canonical_root)),
            state_root: Some(state_root),
        }
    }

    /// An existing state folder, as recorded in a resolved binding.
    pub fn at(dir: PathBuf) -> Self {
        let norm: PathBuf = dir.components().collect();
        let state_root = norm
            .parent()
            .filter(|parent| parent.file_name() == Some(OsStr::new("workspaces")))
            .and_then(Path::parent)
            .filter(|root| !root.as_os_str().is_empty())
            .map(Path::to_path_buf);
        Self {
            state_root,
            dir: norm,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Whether the folder holds a binding.
    pub fn has_binding(&self) -> bool {
        std::fs::symlink_metadata(self.dir.join(BINDING_FILE)).is_ok_and(|m| m.is_file())
    }

    /// Creates the folder (and its missing parents) owner-only and checks that `state_root`,
    /// `state_root/workspaces`, and `self.dir` are all real, safe directories of this account
    /// (never symbolic links).
    pub fn ensure(&self) -> Result<(), Refusal> {
        self.preflight()?;
        org_platform::fs::create_owner_only_dir_all(&self.dir)
            .map_err(|e| refusal(format!("cannot create the workspace state folder: {e}")))?;
        self.verify()
    }

    /// Checks every existing directory in `[state_root, state_root/workspaces, self.dir]` without
    /// creating any directory: `state_root` must be a real directory owned by the current uid,
    /// and `state_root/workspaces` and `self.dir` must be owner-only (`0700`) real directories.
    pub fn preflight(&self) -> Result<(), Refusal> {
        let uid = org_platform::fs::effective_uid()
            .map_err(|e| refusal(format!("cannot read this account's id: {e}")))?;
        if let Some(root) = &self.state_root {
            if !check_real_owned_dir(root, uid, false)? {
                return Ok(());
            }
            if !check_owner_only_dir(&root.join("workspaces"), uid, false)? {
                return Ok(());
            }
        }
        let _ = check_owner_only_dir(&self.dir, uid, false)?;
        Ok(())
    }

    fn verify(&self) -> Result<(), Refusal> {
        let uid = org_platform::fs::effective_uid()
            .map_err(|e| refusal(format!("cannot read this account's id: {e}")))?;
        if let Some(root) = &self.state_root {
            check_real_owned_dir(root, uid, true)?;
            check_owner_only_dir(&root.join("workspaces"), uid, true)?;
        }
        check_owner_only_dir(&self.dir, uid, true)?;
        Ok(())
    }

    fn read_file(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>, Refusal> {
        let path = self.dir.join(name);
        let file = match org_platform::fs::open_nofollow_ro(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(refusal(format!("cannot read {name}: {e}"))),
        };
        let mut bytes = Zeroizing::new(Vec::new());
        file.take(MAX_STATE_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| refusal(format!("cannot read {name}: {e}")))?;
        if bytes.len() as u64 > MAX_STATE_FILE_BYTES {
            return Err(refusal(format!("{name} is too large")));
        }
        Ok(Some(bytes))
    }

    /// Writes `name` atomically: a fresh `0600` file (never through a link), then a rename.
    fn write_file(&self, name: &str, bytes: &[u8]) -> Result<(), Refusal> {
        self.verify()?;
        let suffix = crate::runtime::keys::try_new_uuid_v7().unwrap_or_else(|| "tmp".into());
        let tmp = self.dir.join(format!(".{name}.{suffix}.tmp"));
        let written = org_platform::fs::write_owner_only_file(&tmp, bytes)
            .and_then(|()| std::fs::rename(&tmp, self.dir.join(name)));
        written.map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            refusal(format!("cannot write {name}: {e}"))
        })
    }

    pub fn read_binding(&self) -> Result<Option<WorkspaceBinding>, Refusal> {
        self.verify()?;
        let Some(bytes) = self.read_file(BINDING_FILE)? else {
            return Ok(None);
        };
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| refusal("binding.v1.json is not a valid workspace binding"))
    }

    pub fn write_binding(&self, binding: &WorkspaceBinding) -> Result<(), Refusal> {
        let bytes = serde_json::to_vec_pretty(binding).map_err(|e| refusal(e.to_string()))?;
        self.write_file(BINDING_FILE, &bytes)
    }

    /// The workspace's Ed25519 private key (PKCS#8 DER). Never printed.
    pub fn read_key(&self) -> Result<Option<Zeroizing<Vec<u8>>>, Refusal> {
        self.verify()?;
        self.read_file(KEY_FILE)
    }

    pub fn write_key(&self, pkcs8: &[u8]) -> Result<(), Refusal> {
        self.write_file(KEY_FILE, pkcs8)
    }

    pub fn read_token(&self) -> Result<Option<TokenFile>, Refusal> {
        self.verify()?;
        let Some(bytes) = self.read_file(TOKEN_FILE)? else {
            return Ok(None);
        };
        Ok(serde_json::from_slice(&bytes).ok())
    }

    pub fn write_token(&self, token: &TokenFile) -> Result<(), Refusal> {
        let bytes = Zeroizing::new(serde_json::to_vec(token).map_err(|e| refusal(e.to_string()))?);
        self.write_file(TOKEN_FILE, &bytes)
    }

    /// Removes any cached fast token (`token.v1`), ignoring a missing file.
    pub fn remove_token(&self) -> Result<(), Refusal> {
        self.preflight()?;
        match std::fs::remove_file(self.dir.join(TOKEN_FILE)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(refusal(format!("cannot remove {TOKEN_FILE}: {e}"))),
        }
    }

    /// Removes the binding, key and token (and the folder when it is then empty).
    pub fn remove(&self) -> Result<(), Refusal> {
        for name in [TOKEN_FILE, KEY_FILE, BINDING_FILE] {
            match std::fs::remove_file(self.dir.join(name)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(refusal(format!("cannot remove {name}: {e}"))),
            }
        }
        let _ = std::fs::remove_dir(&self.dir);
        Ok(())
    }
}

/// The signed-in workspace for `cwd`: the nearest candidate root with a binding.
pub fn find_workspace(root: &Path, cwd: &Path) -> Option<(PathBuf, WorkspaceState)> {
    candidate_roots(cwd).into_iter().find_map(|dir| {
        let state = WorkspaceState::for_root(root, &dir);
        state.has_binding().then_some((dir, state))
    })
}

/// `~/…` for a path under `HOME`, for a readable workspace label.
pub fn tilde_path(path: &Path, home: Option<&Path>) -> String {
    if let Some(rest) = home.and_then(|home| path.strip_prefix(home).ok()) {
        let rest: PathBuf = rest
            .components()
            .filter(|c| matches!(c, Component::Normal(_)))
            .collect();
        if rest.as_os_str().is_empty() {
            return "~".into();
        }
        return format!("~/{}", rest.display());
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests;
