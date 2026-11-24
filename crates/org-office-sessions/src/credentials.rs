//! Wrapped provider credential custody and browser-driven CLI enrollment (`ADR-0032`,
//! `ENROLL-1..6`, `FC-189`).

use std::collections::HashMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use org_pki::wrap::{
    derive_wrapping_key, unwrap_provider_credential, wrap_provider_credential, KeyClass,
    ProviderCredentialBinding, WrapError,
};
use org_proto::costs::SubscriptionUsageCredentialView;
use org_proto::sessions::{EnrollmentSessionView, EnrollmentState, ProviderCredentialView};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// Default enrollment timeout: 15 minutes (`ADR-0032` / `ENROLL-4`).
pub const DEFAULT_ENROLLMENT_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MAX_CODE_BYTES: usize = 1024;
const CREDENTIAL_VERSION: u32 = 1;

/// Errors returned by the credential vault and enrollment manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnrollmentVaultError {
    MissingSecretsKey,
    InvalidInput(String),
    NotAuthorized(String),
    Conflict(String),
    NotFound(String),
    Expired(String),
    Crypto(String),
    Io(String),
}

impl fmt::Display for EnrollmentVaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSecretsKey => {
                f.write_str("office secrets key unavailable; cannot wrap provider credentials")
            }
            Self::InvalidInput(msg)
            | Self::NotAuthorized(msg)
            | Self::Conflict(msg)
            | Self::NotFound(msg)
            | Self::Expired(msg)
            | Self::Crypto(msg)
            | Self::Io(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for EnrollmentVaultError {}

impl From<WrapError> for EnrollmentVaultError {
    fn from(err: WrapError) -> Self {
        Self::Crypto(err.to_string())
    }
}

/// Unwrapped secret together with its non-secret custody metadata (`TASK-79`).
pub struct UnwrappedCredential {
    pub secret: Zeroizing<Vec<u8>>,
    pub enrolled_at: String,
    pub expires_at: Option<String>,
    pub account_hint: Option<String>,
    pub bound_origin: Option<String>,
}

/// Non-secret metadata stored alongside wrapped ciphertext (`ENROLL-3`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialMetadata {
    pub version: u32,
    pub organization_id: String,
    pub office_id: String,
    pub connection_id: String,
    pub provider: String,
    pub enrolled_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound_origin: Option<String>,
    pub wrapped_b64: String,
}

/// Persists wrapped provider credentials under `<durable>/local/offices/<office_id>/auth/<connection_id>/credential.v1.json`.
#[derive(Clone)]
pub struct CredentialVault {
    durable_root: PathBuf,
    organization_id: String,
    office_id: String,
    office_secrets_key: Option<Arc<Zeroizing<[u8; 32]>>>,
}

impl fmt::Debug for CredentialVault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CredentialVault")
            .field("organization_id", &self.organization_id)
            .field("office_id", &self.office_id)
            .field("has_key", &self.office_secrets_key.is_some())
            .finish()
    }
}

impl CredentialVault {
    pub fn new(
        durable_root: PathBuf,
        organization_id: impl Into<String>,
        office_id: impl Into<String>,
        office_secrets_key: Option<[u8; 32]>,
    ) -> Self {
        Self {
            durable_root,
            organization_id: organization_id.into(),
            office_id: office_id.into(),
            office_secrets_key: office_secrets_key.map(|k| Arc::new(Zeroizing::new(k))),
        }
    }

    pub fn credential_path(&self, connection_id: &str) -> Result<PathBuf, EnrollmentVaultError> {
        if !is_safe_id(connection_id) {
            return Err(EnrollmentVaultError::InvalidInput(
                "invalid connection_id".to_owned(),
            ));
        }
        Ok(self
            .durable_root
            .join("local")
            .join("offices")
            .join(&self.office_id)
            .join("auth")
            .join(connection_id)
            .join("credential.v1.json"))
    }

    fn wrapping_key(&self) -> Result<Zeroizing<[u8; 32]>, EnrollmentVaultError> {
        let Some(master) = &self.office_secrets_key else {
            return Err(EnrollmentVaultError::MissingSecretsKey);
        };
        Ok(derive_wrapping_key(master, KeyClass::ProviderCredential)?)
    }

    fn binding<'a>(&'a self, connection_id: &'a str) -> ProviderCredentialBinding<'a> {
        ProviderCredentialBinding {
            organization_id: &self.organization_id,
            office_id: &self.office_id,
            connection_id,
        }
    }

    /// Encrypts and atomically writes `secret` with mode `0600` under `KeyClass::ProviderCredential`.
    pub fn store_wrapped(
        &self,
        connection_id: &str,
        provider: &str,
        secret: &[u8],
        expires_at: Option<String>,
        account_hint: Option<String>,
    ) -> Result<ProviderCredentialView, EnrollmentVaultError> {
        let key = self.wrapping_key()?;
        let path = self.credential_path(connection_id)?;
        let parent = path
            .parent()
            .ok_or_else(|| EnrollmentVaultError::Io("credential path has no parent".into()))?;
        ensure_owner_dirs(parent)?;

        let wrapped = wrap_provider_credential(&key, &self.binding(connection_id), secret)?;
        let enrolled_at = rfc3339_now();
        let record = CredentialMetadata {
            version: CREDENTIAL_VERSION,
            organization_id: self.organization_id.clone(),
            office_id: self.office_id.clone(),
            connection_id: connection_id.to_owned(),
            provider: provider.to_owned(),
            enrolled_at: enrolled_at.clone(),
            expires_at: expires_at.clone(),
            account_hint: account_hint.clone(),
            bound_origin: None,
            wrapped_b64: BASE64.encode(&wrapped),
        };
        let bytes = serde_json::to_vec_pretty(&record)
            .map_err(|_| EnrollmentVaultError::Io("could not encode credential metadata".into()))?;
        write_private_atomic(parent, &path, &bytes)?;
        Ok(ProviderCredentialView {
            connection_id: connection_id.to_owned(),
            office_id: self.office_id.clone(),
            enrolled: true,
            enrolled_at: Some(enrolled_at),
            expires_at,
            account_hint,
        })
    }

    /// Unwraps and returns the raw credential in a zeroizing buffer.
    pub fn load_unwrapped(
        &self,
        connection_id: &str,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, EnrollmentVaultError> {
        Ok(self
            .load_unwrapped_with_meta(connection_id)?
            .map(|c| c.secret))
    }

    /// Unwraps and returns the enrolled credential together with expiry & account metadata.
    pub fn load_unwrapped_with_meta(
        &self,
        connection_id: &str,
    ) -> Result<Option<UnwrappedCredential>, EnrollmentVaultError> {
        let path = self.credential_path(connection_id)?;
        self.load_unwrapped_from_path(&path, connection_id, connection_id)
    }

    /// Returns the office-local path for a connection's dedicated usage-API credential (`TASK-79`).
    pub fn usage_credential_path(
        &self,
        connection_id: &str,
    ) -> Result<PathBuf, EnrollmentVaultError> {
        if !is_safe_id(connection_id) {
            return Err(EnrollmentVaultError::InvalidInput(
                "invalid connection_id".to_owned(),
            ));
        }
        Ok(self
            .durable_root
            .join("local")
            .join("offices")
            .join(&self.office_id)
            .join("auth")
            .join(connection_id)
            .join("usage-credential.v1.json"))
    }

    /// Encrypts and atomically stores a private usage-API credential under origin- and
    /// domain-separated AAD (`{connection_id}:usage:{bound_origin}`, mode `0600`, outside org Git).
    pub fn store_wrapped_usage(
        &self,
        connection_id: &str,
        provider: &str,
        bound_origin: &str,
        secret: &[u8],
        expires_at: Option<String>,
        account_hint: Option<String>,
    ) -> Result<SubscriptionUsageCredentialView, EnrollmentVaultError> {
        let key = self.wrapping_key()?;
        let path = self.usage_credential_path(connection_id)?;
        let parent = path.parent().ok_or_else(|| {
            EnrollmentVaultError::Io("usage credential path has no parent".into())
        })?;
        ensure_owner_dirs(parent)?;

        let aad_conn = format!("{connection_id}:usage:{bound_origin}");
        let wrapped = wrap_provider_credential(&key, &self.binding(&aad_conn), secret)?;
        let configured_at = rfc3339_now();
        let record = CredentialMetadata {
            version: CREDENTIAL_VERSION,
            organization_id: self.organization_id.clone(),
            office_id: self.office_id.clone(),
            connection_id: format!("{connection_id}:usage"),
            provider: provider.to_owned(),
            enrolled_at: configured_at.clone(),
            expires_at: expires_at.clone(),
            account_hint: account_hint.clone(),
            bound_origin: Some(bound_origin.to_owned()),
            wrapped_b64: BASE64.encode(&wrapped),
        };
        let bytes = serde_json::to_vec_pretty(&record)
            .map_err(|_| EnrollmentVaultError::Io("could not encode usage credential".into()))?;
        write_private_atomic(parent, &path, &bytes)?;
        Ok(SubscriptionUsageCredentialView {
            connection_id: connection_id.to_owned(),
            office_id: self.office_id.clone(),
            configured: true,
            configured_at: Some(configured_at),
            expires_at,
            account_hint,
        })
    }

    /// Unwraps and returns the dedicated usage-API credential + metadata only when its bound
    /// destination origin matches `expected_origin` (`TASK-79`).
    pub fn load_unwrapped_usage_with_meta(
        &self,
        connection_id: &str,
        expected_origin: &str,
    ) -> Result<Option<UnwrappedCredential>, EnrollmentVaultError> {
        let path = self.usage_credential_path(connection_id)?;
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read(&path)
            .map_err(|_| EnrollmentVaultError::Io("could not read usage credential".into()))?;
        let record: CredentialMetadata = serde_json::from_slice(&raw)
            .map_err(|_| EnrollmentVaultError::Crypto("malformed usage credential file".into()))?;
        let expected_conn = format!("{connection_id}:usage");
        if record.version != CREDENTIAL_VERSION
            || record.organization_id != self.organization_id
            || record.office_id != self.office_id
            || record.connection_id != expected_conn
        {
            return Err(EnrollmentVaultError::Crypto(
                "usage credential binding metadata mismatch".into(),
            ));
        }
        if record.bound_origin.as_deref() != Some(expected_origin) {
            return Err(EnrollmentVaultError::Conflict(
                "usage credential is bound to a different endpoint origin; re-configure credential"
                    .into(),
            ));
        }
        let key = self.wrapping_key()?;
        let aad_conn = format!("{connection_id}:usage:{expected_origin}");
        let ciphertext = BASE64
            .decode(&record.wrapped_b64)
            .map_err(|_| EnrollmentVaultError::Crypto("invalid credential base64".into()))?;
        let plaintext = unwrap_provider_credential(&key, &self.binding(&aad_conn), &ciphertext)?;
        Ok(Some(UnwrappedCredential {
            secret: plaintext,
            enrolled_at: record.enrolled_at,
            expires_at: record.expires_at,
            account_hint: record.account_hint,
            bound_origin: record.bound_origin,
        }))
    }

    /// Returns non-secret status metadata for a connection's dedicated usage-API credential.
    pub fn usage_status(
        &self,
        connection_id: &str,
    ) -> Result<SubscriptionUsageCredentialView, EnrollmentVaultError> {
        let path = self.usage_credential_path(connection_id)?;
        if !path.exists() {
            return Ok(SubscriptionUsageCredentialView {
                connection_id: connection_id.to_owned(),
                office_id: self.office_id.clone(),
                configured: false,
                configured_at: None,
                expires_at: None,
                account_hint: None,
            });
        }
        let raw = fs::read(&path)
            .map_err(|_| EnrollmentVaultError::Io("could not read usage credential".into()))?;
        let record: CredentialMetadata = serde_json::from_slice(&raw)
            .map_err(|_| EnrollmentVaultError::Crypto("malformed usage credential file".into()))?;
        let expected_conn = format!("{connection_id}:usage");
        if record.version != CREDENTIAL_VERSION
            || record.organization_id != self.organization_id
            || record.office_id != self.office_id
            || record.connection_id != expected_conn
        {
            return Err(EnrollmentVaultError::Crypto(
                "usage credential binding metadata mismatch".into(),
            ));
        }
        Ok(SubscriptionUsageCredentialView {
            connection_id: connection_id.to_owned(),
            office_id: self.office_id.clone(),
            configured: true,
            configured_at: Some(record.enrolled_at),
            expires_at: record.expires_at,
            account_hint: record.account_hint,
        })
    }

    /// Revokes and removes a connection's dedicated usage-API credential durably (`TASK-79`).
    pub fn revoke_usage(
        &self,
        connection_id: &str,
    ) -> Result<SubscriptionUsageCredentialView, EnrollmentVaultError> {
        let path = self.usage_credential_path(connection_id)?;
        remove_file_durably(&path)?;
        self.usage_status(connection_id)
    }

    fn load_unwrapped_from_path(
        &self,
        path: &Path,
        expected_connection_id: &str,
        aad_connection_id: &str,
    ) -> Result<Option<UnwrappedCredential>, EnrollmentVaultError> {
        if !path.exists() {
            return Ok(None);
        }
        let key = self.wrapping_key()?;
        let raw = fs::read(path)
            .map_err(|_| EnrollmentVaultError::Io("could not read credential".into()))?;
        let record: CredentialMetadata = serde_json::from_slice(&raw)
            .map_err(|_| EnrollmentVaultError::Crypto("malformed credential file".into()))?;
        if record.version != CREDENTIAL_VERSION
            || record.organization_id != self.organization_id
            || record.office_id != self.office_id
            || record.connection_id != expected_connection_id
        {
            return Err(EnrollmentVaultError::Crypto(
                "credential binding metadata mismatch".into(),
            ));
        }
        let ciphertext = BASE64
            .decode(&record.wrapped_b64)
            .map_err(|_| EnrollmentVaultError::Crypto("invalid credential base64".into()))?;
        let plaintext =
            unwrap_provider_credential(&key, &self.binding(aad_connection_id), &ciphertext)?;
        Ok(Some(UnwrappedCredential {
            secret: plaintext,
            enrolled_at: record.enrolled_at,
            expires_at: record.expires_at,
            account_hint: record.account_hint,
            bound_origin: record.bound_origin,
        }))
    }

    /// Returns non-secret status metadata for `connection_id` (`ENROLL-3`, `ENROLL-5`).
    pub fn status(
        &self,
        connection_id: &str,
    ) -> Result<ProviderCredentialView, EnrollmentVaultError> {
        let path = self.credential_path(connection_id)?;
        if !path.exists() {
            return Ok(ProviderCredentialView {
                connection_id: connection_id.to_owned(),
                office_id: self.office_id.clone(),
                enrolled: false,
                enrolled_at: None,
                expires_at: None,
                account_hint: None,
            });
        }
        let raw = fs::read(&path)
            .map_err(|_| EnrollmentVaultError::Io("could not read credential".into()))?;
        let record: CredentialMetadata = serde_json::from_slice(&raw)
            .map_err(|_| EnrollmentVaultError::Crypto("malformed credential file".into()))?;
        Ok(ProviderCredentialView {
            connection_id: connection_id.to_owned(),
            office_id: self.office_id.clone(),
            enrolled: true,
            enrolled_at: Some(record.enrolled_at),
            expires_at: record.expires_at,
            account_hint: record.account_hint,
        })
    }

    /// Revokes and removes the stored wrapped credential (`ENROLL-5`).
    pub fn revoke(
        &self,
        connection_id: &str,
    ) -> Result<ProviderCredentialView, EnrollmentVaultError> {
        let path = self.credential_path(connection_id)?;
        remove_file_durably(&path)?;
        self.status(connection_id)
    }
}

fn remove_file_durably(path: &Path) -> Result<(), EnrollmentVaultError> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(_) => {
            return Err(EnrollmentVaultError::Io(
                "could not remove credential".into(),
            ))
        }
    }
    if let Some(parent) = path.parent() {
        if parent.exists() {
            sync_dir(parent)?;
        }
    }
    Ok(())
}

fn sync_dir(dir: &Path) -> Result<(), EnrollmentVaultError> {
    let handle = fs::File::open(dir)
        .map_err(|_| EnrollmentVaultError::Io("could not open credential directory".into()))?;
    org_platform::fs::full_sync(&handle)
        .map_err(|_| EnrollmentVaultError::Io("could not sync credential directory".into()))?;
    Ok(())
}

fn sync_ancestor_dirs(dir: &Path, depth: usize) -> Result<(), EnrollmentVaultError> {
    let mut cur = Some(dir);
    for _ in 0..depth {
        let Some(d) = cur else {
            break;
        };
        if d.as_os_str().is_empty() || d.parent().is_none() {
            break;
        }
        sync_dir(d)?;
        cur = d.parent();
    }
    Ok(())
}

/// Redacts all known active secret values and recognizable provider token prefixes (`ENROLL-3`).
pub fn redact_secrets(text: &str, active_secrets: &[&str]) -> String {
    let mut out = text.to_owned();
    for secret in active_secrets {
        if !secret.is_empty() && secret.len() >= 6 {
            out = out.replace(secret, "[REDACTED]");
        }
    }
    for prefix in [
        "sk-ant-oat",
        "sk-ant-api",
        "sk-ant-ort",
        "sk-proj-",
        "sk-svcacct-",
        "sk-admin-",
        "AIza",
        "gho_",
        "ghp_",
    ] {
        out = redact_prefixed_tokens(&out, prefix);
    }
    redact_bearer_tokens(&out)
}

fn redact_prefixed_tokens(input: &str, prefix: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = rest.find(prefix) {
        result.push_str(&rest[..idx]);
        result.push_str("[REDACTED]");
        let after = &rest[idx + prefix.len()..];
        let token_len = after
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            .count();
        rest = &after[token_len..];
    }
    result.push_str(rest);
    result
}

fn redact_bearer_tokens(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = rest.find("Bearer ") {
        result.push_str(&rest[..idx]);
        result.push_str("Bearer [REDACTED]");
        let after = &rest[idx + "Bearer ".len()..];
        let token_len = after
            .bytes()
            .take_while(|b| !b.is_ascii_whitespace() && *b != b'"' && *b != b'\'')
            .count();
        rest = &after[token_len..];
    }
    result.push_str(rest);
    result
}

/// Validates a pasted callback/auth code from the browser (`ENROLL-2`): strictly printable ASCII,
/// non-empty, no whitespace/control characters, bounded length.
pub fn validate_pasted_code(code: &str) -> Result<&str, EnrollmentVaultError> {
    let trimmed = code.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_CODE_BYTES {
        return Err(EnrollmentVaultError::InvalidInput(
            "enrollment code must be 1..=1024 ASCII bytes".to_owned(),
        ));
    }
    if !trimmed
        .bytes()
        .all(|b| b.is_ascii_graphic() && b != b'\\' && b != b'\'' && b != b'"')
    {
        return Err(EnrollmentVaultError::InvalidInput(
            "enrollment code contains disallowed characters".to_owned(),
        ));
    }
    Ok(trimmed)
}

/// Initial prompt returned by an [`EnrollmentDriver`] when a login starts.
#[derive(Debug, Clone)]
pub struct EnrollmentPrompt {
    pub state: EnrollmentState,
    pub auth_url: Option<String>,
    pub device_code: Option<String>,
    pub message: Option<String>,
}

/// Completion outcome returned by an [`EnrollmentDriver`] after submitting a code or polling.
#[derive(Clone)]
pub enum EnrollmentOutcome {
    Pending {
        state: EnrollmentState,
        message: Option<String>,
    },
    Completed {
        secret: Zeroizing<Vec<u8>>,
        expires_at: Option<String>,
        account_hint: Option<String>,
    },
    Failed(String),
}

/// Pluggable backend for CLI sign-in flows (`claude setup-token`, `codex login --device-auth`).
pub trait EnrollmentDriver: Send + Sync {
    fn start(
        &self,
        provider: &str,
        temp_dir: &Path,
    ) -> Result<EnrollmentPrompt, EnrollmentVaultError>;

    fn submit_code(
        &self,
        provider: &str,
        temp_dir: &Path,
        code: &str,
    ) -> Result<EnrollmentOutcome, EnrollmentVaultError>;

    fn poll(
        &self,
        provider: &str,
        temp_dir: &Path,
    ) -> Result<EnrollmentOutcome, EnrollmentVaultError>;
}

/// Default CLI enrollment driver (`ADR-0032`): prepares an isolated `0700` temp directory and
/// validates pasted setup tokens / authorization codes. Interactive PTY execution of external
/// vendor CLIs (`claude setup-token`, `codex login --device-auth`) plugs into [`EnrollmentDriver`].
#[derive(Debug, Default)]
pub struct CliEnrollmentDriver;

impl EnrollmentDriver for CliEnrollmentDriver {
    fn start(
        &self,
        provider: &str,
        _temp_dir: &Path,
    ) -> Result<EnrollmentPrompt, EnrollmentVaultError> {
        let normalized = org_office::accounting::canonical_provider_namespace(provider);
        match normalized.as_str() {
            "anthropic" => Ok(EnrollmentPrompt {
                state: EnrollmentState::AwaitingCode,
                auth_url: Some("https://claude.ai/oauth/authorize".to_owned()),
                device_code: None,
                message: Some(
                    "Open the Anthropic authorization URL in your browser and paste the returned setup token or callback code."
                        .to_owned(),
                ),
            }),
            "openai" => Ok(EnrollmentPrompt {
                state: EnrollmentState::PendingBrowser,
                auth_url: Some("https://auth.openai.com/codex/device".to_owned()),
                device_code: Some(format!(
                    "{}-{}",
                    random_hex(2)?.to_ascii_uppercase(),
                    random_hex(2)?.to_ascii_uppercase()
                )),
                message: Some(
                    "Enter the device code at the OpenAI authorization page or paste the issued token."
                        .to_owned(),
                ),
            }),
            "google" => {
                Ok(EnrollmentPrompt {
                    state: EnrollmentState::AwaitingCode,
                    auth_url: Some("https://antigravity.google/auth".to_owned()),
                    device_code: None,
                    message: Some(
                        "Authenticate with Antigravity (agy) in your browser and paste the returned authorization code or API key."
                            .to_owned(),
                    ),
                })
            }
            _ => Err(EnrollmentVaultError::InvalidInput(format!(
                "unsupported enrollment provider `{provider}`"
            ))),
        }
    }

    fn submit_code(
        &self,
        provider: &str,
        _temp_dir: &Path,
        code: &str,
    ) -> Result<EnrollmentOutcome, EnrollmentVaultError> {
        let validated = validate_pasted_code(code)?;
        Ok(EnrollmentOutcome::Completed {
            secret: Zeroizing::new(validated.as_bytes().to_vec()),
            expires_at: None,
            account_hint: Some(format!("{provider}-cli-enrolled")),
        })
    }

    fn poll(
        &self,
        _provider: &str,
        _temp_dir: &Path,
    ) -> Result<EnrollmentOutcome, EnrollmentVaultError> {
        Ok(EnrollmentOutcome::Pending {
            state: EnrollmentState::AwaitingCode,
            message: Some("Waiting for pasted authorization code.".to_owned()),
        })
    }
}

struct ActiveEnrollment {
    login_id: String,
    owner_member_id: String,
    connection_id: String,
    provider: String,
    temp_dir: PathBuf,
    state: EnrollmentState,
    auth_url: Option<String>,
    device_code: Option<String>,
    message: Option<String>,
    expires_at_ms: u64,
    expires_at_rfc3339: String,
}

impl ActiveEnrollment {
    fn view(&self) -> EnrollmentSessionView {
        EnrollmentSessionView {
            login_id: self.login_id.clone(),
            connection_id: self.connection_id.clone(),
            provider: self.provider.clone(),
            state: self.state,
            auth_url: self.auth_url.clone(),
            device_code: self.device_code.clone(),
            expires_at: self.expires_at_rfc3339.clone(),
            message: self.message.clone(),
        }
    }

    fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            EnrollmentState::Completed
                | EnrollmentState::Cancelled
                | EnrollmentState::Expired
                | EnrollmentState::Failed
        )
    }

    fn wipe_temp_dir(&self) {
        let _ = fs::remove_dir_all(&self.temp_dir);
    }
}

/// Manages browser-driven CLI enrollments for one office:
/// - Caller-bound `login_id` (only the owner who started it may poll/submit/cancel)
/// - At most 1 active enrollment per `(office, connection)` (`ENROLL-4`)
/// - 15-minute expiry (`ENROLL-4`)
/// - Fresh `0700` temp directory wiped on terminal transitions (`ENROLL-1`, `ENROLL-4`)
/// - Releases session lock around driver I/O
pub struct EnrollmentManager {
    vault: CredentialVault,
    temp_root: PathBuf,
    driver: Arc<dyn EnrollmentDriver>,
    timeout: Duration,
    sessions: Mutex<HashMap<String, ActiveEnrollment>>,
}

impl EnrollmentManager {
    pub fn new(
        vault: CredentialVault,
        temp_root: PathBuf,
        driver: Arc<dyn EnrollmentDriver>,
    ) -> Self {
        Self::with_timeout(vault, temp_root, driver, DEFAULT_ENROLLMENT_TIMEOUT)
    }

    pub fn with_timeout(
        vault: CredentialVault,
        temp_root: PathBuf,
        driver: Arc<dyn EnrollmentDriver>,
        timeout: Duration,
    ) -> Self {
        Self {
            vault,
            temp_root,
            driver,
            timeout,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn vault(&self) -> &CredentialVault {
        &self.vault
    }

    /// Starts a new enrollment for `connection_id`, bound to `owner_member_id`.
    pub fn start(
        &self,
        owner_member_id: &str,
        connection_id: &str,
        provider: &str,
    ) -> Result<EnrollmentSessionView, EnrollmentVaultError> {
        if !is_safe_id(connection_id) {
            return Err(EnrollmentVaultError::InvalidInput(
                "invalid connection_id".to_owned(),
            ));
        }
        let login_id = format!("enroll-{}", random_hex(12)?);
        let temp_dir = self.temp_root.join(&login_id);
        let now = now_ms();
        let expires_at_ms = now.saturating_add(self.timeout.as_millis() as u64);
        {
            let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
            Self::expire_stale_locked(&mut sessions, now);
            if sessions
                .values()
                .any(|s| s.connection_id == connection_id && !s.is_terminal())
            {
                return Err(EnrollmentVaultError::Conflict(
                    "an enrollment session is already active for this provider connection"
                        .to_owned(),
                ));
            }
            ensure_owner_dirs(&temp_dir)?;
            sessions.insert(
                login_id.clone(),
                ActiveEnrollment {
                    login_id: login_id.clone(),
                    owner_member_id: owner_member_id.to_owned(),
                    connection_id: connection_id.to_owned(),
                    provider: provider.to_owned(),
                    temp_dir: temp_dir.clone(),
                    state: EnrollmentState::PendingBrowser,
                    auth_url: None,
                    device_code: None,
                    message: None,
                    expires_at_ms,
                    expires_at_rfc3339: format_rfc3339_ms(expires_at_ms),
                },
            );
        }

        // Run driver outside the mutex.
        let started = self.driver.start(provider, &temp_dir);
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        Self::expire_stale_locked(&mut sessions, now_ms());
        let Some(entry) = sessions.get_mut(&login_id) else {
            let _ = fs::remove_dir_all(&temp_dir);
            return Err(EnrollmentVaultError::NotFound(
                "enrollment session not found".into(),
            ));
        };
        if entry.is_terminal() {
            entry.wipe_temp_dir();
            let _ = fs::remove_dir_all(&temp_dir);
            return Ok(entry.view());
        }
        match started {
            Ok(prompt) => {
                entry.state = prompt.state;
                entry.auth_url = prompt.auth_url;
                entry.device_code = prompt.device_code;
                entry.message = prompt.message;
                Ok(entry.view())
            }
            Err(err) => {
                entry.wipe_temp_dir();
                sessions.remove(&login_id);
                Err(err)
            }
        }
    }

    /// Polls enrollment status for `(owner_member_id, connection_id, login_id)`.
    pub fn status(
        &self,
        owner_member_id: &str,
        connection_id: &str,
        login_id: &str,
    ) -> Result<EnrollmentSessionView, EnrollmentVaultError> {
        let (provider, temp_dir) = {
            let now = now_ms();
            let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
            Self::expire_stale_locked(&mut sessions, now);
            let entry = find_owned_mut(&mut sessions, owner_member_id, connection_id, login_id)?;
            if entry.is_terminal() {
                return Ok(entry.view());
            }
            (entry.provider.clone(), entry.temp_dir.clone())
        };
        let polled = self.driver.poll(&provider, &temp_dir);
        self.apply_outcome(owner_member_id, connection_id, login_id, polled, false)
    }

    /// Submits a pasted callback/auth code for `(owner_member_id, connection_id, login_id)` (`ENROLL-2`).
    pub fn submit_code(
        &self,
        owner_member_id: &str,
        connection_id: &str,
        login_id: &str,
        code: &str,
    ) -> Result<EnrollmentSessionView, EnrollmentVaultError> {
        let validated = validate_pasted_code(code)?;
        let (provider, temp_dir) = {
            let now = now_ms();
            let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
            Self::expire_stale_locked(&mut sessions, now);
            let entry = find_owned_mut(&mut sessions, owner_member_id, connection_id, login_id)?;
            if entry.state == EnrollmentState::Expired {
                return Err(EnrollmentVaultError::Expired(
                    "enrollment session has expired".to_owned(),
                ));
            }
            if entry.is_terminal() {
                return Err(EnrollmentVaultError::Conflict(
                    "enrollment session is already finished".to_owned(),
                ));
            }
            (entry.provider.clone(), entry.temp_dir.clone())
        };
        let outcome = self.driver.submit_code(&provider, &temp_dir, validated);
        self.apply_outcome(owner_member_id, connection_id, login_id, outcome, true)
    }

    /// Cancels an enrollment and immediately wipes its `0700` temp dir (`ENROLL-4`).
    pub fn cancel(
        &self,
        owner_member_id: &str,
        connection_id: &str,
        login_id: &str,
    ) -> Result<EnrollmentSessionView, EnrollmentVaultError> {
        let now = now_ms();
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        Self::expire_stale_locked(&mut sessions, now);
        let entry = find_owned_mut(&mut sessions, owner_member_id, connection_id, login_id)?;
        entry.wipe_temp_dir();
        if !entry.is_terminal() {
            entry.state = EnrollmentState::Cancelled;
            entry.message = Some("Enrollment cancelled.".to_owned());
        }
        Ok(entry.view())
    }

    /// Returns the temp directory path for a login_id (for testing cleanup verification).
    pub fn temp_dir_for(&self, login_id: &str) -> PathBuf {
        self.temp_root.join(login_id)
    }

    fn apply_outcome(
        &self,
        owner_member_id: &str,
        connection_id: &str,
        login_id: &str,
        outcome: Result<EnrollmentOutcome, EnrollmentVaultError>,
        reject_if_terminal: bool,
    ) -> Result<EnrollmentSessionView, EnrollmentVaultError> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        Self::expire_stale_locked(&mut sessions, now_ms());
        let entry = find_owned_mut(&mut sessions, owner_member_id, connection_id, login_id)?;
        if entry.is_terminal() {
            entry.wipe_temp_dir();
            if reject_if_terminal {
                return if entry.state == EnrollmentState::Expired {
                    Err(EnrollmentVaultError::Expired(
                        "enrollment session has expired".to_owned(),
                    ))
                } else {
                    Err(EnrollmentVaultError::Conflict(
                        "enrollment session is already finished".to_owned(),
                    ))
                };
            }
            return Ok(entry.view());
        }
        match outcome {
            Ok(EnrollmentOutcome::Completed {
                secret,
                expires_at,
                account_hint,
            }) => {
                let stored = self.vault.store_wrapped(
                    &entry.connection_id,
                    &entry.provider,
                    &secret,
                    expires_at,
                    account_hint,
                );
                entry.wipe_temp_dir();
                match stored {
                    Ok(_) => {
                        entry.state = EnrollmentState::Completed;
                        entry.message = Some("Credential enrolled and wrapped.".to_owned());
                        Ok(entry.view())
                    }
                    Err(err) => {
                        entry.state = EnrollmentState::Failed;
                        entry.message = Some("Failed to store wrapped credential.".to_owned());
                        Err(err)
                    }
                }
            }
            Ok(EnrollmentOutcome::Pending { state, message }) => {
                entry.state = state;
                if message.is_some() {
                    entry.message = message;
                }
                Ok(entry.view())
            }
            Ok(EnrollmentOutcome::Failed(msg)) => {
                entry.wipe_temp_dir();
                entry.state = EnrollmentState::Failed;
                entry.message = Some(msg);
                Ok(entry.view())
            }
            Err(err) => {
                entry.wipe_temp_dir();
                entry.state = EnrollmentState::Failed;
                entry.message = Some(err.to_string());
                Err(err)
            }
        }
    }

    fn expire_stale_locked(sessions: &mut HashMap<String, ActiveEnrollment>, now: u64) {
        for entry in sessions.values_mut() {
            if !entry.is_terminal() && now >= entry.expires_at_ms {
                entry.wipe_temp_dir();
                entry.state = EnrollmentState::Expired;
                entry.message = Some("Enrollment session expired.".to_owned());
            }
        }
    }
}

fn find_owned_mut<'a>(
    sessions: &'a mut HashMap<String, ActiveEnrollment>,
    owner_member_id: &str,
    connection_id: &str,
    login_id: &str,
) -> Result<&'a mut ActiveEnrollment, EnrollmentVaultError> {
    let entry = sessions
        .get_mut(login_id)
        .filter(|e| e.connection_id == connection_id)
        .ok_or_else(|| EnrollmentVaultError::NotFound("enrollment session not found".into()))?;
    if entry.owner_member_id != owner_member_id {
        return Err(EnrollmentVaultError::NotAuthorized(
            "enrollment session belongs to another caller".into(),
        ));
    }
    Ok(entry)
}

fn write_private_atomic(
    parent: &Path,
    dest: &Path,
    bytes: &[u8],
) -> Result<(), EnrollmentVaultError> {
    let tmp = parent.join(format!(".credential.{}.tmp", random_hex(8)?));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|_| EnrollmentVaultError::Io("could not create credential temp file".into()))?;
    if file.write_all(bytes).is_err() || org_platform::fs::full_sync(&file).is_err() {
        let _ = fs::remove_file(&tmp);
        return Err(EnrollmentVaultError::Io(
            "could not write credential temp file".into(),
        ));
    }
    drop(file);
    fs::rename(&tmp, dest).map_err(|_| {
        let _ = fs::remove_file(&tmp);
        EnrollmentVaultError::Io("could not commit credential file".into())
    })?;
    // Sync <conn_id>, auth, <office_id>, offices, and local so retries after any
    // earlier ancestor sync failure always persist the directory chain durably.
    sync_ancestor_dirs(parent, 5)
}

fn ensure_owner_dirs(path: &Path) -> Result<(), EnrollmentVaultError> {
    if path.exists() {
        return Ok(());
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        let prev = current.clone();
        current.push(component);
        if !current.exists() {
            match org_platform::fs::create_owner_only_dir(&current) {
                Ok(()) => {
                    if !prev.as_os_str().is_empty() {
                        sync_dir(&prev)?;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(_) => {
                    return Err(EnrollmentVaultError::Io(
                        "could not create owner-only directory".into(),
                    ))
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub(crate) fn random_hex(num_bytes: usize) -> Result<String, EnrollmentVaultError> {
    let mut bytes = vec![0u8; num_bytes];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| EnrollmentVaultError::Crypto("system entropy unavailable".into()))?;
    let mut out = String::with_capacity(num_bytes * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    Ok(out)
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn rfc3339_now() -> String {
    format_rfc3339_ms(now_ms())
}

pub(crate) fn format_rfc3339_ms(ms: u64) -> String {
    let secs = ms / 1000;
    let millis = ms % 1000;
    let days = (secs / 86_400) as i64;
    let sod = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        sod / 3600,
        (sod % 3600) / 60,
        sod % 60
    )
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m as u32, d as u32)
}
