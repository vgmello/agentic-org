//! Browser-driven provider CLI enrollment: drivers, the single active enrollment, expiry and
//! wipe (`ENROLL-1..7`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use org_proto::sessions::{EnrollmentSessionView, EnrollmentState};
use zeroize::Zeroizing;

use super::vault::ensure_owner_dirs;
use super::{
    format_rfc3339_ms, is_safe_id, now_ms, random_hex, validate_pasted_code, CredentialVault,
    EnrollmentVaultError, DEFAULT_ENROLLMENT_TIMEOUT,
};

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
                let is_gemini_cli = provider.eq_ignore_ascii_case("gemini")
                    || provider.eq_ignore_ascii_case("gemini-cli");
                let (auth_url, message) = if is_gemini_cli {
                    (
                        "https://aistudio.google.com/app/apikey",
                        "Authenticate with Gemini CLI in your browser and paste the returned authorization code or API key.",
                    )
                } else {
                    (
                        "https://antigravity.google/auth",
                        "Authenticate with Antigravity (agy) in your browser and paste the returned authorization code or API key.",
                    )
                };
                Ok(EnrollmentPrompt {
                    state: EnrollmentState::AwaitingCode,
                    auth_url: Some(auth_url.to_owned()),
                    device_code: None,
                    message: Some(message.to_owned()),
                })
            }
            "kimi" | "kimi-cli" | "kimi-code" | "moonshot" => Ok(EnrollmentPrompt {
                state: EnrollmentState::AwaitingCode,
                auth_url: Some("https://platform.moonshot.ai/console/api-keys".to_owned()),
                device_code: None,
                message: Some(
                    "Authenticate with Kimi Code CLI and paste the returned OAuth token or API key."
                        .to_owned(),
                ),
            }),
            "pi" | "pi-acp" | "pi-coding-agent" => Ok(EnrollmentPrompt {
                state: EnrollmentState::AwaitingCode,
                auth_url: None,
                device_code: None,
                message: Some(
                    "Paste the API key or OAuth token for your configured Pi ACP provider."
                        .to_owned(),
                ),
            }),
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

pub(super) struct ActiveEnrollment {
    pub(super) login_id: String,
    pub(super) owner_member_id: String,
    pub(super) connection_id: String,
    pub(super) provider: String,
    pub(super) temp_dir: PathBuf,
    pub(super) state: EnrollmentState,
    pub(super) auth_url: Option<String>,
    pub(super) device_code: Option<String>,
    pub(super) message: Option<String>,
    pub(super) expires_at_ms: u64,
    pub(super) expires_at_rfc3339: String,
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
    pub(super) vault: CredentialVault,
    pub(super) temp_root: PathBuf,
    pub(super) driver: Arc<dyn EnrollmentDriver>,
    pub(super) timeout: Duration,
    pub(super) sessions: Mutex<HashMap<String, ActiveEnrollment>>,
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

pub(super) fn find_owned_mut<'a>(
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
