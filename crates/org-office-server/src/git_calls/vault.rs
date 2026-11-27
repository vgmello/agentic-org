//! Encrypted per-repository Git HTTPS credential custody (`TASK-135`, `doc-35`, `ADR-0006`).
//!
//! Stores scoped per-repository HTTPS tokens strictly outside `durable/` and `runtime/` under
//! `<office.folder()>/git-credentials/repos/<repository_id>/credential.v1.json`, wrapped with
//! `org_pki::wrap` (`KeyClass::GitCredential`, AES-256-GCM) and bound to
//! `(organization_id, office_id, repository_id, scope_url)`.
//!
//! Credentials never enter agent homes, agent sandboxes, `.git/config`, bare-cache `config`,
//! `git-state.json`, durable backups, or logs.

use std::fs;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use org_git::{validate_remote_url, AllowedProtocol, GitCredential};
use org_office::organization_family::records::{Repository, RepositoryKind};
use org_office::Office;
use org_pki::wrap::{
    derive_wrapping_key, unwrap_git_credential, wrap_git_credential, GitCredentialBinding, KeyClass,
};
use org_proto::git::RepoCredentialView;
use org_proto::{ApiError, ErrorCode};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const VAULT_RECORD_VERSION: u32 = 1;
const CREDENTIAL_FILE_NAME: &str = "credential.v1.json";
const DEFAULT_HTTPS_USERNAME: &str = "x-access-token";
const MAX_USERNAME_BYTES: usize = 128;
const MAX_TOKEN_BYTES: usize = 4096;
const MAX_VAULT_FILE_BYTES: u64 = 16 * 1024;

/// Non-secret envelope persisted at `<office.folder()>/git-credentials/repos/<repo_id>/credential.v1.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredGitCredential {
    version: u32,
    organization_id: String,
    office_id: String,
    repository_id: String,
    scope_url: String,
    username: String,
    updated_at: String,
    wrapped_b64: String,
}

/// Decrypted secret payload wrapped inside [`StoredGitCredential::wrapped_b64`].
#[derive(Serialize, Deserialize)]
struct GitTokenSecret {
    username: String,
    token: String,
}

impl Zeroize for GitTokenSecret {
    fn zeroize(&mut self) {
        self.username.zeroize();
        self.token.zeroize();
    }
}

impl Drop for GitTokenSecret {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl ZeroizeOnDrop for GitTokenSecret {}

/// Validates and canonicalizes an HTTPS remote URL into a repository-exact credential scope
/// (`https://<host>[:<port>]/<path>`, trimmed of trailing `/`, rejecting embedded credentials,
/// wildcards, queries, fragments, and option/config-injection characters).
pub(super) fn canonical_https_repo_scope(url: &str) -> Result<String, ApiError> {
    let trimmed = url.trim().trim_end_matches('/');
    let protocol = validate_remote_url(trimmed).map_err(|_| {
        ApiError::new(
            ErrorCode::InvalidRequest,
            "repository credential requires a valid https:// remote_url",
        )
    })?;
    if protocol != AllowedProtocol::Https {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "scoped vault tokens apply only to https:// repositories",
        ));
    }
    if trimmed.contains(['?', '#', '=', '*', '\'', '"', ';', '\\']) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "HTTPS remote_url contains disallowed characters for scoped credentials",
        ));
    }
    Ok(trimmed.to_owned())
}

fn validate_username(raw: Option<&str>) -> Result<String, ApiError> {
    let user = raw.map(str::trim).unwrap_or(DEFAULT_HTTPS_USERNAME);
    let effective = if user.is_empty() {
        DEFAULT_HTTPS_USERNAME
    } else {
        user
    };
    if effective.len() > MAX_USERNAME_BYTES
        || effective
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == ':')
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "username must be 1..=128 characters without whitespace, control chars, or `:`",
        ));
    }
    Ok(effective.to_owned())
}

fn validate_token(raw: &str) -> Result<Zeroizing<String>, ApiError> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.len() > MAX_TOKEN_BYTES
        || trimmed.chars().any(char::is_control)
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "repository token must be 1..=4096 bytes with no control characters",
        ));
    }
    Ok(Zeroizing::new(trimmed.to_owned()))
}

fn require_valid_repo_id(repo_id: &str) -> Result<(), ApiError> {
    if !org_proto::is_uuid_v7(repo_id) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "invalid repository_id for credential storage",
        ));
    }
    Ok(())
}

fn derive_git_wrapping_key(office: &Office) -> Result<Zeroizing<[u8; 32]>, ApiError> {
    let Some(master) = office.secrets_key() else {
        return Err(ApiError::new(
            ErrorCode::Unavailable,
            "office secrets key unavailable; cannot store or unwrap encrypted repository Git credentials",
        ));
    };
    derive_wrapping_key(master.expose_bytes(), KeyClass::GitCredential).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not derive office Git credential wrapping key",
        )
    })
}

fn repo_cred_dirs(office: &Office, repo_id: &str) -> (PathBuf, PathBuf, PathBuf) {
    let creds_root = office.folder().join("git-credentials");
    let repos_dir = creds_root.join("repos");
    let repo_dir = repos_dir.join(repo_id);
    (creds_root, repos_dir, repo_dir)
}

fn effective_uid_or_err() -> Result<u32, ApiError> {
    org_platform::fs::effective_uid().map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "office git credentials are unsupported on this platform",
        )
    })
}

fn ensure_owner_only_vault_dirs(office: &Office, repo_id: &str) -> Result<PathBuf, ApiError> {
    let uid = effective_uid_or_err()?;
    let (creds_root, repos_dir, repo_dir) = repo_cred_dirs(office, repo_id);
    org_platform::fs::set_owner_only_dir(office.folder()).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "office folder failed owner-only verification",
        )
    })?;
    for dir in [&creds_root, &repos_dir, &repo_dir] {
        org_platform::fs::create_owner_only_dir_all(dir)
            .and_then(|()| org_platform::fs::set_owner_only_dir(dir))
            .map_err(|_| {
                ApiError::new(
                    ErrorCode::StorageError,
                    "could not prepare owner-only office git-credentials directory",
                )
            })?;
    }
    org_platform::fs::verify_owner_only_ancestors(&repo_dir, office.folder(), uid).map_err(
        |_| {
            ApiError::new(
                ErrorCode::StorageError,
                "office git-credentials directory hierarchy failed owner-only verification",
            )
        },
    )?;
    fs::canonicalize(&repo_dir).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not resolve office git-credentials directory",
        )
    })
}

fn sync_path(path: &Path) -> Result<(), ApiError> {
    let file = fs::File::open(path).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not open office git-credentials path for sync",
        )
    })?;
    org_platform::fs::full_sync(&file).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not flush office git-credentials to disk",
        )
    })
}

fn read_verified_envelope(
    office: &Office,
    repo_id: &str,
) -> Result<Option<StoredGitCredential>, ApiError> {
    require_valid_repo_id(repo_id)?;
    let (creds_root, repos_dir, repo_dir) = repo_cred_dirs(office, repo_id);
    for (dir, label) in [
        (&creds_root, "git-credentials directory"),
        (&repos_dir, "git-credentials repos directory"),
        (&repo_dir, "git-credentials repository directory"),
    ] {
        match fs::symlink_metadata(dir) {
            Ok(meta) if meta.file_type().is_dir() => {}
            Ok(_) => {
                return Err(ApiError::new(
                    ErrorCode::StorageError,
                    format!("office {label} must be a real directory (never a symlink)"),
                ));
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => {
                return Err(ApiError::new(
                    ErrorCode::StorageError,
                    format!("could not inspect office {label}"),
                ));
            }
        }
    }
    let cred_path = repo_dir.join(CREDENTIAL_FILE_NAME);
    match fs::symlink_metadata(&cred_path) {
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(ApiError::new(
                ErrorCode::StorageError,
                "could not inspect office repository git credential",
            ));
        }
    }
    let uid = effective_uid_or_err()?;
    org_platform::fs::verify_owner_only_ancestors(&repo_dir, office.folder(), uid).map_err(
        |_| {
            ApiError::new(
                ErrorCode::StorageError,
                "office git-credentials directory hierarchy failed owner-only verification",
            )
        },
    )?;
    let verified_path =
        org_platform::fs::verify_owner_only_file(&cred_path, uid, false).map_err(|_| {
            ApiError::new(
                ErrorCode::StorageError,
                "office repository git credential failed owner-only verification",
            )
        })?;
    let file = org_platform::fs::open_nofollow_ro(&verified_path).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not open office repository git credential",
        )
    })?;
    let mut raw = Vec::new();
    file.take(MAX_VAULT_FILE_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|_| {
            ApiError::new(
                ErrorCode::StorageError,
                "could not read office repository git credential",
            )
        })?;
    if raw.len() as u64 > MAX_VAULT_FILE_BYTES {
        return Err(ApiError::new(
            ErrorCode::StorageError,
            "office repository git credential exceeds maximum size",
        ));
    }
    let record: StoredGitCredential = serde_json::from_slice(&raw).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "malformed office repository git credential file",
        )
    })?;
    let info = office.info();
    if record.version != VAULT_RECORD_VERSION
        || record.organization_id != info.org_id
        || record.office_id != info.office_id
        || record.repository_id != repo_id
    {
        return Err(ApiError::new(
            ErrorCode::StorageError,
            "office repository git credential binding metadata mismatch",
        ));
    }
    Ok(Some(record))
}

/// Encrypts and atomically stores a scoped per-repository HTTPS token in this office's local
/// vault (`<office.folder()>/git-credentials/repos/<repo_id>/credential.v1.json`).
pub(super) fn store_repo_https_token(
    office: &Office,
    repo: &Repository,
    username: Option<&str>,
    token: &str,
) -> Result<RepoCredentialView, ApiError> {
    if repo.kind != RepositoryKind::Remote {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "local repositories do not accept remote credentials",
        ));
    }
    let url = repo.remote_url.as_deref().ok_or_else(|| {
        ApiError::new(
            ErrorCode::InvalidRequest,
            "remote repository has no remote_url",
        )
    })?;
    let scope_url = canonical_https_repo_scope(url)?;
    let repo_id = repo.id.as_str();
    require_valid_repo_id(repo_id)?;
    let username = validate_username(username)?;
    let token = validate_token(token)?;
    let key = derive_git_wrapping_key(office)?;

    let secret = GitTokenSecret {
        username: username.clone(),
        token: token.as_str().to_owned(),
    };
    let plaintext = Zeroizing::new(serde_json::to_vec(&secret).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not serialize repository git credential payload",
        )
    })?);
    let info = office.info();
    let binding = GitCredentialBinding {
        organization_id: &info.org_id,
        office_id: &info.office_id,
        repository_id: repo_id,
        scope_url: &scope_url,
    };
    let wrapped = wrap_git_credential(&key, &binding, &plaintext).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not encrypt repository git credential",
        )
    })?;

    let updated_at = now_rfc3339();
    let envelope = StoredGitCredential {
        version: VAULT_RECORD_VERSION,
        organization_id: info.org_id.clone(),
        office_id: info.office_id.clone(),
        repository_id: repo_id.to_owned(),
        scope_url: scope_url.clone(),
        username: username.clone(),
        updated_at: updated_at.clone(),
        wrapped_b64: BASE64.encode(&wrapped),
    };
    let mut bytes = serde_json::to_vec_pretty(&envelope).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not encode repository git credential envelope",
        )
    })?;
    bytes.push(b'\n');

    let repo_dir = ensure_owner_only_vault_dirs(office, repo_id)?;
    org_platform::fs::write_regular_beneath(&repo_dir, Path::new(CREDENTIAL_FILE_NAME), &bytes)
        .map_err(|_| {
            ApiError::new(
                ErrorCode::StorageError,
                "could not persist encrypted repository git credential",
            )
        })?;
    sync_path(&repo_dir.join(CREDENTIAL_FILE_NAME))?;
    sync_path(&repo_dir)?;

    Ok(RepoCredentialView {
        office_id: info.office_id.clone(),
        repository_id: repo_id.to_owned(),
        repository_key: repo.key.clone(),
        configured: true,
        source: Some("vault".into()),
        scope_url: Some(scope_url),
        username: Some(username),
        updated_at: Some(updated_at),
    })
}

/// Unwraps a per-repository HTTPS token from this office's local vault if present and bound to
/// `url`. Fails closed if the file exists but cannot be verified or decrypted.
pub(super) fn load_repo_https_token(
    office: &Office,
    repo_id: &str,
    url: &str,
) -> Result<Option<GitCredential>, ApiError> {
    let Some(record) = read_verified_envelope(office, repo_id)? else {
        return Ok(None);
    };
    let scope_url = canonical_https_repo_scope(url)?;
    if record.scope_url != scope_url {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "stored repository git credential is bound to a different remote URL; re-configure it",
        ));
    }
    let key = derive_git_wrapping_key(office)?;
    let ciphertext = BASE64.decode(&record.wrapped_b64).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "office repository git credential ciphertext is malformed",
        )
    })?;
    let info = office.info();
    let binding = GitCredentialBinding {
        organization_id: &info.org_id,
        office_id: &info.office_id,
        repository_id: repo_id,
        scope_url: &scope_url,
    };
    let plaintext = unwrap_git_credential(&key, &binding, &ciphertext).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "office repository git credential failed decryption or scope verification",
        )
    })?;
    let secret: GitTokenSecret = serde_json::from_slice(&plaintext).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "office repository git credential payload is malformed",
        )
    })?;
    let username = validate_username(Some(&secret.username))?;
    let token = validate_token(&secret.token)?;
    Ok(Some(GitCredential::HttpsToken {
        username,
        token,
        scope_url,
    }))
}

/// Reads non-secret metadata for a per-repository vault credential without decrypting the token.
pub(super) fn vault_credential_status(
    office: &Office,
    repo: &Repository,
) -> Result<Option<RepoCredentialView>, ApiError> {
    let Some(record) = read_verified_envelope(office, repo.id.as_str())? else {
        return Ok(None);
    };
    Ok(Some(RepoCredentialView {
        office_id: office.info().office_id.clone(),
        repository_id: repo.id.as_str().to_owned(),
        repository_key: repo.key.clone(),
        configured: true,
        source: Some("vault".into()),
        scope_url: Some(record.scope_url),
        username: Some(record.username),
        updated_at: Some(record.updated_at),
    }))
}

/// Durably revokes a per-repository vault credential (idempotent if already absent).
pub(super) fn revoke_repo_https_token(office: &Office, repo: &Repository) -> Result<(), ApiError> {
    let repo_id = repo.id.as_str();
    require_valid_repo_id(repo_id)?;
    let (creds_root, repos_dir, repo_dir) = repo_cred_dirs(office, repo_id);
    for dir in [&creds_root, &repos_dir, &repo_dir] {
        match fs::symlink_metadata(dir) {
            Ok(meta) if meta.file_type().is_dir() => {}
            Ok(_) => {
                return Err(ApiError::new(
                    ErrorCode::StorageError,
                    "office git-credentials directory is not a real directory",
                ));
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(_) => {
                return Err(ApiError::new(
                    ErrorCode::StorageError,
                    "could not inspect office git-credentials directory",
                ));
            }
        }
    }
    let cred_path = repo_dir.join(CREDENTIAL_FILE_NAME);
    match fs::symlink_metadata(&cred_path) {
        Ok(meta) if meta.file_type().is_file() => {
            fs::remove_file(&cred_path).map_err(|_| {
                ApiError::new(
                    ErrorCode::StorageError,
                    "could not remove office repository git credential",
                )
            })?;
            sync_path(&repo_dir)?;
        }
        Ok(_) => {
            return Err(ApiError::new(
                ErrorCode::StorageError,
                "office repository git credential entry is not a regular file",
            ));
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(_) => {
            return Err(ApiError::new(
                ErrorCode::StorageError,
                "could not inspect office repository git credential",
            ));
        }
    }
    Ok(())
}

fn now_rfc3339() -> String {
    let epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    org_proto::format_rfc3339_ms(epoch_ms).unwrap_or_else(|| "1970-01-01T00:00:00.000Z".into())
}
