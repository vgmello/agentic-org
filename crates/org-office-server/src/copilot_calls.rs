//! Office RPC dispatcher for `copilot.features` and `copilot.draft` (`TASK-60` AC #3).
//!
//! Wires [`org_copilot::Copilot`] with its built-in drafting features (`text.improve` v1 and
//! `bug.from_sentence` v2) onto a tool-less runtime adapter backed by the office's configured
//! provider connections and model profiles (via [`org_channel_watcher::CliWatcherRuntimeProvider`]).
//!
//! Security invariants:
//! - Only an active [`org_proto::MemberKind::Person`] of this organization may invoke `copilot.*`
//!   (unbound callers, offboarded/removed persons, and agents fail closed with `NotAuthorized`).
//! - All input/context passes through `org_copilot`'s strict JSON Schema validator, byte/char caps,
//!   prompt-injection fencing (`<<<DATA` / `DATA>>>`), and secret rejection on both input and output.
//! - When no enabled tool-less provider/profile is configured (or the organization is under a
//!   budget hold), `copilot.features` returns an empty list and `copilot.draft` fails closed with
//!   `ErrorCode::CopilotUnavailable`.
//! - Run logs record ONLY non-body metadata (never input text, context, prompts, or draft output).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use org_channel_watcher::{CliWatcherRuntimeProvider, WatcherRuntimeProvider};
use org_copilot::{
    builtin, CancelToken, ContextItem, Copilot, CopilotError, CopilotRuntime, DraftRequest,
    FailReason, InvalidReason, Limits, RunLog, RunOutput, RunRecord, RunRequest, RuntimeError,
};
use org_office::organization_family::records::{ModelProfile, ProviderConnection};
use org_office::Office;
use org_office_sessions::{RuntimeRegistry, SessionManager};
use org_proto::copilot::{
    CopilotDraft, CopilotDraftParams, CopilotDraftResult, CopilotFeatureInfo, CopilotFeatures,
    CopilotFeaturesResult,
};
use org_proto::{ApiError, Call, ErrorCode, MemberKind, Request, Response};

/// Short cache lifetime for a positive availability probe so `copilot.draft` reuses the resolved
/// runtime from `Copilot::run`'s `available()` check instead of spawning a second CLI probe.
const POSITIVE_PROBE_CACHE_TTL: Duration = Duration::from_secs(10);
/// Shorter cache lifetime for a negative probe so newly installed runtimes become visible quickly.
const NEGATIVE_PROBE_CACHE_TTL: Duration = Duration::from_secs(2);

/// Explicit `request.is::<Call>()` predicate recognized by the host operation inventory AST scan.
pub(crate) fn is_copilot_method(request: &Request) -> bool {
    request.is::<CopilotFeatures>() || request.is::<CopilotDraft>()
}

/// Refusal returned when a caller is unbound, non-person, or no longer an active member.
pub(crate) fn not_authorized(id: u64) -> Response {
    Response::error(
        id,
        ApiError::new(
            ErrorCode::NotAuthorized,
            "only an active person of this organization can use the copilot",
        ),
    )
}

/// Wrapper allowing `Copilot<ArcRuntime>` to hold either the dynamic production runtime or a
/// scripted test runtime behind one uniform type.
struct ArcRuntime(Arc<dyn CopilotRuntime>);

impl CopilotRuntime for ArcRuntime {
    fn available(&self) -> bool {
        self.0.available()
    }

    fn run(&self, request: &RunRequest, cancel: &CancelToken) -> Result<RunOutput, RuntimeError> {
        self.0.run(request, cancel)
    }
}

/// Non-body metadata logger for copilot runs (never logs prompts, inputs, context, or answers).
struct CopilotStderrLog;

impl RunLog for CopilotStderrLog {
    fn record(&self, record: &RunRecord) {
        let line = format!(
            "agentic-org-office: copilot feature={} v={} outcome={:?} duration_ms={} input_bytes={} output_bytes={}",
            record.feature,
            record.version,
            record.outcome,
            record.duration.as_millis(),
            record.input_bytes,
            record.output_bytes,
        );
        eprintln!("{}", org_redact::redact_text(&line, &[]));
    }
}

#[derive(Clone, PartialEq, Eq)]
struct CandidateKey {
    profile_id: String,
    profile_key: String,
    profile_rev: u64,
    conn_id: String,
    driver: String,
    conn_rev: u64,
}

#[derive(Clone, PartialEq, Eq)]
struct ConfigFingerprint {
    watching_profile_id: Option<String>,
    candidates: Vec<CandidateKey>,
}

struct CachedProbe {
    fingerprint: ConfigFingerprint,
    runtime: Option<Arc<dyn CopilotRuntime>>,
    checked_at: Instant,
}

/// Dynamic tool-less copilot runtime resolved from the office's configured provider connections
/// and model profiles. Fails closed (`available() == false` / `RuntimeError::NotSetUp`) when no
/// enabled tool-less runtime is configured or when a budget hold is active.
///
/// Caches the last availability probe result keyed by the live office profile/connection
/// fingerprint so a `copilot.draft` request never probes the CLI binary twice (while budget holds
/// are always evaluated live and never cached).
struct OfficeCopilotRuntime {
    office: Arc<Office>,
    sessions: Option<Arc<SessionManager>>,
    provider: Arc<dyn WatcherRuntimeProvider>,
    probe_cache: Mutex<Option<CachedProbe>>,
}

impl OfficeCopilotRuntime {
    fn new(
        office: Arc<Office>,
        sessions: Option<Arc<SessionManager>>,
        provider: Arc<dyn WatcherRuntimeProvider>,
    ) -> Self {
        Self {
            office,
            sessions,
            provider,
            probe_cache: Mutex::new(None),
        }
    }

    fn has_budget_hold(&self) -> bool {
        self.sessions
            .as_deref()
            .is_some_and(SessionManager::has_budget_hold)
    }

    fn collect_candidates(&self) -> (ConfigFingerprint, Vec<(ModelProfile, ProviderConnection)>) {
        let watching_profile_id = self
            .office
            .watching_model_setting()
            .ok()
            .flatten()
            .and_then(|s| s.model_profile_id)
            .map(|id| id.as_str().to_owned());

        let mut ordered_profiles: Vec<ModelProfile> = Vec::new();
        if let Some(ref pid) = watching_profile_id {
            if let Some(profile) = self
                .office
                .load_model_profile(pid)
                .ok()
                .flatten()
                .filter(|p| !p.archived)
            {
                ordered_profiles.push(profile);
            }
        }
        if let Ok(mut profiles) = self.office.load_model_profiles() {
            profiles.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
            for profile in profiles.into_iter().filter(|p| !p.archived) {
                if !ordered_profiles.iter().any(|p| p.id == profile.id) {
                    ordered_profiles.push(profile);
                }
            }
        }

        let mut pairs = Vec::new();
        let mut keys = Vec::new();
        for profile in ordered_profiles {
            for conn_id in &profile.connection_ids {
                let Some(conn) = self
                    .office
                    .load_provider_connection(conn_id.as_str())
                    .ok()
                    .flatten()
                else {
                    continue;
                };
                if !conn.enabled || conn.archived {
                    continue;
                }
                keys.push(CandidateKey {
                    profile_id: profile.id.as_str().to_owned(),
                    profile_key: profile.key.clone(),
                    profile_rev: profile.meta.revision,
                    conn_id: conn.id.as_str().to_owned(),
                    driver: conn.runtime_driver.clone(),
                    conn_rev: conn.meta.revision,
                });
                pairs.push((profile.clone(), conn));
            }
        }
        (
            ConfigFingerprint {
                watching_profile_id,
                candidates: keys,
            },
            pairs,
        )
    }

    fn cached_runtime(
        &self,
        fingerprint: &ConfigFingerprint,
    ) -> Option<Option<Arc<dyn CopilotRuntime>>> {
        let guard = self.probe_cache.lock().unwrap_or_else(|p| p.into_inner());
        let entry = guard.as_ref()?;
        if &entry.fingerprint != fingerprint {
            return None;
        }
        let ttl = if entry.runtime.is_some() {
            POSITIVE_PROBE_CACHE_TTL
        } else {
            NEGATIVE_PROBE_CACHE_TTL
        };
        if entry.checked_at.elapsed() < ttl {
            Some(entry.runtime.clone())
        } else {
            None
        }
    }

    fn invalidate_cache(&self) {
        let mut guard = self.probe_cache.lock().unwrap_or_else(|p| p.into_inner());
        *guard = None;
    }

    fn resolve(&self, cancel: Option<&CancelToken>) -> Option<Arc<dyn CopilotRuntime>> {
        if self.has_budget_hold() {
            return None;
        }
        let (fingerprint, candidates) = self.collect_candidates();
        if candidates.is_empty() {
            return None;
        }
        if let Some(cached) = self.cached_runtime(&fingerprint) {
            return cached;
        }

        let mut resolved = None;
        for (profile, conn) in &candidates {
            if cancel.is_some_and(CancelToken::is_cancelled) {
                return None;
            }
            if let Some(rt) = self.provider.runtime_for_profile(profile, Some(conn)) {
                if rt.available() {
                    resolved = Some(rt);
                    break;
                }
            }
        }
        if cancel.is_some_and(CancelToken::is_cancelled) {
            return None;
        }
        let mut guard = self.probe_cache.lock().unwrap_or_else(|p| p.into_inner());
        *guard = Some(CachedProbe {
            fingerprint,
            runtime: resolved.clone(),
            checked_at: Instant::now(),
        });
        resolved
    }
}

impl CopilotRuntime for OfficeCopilotRuntime {
    fn available(&self) -> bool {
        self.resolve(None).is_some()
    }

    fn run(&self, request: &RunRequest, cancel: &CancelToken) -> Result<RunOutput, RuntimeError> {
        if cancel.is_cancelled() {
            return Err(RuntimeError::TimedOut);
        }
        let rt = self.resolve(Some(cancel)).ok_or_else(|| {
            if cancel.is_cancelled() {
                RuntimeError::TimedOut
            } else {
                RuntimeError::NotSetUp
            }
        })?;
        let result = rt.run(request, cancel);
        if matches!(result, Err(RuntimeError::NotSetUp)) {
            self.invalidate_cache();
        }
        result
    }
}

/// Office-level service serving `copilot.features` and `copilot.draft`.
pub(crate) struct CopilotService {
    office: Arc<Office>,
    copilot: Copilot<ArcRuntime>,
}

impl CopilotService {
    /// Production service backed by the office's configured providers and [`RuntimeRegistry`].
    pub(crate) fn new_production(
        office: Arc<Office>,
        sessions: Option<Arc<SessionManager>>,
        runtimes: RuntimeRegistry,
    ) -> Self {
        let provider: Arc<dyn WatcherRuntimeProvider> =
            Arc::new(CliWatcherRuntimeProvider::new(runtimes));
        Self::with_provider(office, sessions, provider, Limits::default())
    }

    /// Constructs a [`CopilotService`] backed by [`OfficeCopilotRuntime`] with an injected
    /// [`WatcherRuntimeProvider`] (production composition and unit tests).
    pub(crate) fn with_provider(
        office: Arc<Office>,
        sessions: Option<Arc<SessionManager>>,
        provider: Arc<dyn WatcherRuntimeProvider>,
        limits: Limits,
    ) -> Self {
        let runtime: Arc<dyn CopilotRuntime> = Arc::new(OfficeCopilotRuntime::new(
            Arc::clone(&office),
            sessions,
            provider,
        ));
        Self::with_runtime(office, runtime, limits)
    }

    /// Constructs a [`CopilotService`] with an injected [`CopilotRuntime`] and [`Limits`].
    pub(crate) fn with_runtime(
        office: Arc<Office>,
        runtime: Arc<dyn CopilotRuntime>,
        limits: Limits,
    ) -> Self {
        let copilot = Copilot::new(builtin(), ArcRuntime(runtime), limits)
            .with_log(Arc::new(CopilotStderrLog));
        Self { office, copilot }
    }

    /// Verifies `member` is an active person of this organization (fails closed on agents,
    /// offboarded members, unknown IDs, or snapshot errors).
    fn require_active_person(&self, member: &str) -> Result<(), ApiError> {
        let snapshot = self.office.org_snapshot().map_err(|err| {
            crate::logged("copilot.auth", &err);
            ApiError::new(ErrorCode::StorageError, "the office could not answer")
        })?;
        let allowed = snapshot
            .members
            .iter()
            .any(|m| m.id == member && m.kind == MemberKind::Person);
        if !allowed {
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "only an active person of this organization can use the copilot",
            ));
        }
        Ok(())
    }

    /// Dispatches `copilot.features` and `copilot.draft`.
    pub(crate) fn dispatch(&self, member: Option<&str>, request: &Request) -> Option<Response> {
        if !is_copilot_method(request) {
            return None;
        }
        let Some(member) = member else {
            return Some(not_authorized(request.id));
        };
        if let Err(error) = self.require_active_person(member) {
            return Some(Response::error(request.id, error));
        }
        if request.is::<CopilotFeatures>() {
            return Some(self.handle_features(request));
        }
        if request.is::<CopilotDraft>() {
            return Some(self.handle_draft(member, request));
        }
        None
    }

    fn handle_features(&self, request: &Request) -> Response {
        if request.params::<CopilotFeatures>().is_err() {
            return Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    format!("the params of `{}` are not valid", CopilotFeatures::METHOD),
                ),
            );
        }
        let features = self
            .copilot
            .features()
            .into_iter()
            .map(|f| CopilotFeatureInfo {
                id: f.id,
                version: f.version,
                title: f.title,
            })
            .collect();
        Response::ok::<CopilotFeatures>(request.id, &CopilotFeaturesResult { features })
            .unwrap_or_else(|_| storage_error(request.id))
    }

    fn handle_draft(&self, member: &str, request: &Request) -> Response {
        let Ok(CopilotDraftParams {
            feature,
            input,
            context,
        }) = request.params::<CopilotDraft>()
        else {
            return Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    format!("the params of `{}` are not valid", CopilotDraft::METHOD),
                ),
            );
        };
        let draft_req = DraftRequest {
            feature,
            input,
            context: context
                .into_iter()
                .map(|c| ContextItem {
                    kind: c.kind,
                    label: c.label,
                    text: c.text,
                })
                .collect(),
        };
        match self.copilot.draft(member, &draft_req) {
            Ok(res) => Response::ok::<CopilotDraft>(
                request.id,
                &CopilotDraftResult {
                    draft: res.draft,
                    notes: res.notes,
                },
            )
            .unwrap_or_else(|_| storage_error(request.id)),
            Err(err) => Response::error(request.id, map_copilot_error(err)),
        }
    }
}

fn storage_error(id: u64) -> Response {
    Response::error(
        id,
        ApiError::new(ErrorCode::StorageError, "the office could not answer"),
    )
}

/// Maps [`CopilotError`] to fixed-message wire [`ApiError`] (never echoes input/context/output).
fn map_copilot_error(err: CopilotError) -> ApiError {
    let code = match err {
        CopilotError::Unavailable | CopilotError::Invalid(InvalidReason::UnknownFeature) => {
            ErrorCode::CopilotUnavailable
        }
        CopilotError::Invalid(InvalidReason::TooLarge) => ErrorCode::TooLarge,
        CopilotError::Invalid(
            InvalidReason::BadInput
            | InvalidReason::ContextNotAllowed
            | InvalidReason::LooksLikeSecret,
        ) => ErrorCode::InvalidRequest,
        CopilotError::Busy | CopilotError::Overloaded => ErrorCode::CopilotBusy,
        CopilotError::Timeout => ErrorCode::Timeout,
        CopilotError::Failed(
            FailReason::Runtime
            | FailReason::AnswerTooLarge
            | FailReason::NotJson
            | FailReason::SchemaMismatch
            | FailReason::SecretInAnswer,
        ) => ErrorCode::CopilotFailed,
    };
    ApiError::new(code, err.to_string())
}

#[cfg(test)]
#[path = "copilot_calls_tests.rs"]
mod tests;
