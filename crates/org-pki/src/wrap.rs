//! Office private-key wrapping (ADR-0016 decision 3, ENC-5/ENC-6).
//!
//! An office keeps each private key in its own store as a record encrypted and
//! authenticated with its 32-byte office secrets key. The record is bound to the
//! organization, the office and the key's purpose, and its format is versioned.
//! This module takes raw bytes only: it knows no host, transport or storage types.
//!
//! Record: `version(1) || nonce(12) || AES-256-GCM(ciphertext || tag(16))`.
//! Authenticated data: `DOMAIN || version || purpose || organization || office`,
//! each of the last three prefixed with its length as one byte.
use ring::{
    aead::{self, Aad, LessSafeKey, Nonce, UnboundKey},
    digest::{digest, SHA256},
    hkdf,
    rand::{SecureRandom, SystemRandom},
};
use zeroize::Zeroizing;

const DOMAIN: &[u8] = b"agentic-org/office-private-key/v1";
const PROVIDER_CREDENTIAL_DOMAIN: &[u8] = b"agentic-org/provider-credential/v1";
const GIT_CREDENTIAL_DOMAIN: &[u8] = b"agentic-org/git-credential/v1";
const GITHUB_APP_DOMAIN: &[u8] = b"agentic-org/github-app/v1";
const HKDF_SALT: &[u8] = b"agentic-org/hkdf-salt/v1";
const HKDF_INFO_OFFICE_PRIVATE_KEY: &[u8] = b"agentic-org/wrap-key/office-private-key/v1";
const HKDF_INFO_PROVIDER_CREDENTIAL: &[u8] = b"agentic-org/wrap-key/provider-credential/v1";
const HKDF_INFO_GIT_CREDENTIAL: &[u8] = b"agentic-org/wrap-key/git-credential/v1";
const HKDF_INFO_GITHUB_APP: &[u8] = b"agentic-org/wrap-key/github-app/v1";
const VERSION: u8 = 1;
const NONCE: usize = 12;
const TAG: usize = 16;
/// Largest private-key encoding accepted for wrapping.
pub const MAX_KEY_BYTES: usize = 4096;

/// Cryptographically distinct key classes derived from a per-office master key (ADR-0016, ADR-0032, TASK-11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyClass {
    /// Wrapping key for office-owned private keys (Organization CA, TLS identity, invitation keys).
    OfficePrivateKey,
    /// Separate wrapping key for provider OAuth tokens and vendor sign-in credentials.
    ProviderCredential,
    /// Separate wrapping key for per-repository scoped Git HTTPS tokens (`TASK-135`, `doc-35`).
    GitCredential,
    /// Separate wrapping key for GitHub App private keys and webhook secrets (`TASK-272`).
    GitHubApp,
}

impl KeyClass {
    fn hkdf_info(self) -> &'static [u8] {
        match self {
            Self::OfficePrivateKey => HKDF_INFO_OFFICE_PRIVATE_KEY,
            Self::ProviderCredential => HKDF_INFO_PROVIDER_CREDENTIAL,
            Self::GitCredential => HKDF_INFO_GIT_CREDENTIAL,
            Self::GitHubApp => HKDF_INFO_GITHUB_APP,
        }
    }
}

struct Key32Len;
impl hkdf::KeyType for Key32Len {
    fn len(&self) -> usize {
        32
    }
}

/// Derives an independent 32-byte AES-256-GCM wrapping subkey for `class` via HKDF-SHA256.
/// Org CA private keys and provider OAuth credentials MUST use distinct wrapping keys.
pub fn derive_wrapping_key(
    office_secrets_key: &[u8; 32],
    class: KeyClass,
) -> Result<Zeroizing<[u8; 32]>, WrapError> {
    let salt = hkdf::Salt::new(hkdf::HKDF_SHA256, HKDF_SALT);
    let prk = salt.extract(office_secrets_key);
    let info = [class.hkdf_info()];
    let okm = prk
        .expand(&info, Key32Len)
        .map_err(|_| WrapError::Unavailable)?;
    let mut out = Zeroizing::new([0u8; 32]);
    okm.fill(out.as_mut()).map_err(|_| WrapError::Unavailable)?;
    Ok(out)
}

/// Binding for a wrapped provider OAuth credential (ADR-0016 decision 3, ADR-0032).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProviderCredentialBinding<'a> {
    pub organization_id: &'a str,
    pub office_id: &'a str,
    pub connection_id: &'a str,
}

/// Binding for a wrapped per-repository Git credential (`TASK-135`, `doc-35`, ADR-0016).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GitCredentialBinding<'a> {
    pub organization_id: &'a str,
    pub office_id: &'a str,
    pub repository_id: &'a str,
    pub scope_url: &'a str,
}

/// Binding for a wrapped GitHub App secret (`TASK-272`, ADR-0016): private key PEM or webhook secret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GitHubAppCredentialBinding<'a> {
    pub organization_id: &'a str,
    pub office_id: &'a str,
    pub app_id: u64,
    pub purpose: &'a str,
}

/// The role of the wrapped private key (ADR-0016 decision 3, `doc-67`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrapPurpose {
    OrganizationCa,
    OfficeTlsKey,
    InvitationKey,
}
impl WrapPurpose {
    fn label(self) -> &'static [u8] {
        match self {
            Self::OrganizationCa => b"organization-ca",
            Self::OfficeTlsKey => b"office-tls-key",
            Self::InvitationKey => b"invitation-key",
        }
    }
}

/// What a wrapped record is bound to. A record opens only for the same binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WrapBinding<'a> {
    pub organization_id: &'a str,
    pub office_id: &'a str,
    pub purpose: WrapPurpose,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrapError {
    /// The binding or key encoding is outside the accepted bounds.
    Invalid,
    /// The record is malformed, of another version, or fails authentication
    /// (wrong key, organization, office or purpose).
    Rejected,
    /// The random source or cipher is unavailable.
    Unavailable,
}
impl std::fmt::Display for WrapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Invalid => "invalid key wrapping input",
            Self::Rejected => "wrapped key record rejected",
            Self::Unavailable => "key wrapping unavailable",
        })
    }
}
impl std::error::Error for WrapError {}

/// Encrypts `secret` (a private-key encoding) under `key` for `binding`.
pub fn wrap(
    key: &[u8; 32],
    binding: &WrapBinding<'_>,
    secret: &[u8],
) -> Result<Vec<u8>, WrapError> {
    if secret.is_empty() || secret.len() > MAX_KEY_BYTES {
        return Err(WrapError::Invalid);
    }
    let aad = aad(binding)?;
    wrap_with_aad(key, &aad, secret)
}

/// Opens a record made by [`wrap`] with the same key and binding.
pub fn unwrap(
    key: &[u8; 32],
    binding: &WrapBinding<'_>,
    record: &[u8],
) -> Result<Zeroizing<Vec<u8>>, WrapError> {
    let aad = aad(binding)?;
    unwrap_with_aad(key, &aad, record)
}

/// Encrypts a provider OAuth credential under a dedicated `KeyClass::ProviderCredential`
/// wrapping key (derived via [`derive_wrapping_key`] or provisioned separately) and the
/// `agentic-org/provider-credential/v1` AAD domain (ADR-0016, ADR-0032, TASK-11).
pub fn wrap_provider_credential(
    provider_key: &[u8; 32],
    binding: &ProviderCredentialBinding<'_>,
    secret: &[u8],
) -> Result<Vec<u8>, WrapError> {
    if secret.is_empty() || secret.len() > MAX_KEY_BYTES {
        return Err(WrapError::Invalid);
    }
    let aad = provider_credential_aad(binding)?;
    wrap_with_aad(provider_key, &aad, secret)
}

/// Opens a provider credential record made by [`wrap_provider_credential`].
pub fn unwrap_provider_credential(
    provider_key: &[u8; 32],
    binding: &ProviderCredentialBinding<'_>,
    record: &[u8],
) -> Result<Zeroizing<Vec<u8>>, WrapError> {
    let aad = provider_credential_aad(binding)?;
    unwrap_with_aad(provider_key, &aad, record)
}

/// Encrypts a per-repository scoped Git credential under a dedicated `KeyClass::GitCredential`
/// wrapping key and the `agentic-org/git-credential/v1` AAD domain (`TASK-135`, `doc-35`).
pub fn wrap_git_credential(
    git_key: &[u8; 32],
    binding: &GitCredentialBinding<'_>,
    secret: &[u8],
) -> Result<Vec<u8>, WrapError> {
    if secret.is_empty() || secret.len() > MAX_KEY_BYTES {
        return Err(WrapError::Invalid);
    }
    let aad = git_credential_aad(binding)?;
    wrap_with_aad(git_key, &aad, secret)
}

/// Opens a per-repository Git credential record made by [`wrap_git_credential`].
pub fn unwrap_git_credential(
    git_key: &[u8; 32],
    binding: &GitCredentialBinding<'_>,
    record: &[u8],
) -> Result<Zeroizing<Vec<u8>>, WrapError> {
    let aad = git_credential_aad(binding)?;
    unwrap_with_aad(git_key, &aad, record)
}

/// Encrypts a GitHub App secret (private key or webhook secret) under a dedicated
/// `KeyClass::GitHubApp` wrapping key and the `agentic-org/github-app/v1` AAD domain (`TASK-272`).
pub fn wrap_github_app_credential(
    github_key: &[u8; 32],
    binding: &GitHubAppCredentialBinding<'_>,
    secret: &[u8],
) -> Result<Vec<u8>, WrapError> {
    if secret.is_empty() || secret.len() > MAX_KEY_BYTES {
        return Err(WrapError::Invalid);
    }
    let aad = github_app_credential_aad(binding)?;
    wrap_with_aad(github_key, &aad, secret)
}

/// Opens a GitHub App secret record made by [`wrap_github_app_credential`].
pub fn unwrap_github_app_credential(
    github_key: &[u8; 32],
    binding: &GitHubAppCredentialBinding<'_>,
    record: &[u8],
) -> Result<Zeroizing<Vec<u8>>, WrapError> {
    let aad = github_app_credential_aad(binding)?;
    unwrap_with_aad(github_key, &aad, record)
}

fn wrap_with_aad(key: &[u8; 32], aad: &[u8], secret: &[u8]) -> Result<Vec<u8>, WrapError> {
    let mut nonce = [0; NONCE];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| WrapError::Unavailable)?;
    let mut sealed = Zeroizing::new(Vec::with_capacity(secret.len() + TAG));
    sealed.extend_from_slice(secret);
    cipher(key)?
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(aad),
            &mut *sealed,
        )
        .map_err(|_| WrapError::Unavailable)?;
    let mut record = Vec::with_capacity(1 + NONCE + sealed.len());
    record.push(VERSION);
    record.extend_from_slice(&nonce);
    record.extend_from_slice(&sealed);
    Ok(record)
}

fn unwrap_with_aad(
    key: &[u8; 32],
    aad: &[u8],
    record: &[u8],
) -> Result<Zeroizing<Vec<u8>>, WrapError> {
    if record.len() <= 1 + NONCE + TAG || record.len() > 1 + NONCE + MAX_KEY_BYTES + TAG {
        return Err(WrapError::Rejected);
    }
    if record[0] != VERSION {
        return Err(WrapError::Rejected);
    }
    let nonce: [u8; NONCE] = record[1..1 + NONCE]
        .try_into()
        .map_err(|_| WrapError::Rejected)?;
    let mut buffer = Zeroizing::new(record[1 + NONCE..].to_vec());
    let len = cipher(key)?
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(aad),
            &mut buffer,
        )
        .map_err(|_| WrapError::Rejected)?
        .len();
    buffer.truncate(len);
    Ok(buffer)
}

fn cipher(key: &[u8; 32]) -> Result<LessSafeKey, WrapError> {
    Ok(LessSafeKey::new(
        UnboundKey::new(&aead::AES_256_GCM, key).map_err(|_| WrapError::Unavailable)?,
    ))
}

fn aad(binding: &WrapBinding<'_>) -> Result<Vec<u8>, WrapError> {
    encode_aad(
        DOMAIN,
        &[
            binding.purpose.label(),
            binding.organization_id.as_bytes(),
            binding.office_id.as_bytes(),
        ],
    )
}

fn provider_credential_aad(binding: &ProviderCredentialBinding<'_>) -> Result<Vec<u8>, WrapError> {
    encode_aad(
        PROVIDER_CREDENTIAL_DOMAIN,
        &[
            b"provider-credential",
            binding.organization_id.as_bytes(),
            binding.office_id.as_bytes(),
            binding.connection_id.as_bytes(),
        ],
    )
}

fn git_credential_aad(binding: &GitCredentialBinding<'_>) -> Result<Vec<u8>, WrapError> {
    if binding.scope_url.is_empty() {
        return Err(WrapError::Invalid);
    }
    let scope_digest = digest(&SHA256, binding.scope_url.as_bytes());
    encode_aad(
        GIT_CREDENTIAL_DOMAIN,
        &[
            b"git-credential",
            binding.organization_id.as_bytes(),
            binding.office_id.as_bytes(),
            binding.repository_id.as_bytes(),
            scope_digest.as_ref(),
        ],
    )
}

fn github_app_credential_aad(
    binding: &GitHubAppCredentialBinding<'_>,
) -> Result<Vec<u8>, WrapError> {
    if binding.app_id == 0 || binding.purpose.is_empty() {
        return Err(WrapError::Invalid);
    }
    let app_id_bytes = binding.app_id.to_be_bytes();
    encode_aad(
        GITHUB_APP_DOMAIN,
        &[
            b"github-app",
            binding.organization_id.as_bytes(),
            binding.office_id.as_bytes(),
            &app_id_bytes,
            binding.purpose.as_bytes(),
        ],
    )
}

fn encode_aad(domain: &[u8], fields: &[&[u8]]) -> Result<Vec<u8>, WrapError> {
    let mut aad =
        Vec::with_capacity(domain.len() + 1 + fields.iter().map(|f| 1 + f.len()).sum::<usize>());
    aad.extend_from_slice(domain);
    aad.push(VERSION);
    for field in fields {
        let len = u8::try_from(field.len()).map_err(|_| WrapError::Invalid)?;
        if len == 0 {
            return Err(WrapError::Invalid);
        }
        aad.push(len);
        aad.extend_from_slice(field);
    }
    Ok(aad)
}

#[cfg(test)]
mod tests;
