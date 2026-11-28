//! Office-local subscription usage snapshots: validated persistence and the owner-only
//! `cost.subscription_usage.list` view with byte-bounded totals and snapshot pagination.

use org_proto::costs::{CostSubscriptionUsageListParams, CostSubscriptionUsageListResult};
use org_proto::{ApiError, ErrorCode};

use super::super::Office;
use super::access::require_owner_for;
use crate::accounting::{
    aggregate_totals_by_model, build_snapshot_view, paginate_snapshots, paginate_totals,
    validate_stored_snapshot, StoredSubscriptionUsageSnapshot, SubscriptionConnectionMeta,
    SubscriptionCredentialPresence,
};
use crate::error::OfficeError;
use crate::organization::records::ProviderAuthMode;

pub(super) fn op_save_subscription_usage(
    office: &Office,
    snapshot: StoredSubscriptionUsageSnapshot,
) -> Result<Result<(), ApiError>, OfficeError> {
    if snapshot.organization_id != office.info.org_id || snapshot.office_id != office.info.office_id
    {
        return Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "subscription usage snapshot organization or office mismatch",
        )));
    }
    if let Err(err) = validate_stored_snapshot(&snapshot) {
        return Ok(Err(err));
    }
    office
        .accounting
        .save_subscription_usage(&office.info.org_id, snapshot)?;
    Ok(Ok(()))
}

pub(super) fn op_subscription_usage_list(
    office: &Office,
    actor: &str,
    params: CostSubscriptionUsageListParams,
    cred_lookup: impl Fn(&str) -> SubscriptionCredentialPresence,
) -> Result<Result<CostSubscriptionUsageListResult, ApiError>, OfficeError> {
    if let Err(e) = require_owner_for(office, actor)? {
        return Ok(Err(e));
    }
    let conns = office.load_provider_connections()?;
    let stored_map: std::collections::BTreeMap<String, StoredSubscriptionUsageSnapshot> = office
        .accounting
        .list_subscription_usage(&office.info.org_id)?
        .into_iter()
        .map(|s| (s.connection_id.clone(), s))
        .collect();
    let now_ms = office.current_time_ms();
    let provider_filter = params
        .provider_type
        .as_deref()
        .map(|p| p.trim().to_ascii_lowercase())
        .filter(|p| !p.is_empty());

    let mut all_snapshots = Vec::new();
    let mut collected_as_of: Option<String> = None;
    for conn in &conns {
        if conn.auth_mode != ProviderAuthMode::SubscriptionCli {
            continue;
        }
        if !params.include_archived && conn.archived {
            continue;
        }
        if let Some(ref cid) = params.connection_id {
            let matches = if org_proto::is_uuid_v7(cid) {
                conn.id.as_str() == cid
            } else {
                conn.key == *cid
            };
            if !matches {
                continue;
            }
        }
        if let Some(ref pf) = provider_filter {
            if conn.provider_type.to_ascii_lowercase() != *pf {
                continue;
            }
        }
        let stored = stored_map.get(conn.id.as_str());
        let cred = cred_lookup(conn.id.as_str());
        let view = build_snapshot_view(
            SubscriptionConnectionMeta {
                id: conn.id.as_str(),
                key: &conn.key,
                name: &conn.name,
                provider_type: &conn.provider_type,
                subscription_identity: conn.subscription_identity.as_deref(),
            },
            &office.info.office_id,
            stored,
            &cred,
            now_ms,
        );
        if let Some(ref at) = view.collected_at {
            if collected_as_of
                .as_deref()
                .is_none_or(|cur| at.as_str() > cur)
            {
                collected_as_of = Some(at.clone());
            }
        }
        all_snapshots.push(view);
    }
    all_snapshots.sort_by(|a, b| a.connection_id.cmp(&b.connection_id));
    let rollup = aggregate_totals_by_model(&all_snapshots);
    let (totals_by_model, next_totals_cursor, totals_bytes) =
        paginate_totals(rollup.totals_by_model, params.totals_cursor.as_deref());
    let (snapshots, next_cursor) = paginate_snapshots(
        all_snapshots,
        params.cursor.as_deref(),
        params.limit,
        totals_bytes,
    );
    Ok(Ok(CostSubscriptionUsageListResult {
        snapshots,
        totals_by_model,
        collected_as_of,
        next_cursor,
        next_totals_cursor,
        partial_totals: rollup.partial_totals,
        excluded_snapshot_count: rollup.excluded_snapshot_count,
        is_api_equivalent_estimate: false,
    }))
}
