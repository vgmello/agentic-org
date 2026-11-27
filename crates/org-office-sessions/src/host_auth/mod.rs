//! Runtime `TMPDIR` placement and vault-native per-connection OAuth lifecycle (`TASK-132`).

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use org_agent_runtime::{AgentRuntime, SpecPath};
use org_office::organization_family::records::ProviderAuthMode;
use zeroize::Zeroizing;

#[cfg(test)]
mod tests;
mod token;

use crate::credentials::{
    format_rfc3339_ms, CredentialVault, EnrollmentVaultError, UnwrappedCredential,
};

pub(crate) use token::{
    collect_vault_redaction_secrets, hex_short_sha256, is_oauth_expiring,
    parse_vault_oauth_credential, VaultAuthError, VaultOAuthCredential,
};
#[cfg(test)]
pub(crate) use token::{
    oauth_token_fingerprint, MAX_VAULT_OAUTH_CRED_BYTES, OAUTH_REFRESH_LEAD_MS,
};

const MAX_SHORT_TMPDIR_BYTES: usize = 70;
const MAX_REFRESH_DETAIL_BYTES: usize = 200;
const MAX_REFRESH_LOCK_ENTRIES: usize = 256;

pub(crate) fn office_runtime_dir(office_socket: &Path) -> Option<&Path> {
    org_proto::location::runtime_dir_of_office_socket(office_socket)
        .or_else(|| office_socket.parent())
}

pub(crate) fn agent_tmpdir_slug(office_id: &str, employee_id: &str) -> String {
    hex_short_sha256(format!("{office_id}:{employee_id}").as_bytes())[..8].to_owned()
}

pub(crate) fn resolve_agent_tmpdir(
    office_socket: &Path,
    durable_fallback: &Path,
    office_id: &str,
    employee_id: &str,
) -> PathBuf {
    let Some(runtime) = office_runtime_dir(office_socket) else {
        return durable_fallback.to_path_buf();
    };
    let candidate = runtime
        .join("t")
        .join(agent_tmpdir_slug(office_id, employee_id));
    if candidate.as_os_str().len() <= MAX_SHORT_TMPDIR_BYTES
        && candidate.as_os_str().len() < durable_fallback.as_os_str().len()
    {
        candidate
    } else {
        durable_fallback.to_path_buf()
    }
}

pub(crate) fn allows_host_subscription_auth(
    auth_env: &[(OsString, OsString)],
    auth_mode: Option<ProviderAuthMode>,
    uses_host_oauth_token_injection: bool,
) -> bool {
    !uses_host_oauth_token_injection
        && auth_env.is_empty()
        && matches!(auth_mode, None | Some(ProviderAuthMode::SubscriptionCli))
}

pub(crate) fn host_config_dir_for(
    host_config_subdir: Option<&str>,
    allowed: bool,
) -> Option<SpecPath> {
    if !allowed {
        return None;
    }
    let candidate = std::env::var_os("HOME")
        .map(PathBuf::from)?
        .join(host_config_subdir?);
    if !candidate.is_dir() {
        return None;
    }
    let resolved = candidate.canonicalize().unwrap_or(candidate);
    SpecPath::try_from(resolved.display().to_string().as_str()).ok()
}

pub(crate) struct ResolvedVaultOAuth {
    pub access_token: Zeroizing<String>,
    pub expires_at_ms: Option<u64>,
    pub token_fp: String,
}

impl ResolvedVaultOAuth {
    fn from_credential(cred: VaultOAuthCredential) -> Self {
        let token_fp = cred.fingerprint();
        Self {
            access_token: cred.access_token,
            expires_at_ms: cred.expires_at_ms,
            token_fp,
        }
    }
}

pub(crate) trait VaultTokenRefresher: Send + Sync {
    fn refresh(
        &self,
        connection_id: &str,
        provider: &str,
        refresh_token: &str,
    ) -> Result<VaultOAuthCredential, String>;
}

#[derive(Debug, Default)]
pub(crate) struct NoopVaultTokenRefresher;

impl VaultTokenRefresher for NoopVaultTokenRefresher {
    fn refresh(
        &self,
        _connection_id: &str,
        _provider: &str,
        _refresh_token: &str,
    ) -> Result<VaultOAuthCredential, String> {
        Err("no automatic OAuth token refresher is configured".to_owned())
    }
}

pub(crate) struct VaultRefreshCoordinator {
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    refresher: Mutex<Arc<dyn VaultTokenRefresher>>,
}

impl VaultRefreshCoordinator {
    pub(crate) fn new(refresher: Arc<dyn VaultTokenRefresher>) -> Self {
        Self {
            locks: Mutex::new(HashMap::new()),
            refresher: Mutex::new(refresher),
        }
    }

    pub(crate) fn noop() -> Self {
        Self::new(Arc::new(NoopVaultTokenRefresher))
    }

    #[cfg(test)]
    pub(crate) fn set_refresher(&self, refresher: Arc<dyn VaultTokenRefresher>) {
        *self.refresher.lock().unwrap_or_else(|p| p.into_inner()) = refresher;
    }

    /// Resolves a connection's vault OAuth credential for launch, proactively refreshing once
    /// under per-connection single-flight serialization when expiring or expired.
    pub(crate) fn resolve_for_launch(
        &self,
        vault: &CredentialVault,
        connection_id: &str,
        provider: &str,
        now_ms: u64,
    ) -> Result<ResolvedVaultOAuth, VaultAuthError> {
        let (cred, _) = load_vault_oauth_with_meta(vault, connection_id)?;
        if !is_oauth_expiring(cred.expires_at_ms, now_ms) {
            return Ok(ResolvedVaultOAuth::from_credential(cred));
        }
        let conn_lock = self.connection_lock(connection_id);
        let _flight = conn_lock.lock().unwrap_or_else(|p| p.into_inner());
        let (current, meta) = load_vault_oauth_with_meta(vault, connection_id)?;
        if !is_oauth_expiring(current.expires_at_ms, now_ms) {
            return Ok(ResolvedVaultOAuth::from_credential(current));
        }
        self.perform_refresh_locked(
            vault,
            connection_id,
            provider,
            &current,
            meta.account_hint,
            None,
            false,
            now_ms,
        )
    }

    /// Reactive single-flight refresh invoked after a pre-side-effect `auth_expired` failure.
    /// Immediately adopts an already-rotated fresh vault token without calling the refresher.
    pub(crate) fn refresh_reactive(
        &self,
        vault: &CredentialVault,
        connection_id: &str,
        provider: &str,
        failed_token_fp: Option<&str>,
        now_ms: u64,
    ) -> Result<ResolvedVaultOAuth, VaultAuthError> {
        let conn_lock = self.connection_lock(connection_id);
        let _flight = conn_lock.lock().unwrap_or_else(|p| p.into_inner());
        let (current, meta) = load_vault_oauth_with_meta(vault, connection_id)?;
        let already_rotated = failed_token_fp.is_some_and(|fp| current.fingerprint() != fp);
        if already_rotated && !is_oauth_expiring(current.expires_at_ms, now_ms) {
            return Ok(ResolvedVaultOAuth::from_credential(current));
        }
        self.perform_refresh_locked(
            vault,
            connection_id,
            provider,
            &current,
            meta.account_hint,
            failed_token_fp,
            true,
            now_ms,
        )
    }

    fn connection_lock(&self, connection_id: &str) -> Arc<Mutex<()>> {
        let mut map = self.locks.lock().unwrap_or_else(|p| p.into_inner());
        if map.len() >= MAX_REFRESH_LOCK_ENTRIES {
            map.retain(|_, lock| Arc::strong_count(lock) > 1);
        }
        Arc::clone(
            map.entry(connection_id.to_owned())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "single-flight refresh requires vault, connection identity, current credential, custody hint, rejected fingerprint, mode, and clock"
    )]
    fn perform_refresh_locked(
        &self,
        vault: &CredentialVault,
        connection_id: &str,
        provider: &str,
        current: &VaultOAuthCredential,
        account_hint: Option<String>,
        failed_token_fp: Option<&str>,
        is_reactive: bool,
        now_ms: u64,
    ) -> Result<ResolvedVaultOAuth, VaultAuthError> {
        let reason = if is_reactive {
            "auth_expired"
        } else {
            "vault_oauth_expired"
        };
        let Some(ref refresh_token) = current.refresh_token else {
            return Err(VaultAuthError::ReauthRequired {
                reason,
                detail: "vault credential has no refresh_token".to_owned(),
            });
        };
        let refresher = Arc::clone(&self.refresher.lock().unwrap_or_else(|p| p.into_inner()));
        let mut rotated = refresher
            .refresh(connection_id, provider, refresh_token.as_str())
            .map_err(|raw_err| VaultAuthError::ReauthRequired {
                reason,
                detail: redact_refresh_detail(
                    &raw_err,
                    current.access_token.as_str(),
                    refresh_token.as_str(),
                ),
            })?;
        validate_rotated_credential(
            current,
            &rotated,
            failed_token_fp,
            is_reactive,
            now_ms,
            reason,
        )?;
        if rotated.refresh_token.is_none() {
            rotated.refresh_token = current.refresh_token.clone();
        }
        let encoded = rotated.to_vault_json()?;
        let meta_expires = rotated.expires_at_ms.map(format_rfc3339_ms);
        vault
            .store_wrapped(
                connection_id,
                provider,
                &encoded,
                meta_expires,
                account_hint,
            )
            .map_err(|_| VaultAuthError::VaultPersistFailed)?;
        Ok(ResolvedVaultOAuth::from_credential(rotated))
    }
}

/// Resolved vault-native auth environment and OAuth expiry metadata for an admitted connection.
pub(crate) struct ConnectionVaultAuth {
    pub auth_env: Vec<(OsString, OsString)>,
    pub oauth_expires_at_ms: Option<u64>,
    pub oauth_token_fp: Option<String>,
}

/// Failure resolving a provider connection's vault credential at route admission.
pub(crate) enum ConnectionAuthError {
    VaultAuth(VaultAuthError),
    Fatal(org_proto::ApiError),
}

/// Resolves per-connection vault auth for any [`ProviderAuthMode`] (`TASK-132`).
///
/// For `SubscriptionCli`, parses raw tokens or structured JSON envelopes, proactively refreshes
/// expiring OAuth tokens under single-flight locking, and fails closed on missing credentials when
/// the runtime requires injected OAuth tokens (`uses_host_oauth_token_injection`).
pub(crate) fn resolve_connection_vault_auth(
    vault: &CredentialVault,
    coordinator: &VaultRefreshCoordinator,
    adapter: &dyn AgentRuntime,
    connection_id: &str,
    provider_type: &str,
    auth_mode: ProviderAuthMode,
    now_ms: u64,
) -> Result<ConnectionVaultAuth, ConnectionAuthError> {
    if auth_mode == ProviderAuthMode::SubscriptionCli {
        return resolve_subscription_cli_vault_auth(
            vault,
            coordinator,
            adapter,
            connection_id,
            provider_type,
            now_ms,
        )
        .map_err(|err| match err {
            VaultAuthError::VaultUnavailable(msg) => ConnectionAuthError::Fatal(
                org_proto::ApiError::new(org_proto::ErrorCode::Unavailable, msg),
            ),
            other => ConnectionAuthError::VaultAuth(other),
        });
    }
    let mut auth_env = Vec::new();
    if let Some(secret) = vault.load_unwrapped(connection_id).map_err(|_| {
        ConnectionAuthError::Fatal(org_proto::ApiError::new(
            org_proto::ErrorCode::Unavailable,
            "provider credential could not be unwrapped",
        ))
    })? {
        let secret_str = std::str::from_utf8(&secret).map_err(|_| {
            ConnectionAuthError::Fatal(org_proto::ApiError::new(
                org_proto::ErrorCode::Unavailable,
                "provider credential is not UTF-8",
            ))
        })?;
        let var_name = adapter.auth_env_var(false, secret_str).ok_or_else(|| {
            ConnectionAuthError::Fatal(org_proto::ApiError::new(
                org_proto::ErrorCode::Unavailable,
                "agent runtime does not declare an auth environment variable",
            ))
        })?;
        auth_env.push((OsString::from(var_name), OsString::from(secret_str)));
    }
    Ok(ConnectionVaultAuth {
        auth_env,
        oauth_expires_at_ms: None,
        oauth_token_fp: None,
    })
}

fn resolve_subscription_cli_vault_auth(
    vault: &CredentialVault,
    coordinator: &VaultRefreshCoordinator,
    adapter: &dyn AgentRuntime,
    connection_id: &str,
    provider_type: &str,
    now_ms: u64,
) -> Result<ConnectionVaultAuth, VaultAuthError> {
    let requires_vault_token = adapter.host_profile().uses_host_oauth_token_injection;
    let resolved = match coordinator.resolve_for_launch(vault, connection_id, provider_type, now_ms)
    {
        Ok(cred) => cred,
        Err(VaultAuthError::MissingCredential) if !requires_vault_token => {
            return Ok(ConnectionVaultAuth {
                auth_env: Vec::new(),
                oauth_expires_at_ms: None,
                oauth_token_fp: None,
            });
        }
        Err(err) => return Err(err),
    };
    let var_name = adapter
        .auth_env_var(true, resolved.access_token.as_str())
        .ok_or(VaultAuthError::MalformedCredential(
            "agent runtime does not declare an auth environment variable",
        ))?;
    Ok(ConnectionVaultAuth {
        auth_env: vec![(
            OsString::from(var_name),
            OsString::from(resolved.access_token.as_str()),
        )],
        oauth_expires_at_ms: resolved.expires_at_ms,
        oauth_token_fp: Some(resolved.token_fp),
    })
}

fn map_unwrap_error(err: EnrollmentVaultError) -> VaultAuthError {
    match err {
        EnrollmentVaultError::MissingSecretsKey => {
            VaultAuthError::VaultUnavailable("office secrets key is unavailable")
        }
        EnrollmentVaultError::Io(_) => {
            VaultAuthError::VaultUnavailable("provider credential could not be read")
        }
        EnrollmentVaultError::Crypto(_) | EnrollmentVaultError::InvalidInput(_) => {
            VaultAuthError::MalformedCredential("provider credential could not be unwrapped")
        }
        _ => VaultAuthError::VaultUnavailable("provider credential vault is unavailable"),
    }
}

fn load_vault_oauth_with_meta(
    vault: &CredentialVault,
    connection_id: &str,
) -> Result<(VaultOAuthCredential, UnwrappedCredential), VaultAuthError> {
    let unwrapped = vault
        .load_unwrapped_with_meta(connection_id)
        .map_err(map_unwrap_error)?
        .ok_or(VaultAuthError::MissingCredential)?;
    let mut cred = parse_vault_oauth_credential(&unwrapped.secret)?;
    if cred.expires_at_ms.is_none() {
        if let Some(ref raw_exp) = unwrapped.expires_at {
            let ms = org_proto::parse_rfc3339_ms(raw_exp).ok_or(
                VaultAuthError::MalformedCredential("vault credential has invalid expires_at"),
            )?;
            cred.expires_at_ms = Some(ms);
        }
    }
    Ok((cred, unwrapped))
}

fn reauth_required(reason: &'static str, detail: &str) -> VaultAuthError {
    VaultAuthError::ReauthRequired {
        reason,
        detail: detail.to_owned(),
    }
}

fn validate_rotated_credential(
    current: &VaultOAuthCredential,
    rotated: &VaultOAuthCredential,
    failed_token_fp: Option<&str>,
    is_reactive: bool,
    now_ms: u64,
    reason: &'static str,
) -> Result<(), VaultAuthError> {
    rotated
        .validate()
        .map_err(|_| reauth_required(reason, "OAuth refresh returned an invalid access token"))?;
    if is_oauth_expiring(rotated.expires_at_ms, now_ms) {
        return Err(reauth_required(
            reason,
            "OAuth refresh returned an already-expiring token",
        ));
    }
    ensure_token_progressed(current, rotated, failed_token_fp, is_reactive, reason)
}

fn ensure_token_progressed(
    current: &VaultOAuthCredential,
    rotated: &VaultOAuthCredential,
    failed_token_fp: Option<&str>,
    is_reactive: bool,
    reason: &'static str,
) -> Result<(), VaultAuthError> {
    if failed_token_fp.is_some_and(|fp| rotated.fingerprint() == fp) {
        return Err(reauth_required(
            reason,
            "OAuth refresh did not rotate the access token",
        ));
    }
    if rotated.access_token.as_str() != current.access_token.as_str() {
        return Ok(());
    }
    if is_reactive || rotated.expires_at_ms.is_none() {
        return Err(reauth_required(
            reason,
            "OAuth refresh did not rotate the access token",
        ));
    }
    if rotated.expires_at_ms <= current.expires_at_ms {
        return Err(reauth_required(
            reason,
            "OAuth refresh did not extend token expiry",
        ));
    }
    Ok(())
}

fn redact_refresh_detail(raw: &str, access_token: &str, refresh_token: &str) -> String {
    let scrubbed = crate::redact::redact_text(raw, &[access_token, refresh_token]);
    let trimmed = scrubbed.trim();
    if trimmed.is_empty() {
        return "OAuth token refresh failed".to_owned();
    }
    let cut = trimmed.floor_char_boundary(MAX_REFRESH_DETAIL_BYTES.min(trimmed.len()));
    trimmed[..cut].to_owned()
}
