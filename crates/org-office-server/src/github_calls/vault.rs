//! Encrypted office-local GitHub App credential custody (`TASK-272`, AC #1).
//!
//! Stores the GitHub App's RSA private key PEM and optional webhook HMAC-SHA256 secret strictly
//! outside `durable/state/`, SQLite, and Git, at `<office.folder()>/github-app/app.v1.json`
//! (`0700`/`0600`, atomic write, `O_NOFOLLOW`), encrypted with `org_pki::wrap`
//! (`KeyClass::GitHubApp`, AES-256-GCM) and AAD-bound to
//! `(organization_id, office_id, app_id, purpose)`.

use std::fs;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use org_github::{GitHubAppConfig, GitHubAppPrivateKey, ValidatedApiBaseUrl};
use org_office::Office;
use org_pki::wrap::{
    derive_wrapping_key, unwrap_github_app_credential, wrap_github_app_credential,
    GitHubAppCredentialBinding, KeyClass,
};
use org_proto::github::GitHubAppConfigureParams;
use org_proto::{ApiError, ErrorCode};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

const VAULT_RECORD_VERSION: u32 = 1;
const APP_FILE_NAME: &str = "app.v1.json";
const MAX_VAULT_FILE_BYTES: u64 = 32 * 1024;
const MAX_WEBHOOK_SECRET_BYTES: usize = 2048;
const PURPOSE_PRIVATE_KEY: &str = "private-key";
const PURPOSE_WEBHOOK_SECRET: &str = "webhook-secret";

/// Non-secret envelope persisted at `<office.folder()>/github-app/app.v1.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredGitHubApp {
    version: u32,
    organization_id: String,
    office_id: String,
    app_id: u64,
    installation_id: u64,
    api_base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    account_hint: Option<String>,
    updated_at: String,
    wrapped_private_key_b64: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wrapped_webhook_secret_b64: Option<String>,
}

/// Non-secret metadata returned for `github.app.status` without decrypting secrets.
#[derive(Debug, Clone)]
pub(super) struct StoredGitHubAppMeta {
    pub app_id: u64,
    pub installation_id: u64,
    pub api_base_url: String,
    pub webhook_configured: bool,
    pub account_hint: Option<String>,
    pub updated_at: String,
}

fn app_vault_dir(office: &Office) -> PathBuf {
    office.folder().join("github-app")
}

fn derive_app_wrapping_key(office: &Office) -> Result<Zeroizing<[u8; 32]>, ApiError> {
    let Some(master) = office.secrets_key() else {
        return Err(ApiError::new(
            ErrorCode::Unavailable,
            "office secrets key unavailable; cannot store or unwrap GitHub App credentials",
        ));
    };
    derive_wrapping_key(master.expose_bytes(), KeyClass::GitHubApp).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not derive office GitHub App wrapping key",
        )
    })
}

fn effective_uid_or_err() -> Result<u32, ApiError> {
    org_platform::fs::effective_uid().map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "office GitHub App credentials are unsupported on this platform",
        )
    })
}

fn ensure_owner_only_app_dir(office: &Office) -> Result<PathBuf, ApiError> {
    let uid = effective_uid_or_err()?;
    let dir = app_vault_dir(office);
    org_platform::fs::set_owner_only_dir(office.folder()).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "office folder failed owner-only verification",
        )
    })?;
    org_platform::fs::create_owner_only_dir_all(&dir)
        .and_then(|()| org_platform::fs::set_owner_only_dir(&dir))
        .map_err(|_| {
            ApiError::new(
                ErrorCode::StorageError,
                "could not prepare owner-only office github-app directory",
            )
        })?;
    org_platform::fs::verify_owner_only_ancestors(&dir, office.folder(), uid).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "office github-app directory failed owner-only verification",
        )
    })?;
    fs::canonicalize(&dir).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not resolve office github-app directory",
        )
    })
}

fn sync_path(path: &Path) -> Result<(), ApiError> {
    let file = fs::File::open(path).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not open office github-app path for sync",
        )
    })?;
    org_platform::fs::full_sync(&file).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not flush office github-app credential to disk",
        )
    })
}

fn read_verified_envelope(office: &Office) -> Result<Option<StoredGitHubApp>, ApiError> {
    let dir = app_vault_dir(office);
    match fs::symlink_metadata(&dir) {
        Ok(meta) if meta.file_type().is_dir() => {}
        Ok(_) => {
            return Err(ApiError::new(
                ErrorCode::StorageError,
                "office github-app directory must be a real directory (never a symlink)",
            ));
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(ApiError::new(
                ErrorCode::StorageError,
                "could not inspect office github-app directory",
            ));
        }
    }
    let path = dir.join(APP_FILE_NAME);
    match fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(ApiError::new(
                ErrorCode::StorageError,
                "could not inspect office GitHub App credential file",
            ));
        }
    }
    let uid = effective_uid_or_err()?;
    org_platform::fs::verify_owner_only_ancestors(&dir, office.folder(), uid).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "office github-app directory failed owner-only verification",
        )
    })?;
    let verified = org_platform::fs::verify_owner_only_file(&path, uid, false).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "office GitHub App credential file failed owner-only verification",
        )
    })?;
    let file = org_platform::fs::open_nofollow_ro(&verified).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not open office GitHub App credential file",
        )
    })?;
    let mut raw = Vec::new();
    file.take(MAX_VAULT_FILE_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|_| {
            ApiError::new(
                ErrorCode::StorageError,
                "could not read office GitHub App credential file",
            )
        })?;
    if raw.len() as u64 > MAX_VAULT_FILE_BYTES {
        return Err(ApiError::new(
            ErrorCode::StorageError,
            "office GitHub App credential file exceeds maximum size",
        ));
    }
    let record: StoredGitHubApp = serde_json::from_slice(&raw).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "malformed office GitHub App credential file",
        )
    })?;
    let info = office.info();
    if record.version != VAULT_RECORD_VERSION
        || record.organization_id != info.org_id
        || record.office_id != info.office_id
        || record.app_id == 0
        || record.installation_id == 0
    {
        return Err(ApiError::new(
            ErrorCode::StorageError,
            "office GitHub App credential binding metadata mismatch",
        ));
    }
    Ok(Some(record))
}

/// Validates and encrypts a GitHub App's RSA private key PEM and optional webhook secret into
/// `<office.folder()>/github-app/app.v1.json`.
pub(super) fn store_github_app(
    office: &Office,
    params: &GitHubAppConfigureParams,
) -> Result<StoredGitHubAppMeta, ApiError> {
    let config = GitHubAppConfig::new(
        params.app_id,
        params.installation_id,
        params.api_base_url.as_deref(),
        params.account_hint.as_deref(),
    )
    .map_err(|e| ApiError::new(ErrorCode::InvalidRequest, e.to_string()))?;

    let pem_trimmed = Zeroizing::new(params.private_key_pem.expose().trim().to_owned());
    let _validated_key = GitHubAppPrivateKey::from_pem(pem_trimmed.as_bytes())
        .map_err(|e| ApiError::new(ErrorCode::InvalidRequest, e.to_string()))?;
    if pem_trimmed.len() > org_pki::wrap::MAX_KEY_BYTES {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "private key PEM exceeds maximum encrypted payload size",
        ));
    }

    let validated_webhook = match params.webhook_secret.as_ref() {
        Some(secret) => {
            let trimmed = Zeroizing::new(secret.expose().trim().to_owned());
            if trimmed.is_empty()
                || trimmed.len() > MAX_WEBHOOK_SECRET_BYTES
                || trimmed.chars().any(char::is_control)
            {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "webhook_secret must be 1..=2048 bytes with no control characters",
                ));
            }
            Some(trimmed)
        }
        None => None,
    };

    let key = derive_app_wrapping_key(office)?;
    let info = office.info();

    let pk_binding = GitHubAppCredentialBinding {
        organization_id: &info.org_id,
        office_id: &info.office_id,
        app_id: config.app_id,
        purpose: PURPOSE_PRIVATE_KEY,
    };
    let wrapped_pk = wrap_github_app_credential(&key, &pk_binding, pem_trimmed.as_bytes())
        .map_err(|_| {
            ApiError::new(
                ErrorCode::StorageError,
                "could not encrypt GitHub App private key",
            )
        })?;

    let wrapped_webhook_secret_b64 = match validated_webhook {
        Some(secret) => {
            let wh_binding = GitHubAppCredentialBinding {
                organization_id: &info.org_id,
                office_id: &info.office_id,
                app_id: config.app_id,
                purpose: PURPOSE_WEBHOOK_SECRET,
            };
            let wrapped = wrap_github_app_credential(&key, &wh_binding, secret.as_bytes())
                .map_err(|_| {
                    ApiError::new(
                        ErrorCode::StorageError,
                        "could not encrypt GitHub App webhook secret",
                    )
                })?;
            Some(BASE64.encode(&wrapped))
        }
        None => None,
    };

    let updated_at = now_rfc3339();
    let webhook_configured = wrapped_webhook_secret_b64.is_some();
    let envelope = StoredGitHubApp {
        version: VAULT_RECORD_VERSION,
        organization_id: info.org_id.clone(),
        office_id: info.office_id.clone(),
        app_id: config.app_id,
        installation_id: config.installation_id,
        api_base_url: config.api_base_url.as_str().to_owned(),
        account_hint: config.account_hint.clone(),
        updated_at: updated_at.clone(),
        wrapped_private_key_b64: BASE64.encode(&wrapped_pk),
        wrapped_webhook_secret_b64,
    };

    let mut bytes = serde_json::to_vec_pretty(&envelope).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not encode GitHub App credential envelope",
        )
    })?;
    bytes.push(b'\n');

    let dir = ensure_owner_only_app_dir(office)?;
    org_platform::fs::write_regular_beneath(&dir, Path::new(APP_FILE_NAME), &bytes).map_err(
        |_| {
            ApiError::new(
                ErrorCode::StorageError,
                "could not persist encrypted GitHub App credential",
            )
        },
    )?;
    sync_path(&dir.join(APP_FILE_NAME))?;
    sync_path(&dir)?;

    Ok(StoredGitHubAppMeta {
        app_id: config.app_id,
        installation_id: config.installation_id,
        api_base_url: config.api_base_url.as_str().to_owned(),
        webhook_configured,
        account_hint: config.account_hint,
        updated_at,
    })
}

/// Reads non-secret metadata for the configured GitHub App without unwrapping any secret.
pub(super) fn load_github_app_meta(
    office: &Office,
) -> Result<Option<StoredGitHubAppMeta>, ApiError> {
    let Some(record) = read_verified_envelope(office)? else {
        return Ok(None);
    };
    Ok(Some(StoredGitHubAppMeta {
        app_id: record.app_id,
        installation_id: record.installation_id,
        api_base_url: record.api_base_url,
        webhook_configured: record.wrapped_webhook_secret_b64.is_some(),
        account_hint: record.account_hint,
        updated_at: record.updated_at,
    }))
}

/// Unwraps the GitHub App configuration and RSA private key into memory for JWT signing.
pub(super) fn load_github_app_private_key(
    office: &Office,
) -> Result<Option<(GitHubAppConfig, GitHubAppPrivateKey)>, ApiError> {
    let Some(record) = read_verified_envelope(office)? else {
        return Ok(None);
    };
    let api_base_url = ValidatedApiBaseUrl::parse(&record.api_base_url).map_err(|e| {
        ApiError::new(
            ErrorCode::StorageError,
            format!("stored GitHub App API base URL is invalid: {e}"),
        )
    })?;
    let key = derive_app_wrapping_key(office)?;
    let ciphertext = BASE64
        .decode(&record.wrapped_private_key_b64)
        .map_err(|_| {
            ApiError::new(
                ErrorCode::StorageError,
                "stored GitHub App private key ciphertext is malformed",
            )
        })?;
    let info = office.info();
    let binding = GitHubAppCredentialBinding {
        organization_id: &info.org_id,
        office_id: &info.office_id,
        app_id: record.app_id,
        purpose: PURPOSE_PRIVATE_KEY,
    };
    let plaintext = unwrap_github_app_credential(&key, &binding, &ciphertext).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "stored GitHub App private key failed decryption or binding verification",
        )
    })?;
    let private_key = GitHubAppPrivateKey::from_pem(&plaintext).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "stored GitHub App private key payload is malformed",
        )
    })?;
    let config = GitHubAppConfig {
        app_id: record.app_id,
        installation_id: record.installation_id,
        api_base_url,
        account_hint: record.account_hint,
    };
    Ok(Some((config, private_key)))
}

/// Unwraps the configured GitHub webhook HMAC-SHA256 secret into a zeroizing string.
pub(super) fn load_github_webhook_secret(
    office: &Office,
) -> Result<Option<Zeroizing<String>>, ApiError> {
    let Some(record) = read_verified_envelope(office)? else {
        return Ok(None);
    };
    let Some(wrapped_b64) = record.wrapped_webhook_secret_b64.as_deref() else {
        return Ok(None);
    };
    let key = derive_app_wrapping_key(office)?;
    let ciphertext = BASE64.decode(wrapped_b64).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "stored GitHub webhook secret ciphertext is malformed",
        )
    })?;
    let info = office.info();
    let binding = GitHubAppCredentialBinding {
        organization_id: &info.org_id,
        office_id: &info.office_id,
        app_id: record.app_id,
        purpose: PURPOSE_WEBHOOK_SECRET,
    };
    let mut plaintext =
        unwrap_github_app_credential(&key, &binding, &ciphertext).map_err(|_| {
            ApiError::new(
                ErrorCode::StorageError,
                "stored GitHub webhook secret failed decryption or binding verification",
            )
        })?;
    if std::str::from_utf8(&plaintext).is_err() {
        return Err(ApiError::new(
            ErrorCode::StorageError,
            "stored GitHub webhook secret is not valid UTF-8",
        ));
    }
    let text = Zeroizing::new(String::from_utf8(std::mem::take(&mut *plaintext)).map_err(
        |_| {
            ApiError::new(
                ErrorCode::StorageError,
                "stored GitHub webhook secret is not valid UTF-8",
            )
        },
    )?);
    Ok(Some(text))
}

/// Durably removes `<office.folder()>/github-app/app.v1.json` (idempotent when absent).
pub(super) fn revoke_github_app(office: &Office) -> Result<(), ApiError> {
    let dir = app_vault_dir(office);
    match fs::symlink_metadata(&dir) {
        Ok(meta) if meta.file_type().is_dir() => {}
        Ok(_) => {
            return Err(ApiError::new(
                ErrorCode::StorageError,
                "office github-app directory is not a real directory",
            ));
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => {
            return Err(ApiError::new(
                ErrorCode::StorageError,
                "could not inspect office github-app directory",
            ));
        }
    }
    let uid = effective_uid_or_err()?;
    org_platform::fs::verify_owner_only_ancestors(&dir, office.folder(), uid).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "office github-app directory failed owner-only verification",
        )
    })?;
    let path = dir.join(APP_FILE_NAME);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_file() => {
            fs::remove_file(&path).map_err(|_| {
                ApiError::new(
                    ErrorCode::StorageError,
                    "could not remove office GitHub App credential",
                )
            })?;
            sync_path(&dir)?;
        }
        Ok(_) => {
            return Err(ApiError::new(
                ErrorCode::StorageError,
                "office GitHub App credential entry is not a regular file",
            ));
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(_) => {
            return Err(ApiError::new(
                ErrorCode::StorageError,
                "could not inspect office GitHub App credential",
            ));
        }
    }
    Ok(())
}

pub(super) fn now_rfc3339() -> String {
    let epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    org_proto::format_rfc3339_ms(epoch_ms).unwrap_or_else(|| "1970-01-01T00:00:00.000Z".into())
}
