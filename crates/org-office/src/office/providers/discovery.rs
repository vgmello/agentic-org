//! Refreshing a provider connection and discovering its models.

use std::collections::BTreeMap;

use org_proto::providers::{
    ConnectionReadinessStatus, DiscoveredModelEntitlement, ModelProfileModelsParams,
    ModelProfileModelsResult, ProfileModelEntry, ProviderConnectionRefreshParams,
    ProviderConnectionRefreshResult,
};
use org_proto::{ApiError, ErrorCode};

use crate::error::OfficeError;
use crate::office::Office;
use crate::organization::records::{ProviderAuthMode, ProviderConnection};
use crate::providers::{
    OfficeConnectionBinding, OfficeConnectionModel, OfficeConnectionState, MAX_OBSERVATION_TTL_SECS,
};
use crate::time::format_rfc3339_ms;

use super::require_office_operator;

pub(super) fn op_connection_refresh(
    office: &Office,
    actor: &str,
    params: ProviderConnectionRefreshParams,
) -> Result<Result<ProviderConnectionRefreshResult, ApiError>, OfficeError> {
    if let Err(e) = require_office_operator(office, actor)? {
        return Ok(Err(e));
    }
    let Some(conn) = office.load_provider_connection(&params.connection_id)? else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "provider connection not found",
        )));
    };
    let Some(binding) = office.providers.get_binding(
        &office.info.org_id,
        &office.info.office_id,
        &params.connection_id,
    )?
    else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "office has no local binding for this connection",
        )));
    };
    let now_ms = office.monotonic_now_ms();
    let now_utc = format_rfc3339_ms(now_ms);
    let ttl_secs = params
        .ttl_seconds
        .unwrap_or(MAX_OBSERVATION_TTL_SECS)
        .clamp(1, MAX_OBSERVATION_TTL_SECS);
    let valid_until = format_rfc3339_ms(now_ms.saturating_add(ttl_secs * 1000));

    let (status, issue_code) = evaluate_refresh_status(&conn, &binding, &params);
    let state = OfficeConnectionState {
        organization_id: office.info.org_id.clone(),
        office_id: office.info.office_id.clone(),
        connection_id: conn.id.as_str().to_owned(),
        status,
        binding_revision: binding.revision,
        driver_version: params.driver_version.clone(),
        observed_at: Some(now_utc.clone()),
        valid_until: (status == ConnectionReadinessStatus::Ready).then(|| valid_until.clone()),
        issue_code,
    };
    let mut models = Vec::new();
    let mut seen_models = std::collections::BTreeSet::new();
    if status == ConnectionReadinessStatus::Ready {
        for m in params.models {
            if m.model_id.trim().is_empty() || m.driver_model_id.trim().is_empty() {
                return Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "discovered model_id and driver_model_id must be non-empty",
                )));
            }
            if !seen_models.insert(m.model_id.clone()) {
                return Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "duplicate model_id in refresh payload",
                )));
            }
            let caps_ok = serde_json::from_str::<serde_json::Value>(&m.capabilities_json)
                .is_ok_and(|v| v.is_object());
            if !caps_ok {
                return Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "capabilities_json must be a JSON object",
                )));
            }
            models.push(OfficeConnectionModel {
                organization_id: office.info.org_id.clone(),
                office_id: office.info.office_id.clone(),
                connection_id: conn.id.as_str().to_owned(),
                model_id: m.model_id,
                driver_model_id: m.driver_model_id,
                entitlement: m.entitlement,
                capabilities_json: m.capabilities_json,
                observed_at: now_utc.clone(),
                valid_until: valid_until.clone(),
            });
        }
    }
    office.providers.save_observation(&state, &models)?;
    Ok(Ok(ProviderConnectionRefreshResult {
        state: state.to_view(binding.revision, now_ms),
        models: models.into_iter().map(|m| m.to_view(now_ms)).collect(),
    }))
}

fn evaluate_refresh_status(
    conn: &ProviderConnection,
    binding: &OfficeConnectionBinding,
    params: &ProviderConnectionRefreshParams,
) -> (ConnectionReadinessStatus, Option<String>) {
    if let Some(ref issue) = params.issue_code {
        return (ConnectionReadinessStatus::Unavailable, Some(issue.clone()));
    }
    match params.auth_signed_in {
        Some(false) => {
            return (
                ConnectionReadinessStatus::AuthExpired,
                Some("AUTH_EXPIRED".into()),
            );
        }
        None => {
            return (
                ConnectionReadinessStatus::Unknown,
                Some("AUTH_UNVERIFIED".into()),
            );
        }
        Some(true) => {}
    }
    // `doc-88` §4: compare subscription account evidence against registered subscription_identity!
    if conn.auth_mode == ProviderAuthMode::SubscriptionCli {
        let expected = conn.subscription_identity.as_deref();
        let observed = params
            .observed_subscription_identity
            .as_deref()
            .or(binding.account_identity_ref.as_deref());
        match (expected, observed, params.operator_attested_subscription) {
            (Some(exp), Some(obs), _) if obs != exp => {
                return (
                    ConnectionReadinessStatus::Unavailable,
                    Some("SUBSCRIPTION_IDENTITY_MISMATCH".into()),
                );
            }
            (Some(_), None, false) => {
                return (
                    ConnectionReadinessStatus::Unavailable,
                    Some("SUBSCRIPTION_IDENTITY_UNVERIFIED".into()),
                );
            }
            _ => {}
        }
    }
    if params
        .driver_version
        .as_deref()
        .is_none_or(|v| v.trim().is_empty())
    {
        return (
            ConnectionReadinessStatus::Unavailable,
            Some("DRIVER_VERSION_UNKNOWN".into()),
        );
    }
    (ConnectionReadinessStatus::Ready, None)
}

pub(super) fn op_profile_models(
    office: &Office,
    actor: &str,
    params: ModelProfileModelsParams,
) -> Result<Result<ModelProfileModelsResult, ApiError>, OfficeError> {
    if let Err(e) = office.require_active_member(actor)? {
        return Ok(Err(e));
    }
    let Some(profile) = office.load_model_profile(&params.model_profile_id)? else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "model profile not found",
        )));
    };
    let now_ms = office.monotonic_now_ms();
    let mut entries: BTreeMap<String, ProfileModelEntry> = BTreeMap::new();
    for (idx, cid) in profile.connection_ids.iter().enumerate() {
        let priority = (idx as u64) + 1;
        let conn = office.load_provider_connection(cid.as_str())?;
        let enabled = conn.as_ref().is_some_and(|c| c.enabled && !c.archived);
        let binding = office.load_connection_binding(cid.as_str())?;
        let state = office.load_connection_state(cid.as_str())?;
        let conn_ready = enabled
            && binding.as_ref().is_some_and(|b| {
                state.as_ref().is_some_and(|s| {
                    s.status == ConnectionReadinessStatus::Ready && !s.is_stale(b.revision, now_ms)
                })
            });
        for m in office.load_connection_models(cid.as_str())? {
            if m.entitlement != DiscoveredModelEntitlement::Confirmed {
                continue;
            }
            let model_stale = m.is_stale(now_ms);
            let ready_inc = u64::from(conn_ready && !model_stale);
            let entry = entries
                .entry(m.model_id.clone())
                .or_insert_with(|| ProfileModelEntry {
                    model_id: m.model_id.clone(),
                    supporting_connections: Vec::new(),
                    first_priority: priority,
                    as_of: Some(m.observed_at.clone()),
                    stale: model_stale,
                    ready_connections: 0,
                });
            entry.supporting_connections.push(cid.as_str().to_owned());
            entry.first_priority = entry.first_priority.min(priority);
            entry.stale = entry.stale && model_stale;
            entry.ready_connections += ready_inc;
            if entry.as_of.as_deref() < Some(m.observed_at.as_str()) {
                entry.as_of = Some(m.observed_at);
            }
        }
    }
    Ok(Ok(ModelProfileModelsResult {
        model_profile_id: profile.id.as_str().to_owned(),
        models: entries.into_values().collect(),
    }))
}
