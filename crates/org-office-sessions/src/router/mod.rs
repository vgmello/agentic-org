//! Deterministic `L4` provider pool connection router, `endpoint_url` env plumbing, and
//! §7 retry-eligible failover classification (`TASK-54`, `FC-098`, `FC-099`, `doc-88` §6-§7).

use std::collections::HashSet;
use std::ffi::OsString;
use std::sync::Arc;

use org_agent_runtime::{AgentRuntime, Token};
use org_office::organization_family::features::validate_endpoint_url;
use org_office::organization_family::records::{
    PermissionPreset, ProviderAuthMode, ProviderConnection, ReasoningEffort,
};
use org_office::providers::PermitAcquireOutcome;
use org_office::Office;
use org_proto::providers::{
    ConnectionReadinessStatus, DiscoveredModelEntitlement, PermitAuthority,
};
use org_proto::{ApiError, ErrorCode};

use crate::credentials::CredentialVault;
use crate::runtime_registry::RuntimeRegistry;

mod warm_permit;

use warm_permit::compute_route_digest;

#[cfg(test)]
pub(crate) use warm_permit::compute_route_digest_with_wt;
pub(crate) use warm_permit::{
    acquire_warm_permit_for_office, compute_worktree_sandbox_digest, WarmPermitOutcome,
};

pub(crate) struct AdmittedRoute {
    pub adapter: Arc<dyn AgentRuntime>,
    pub model_token: Option<Token>,
    pub model_supports_vision: bool,
    pub effort: Option<Token>,
    pub auth_mode: Option<ProviderAuthMode>,
    pub auth_env: Vec<(OsString, OsString)>,
    pub endpoint_env: Vec<(OsString, OsString)>,
    pub oauth_expires_at_ms: Option<u64>,
    pub oauth_token_fp: Option<String>,
    pub preset: PermissionPreset,
    pub sandbox_enabled: bool,
    pub connection_id: Option<String>,
    pub permit: Option<(String, u64)>,
    pub route_digest_suffix: String,
}

pub fn endpoint_env_for_adapter(
    adapter: &dyn AgentRuntime,
    endpoint_url: &str,
) -> Result<(&'static str, String), ApiError> {
    if !validate_endpoint_url(endpoint_url) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "invalid provider connection endpoint_url",
        ));
    }
    let var = adapter.endpoint_env_var().ok_or_else(|| {
        ApiError::new(
            ErrorCode::InvalidRequest,
            "agent runtime does not support custom endpoint_url",
        )
    })?;
    Ok((var, endpoint_url.to_owned()))
}

pub fn endpoint_env_for(
    runtimes: &RuntimeRegistry,
    driver: &str,
    endpoint_url: &str,
) -> Result<(&'static str, String), ApiError> {
    if !validate_endpoint_url(endpoint_url) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "invalid provider connection endpoint_url",
        ));
    }
    let adapter = runtimes
        .resolve(driver)
        .map_err(|_| ApiError::new(ErrorCode::Unavailable, "agent runtime is unavailable"))?;
    endpoint_env_for_adapter(adapter.as_ref(), endpoint_url)
}

pub fn auth_env_var_for_adapter(
    adapter: &dyn AgentRuntime,
    mode: ProviderAuthMode,
    secret: &str,
) -> Result<&'static str, ApiError> {
    adapter
        .auth_env_var(mode == ProviderAuthMode::SubscriptionCli, secret)
        .ok_or_else(|| {
            ApiError::new(
                ErrorCode::Unavailable,
                "agent runtime does not declare an auth environment variable",
            )
        })
}

const FAILOVER_PATTERNS: &[(&[&str], &str)] = &[
    (
        &["rate_limit", "rate limit", "429", "too many requests"],
        "rate_limited",
    ),
    (
        &[
            "quota_exhausted",
            "quota exceeded",
            "usage limit",
            "credit balance",
        ],
        "quota_exhausted",
    ),
    (
        &[
            "auth_expired",
            "unauthorized",
            "invalid_api_key",
            "token expired",
            "401",
        ],
        "auth_expired",
    ),
    (
        &["model_unavailable", "model not found", "overloaded", "529"],
        "model_unavailable",
    ),
];

/// Classifies whether a turn failure is retry-eligible for §7 connection failover (`doc-88` §7).
/// Only `rate_limited`, `quota_exhausted`, `auth_expired`, and `model_unavailable` qualify.
pub fn classify_failover_error(errors: &[String], rate_limited: bool) -> Option<&'static str> {
    if rate_limited {
        return Some("rate_limited");
    }
    for raw in errors {
        let lower = raw.to_ascii_lowercase();
        for (needles, label) in FAILOVER_PATTERNS {
            if needles.iter().any(|needle| lower.contains(needle)) {
                return Some(label);
            }
        }
    }
    None
}

struct CandidateSkip {
    reason: String,
    cooldown_until: Option<String>,
    auth_error: Option<String>,
    is_auth_expired: bool,
}

impl CandidateSkip {
    fn plain(reason: String) -> CandidateOutcome {
        CandidateOutcome::Skipped(Self {
            reason,
            cooldown_until: None,
            auth_error: None,
            is_auth_expired: false,
        })
    }
}

enum CandidateOutcome {
    Admitted(AdmittedRoute),
    Skipped(CandidateSkip),
}

/// Parameters for requesting a shared `HqPool` turn permit from HQ (`TASK-52` M-21).
#[derive(Clone, Debug)]
pub struct HqPermitAcquireSpec {
    pub connection_id: String,
    pub employee_id: String,
    pub task_id: Option<String>,
    pub model_id: String,
    pub is_failover_switch: bool,
}

/// Consumer-owned adapter connecting a satellite's session router to HQ's shared `HqPool`
/// permit coordinator (`TASK-52` M-21, `doc-8` adapter rule).
pub trait HqPermitCoordinator: Send + Sync {
    fn acquire_permit(&self, req: HqPermitAcquireSpec) -> Result<PermitAcquireOutcome, String>;
    fn renew_permit(&self, permit_id: &str, coordinator_generation: u64) -> Result<bool, String>;
    fn release_permit(&self, permit_id: &str) -> Result<(), String>;
    fn report_cooldown(
        &self,
        connection_id: &str,
        resets_at: Option<&str>,
        reason: Option<&str>,
    ) -> Result<(), String>;
}

/// Renews a turn permit on the local office or, on a satellite holding an HQ-issued `HqPool`
/// permit, routes heartbeat renewal to HQ's coordinator (`TASK-52` #12).
pub(crate) fn renew_turn_permit_any(
    office: &Office,
    hq_permits: Option<&dyn HqPermitCoordinator>,
    permit_id: &str,
    generation: u64,
) -> bool {
    if office
        .renew_turn_permit(permit_id, generation)
        .is_ok_and(|r| r.is_ok())
    {
        return true;
    }
    if office.info().role == org_office::OfficeRole::Satellite {
        if let Some(coord) = hq_permits {
            return coord.renew_permit(permit_id, generation).unwrap_or(false);
        }
    }
    false
}

/// Releases a turn permit on the local office and, when on a satellite with an HQ coordinator,
/// forwards the release to HQ so shared `HqPool` slots are freed promptly (`TASK-52` M-21).
pub(crate) fn release_turn_permit_any(
    office: &Office,
    hq_permits: Option<&dyn HqPermitCoordinator>,
    permit_id: &str,
) {
    let _ = office.release_turn_permit(permit_id);
    if office.info().role == org_office::OfficeRole::Satellite {
        if let Some(coord) = hq_permits {
            let _ = coord.release_permit(permit_id);
        }
    }
}

/// Reports a provider cooldown locally and, when on a satellite with an HQ coordinator, forwards
/// it to HQ so all offices share the cooldown state (`TASK-52` M-21).
pub(crate) fn report_cooldown_any(
    office: &Office,
    hq_permits: Option<&dyn HqPermitCoordinator>,
    connection_id: &str,
    resets_at: Option<&str>,
    reason: Option<&str>,
) {
    if office.info().role == org_office::OfficeRole::Satellite {
        if let Some(coord) = hq_permits {
            let _ = coord.report_cooldown(connection_id, resets_at, reason);
        }
    }
}

struct RouteWalkCtx<'a> {
    office: &'a Office,
    vault: &'a CredentialVault,
    runtimes: &'a RuntimeRegistry,
    hq_permits: Option<&'a dyn HqPermitCoordinator>,
    oauth_refresh: &'a crate::host_auth::VaultRefreshCoordinator,
    employee_id: &'a str,
    task_id: Option<&'a str>,
    model_id: &'a str,
    profile_key: &'a str,
    preset: PermissionPreset,
    effort: Option<ReasoningEffort>,
    exec_rev: u64,
    is_failover_switch: bool,
    now_ms: u64,
    excluded: &'a HashSet<String>,
}

fn permit_authority_for(office: &Office, conn: &ProviderConnection) -> PermitAuthority {
    let multi_office = office.is_multi_office().unwrap_or(false);
    // Only shared org-wide subscription pools (explicit `subscription_identity`) require HQ
    // pool coordination across offices; independent per-office bindings stay `OfficeLocal` so
    // satellites keep working offline without over-granting shared capacity (`TASK-52`).
    if multi_office
        && conn.auth_mode == ProviderAuthMode::SubscriptionCli
        && conn.subscription_identity.is_some()
    {
        PermitAuthority::HqPool
    } else {
        PermitAuthority::OfficeLocal
    }
}

struct CandidatePermitGuard<'a> {
    office: &'a Office,
    hq_permits: Option<&'a dyn HqPermitCoordinator>,
    permit: Option<(String, u64)>,
}

impl Drop for CandidatePermitGuard<'_> {
    fn drop(&mut self) {
        if let Some((pid, _)) = self.permit.take() {
            release_turn_permit_any(self.office, self.hq_permits, &pid);
        }
    }
}

fn try_admit_candidate(ctx: &RouteWalkCtx<'_>, cid: &str) -> Result<CandidateOutcome, ApiError> {
    if ctx.excluded.contains(cid) {
        return Ok(CandidateSkip::plain(format!("{cid}:ALREADY_TRIED")));
    }
    let Some(conn) = ctx.office.load_provider_connection(cid).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not load provider connection",
        )
    })?
    else {
        return Ok(CandidateSkip::plain(format!("{cid}:CONNECTION_NOT_FOUND")));
    };
    if conn.archived || !conn.enabled {
        return Ok(CandidateSkip::plain(format!("{cid}:CONNECTION_DISABLED")));
    }
    let (binding_rev, resolved_model, model_supports_vision) =
        match check_local_readiness(ctx.office, &conn, ctx.model_id, ctx.now_ms)? {
            Ok(v) => v,
            Err(reason) => {
                return Ok(CandidateOutcome::Skipped(CandidateSkip {
                    reason: format!("{cid}:{reason}"),
                    cooldown_until: None,
                    auth_error: None,
                    is_auth_expired: reason == "AUTH_EXPIRED",
                }));
            }
        };
    let permit = match acquire_candidate_permit(ctx, &conn, cid)? {
        Ok(p) => p,
        Err(skip) => return Ok(skip),
    };
    let mut permit_guard = CandidatePermitGuard {
        office: ctx.office,
        hq_permits: ctx.hq_permits,
        permit: Some(permit.clone()),
    };
    match finalize_admitted_connection(
        ctx,
        &conn,
        binding_rev,
        &resolved_model,
        model_supports_vision,
        permit,
    ) {
        Ok(route) => {
            permit_guard.permit = None;
            Ok(CandidateOutcome::Admitted(route))
        }
        Err(crate::host_auth::ConnectionAuthError::VaultAuth(auth_err)) => {
            drop(permit_guard);
            let _ = ctx
                .office
                .mark_connection_auth_expired_internal(cid, auth_err.issue_code());
            Ok(CandidateOutcome::Skipped(CandidateSkip {
                reason: format!("{cid}:AUTH_EXPIRED({})", auth_err.issue_code()),
                cooldown_until: None,
                auth_error: Some(auth_err.actionable_message(&conn.key)),
                is_auth_expired: true,
            }))
        }
        Err(crate::host_auth::ConnectionAuthError::Fatal(err)) => Err(err),
    }
}

fn acquire_candidate_permit(
    ctx: &RouteWalkCtx<'_>,
    conn: &ProviderConnection,
    cid: &str,
) -> Result<Result<(String, u64), CandidateOutcome>, ApiError> {
    let authority = permit_authority_for(ctx.office, conn);
    let permit_outcome = if ctx.office.info().role == org_office::OfficeRole::Satellite
        && authority == PermitAuthority::HqPool
    {
        let Some(coord) = ctx.hq_permits else {
            return Ok(Err(CandidateSkip::plain(format!(
                "{cid}:HQ_POOL_UNAVAILABLE"
            ))));
        };
        let spec = HqPermitAcquireSpec {
            connection_id: cid.to_owned(),
            employee_id: ctx.employee_id.to_owned(),
            task_id: ctx.task_id.map(str::to_owned),
            model_id: ctx.model_id.to_owned(),
            is_failover_switch: ctx.is_failover_switch,
        };
        match coord.acquire_permit(spec) {
            Ok(outcome) => outcome,
            Err(_) => {
                return Ok(Err(CandidateSkip::plain(format!(
                    "{cid}:HQ_POOL_UNAVAILABLE"
                ))))
            }
        }
    } else {
        ctx.office
            .acquire_turn_permit(
                cid,
                ctx.employee_id,
                ctx.task_id,
                ctx.model_id,
                authority,
                ctx.is_failover_switch,
            )
            .map_err(|_| {
                ApiError::new(ErrorCode::StorageError, "could not acquire turn permit")
            })??
    };
    match permit_outcome {
        PermitAcquireOutcome::Granted(p) => Ok(Ok((p.id, p.coordinator_generation))),
        PermitAcquireOutcome::CoolingDown { cooldown_until } => {
            Ok(Err(CandidateOutcome::Skipped(CandidateSkip {
                reason: format!("{cid}:COOLDOWN({cooldown_until})"),
                cooldown_until: Some(cooldown_until),
                auth_error: None,
                is_auth_expired: false,
            })))
        }
        PermitAcquireOutcome::CapacityFull { occupied, max } => Ok(Err(CandidateSkip::plain(
            format!("{cid}:CAPACITY_FULL({occupied}/{max})"),
        ))),
    }
}

/// Input parameters for [`select_and_admit_route`].
pub(crate) struct AdmitRouteRequest<'a> {
    pub office: &'a Office,
    pub vault: &'a CredentialVault,
    pub runtimes: &'a RuntimeRegistry,
    pub hq_permits: Option<&'a dyn HqPermitCoordinator>,
    pub oauth_refresh: Option<&'a crate::host_auth::VaultRefreshCoordinator>,
    pub employee_id: &'a str,
    pub task_id: Option<&'a str>,
    pub is_failover_switch: bool,
    pub excluded_connections: &'a HashSet<String>,
}

/// Deterministic ordered pool selection & permit admission (`doc-88` §6 steps 1-3).
pub(crate) fn select_and_admit_route(
    req: AdmitRouteRequest<'_>,
) -> Result<AdmittedRoute, ApiError> {
    let AdmitRouteRequest {
        office,
        vault,
        runtimes,
        hq_permits,
        oauth_refresh,
        employee_id,
        task_id,
        is_failover_switch,
        excluded_connections,
    } = req;
    let exec = office
        .load_agent_execution(employee_id)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not load execution config"))?;
    let Some(cfg) = exec else {
        let adapter = runtimes
            .default_adapter()
            .ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "agent runtime is unavailable"))?;
        return Ok(AdmittedRoute {
            adapter,
            model_token: None,
            model_supports_vision: true,
            effort: None,
            auth_mode: None,
            auth_env: Vec::new(),
            endpoint_env: Vec::new(),
            oauth_expires_at_ms: None,
            oauth_token_fp: None,
            preset: PermissionPreset::SafeAsk,
            sandbox_enabled: true,
            connection_id: None,
            permit: None,
            route_digest_suffix: String::new(),
        });
    };
    if !cfg.enabled {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "EXECUTION_DISABLED: agent execution configuration is disabled",
        ));
    }
    let Some(profile) = office
        .load_model_profile(cfg.model_profile_id.as_str())
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not load model profile"))?
    else {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "PROFILE_NOT_FOUND: assigned model profile does not exist",
        ));
    };
    if profile.archived {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "PROFILE_ARCHIVED: assigned model profile is archived",
        ));
    }

    let ordered_ids: Vec<&str> = if profile.connection_ids.is_empty() {
        profile
            .generated_for_connection_id
            .iter()
            .map(|id| id.as_str())
            .collect()
    } else {
        profile
            .connection_ids
            .iter()
            .map(|id| id.as_str())
            .collect()
    };
    let fallback_refresh = crate::host_auth::VaultRefreshCoordinator::noop();
    let walk = RouteWalkCtx {
        office,
        vault,
        runtimes,
        hq_permits,
        oauth_refresh: oauth_refresh.unwrap_or(&fallback_refresh),
        employee_id,
        task_id,
        model_id: &cfg.model_id,
        profile_key: &profile.key,
        preset: cfg.permission_preset,
        effort: cfg.effort,
        exec_rev: cfg.meta.revision,
        is_failover_switch,
        now_ms: office.current_time_ms(),
        excluded: excluded_connections,
    };

    let mut skips = Vec::new();
    let mut earliest_cd: Option<String> = None;
    let mut first_auth_error: Option<String> = None;
    let mut all_auth_expired = true;
    for cid in ordered_ids {
        match try_admit_candidate(&walk, cid)? {
            CandidateOutcome::Admitted(route) => return Ok(route),
            CandidateOutcome::Skipped(skip) => {
                if let Some(cd) = skip.cooldown_until {
                    if earliest_cd.as_deref().is_none_or(|e| cd.as_str() < e) {
                        earliest_cd = Some(cd);
                    }
                }
                if first_auth_error.is_none() {
                    first_auth_error = skip.auth_error;
                }
                all_auth_expired &= skip.is_auth_expired;
                skips.push(skip.reason);
            }
        }
    }

    Err(exhausted_pool_error(
        skips,
        earliest_cd,
        first_auth_error,
        all_auth_expired,
    ))
}

fn exhausted_pool_error(
    skips: Vec<String>,
    earliest_cd: Option<String>,
    first_auth_error: Option<String>,
    all_auth_expired: bool,
) -> ApiError {
    if !skips.is_empty() && all_auth_expired {
        let detail = first_auth_error.unwrap_or_else(|| {
            "all provider connections require vault credential enrollment or token rotation via `agentic-org` provider connection enrollment".to_owned()
        });
        return ApiError::new(
            ErrorCode::NotAuthorized,
            format!("AUTH_EXPIRED: {detail} (pool: [{}])", skips.join(", ")),
        );
    }
    let cd_note = earliest_cd
        .map(|u| format!("; earliest_cooldown={u}"))
        .unwrap_or_default();
    ApiError::new(
        ErrorCode::Conflict,
        format!("POOL_EXHAUSTED: [{}]{cd_note}", skips.join(", ")),
    )
}

fn check_local_readiness(
    office: &Office,
    conn: &ProviderConnection,
    target_model: &str,
    now_ms: u64,
) -> Result<Result<(u64, String, bool), &'static str>, ApiError> {
    let cid = conn.id.as_str();
    let binding = office
        .load_connection_binding(cid)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not load binding"))?;
    let Some(binding) = binding else {
        return Ok(Err("NOT_BOUND"));
    };
    let state = office
        .load_connection_state(cid)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not load connection state"))?;
    let Some(state) = state else {
        return Ok(Err("STATE_UNKNOWN"));
    };
    if state.status != ConnectionReadinessStatus::Ready || state.is_stale(binding.revision, now_ms)
    {
        return Ok(Err(match state.status {
            ConnectionReadinessStatus::AuthExpired => "AUTH_EXPIRED",
            ConnectionReadinessStatus::Unavailable => "UNAVAILABLE",
            _ => "STATE_STALE",
        }));
    }
    let models = office
        .load_connection_models(cid)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not load connection models"))?;
    let matched = models.into_iter().find(|m| {
        m.model_id == target_model
            && m.entitlement == DiscoveredModelEntitlement::Confirmed
            && !m.is_stale(now_ms)
    });
    match matched {
        Some(m) => {
            let vision = crate::turn_attachments::model_supports_vision(&m.capabilities_json);
            Ok(Ok((binding.revision, m.driver_model_id, vision)))
        }
        None => Ok(Err("MODEL_UNVERIFIED")),
    }
}

fn finalize_admitted_connection(
    ctx: &RouteWalkCtx<'_>,
    conn: &ProviderConnection,
    binding_rev: u64,
    resolved_model: &str,
    model_supports_vision: bool,
    permit: (String, u64),
) -> Result<AdmittedRoute, crate::host_auth::ConnectionAuthError> {
    use crate::host_auth::ConnectionAuthError;
    let adapter = ctx.runtimes.resolve(&conn.runtime_driver).map_err(|_| {
        ConnectionAuthError::Fatal(ApiError::new(
            ErrorCode::Unavailable,
            "agent runtime is unavailable",
        ))
    })?;
    let auth = crate::host_auth::resolve_connection_vault_auth(
        ctx.vault,
        ctx.oauth_refresh,
        adapter.as_ref(),
        conn.id.as_str(),
        &conn.provider_type,
        conn.auth_mode,
        ctx.now_ms,
    )?;
    let mut endpoint_env = Vec::new();
    if let Some(ref url) = conn.endpoint_url {
        let (var, val) =
            endpoint_env_for_adapter(adapter.as_ref(), url).map_err(ConnectionAuthError::Fatal)?;
        endpoint_env.push((OsString::from(var), OsString::from(val)));
    }
    let model_token = Token::try_from(resolved_model)
        .ok()
        .or_else(|| Token::try_from(ctx.profile_key).ok());
    let effort_token = ctx
        .effort
        .map(|e| Token::try_from(e.as_str()))
        .transpose()
        .map_err(|_| {
            ConnectionAuthError::Fatal(ApiError::new(
                ErrorCode::InvalidRequest,
                "invalid reasoning effort",
            ))
        })?;
    let route_digest_suffix = compute_route_digest(
        ctx.office,
        ctx.employee_id,
        conn,
        binding_rev,
        resolved_model,
        ctx.effort,
        ctx.exec_rev,
    );
    Ok(AdmittedRoute {
        adapter,
        model_token,
        model_supports_vision,
        effort: effort_token,
        auth_mode: Some(conn.auth_mode),
        auth_env: auth.auth_env,
        endpoint_env,
        oauth_expires_at_ms: auth.oauth_expires_at_ms,
        oauth_token_fp: auth.oauth_token_fp,
        preset: ctx.preset,
        sandbox_enabled: true,
        connection_id: Some(conn.id.as_str().to_owned()),
        permit: Some(permit),
        route_digest_suffix,
    })
}

fn hex_short_sha256(bytes: &[u8]) -> String {
    crate::host_auth::hex_short_sha256(bytes)
}
