//! Hiring RPC parameter parsing, store-error mapping, store-reply decoding and
//! record-to-wire conversions shared by the hiring dispatch.

use org_proto::hiring::{
    AgentExecutionRecord, HireProposalWire, HireRequestRecord, ModelProfileRecord, RoleRecord,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{Id, Outcome, Reply, StoreError};

use crate::organization::{
    authority::HIRE_DENIED,
    ops::{OrgRejection, NOT_AUTHORIZED},
    records::{
        self, AgentExecutionConfig, HireProposal, HireRequest, ModelProfile, PermissionPreset,
        ReasoningEffort, Role,
    },
};
use crate::structure::wire::rejection_code;

pub(super) fn parse_id(raw: &str, field: &str) -> Result<Id, ApiError> {
    if !records::is_stable_id(raw) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("{field} must be a lowercase UUIDv7"),
        ));
    }
    Id::parse(raw).map_err(|_| {
        ApiError::new(
            ErrorCode::InvalidRequest,
            format!("{field} must be a lowercase UUIDv7"),
        )
    })
}

pub(super) fn opt_id(raw: Option<&str>, field: &str) -> Result<Option<Id>, ApiError> {
    raw.map(|v| parse_id(v, field)).transpose()
}

pub(super) fn storage_err(_err: StoreError) -> ApiError {
    ApiError::new(ErrorCode::StorageError, "office storage error")
}

pub(super) fn map_store_error(err: StoreError) -> ApiError {
    match err {
        StoreError::Invalid(msg) => {
            if msg == HIRE_DENIED || msg == NOT_AUTHORIZED {
                ApiError::new(ErrorCode::NotAuthorized, msg)
            } else {
                ApiError::new(ErrorCode::InvalidRequest, msg)
            }
        }
        StoreError::Conflict => ApiError::new(
            ErrorCode::OperationReused,
            "command_id reused with different params",
        ),
        _ => ApiError::new(ErrorCode::StorageError, "office storage error"),
    }
}

pub(super) fn decode_reply<T: serde::de::DeserializeOwned>(reply: Reply) -> Result<T, ApiError> {
    match reply.outcome {
        Outcome::Completed => serde_json::from_slice(&reply.bytes)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "unreadable store reply")),
        Outcome::Rejected => {
            let rej: OrgRejection = serde_json::from_slice(&reply.bytes)
                .map_err(|_| ApiError::new(ErrorCode::StorageError, "unreadable rejection"))?;
            Err(ApiError::new(rejection_code(rej.code), rej.message))
        }
    }
}

/// The reasoning effort a request names, if any: one of `AGENT_EFFORT_LEVELS` or
/// `InvalidRequest` (an empty string or `none` included), never a silent default.
pub(super) fn effort_from_wire(raw: Option<&str>) -> Result<Option<ReasoningEffort>, ApiError> {
    raw.map(|text| {
        ReasoningEffort::parse(text).ok_or_else(|| {
            ApiError::new(
                ErrorCode::InvalidRequest,
                "effort is one of low, medium, high, xhigh, max",
            )
        })
    })
    .transpose()
}

pub(super) fn proposal_from_wire(w: HireProposalWire) -> Result<HireProposal, ApiError> {
    let permission_preset = match w.permission_preset.as_deref() {
        Some(raw) => Some(PermissionPreset::parse(raw).ok_or_else(|| {
            ApiError::new(ErrorCode::InvalidRequest, "invalid permission_preset")
        })?),
        None => None,
    };
    let effort = effort_from_wire(w.effort.as_deref())?;
    Ok(HireProposal {
        name: w.name,
        handle: w.handle,
        title: w.title,
        focus: w.focus,
        role_text: w.role_text,
        title_in_team: w.title_in_team,
        reports_to_employee_id: w.reports_to_employee_id,
        office_id: w.office_id,
        model_profile_id: w.model_profile_id,
        model_id: w.model_id,
        permission_preset,
        effort,
        tools_config: w.tools_config,
        system_prompt_markdown: w.system_prompt_markdown,
        body: w.body,
    })
}

fn proposal_to_wire(p: &HireProposal) -> HireProposalWire {
    HireProposalWire {
        name: p.name.clone(),
        handle: p.handle.clone(),
        title: p.title.clone(),
        focus: p.focus.clone(),
        role_text: p.role_text.clone(),
        title_in_team: p.title_in_team.clone(),
        reports_to_employee_id: p.reports_to_employee_id.clone(),
        office_id: p.office_id.clone(),
        model_profile_id: p.model_profile_id.clone(),
        model_id: p.model_id.clone(),
        permission_preset: p.permission_preset.map(|x| x.as_str().to_owned()),
        effort: p.effort.map(|e| e.as_str().to_owned()),
        tools_config: p.tools_config.clone(),
        system_prompt_markdown: p.system_prompt_markdown.clone(),
        body: p.body.clone(),
    }
}

fn by_to_wire(by: &crate::organization::records::By) -> (Option<String>, Option<String>) {
    match by {
        crate::organization::records::By::Employee(id) => {
            (Some("employee".to_owned()), Some(id.as_str().to_owned()))
        }
        crate::organization::records::By::System => (Some("system".to_owned()), None),
        crate::organization::records::By::Unknown => (Some("unknown".to_owned()), None),
    }
}

/// `9999-12-31T23:59:59.999Z`: largest UTC millisecond with a four-digit RFC 3339 year.
const MAX_RFC3339_4DIGIT_YEAR_MS: u64 = 253_402_300_799_999;

fn normalize_rfc3339_case(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            't' => 'T',
            'z' => 'Z',
            other => other,
        })
        .collect()
}

fn canonical_wire_time(raw: Option<&str>) -> Option<String> {
    raw.map(|value| match crate::time::parse_rfc3339_ms(value) {
        Some(ms) if ms <= MAX_RFC3339_4DIGIT_YEAR_MS => crate::time::format_rfc3339_ms(ms),
        Some(_) if value.as_bytes().get(17..19) == Some(b"60") => {
            crate::time::format_rfc3339_ms(MAX_RFC3339_4DIGIT_YEAR_MS)
        }
        Some(_) => normalize_rfc3339_case(value),
        None => value.to_owned(),
    })
}

pub(super) fn role_to_wire(r: &Role) -> RoleRecord {
    let (created_by_kind, created_by_id) = by_to_wire(&r.meta.created_by);
    let (updated_by_kind, updated_by_id) = by_to_wire(&r.meta.updated_by);
    RoleRecord {
        id: r.id.as_str().to_owned(),
        key: r.key.clone(),
        name: r.name.clone(),
        body: r.body.clone(),
        revision: r.meta.revision,
        created_at: canonical_wire_time(r.meta.created_at.as_deref()),
        created_by_kind,
        created_by_id,
        updated_at: canonical_wire_time(r.meta.updated_at.as_deref()),
        updated_by_kind,
        updated_by_id,
    }
}

pub(super) fn hire_to_wire(h: &HireRequest) -> HireRequestRecord {
    HireRequestRecord {
        id: h.id.as_str().to_owned(),
        revision: h.revision,
        state: h.state.as_str().to_owned(),
        requested_by_employee_id: h.requested_by_employee_id.as_str().to_owned(),
        home_team_id: h.home_team_id.as_str().to_owned(),
        role_id: h.role_id.as_ref().map(|i| i.as_str().to_owned()),
        proposal: proposal_to_wire(&h.proposal),
        reason: h.reason.clone(),
        confirmed_at: h.confirmed_at.clone(),
        closed_reason: h.closed_reason.clone(),
        employee_id: h.employee_id.as_ref().map(|i| i.as_str().to_owned()),
        failed_step: h.failed_step.clone(),
        created_at: h.created_at.clone(),
        updated_at: h.updated_at.clone(),
    }
}

pub(super) fn exec_to_wire(e: &AgentExecutionConfig) -> AgentExecutionRecord {
    AgentExecutionRecord {
        employee_id: e.employee_id.as_str().to_owned(),
        model_profile_id: e.model_profile_id.as_str().to_owned(),
        model_id: e.model_id.clone(),
        tools_config: e.tools_config.clone(),
        permission_preset: e.permission_preset.as_str().to_owned(),
        effort: e.effort.map(|x| x.as_str().to_owned()),
        enabled: e.enabled,
        system_prompt_markdown: e.system_prompt_markdown.clone(),
        revision: e.meta.revision,
    }
}

pub(super) fn profile_to_wire(p: &ModelProfile) -> ModelProfileRecord {
    ModelProfileRecord {
        id: p.id.as_str().to_owned(),
        key: p.key.clone(),
        name: p.name.clone(),
        description: p.description.clone(),
        is_auto_generated: p.is_auto_generated,
        generated_for_connection_id: p
            .generated_for_connection_id
            .as_ref()
            .map(|i| i.as_str().to_owned()),
        archived: p.archived,
        connection_ids: p
            .connection_ids
            .iter()
            .map(|i| i.as_str().to_owned())
            .collect(),
        revision: p.meta.revision,
    }
}
