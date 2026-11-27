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

pub(crate) use warm_permit::{acquire_warm_permit_for_office, WarmPermitOutcome};
#[cfg(test)]
pub(crate) use warm_permit::{compute_route_digest_with_wt, compute_worktree_sandbox_digest};

/// Fully admitted connection route ready to spawn a supervised agent session.
pub(crate) struct AdmittedRoute {
    pub adapter: Arc<dyn AgentRuntime>,
    pub model_token: Option<Token>,
    /// Whether the discovered model supports vision/image inputs (`TASK-98.3`).
    pub model_supports_vision: bool,
    /// The agent's reasoning effort (`TASK-181`) as the session spec's token, in the office's
    /// names; each runtime adapter maps it onto its own. `None` keeps the runtime's default.
    pub effort: Option<Token>,
    pub auth_mode: Option<ProviderAuthMode>,
    pub auth_env: Vec<(OsString, OsString)>,
    pub endpoint_env: Vec<(OsString, OsString)>,
    pub preset: PermissionPreset,
    pub sandbox_enabled: bool,
    pub connection_id: Option<String>,
    pub permit: Option<(String, u64)>,
    pub route_digest_suffix: String,
}

/// Maps a validated per-connection `endpoint_url` into the adapter's non-secret base-URL env var.
pub fn endpoint_env_for(
    driver: &str,
    endpoint_url: &str,
) -> Result<(&'static str, String), ApiError> {
    if !validate_endpoint_url(endpoint_url) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "invalid provider connection endpoint_url",
        ));
    }
    let lower = driver.to_ascii_lowercase();
    let var = if lower.contains("google")
        || lower.contains("agy")
        || lower.contains("antigravity")
        || lower.contains("gemini")
    {
        "GOOGLE_GEMINI_BASE_URL"
    } else if lower.contains("openai") || lower.contains("codex") || lower.contains("gpt") {
        "OPENAI_BASE_URL"
    } else {
        "ANTHROPIC_BASE_URL"
    };
    Ok((var, endpoint_url.to_owned()))
}

/// Maps a provider driver and auth mode to the runtime's allowlisted auth env variable name.
pub fn auth_env_var_for(driver: &str, mode: ProviderAuthMode, secret: &str) -> &'static str {
    let lower = driver.to_ascii_lowercase();
    if lower.contains("google")
        || lower.contains("agy")
        || lower.contains("antigravity")
        || lower.contains("gemini")
    {
        return if mode == ProviderAuthMode::SubscriptionCli {
            "AGY_AUTH_TOKEN"
        } else {
            "GEMINI_API_KEY"
        };
    }
    if lower.contains("openai") || lower.contains("codex") || lower.contains("gpt") {
        return if mode == ProviderAuthMode::SubscriptionCli {
            "CODEX_ACCESS_TOKEN"
        } else {
            "OPENAI_API_KEY"
        };
    }
    if mode == ProviderAuthMode::SubscriptionCli || secret.starts_with("sk-ant-oat") {
        "CLAUDE_CODE_OAUTH_TOKEN"
    } else {
        "ANTHROPIC_API_KEY"
    }
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

enum CandidateOutcome {
    Admitted(AdmittedRoute),
    Skipped {
        reason: String,
        cooldown_until: Option<String>,
    },
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

fn try_admit_candidate(ctx: &RouteWalkCtx<'_>, cid: &str) -> Result<CandidateOutcome, ApiError> {
    if ctx.excluded.contains(cid) {
        return Ok(CandidateOutcome::Skipped {
            reason: format!("{cid}:ALREADY_TRIED"),
            cooldown_until: None,
        });
    }
    let Some(conn) = ctx.office.load_provider_connection(cid).map_err(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "could not load provider connection",
        )
    })?
    else {
        return Ok(CandidateOutcome::Skipped {
            reason: format!("{cid}:CONNECTION_NOT_FOUND"),
            cooldown_until: None,
        });
    };
    if conn.archived || !conn.enabled {
        return Ok(CandidateOutcome::Skipped {
            reason: format!("{cid}:CONNECTION_DISABLED"),
            cooldown_until: None,
        });
    }
    let (binding_rev, resolved_model, model_supports_vision) =
        match check_local_readiness(ctx.office, &conn, ctx.model_id, ctx.now_ms)? {
            Ok(v) => v,
            Err(reason) => {
                return Ok(CandidateOutcome::Skipped {
                    reason: format!("{cid}:{reason}"),
                    cooldown_until: None,
                });
            }
        };
    let authority = permit_authority_for(ctx.office, &conn);
    // On a satellite, shared `HqPool` capacity MUST be granted by HQ's coordinator; if HQ is
    // unavailable or unconfigured, fail closed on shared bindings while `OfficeLocal` proceeds.
    let permit_outcome = if ctx.office.info().role == org_office::OfficeRole::Satellite
        && authority == PermitAuthority::HqPool
    {
        let Some(coord) = ctx.hq_permits else {
            return Ok(CandidateOutcome::Skipped {
                reason: format!("{cid}:HQ_POOL_UNAVAILABLE"),
                cooldown_until: None,
            });
        };
        match coord.acquire_permit(HqPermitAcquireSpec {
            connection_id: cid.to_owned(),
            employee_id: ctx.employee_id.to_owned(),
            task_id: ctx.task_id.map(str::to_owned),
            model_id: ctx.model_id.to_owned(),
            is_failover_switch: ctx.is_failover_switch,
        }) {
            Ok(outcome) => outcome,
            Err(_) => {
                return Ok(CandidateOutcome::Skipped {
                    reason: format!("{cid}:HQ_POOL_UNAVAILABLE"),
                    cooldown_until: None,
                });
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
    let permit = match permit_outcome {
        PermitAcquireOutcome::Granted(p) => p,
        PermitAcquireOutcome::CoolingDown { cooldown_until } => {
            return Ok(CandidateOutcome::Skipped {
                reason: format!("{cid}:COOLDOWN({cooldown_until})"),
                cooldown_until: Some(cooldown_until),
            });
        }
        PermitAcquireOutcome::CapacityFull { occupied, max } => {
            return Ok(CandidateOutcome::Skipped {
                reason: format!("{cid}:CAPACITY_FULL({occupied}/{max})"),
                cooldown_until: None,
            });
        }
    };
    match finalize_admitted_connection(
        ctx.office,
        ctx.vault,
        ctx.runtimes,
        &conn,
        AdmitParams {
            employee_id: ctx.employee_id,
            preset: ctx.preset,
            resolved_model: &resolved_model,
            model_supports_vision,
            profile_key: ctx.profile_key,
            effort: ctx.effort,
            binding_rev,
            exec_rev: ctx.exec_rev,
            permit: (permit.id.clone(), permit.coordinator_generation),
        },
    ) {
        Ok(route) => Ok(CandidateOutcome::Admitted(route)),
        Err(err) => {
            release_turn_permit_any(ctx.office, ctx.hq_permits, &permit.id);
            Err(err)
        }
    }
}

/// Input parameters for [`select_and_admit_route`].
pub(crate) struct AdmitRouteRequest<'a> {
    pub office: &'a Office,
    pub vault: &'a CredentialVault,
    pub runtimes: &'a RuntimeRegistry,
    pub hq_permits: Option<&'a dyn HqPermitCoordinator>,
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
            .resolve("claude")
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "agent runtime is unavailable"))?;
        return Ok(AdmittedRoute {
            adapter,
            model_token: None,
            model_supports_vision: true,
            effort: None,
            auth_mode: None,
            auth_env: Vec::new(),
            endpoint_env: Vec::new(),
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
    let walk = RouteWalkCtx {
        office,
        vault,
        runtimes,
        hq_permits,
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
    for cid in ordered_ids {
        match try_admit_candidate(&walk, cid)? {
            CandidateOutcome::Admitted(route) => return Ok(route),
            CandidateOutcome::Skipped {
                reason,
                cooldown_until,
            } => {
                if let Some(cd) = cooldown_until {
                    if earliest_cd.as_deref().is_none_or(|e| cd.as_str() < e) {
                        earliest_cd = Some(cd);
                    }
                }
                skips.push(reason);
            }
        }
    }

    let cd_note = earliest_cd
        .map(|u| format!("; earliest_cooldown={u}"))
        .unwrap_or_default();
    Err(ApiError::new(
        ErrorCode::Conflict,
        format!("POOL_EXHAUSTED: [{}]{cd_note}", skips.join(", ")),
    ))
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

struct AdmitParams<'a> {
    employee_id: &'a str,
    preset: PermissionPreset,
    resolved_model: &'a str,
    model_supports_vision: bool,
    profile_key: &'a str,
    effort: Option<ReasoningEffort>,
    binding_rev: u64,
    exec_rev: u64,
    permit: (String, u64),
}

fn finalize_admitted_connection(
    office: &Office,
    vault: &CredentialVault,
    runtimes: &RuntimeRegistry,
    conn: &ProviderConnection,
    params: AdmitParams<'_>,
) -> Result<AdmittedRoute, ApiError> {
    let AdmitParams {
        employee_id,
        preset,
        resolved_model,
        model_supports_vision,
        profile_key,
        effort,
        binding_rev,
        exec_rev,
        permit,
    } = params;
    let adapter = runtimes
        .resolve(&conn.runtime_driver)
        .map_err(|_| ApiError::new(ErrorCode::Unavailable, "agent runtime is unavailable"))?;
    let mut auth_env = Vec::new();
    if let Some(secret) = vault.load_unwrapped(conn.id.as_str()).map_err(|_| {
        ApiError::new(
            ErrorCode::Unavailable,
            "provider credential could not be unwrapped",
        )
    })? {
        let secret_str = std::str::from_utf8(&secret).map_err(|_| {
            ApiError::new(ErrorCode::Unavailable, "provider credential is not UTF-8")
        })?;
        let var_name = auth_env_var_for(&conn.runtime_driver, conn.auth_mode, secret_str);
        auth_env.push((OsString::from(var_name), OsString::from(secret_str)));
    }
    let mut endpoint_env = Vec::new();
    if let Some(ref url) = conn.endpoint_url {
        let (var, val) = endpoint_env_for(&conn.runtime_driver, url)?;
        endpoint_env.push((OsString::from(var), OsString::from(val)));
    }
    let model_token = Token::try_from(resolved_model)
        .ok()
        .or_else(|| Token::try_from(profile_key).ok());
    // Every level is a valid token; were one ever not, refuse rather than drop the effort.
    let effort_token = effort
        .map(|e| Token::try_from(e.as_str()))
        .transpose()
        .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid reasoning effort"))?;
    let route_digest_suffix = compute_route_digest(
        office,
        employee_id,
        conn,
        binding_rev,
        resolved_model,
        effort,
        exec_rev,
    );
    Ok(AdmittedRoute {
        adapter,
        model_token,
        model_supports_vision,
        effort: effort_token,
        auth_mode: Some(conn.auth_mode),
        auth_env,
        endpoint_env,
        preset,
        sandbox_enabled: true,
        connection_id: Some(conn.id.as_str().to_owned()),
        permit: Some(permit),
        route_digest_suffix,
    })
}

fn hex_short_sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    let mut out = String::with_capacity(16);
    for &b in &digest.as_ref()[..8] {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}
