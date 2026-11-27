//! Office and credential-vault orchestration of subscription usage listing, credentials, and
//! bounded concurrent collection (called by `SessionManager`).

use std::time::{Duration, Instant};

use org_office::accounting::{
    build_snapshot_view, StoredSubscriptionUsageSnapshot, SubscriptionCredentialPresence,
    SUBSCRIPTION_USAGE_FRESHNESS_MS,
};
use org_office::organization_family::records::{ProviderAuthMode, ProviderConnection};
use org_office::{format_rfc3339_ms, Office};
use org_proto::costs::{
    CostSubscriptionUsageCollectParams, CostSubscriptionUsageCollectResult,
    CostSubscriptionUsageCredentialRevokeParams, CostSubscriptionUsageCredentialRevokeResult,
    CostSubscriptionUsageCredentialSetParams, CostSubscriptionUsageCredentialSetResult,
    CostSubscriptionUsageListParams, CostSubscriptionUsageListResult, SubscriptionUsageAuthSource,
    SubscriptionUsageSnapshotView, SubscriptionUsageStatus,
};
use org_proto::{ApiError, ErrorCode};

use super::endpoint::{
    is_valid_bearer_token, resolve_expected_origin, transport_error_msg,
    validate_usage_endpoint_url,
};
use super::oauth::parse_oauth_envelope;
use super::transport::{attempt_schedule_map, record_scheduling_attempt};
use super::{
    SubscriptionUsageCollectOutcome, SubscriptionUsageCollectRequest, SubscriptionUsageCollector,
    UnwrappedUsageCredential, UsageCredentialError, UsageCredentialLookup, MAX_ACCOUNT_HINT_BYTES,
    MAX_COLLECT_RESPONSE_BYTES, MAX_CONCURRENT_COLLECTORS, TOTAL_COLLECTION_BUDGET,
};

// ---------------------------------------------------------------------------
// Office + UsageCredentialLookup orchestration (called by SessionManager)
// ---------------------------------------------------------------------------

pub(crate) fn credential_presence_for(
    vault: &dyn UsageCredentialLookup,
    connection_id: &str,
) -> SubscriptionCredentialPresence {
    let usage = vault.usage_status(connection_id).ok();
    let enrolled = vault.enrolled_status(connection_id).ok();
    let has_usage_credential = usage.as_ref().is_some_and(|u| u.configured);
    let has_enrolled_credential = enrolled.as_ref().is_some_and(|e| e.enrolled);
    let account_hint = usage
        .and_then(|u| u.account_hint)
        .or_else(|| enrolled.and_then(|e| e.account_hint));
    SubscriptionCredentialPresence {
        has_usage_credential,
        has_enrolled_credential,
        account_hint,
    }
}

pub fn list_subscription_usage(
    office: &Office,
    vault: &dyn UsageCredentialLookup,
    actor: &str,
    params: CostSubscriptionUsageListParams,
) -> Result<CostSubscriptionUsageListResult, ApiError> {
    office
        .subscription_usage_list_with_credentials(actor, params, |cid| {
            credential_presence_for(vault, cid)
        })
        .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))?
}

pub fn set_subscription_usage_credential(
    office: &Office,
    vault: &dyn UsageCredentialLookup,
    actor: &str,
    params: CostSubscriptionUsageCredentialSetParams,
) -> Result<CostSubscriptionUsageCredentialSetResult, ApiError> {
    require_owner(office, actor)?;
    let conn = find_subscription_connection(office, &params.connection_id, false)?;
    let bound_origin = resolve_expected_origin(&conn)?;
    verify_expected_origin(&conn, &bound_origin, params.expected_origin.as_deref())?;
    let trimmed_secret = params.secret.trim();
    if !is_valid_bearer_token(trimmed_secret) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "usage credential secret must be 1..=4096 printable ASCII bytes without quotes or backslashes",
        ));
    }
    let expires_at = match params.expires_at {
        Some(ref raw) if !raw.trim().is_empty() => {
            let canon = org_proto::canonicalize_rfc3339_utc(raw).ok_or_else(|| {
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "expires_at must be a valid RFC 3339 timestamp",
                )
            })?;
            Some(canon)
        }
        _ => None,
    };
    let account_hint = match params.account_hint {
        Some(ref h) if !h.trim().is_empty() => {
            let clean = h.trim();
            if clean.len() > MAX_ACCOUNT_HINT_BYTES || clean.contains('\n') || clean.contains('\r')
            {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "account_hint must be a single line of at most 256 bytes",
                ));
            }
            Some(clean.to_owned())
        }
        _ => None,
    };
    let credential = vault
        .store_wrapped_usage(
            conn.id.as_str(),
            &conn.provider_type,
            &bound_origin,
            trimmed_secret.as_bytes(),
            expires_at,
            account_hint,
        )
        .map_err(map_vault_error)?;
    Ok(CostSubscriptionUsageCredentialSetResult { credential })
}

pub fn revoke_subscription_usage_credential(
    office: &Office,
    vault: &dyn UsageCredentialLookup,
    actor: &str,
    params: CostSubscriptionUsageCredentialRevokeParams,
) -> Result<CostSubscriptionUsageCredentialRevokeResult, ApiError> {
    require_owner(office, actor)?;
    let conn = find_subscription_connection(office, &params.connection_id, true)?;
    let credential = vault
        .revoke_usage(conn.id.as_str())
        .map_err(map_vault_error)?;
    Ok(CostSubscriptionUsageCredentialRevokeResult { credential })
}

#[derive(Clone, Copy)]
pub(super) struct CollectCtx<'a> {
    pub(super) office: &'a Office,
    pub(super) vault: &'a dyn UsageCredentialLookup,
    pub(super) collector: &'a dyn SubscriptionUsageCollector,
    pub(super) time_zone: &'a str,
    pub(super) collected_at: &'a str,
    pub(super) now_ms: u64,
    pub(super) deadline: Instant,
}

pub fn collect_subscription_usage(
    office: &Office,
    vault: &dyn UsageCredentialLookup,
    collector: &dyn SubscriptionUsageCollector,
    actor: &str,
    params: CostSubscriptionUsageCollectParams,
) -> Result<CostSubscriptionUsageCollectResult, ApiError> {
    require_owner(office, actor)?;
    let time_zone = params.time_zone.unwrap_or_else(|| "UTC".to_owned());
    if !org_proto::tz::validate_iana_timezone(&time_zone) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "time_zone must be a valid IANA time zone",
        ));
    }
    let mut targets = resolve_collection_targets(office, params.connection_id.as_deref())?;
    prioritize_oldest_targets(office, &mut targets);
    let now_ms = office.current_time_ms();
    let collected_at = format_rfc3339_ms(now_ms);
    let ctx = CollectCtx {
        office,
        vault,
        collector,
        time_zone: &time_zone,
        collected_at: &collected_at,
        now_ms,
        deadline: Instant::now() + TOTAL_COLLECTION_BUDGET,
    };
    let (mut views, mut deferred_connection_ids) = collect_targets_bounded(ctx, &targets)?;
    views.sort_by(|a, b| a.connection_key.cmp(&b.connection_key));
    let (snapshots, overflow_ids) = bound_collect_views(views);
    deferred_connection_ids.extend(overflow_ids);
    deferred_connection_ids.sort();
    deferred_connection_ids.dedup();
    Ok(CostSubscriptionUsageCollectResult {
        snapshots,
        collected_at,
        deferred_connection_ids,
    })
}

pub(super) fn verify_expected_origin(
    conn: &ProviderConnection,
    bound_origin: &str,
    supplied: Option<&str>,
) -> Result<(), ApiError> {
    let has_custom_endpoint = conn
        .endpoint_url
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    match supplied.map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => {
            let normalized = if raw.starts_with("provider:") {
                raw.to_ascii_lowercase()
            } else {
                let ep = validate_usage_endpoint_url(raw).map_err(|err| {
                    ApiError::new(ErrorCode::InvalidRequest, transport_error_msg(err))
                })?;
                ep.origin()
            };
            if normalized != bound_origin {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "connection destination origin changed; refresh and confirm expected_origin before saving credential",
                ));
            }
            Ok(())
        }
        None if has_custom_endpoint => Err(ApiError::new(
            ErrorCode::Conflict,
            "expected_origin is required when configuring a credential for a custom endpoint URL",
        )),
        None => Ok(()),
    }
}

pub(super) fn prioritize_oldest_targets(office: &Office, targets: &mut [ProviderConnection]) {
    let ages = office
        .subscription_usage_collected_at_map()
        .unwrap_or_default();
    let sched = attempt_schedule_map()
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default();
    targets.sort_by(|a, b| {
        let a_sched = sched.get(a.id.as_str()).copied().unwrap_or(0);
        let b_sched = sched.get(b.id.as_str()).copied().unwrap_or(0);
        let a_ts = ages.get(a.id.as_str()).map(String::as_str);
        let b_ts = ages.get(b.id.as_str()).map(String::as_str);
        a_sched
            .cmp(&b_sched)
            .then_with(|| a_ts.cmp(&b_ts))
            .then_with(|| a.key.cmp(&b.key))
    });
}

pub(super) fn bound_collect_views(
    views: Vec<SubscriptionUsageSnapshotView>,
) -> (Vec<SubscriptionUsageSnapshotView>, Vec<String>) {
    let mut kept = Vec::new();
    let mut overflow = Vec::new();
    let mut used = 4096usize;
    for view in views {
        let sz = serde_json::to_vec(&view).map_or(0, |v| v.len()) + 16;
        if !kept.is_empty() && used.saturating_add(sz) > MAX_COLLECT_RESPONSE_BYTES {
            overflow.push(view.connection_id);
            continue;
        }
        used = used.saturating_add(sz);
        kept.push(view);
    }
    (kept, overflow)
}

pub(super) fn collect_targets_bounded(
    ctx: CollectCtx<'_>,
    targets: &[ProviderConnection],
) -> Result<(Vec<SubscriptionUsageSnapshotView>, Vec<String>), ApiError> {
    let mut out = Vec::with_capacity(targets.len());
    let mut deferred = Vec::new();
    for chunk in targets.chunks(MAX_CONCURRENT_COLLECTORS) {
        if Instant::now() >= ctx.deadline {
            deferred.extend(chunk.iter().map(|c| c.id.as_str().to_owned()));
            continue;
        }
        if chunk.len() == 1 {
            if let Some(view) = collect_one_connection(ctx, &chunk[0])? {
                out.push(view);
            } else {
                deferred.push(chunk[0].id.as_str().to_owned());
            }
            continue;
        }
        std::thread::scope(|scope| -> Result<(), ApiError> {
            let handles: Vec<_> = chunk
                .iter()
                .map(|conn| {
                    let cid = conn.id.as_str().to_owned();
                    (cid, scope.spawn(move || collect_one_connection(ctx, conn)))
                })
                .collect();
            for (cid, handle) in handles {
                let maybe_view = handle.join().map_err(|_| {
                    ApiError::new(
                        ErrorCode::StorageError,
                        "subscription usage worker thread panicked",
                    )
                })??;
                match maybe_view {
                    Some(view) => out.push(view),
                    None => deferred.push(cid),
                }
            }
            Ok(())
        })?;
    }
    Ok((out, deferred))
}

pub(super) fn collect_one_connection(
    ctx: CollectCtx<'_>,
    conn: &ProviderConnection,
) -> Result<Option<SubscriptionUsageSnapshotView>, ApiError> {
    if Instant::now() >= ctx.deadline {
        return Ok(None);
    }
    record_scheduling_attempt(conn.id.as_str());
    let attempt_seq = ctx.office.next_subscription_attempt_seq();
    let cred_presence = credential_presence_for(ctx.vault, conn.id.as_str());
    let (auth_source, outcome) = run_collection_attempt(ctx, conn);
    if matches!(outcome, SubscriptionUsageCollectOutcome::Deferred)
        || Instant::now() >= ctx.deadline
    {
        return Ok(None);
    }
    let stored =
        persist_outcome_with_fallback(ctx, conn.id.as_str(), attempt_seq, auth_source, outcome)?;
    Ok(Some(build_snapshot_view(
        conn,
        &ctx.office.info().office_id,
        Some(&stored),
        &cred_presence,
        ctx.office.current_time_ms(),
    )))
}

pub(super) fn run_collection_attempt(
    ctx: CollectCtx<'_>,
    conn: &ProviderConnection,
) -> (SubscriptionUsageAuthSource, SubscriptionUsageCollectOutcome) {
    let expected_origin = match resolve_expected_origin(conn) {
        Ok(origin) => origin,
        Err(err) => {
            return (
                SubscriptionUsageAuthSource::None,
                SubscriptionUsageCollectOutcome::Unavailable {
                    issue_code: "INVALID_ENDPOINT".into(),
                    message: err.message,
                },
            );
        }
    };
    let sample_instant = Instant::now();
    let attempt_now_ms = ctx.office.current_time_ms();
    match load_active_credential(
        ctx.vault,
        conn.id.as_str(),
        &expected_origin,
        attempt_now_ms,
    ) {
        Ok((source, unwrapped)) => {
            let credential_deadline = unwrapped
                .expires_at
                .as_deref()
                .and_then(org_proto::parse_rfc3339_ms)
                .map(|exp_ms| {
                    sample_instant + Duration::from_millis(exp_ms.saturating_sub(attempt_now_ms))
                });
            let req = SubscriptionUsageCollectRequest {
                connection_id: conn.id.as_str().to_owned(),
                provider_type: conn.provider_type.clone(),
                subscription_identity: conn.subscription_identity.clone(),
                endpoint_url: conn.endpoint_url.clone(),
                auth_source: source,
                credential: unwrapped.secret,
                credential_deadline,
                time_zone: ctx.time_zone.to_owned(),
                collected_at: ctx.collected_at.to_owned(),
                now_ms: attempt_now_ms,
                deadline: ctx.deadline,
            };
            (source, ctx.collector.collect(&req))
        }
        Err((source, early_outcome)) => (source, early_outcome),
    }
}

pub(super) fn persist_outcome_with_fallback(
    ctx: CollectCtx<'_>,
    connection_id: &str,
    attempt_seq: u64,
    auth_source: SubscriptionUsageAuthSource,
    outcome: SubscriptionUsageCollectOutcome,
) -> Result<StoredSubscriptionUsageSnapshot, ApiError> {
    let candidate =
        outcome_to_stored_snapshot(ctx, connection_id, attempt_seq, auth_source, outcome);
    let saved = match ctx
        .office
        .save_subscription_usage_snapshot(candidate.clone())
        .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))?
    {
        Ok(()) => candidate,
        Err(_) => {
            let fallback = outcome_to_stored_snapshot(
                ctx,
                connection_id,
                attempt_seq,
                auth_source,
                SubscriptionUsageCollectOutcome::Unavailable {
                    issue_code: "INVALID_PROVIDER_RESPONSE".into(),
                    message: "Provider usage response failed validation and was rejected.".into(),
                },
            );
            ctx.office
                .save_subscription_usage_snapshot(fallback.clone())
                .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))??;
            fallback
        }
    };
    Ok(ctx
        .office
        .get_subscription_usage_snapshot(connection_id)
        .ok()
        .flatten()
        .unwrap_or(saved))
}

pub(super) fn load_active_credential(
    vault: &dyn UsageCredentialLookup,
    connection_id: &str,
    expected_origin: &str,
    now_ms: u64,
) -> Result<
    (SubscriptionUsageAuthSource, UnwrappedUsageCredential),
    (SubscriptionUsageAuthSource, SubscriptionUsageCollectOutcome),
> {
    match vault.load_unwrapped_usage(connection_id, expected_origin) {
        Ok(Some(cred)) => {
            if is_expired(cred.expires_at.as_deref(), now_ms) {
                return Err((
                    SubscriptionUsageAuthSource::DedicatedUsageCredential,
                    SubscriptionUsageCollectOutcome::AuthRequired {
                        issue_code: "CREDENTIAL_EXPIRED".into(),
                        message: "Dedicated usage credential has expired; rotate or revoke it."
                            .into(),
                    },
                ));
            }
            return Ok((SubscriptionUsageAuthSource::DedicatedUsageCredential, cred));
        }
        Ok(None) => {}
        Err(UsageCredentialError::Conflict(_)) => {
            return Err((
                SubscriptionUsageAuthSource::DedicatedUsageCredential,
                SubscriptionUsageCollectOutcome::AuthRequired {
                    issue_code: "ENDPOINT_ORIGIN_CHANGED".into(),
                    message: "Connection endpoint origin changed since this usage credential was stored; re-configure the usage credential.".into(),
                },
            ));
        }
        Err(err) => {
            return Err((
                SubscriptionUsageAuthSource::DedicatedUsageCredential,
                vault_err_to_outcome(err),
            ));
        }
    }
    match vault.load_unwrapped_enrolled(connection_id) {
        Ok(Some(mut cred)) => {
            let expired_err = || {
                (
                    SubscriptionUsageAuthSource::EnrolledSubscriptionCredential,
                    SubscriptionUsageCollectOutcome::AuthRequired {
                        issue_code: "CREDENTIAL_EXPIRED".into(),
                        message: "Enrolled subscription credential has expired; re-authenticate."
                            .into(),
                    },
                )
            };
            let outer_exp_ms = match cred.expires_at.as_deref() {
                Some(raw) => match org_proto::parse_rfc3339_ms(raw) {
                    Some(exp) if now_ms < exp => Some(exp),
                    _ => return Err(expired_err()),
                },
                None => None,
            };
            let Ok(oauth) = parse_oauth_envelope(&cred.secret) else {
                return Err((
                    SubscriptionUsageAuthSource::EnrolledSubscriptionCredential,
                    SubscriptionUsageCollectOutcome::AuthRequired {
                        issue_code: "INVALID_CREDENTIAL_FORMAT".into(),
                        message: "Stored credential cannot be encoded as an HTTPS bearer token; re-authenticate.".into(),
                    },
                ));
            };
            if oauth.expires_at_ms.is_some_and(|exp| now_ms >= exp) {
                return Err(expired_err());
            }
            let effective_exp_ms = match (outer_exp_ms, oauth.expires_at_ms) {
                (Some(outer), Some(inner)) => Some(outer.min(inner)),
                (Some(outer), None) => Some(outer),
                (None, Some(inner)) => Some(inner),
                (None, None) => None,
            };
            cred.expires_at = effective_exp_ms.map(format_rfc3339_ms);
            cred.secret = zeroize::Zeroizing::new(oauth.access_token.as_bytes().to_vec());
            Ok((
                SubscriptionUsageAuthSource::EnrolledSubscriptionCredential,
                cred,
            ))
        }
        Ok(None) => Err((
            SubscriptionUsageAuthSource::None,
            SubscriptionUsageCollectOutcome::AuthRequired {
                issue_code: "AUTH_MISSING".into(),
                message: "No subscription or usage-API credential is configured on this office."
                    .into(),
            },
        )),
        Err(err) => Err((
            SubscriptionUsageAuthSource::EnrolledSubscriptionCredential,
            vault_err_to_outcome(err),
        )),
    }
}

pub(super) fn is_expired(expires_at: Option<&str>, now_ms: u64) -> bool {
    let Some(raw) = expires_at else {
        return false;
    };
    org_proto::parse_rfc3339_ms(raw).is_none_or(|exp| now_ms >= exp)
}

pub(super) fn vault_err_to_outcome(err: UsageCredentialError) -> SubscriptionUsageCollectOutcome {
    match err {
        UsageCredentialError::MissingSecretsKey => SubscriptionUsageCollectOutcome::AuthRequired {
            issue_code: "SECRETS_KEY_UNAVAILABLE".into(),
            message: "Office secrets key is unavailable; cannot unwrap provider credentials."
                .into(),
        },
        _ => SubscriptionUsageCollectOutcome::AuthRequired {
            issue_code: "CREDENTIAL_UNREADABLE".into(),
            message: "Stored provider credential could not be decrypted; re-authenticate.".into(),
        },
    }
}

pub(super) fn outcome_to_stored_snapshot(
    ctx: CollectCtx<'_>,
    connection_id: &str,
    attempt_seq: u64,
    auth_source: SubscriptionUsageAuthSource,
    outcome: SubscriptionUsageCollectOutcome,
) -> StoredSubscriptionUsageSnapshot {
    let info = ctx.office.info();
    let mut snap = StoredSubscriptionUsageSnapshot {
        organization_id: info.org_id.clone(),
        office_id: info.office_id.clone(),
        connection_id: connection_id.to_owned(),
        attempt_seq,
        status: SubscriptionUsageStatus::Unavailable,
        auth_source,
        collected_at: ctx.collected_at.to_owned(),
        last_ok_collected_at: None,
        fresh_until: None,
        issue_code: None,
        message: None,
        models: Vec::new(),
    };
    match outcome {
        SubscriptionUsageCollectOutcome::Collected { models, message } => {
            snap.status = SubscriptionUsageStatus::Ok;
            snap.last_ok_collected_at = Some(ctx.collected_at.to_owned());
            snap.fresh_until = Some(format_rfc3339_ms(
                ctx.now_ms.saturating_add(SUBSCRIPTION_USAGE_FRESHNESS_MS),
            ));
            snap.message = message;
            snap.models = models;
        }
        SubscriptionUsageCollectOutcome::AuthRequired {
            issue_code,
            message,
        } => {
            snap.status = SubscriptionUsageStatus::AuthRequired;
            snap.issue_code = Some(issue_code);
            snap.message = Some(message);
        }
        SubscriptionUsageCollectOutcome::Unavailable {
            issue_code,
            message,
        } => {
            snap.issue_code = Some(issue_code);
            snap.message = Some(message);
        }
        SubscriptionUsageCollectOutcome::Unsupported {
            issue_code,
            message,
        } => {
            snap.status = SubscriptionUsageStatus::Unsupported;
            snap.issue_code = Some(issue_code);
            snap.message = Some(message);
        }
        SubscriptionUsageCollectOutcome::Deferred => {
            snap.issue_code = Some("COLLECTION_DEADLINE_EXCEEDED".into());
            snap.message = Some("Usage collection deferred before completion.".into());
        }
    }
    snap
}

pub(super) fn require_owner(office: &Office, actor: &str) -> Result<(), ApiError> {
    office
        .require_owner_member(actor)
        .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))?
        .map(|_| ())
}

pub(super) fn find_subscription_connection(
    office: &Office,
    connection_ref: &str,
    allow_archived: bool,
) -> Result<ProviderConnection, ApiError> {
    let conns = office
        .load_provider_connections()
        .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))?;
    let mut matches: Vec<ProviderConnection> = if org_proto::is_uuid_v7(connection_ref) {
        conns
            .into_iter()
            .filter(|c| c.id.as_str() == connection_ref)
            .collect()
    } else {
        conns
            .into_iter()
            .filter(|c| c.key == connection_ref)
            .collect()
    };
    if matches.len() > 1 {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "ambiguous provider connection key; specify canonical connection_id",
        ));
    }
    let Some(conn) = matches.pop() else {
        return Err(ApiError::new(
            ErrorCode::NotFound,
            "provider connection not found",
        ));
    };
    if conn.auth_mode != ProviderAuthMode::SubscriptionCli {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "subscription usage collection is only valid for subscription_cli connections",
        ));
    }
    if !allow_archived && conn.archived {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "archived provider connections cannot collect subscription usage",
        ));
    }
    Ok(conn)
}

pub(super) fn resolve_collection_targets(
    office: &Office,
    connection_id: Option<&str>,
) -> Result<Vec<ProviderConnection>, ApiError> {
    if let Some(cid) = connection_id {
        return Ok(vec![find_subscription_connection(office, cid, false)?]);
    }
    let conns = office
        .load_provider_connections()
        .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))?;
    Ok(conns
        .into_iter()
        .filter(|c| !c.archived && c.auth_mode == ProviderAuthMode::SubscriptionCli)
        .collect())
}

pub(super) fn map_vault_error(err: UsageCredentialError) -> ApiError {
    match err {
        UsageCredentialError::MissingSecretsKey => ApiError::new(
            ErrorCode::StorageError,
            "office secrets key unavailable; cannot wrap usage credentials",
        ),
        UsageCredentialError::InvalidInput(msg) => ApiError::new(ErrorCode::InvalidRequest, msg),
        UsageCredentialError::NotAuthorized(msg) => ApiError::new(ErrorCode::NotAuthorized, msg),
        UsageCredentialError::Conflict(msg) => ApiError::new(ErrorCode::Conflict, msg),
        UsageCredentialError::NotFound(msg) => ApiError::new(ErrorCode::NotFound, msg),
        UsageCredentialError::Expired(msg) => ApiError::new(ErrorCode::Expired, msg),
        UsageCredentialError::Crypto(msg) | UsageCredentialError::Io(msg) => {
            ApiError::new(ErrorCode::StorageError, msg)
        }
    }
}
