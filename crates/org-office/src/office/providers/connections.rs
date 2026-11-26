//! Provider connections, model profiles and office-local provider bindings.

use org_proto::hiring::EntityOpResult;
use org_proto::providers::{
    BoundConnectionSummary, ModelProfileCreateParams, ModelProfileUpdateParams,
    ProviderBindingListParams, ProviderBindingListResult, ProviderBindingSetParams,
    ProviderBindingSetResult, ProviderConnectionListParams, ProviderConnectionListResult,
    ProviderConnectionUpdateParams, ProviderConnectionView,
};
use org_proto::{ApiError, ErrorCode};

use crate::error::OfficeError;
use crate::office::Office;
use crate::organization::features::provider_pools::{
    CreateModelProfile, UpdateModelProfile, UpdateProviderConnection,
};
use crate::organization::records::ProviderConnection;
use crate::providers::OfficeConnectionBinding;
use crate::time::format_rfc3339_ms;

use super::permits::hex_sha256;
use super::{parse_uuid, require_hq_owner, require_office_operator, run_org_feature};

fn connection_to_view(c: &ProviderConnection) -> ProviderConnectionView {
    ProviderConnectionView {
        id: c.id.as_str().to_owned(),
        revision: c.meta.revision,
        key: c.key.clone(),
        name: c.name.clone(),
        provider_type: c.provider_type.clone(),
        auth_mode: c.auth_mode.as_str().to_owned(),
        subscription_identity: c.subscription_identity.clone(),
        runtime_driver: c.runtime_driver.clone(),
        endpoint_url: c.endpoint_url.clone(),
        enabled: c.enabled,
        archived: c.archived,
        max_concurrent_turns: c.max_concurrent_turns,
        created_at: c.meta.created_at.clone().unwrap_or_default(),
        updated_at: c.meta.updated_at.clone().unwrap_or_default(),
    }
}

pub(super) fn op_connection_list(
    office: &Office,
    actor: &str,
    params: ProviderConnectionListParams,
) -> Result<Result<ProviderConnectionListResult, ApiError>, OfficeError> {
    let is_sat_operator =
        office.info.role == crate::OfficeRole::Satellite && actor == office.info.office_id;
    if !is_sat_operator {
        if let Err(e) = office.require_active_member(actor)? {
            return Ok(Err(e));
        }
    }
    let conns = office
        .load_provider_connections()?
        .into_iter()
        .filter(|c| params.include_archived || !c.archived)
        .map(|c| connection_to_view(&c))
        .collect();
    Ok(Ok(ProviderConnectionListResult { connections: conns }))
}

pub(super) fn op_connection_update(
    office: &Office,
    actor: &str,
    params: ProviderConnectionUpdateParams,
) -> Result<Result<EntityOpResult, ApiError>, OfficeError> {
    let owner_id = match require_hq_owner(office, actor)? {
        Ok(id) => id,
        Err(e) => return Ok(Err(e)),
    };
    let conn_id = match parse_uuid(&params.connection_id, "connection_id") {
        Ok(id) => id,
        Err(e) => return Ok(Err(e)),
    };
    let cmd = UpdateProviderConnection {
        id: conn_id,
        expected_revision: params.expected_version,
        name: params.name,
        endpoint_url: params.endpoint_url,
        clear_endpoint_url: params.clear_endpoint_url,
        enabled: params.enabled,
        archived: params.archived,
        max_concurrent_turns: params.max_concurrent_turns,
    };
    run_org_feature(office, &params.command_id, owner_id, cmd)
}

pub(super) fn op_profile_create(
    office: &Office,
    actor: &str,
    params: ModelProfileCreateParams,
) -> Result<Result<EntityOpResult, ApiError>, OfficeError> {
    let owner_id = match require_hq_owner(office, actor)? {
        Ok(id) => id,
        Err(e) => return Ok(Err(e)),
    };
    let profile_id = match parse_uuid(&params.profile_id, "profile_id") {
        Ok(id) => id,
        Err(e) => return Ok(Err(e)),
    };
    let mut connection_ids = Vec::with_capacity(params.connection_ids.len());
    for raw in &params.connection_ids {
        match parse_uuid(raw, "connection_id") {
            Ok(id) => connection_ids.push(id),
            Err(e) => return Ok(Err(e)),
        }
    }
    let cmd = CreateModelProfile {
        id: profile_id,
        key: params.key,
        name: params.name,
        description: params.description,
        connection_ids,
    };
    run_org_feature(office, &params.command_id, owner_id, cmd)
}

pub(super) fn op_profile_update(
    office: &Office,
    actor: &str,
    params: ModelProfileUpdateParams,
) -> Result<Result<EntityOpResult, ApiError>, OfficeError> {
    let owner_id = match require_hq_owner(office, actor)? {
        Ok(id) => id,
        Err(e) => return Ok(Err(e)),
    };
    let profile_id = match parse_uuid(&params.profile_id, "profile_id") {
        Ok(id) => id,
        Err(e) => return Ok(Err(e)),
    };
    let connection_ids = match params.connection_ids {
        Some(raws) => {
            let mut out = Vec::with_capacity(raws.len());
            for raw in &raws {
                match parse_uuid(raw, "connection_id") {
                    Ok(id) => out.push(id),
                    Err(e) => return Ok(Err(e)),
                }
            }
            Some(out)
        }
        None => None,
    };
    let cmd = UpdateModelProfile {
        id: profile_id,
        expected_revision: params.expected_version,
        name: params.name,
        description: params.description,
        archived: params.archived,
        connection_ids,
    };
    run_org_feature(office, &params.command_id, owner_id, cmd)
}

fn validate_binding_set_params(params: &ProviderBindingSetParams) -> Result<(), ApiError> {
    if !org_proto::is_uuid_v7(&params.operation_id) || !org_proto::is_uuid_v7(&params.connection_id)
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "operation_id and connection_id must be canonical lowercase UUIDv7",
        ));
    }
    if !crate::organization::records::is_line(&params.auth_context_ref)
        || params.auth_context_ref.len() > 512
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "auth_context_ref must be a non-empty single line",
        ));
    }
    if params
        .runtime_binary_path
        .as_deref()
        .is_some_and(|v| !crate::organization::records::is_line(v) || v.len() > 1024)
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "runtime_binary_path must be a valid single line",
        ));
    }
    if params
        .account_identity_ref
        .as_deref()
        .is_some_and(|v| !crate::organization::records::is_line(v) || v.len() > 512)
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "account_identity_ref must be a valid single line",
        ));
    }
    Ok(())
}

pub(super) fn op_binding_set(
    office: &Office,
    actor: &str,
    params: ProviderBindingSetParams,
) -> Result<Result<ProviderBindingSetResult, ApiError>, OfficeError> {
    if let Err(e) = require_office_operator(office, actor)? {
        return Ok(Err(e));
    }
    if let Err(e) = validate_binding_set_params(&params) {
        return Ok(Err(e));
    }
    let Some(conn) = office.load_provider_connection(&params.connection_id)? else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "provider connection not found",
        )));
    };
    if conn.archived {
        return Ok(Err(ApiError::new(
            ErrorCode::Conflict,
            "cannot bind an archived provider connection",
        )));
    }
    let req_hash = hex_sha256(
        &serde_json::to_vec(&serde_json::json!({
            "connection_id": params.connection_id,
            "expected_revision": params.expected_revision,
            "auth_context_ref": params.auth_context_ref,
            "runtime_binary_path": params.runtime_binary_path,
            "account_identity_ref": params.account_identity_ref,
        }))
        .unwrap_or_default(),
    );
    let existing = office.providers.get_binding(
        &office.info.org_id,
        &office.info.office_id,
        &params.connection_id,
    )?;
    if let Some(ref cur) = existing {
        if cur.last_operation_id.as_deref() == Some(params.operation_id.as_str()) {
            if cur.last_operation_hash.as_deref() == Some(req_hash.as_str()) {
                return Ok(Ok(ProviderBindingSetResult {
                    binding: cur.to_view(),
                    replayed: true,
                }));
            }
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "operation_id reused with different binding payload",
            )));
        }
        if let Some(exp) = params.expected_revision {
            if cur.revision != exp {
                return Ok(Err(ApiError::new(
                    ErrorCode::Conflict,
                    "stale binding revision",
                )));
            }
        }
    } else if params.expected_revision.is_some_and(|r| r != 0) {
        return Ok(Err(ApiError::new(
            ErrorCode::Conflict,
            "binding does not exist yet",
        )));
    }
    let next_rev = existing
        .as_ref()
        .map_or(1, |b| b.revision.saturating_add(1));
    let binding = OfficeConnectionBinding {
        organization_id: office.info.org_id.clone(),
        office_id: office.info.office_id.clone(),
        connection_id: params.connection_id,
        revision: next_rev,
        auth_context_ref: params.auth_context_ref,
        runtime_binary_path: params.runtime_binary_path,
        account_identity_ref: params.account_identity_ref,
        updated_at: format_rfc3339_ms(office.monotonic_now_ms()),
        last_operation_id: Some(params.operation_id),
        last_operation_hash: Some(req_hash),
    };
    office.providers.save_binding_and_invalidate(&binding)?;
    Ok(Ok(ProviderBindingSetResult {
        binding: binding.to_view(),
        replayed: false,
    }))
}

pub(super) fn op_binding_list(
    office: &Office,
    actor: &str,
    params: ProviderBindingListParams,
) -> Result<Result<ProviderBindingListResult, ApiError>, OfficeError> {
    if let Err(e) = require_office_operator(office, actor)? {
        return Ok(Err(e));
    }
    let now_ms = office.monotonic_now_ms();
    let mut bindings = office
        .providers
        .list_bindings(&office.info.org_id, &office.info.office_id)?;
    if let Some(ref cid) = params.connection_id {
        bindings.retain(|b| b.connection_id == *cid);
    }
    let mut out = Vec::with_capacity(bindings.len());
    for b in bindings {
        let state = office
            .providers
            .get_state(
                &office.info.org_id,
                &office.info.office_id,
                &b.connection_id,
            )?
            .map(|s| s.to_view(b.revision, now_ms));
        let models = office
            .providers
            .list_models(
                &office.info.org_id,
                &office.info.office_id,
                &b.connection_id,
            )?
            .into_iter()
            .map(|m| m.to_view(now_ms))
            .collect();
        out.push(BoundConnectionSummary {
            binding: b.to_view(),
            state,
            models,
        });
    }
    Ok(Ok(ProviderBindingListResult { bindings: out }))
}
