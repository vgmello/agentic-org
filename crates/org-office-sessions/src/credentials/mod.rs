//! Wrapped provider credential custody and browser-driven CLI enrollment (`ADR-0032`,
//! `ENROLL-1..6`, `FC-189`).

use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use org_pki::wrap::WrapError;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

mod enrollment;
mod vault;

pub use enrollment::{
    CliEnrollmentDriver, EnrollmentDriver, EnrollmentManager, EnrollmentOutcome, EnrollmentPrompt,
};
pub use vault::CredentialVault;

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

/// Redacts active secrets and every known secret shape (`ENROLL-3`). Kept for callers of the
/// public credentials API; it is the crate's single redaction choke-point,
/// [`crate::redact::redact_text`], and nothing else.
pub fn redact_secrets(text: &str, active_secrets: &[&str]) -> String {
    crate::redact::redact_text(text, active_secrets)
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
