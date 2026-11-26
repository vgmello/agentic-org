//! `cost.usage.correct`: immutable attribution corrections with target resolution, scope
//! authorization, the reached-task-budget guard and exact-replay handling.

use org_proto::costs::{
    CostUsageCorrectParams, CostUsageCorrectResult, UsageRecordView, UsageScopeKind,
};
use org_proto::{ApiError, ErrorCode};

use super::super::Office;
use super::access::resolve_viewer_scope;
use crate::accounting::{build_attribution_correction, CostViewerScope, UsageBuildMeta};
use crate::error::OfficeError;
use crate::organization::records::{Employee, EmployeeKind};
use crate::time::format_rfc3339_ms;

pub(super) fn op_cost_usage_correct(
    office: &Office,
    actor: &str,
    mut params: CostUsageCorrectParams,
) -> Result<Result<CostUsageCorrectResult, ApiError>, OfficeError> {
    let (caller, viewer) = match resolve_viewer_scope(office, actor)? {
        Ok(v) => v,
        Err(e) => return Ok(Err(e)),
    };
    if !org_proto::is_uuid_v7(&params.operation_id) || !org_proto::is_uuid_v7(&params.supersedes_id)
    {
        return Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "operation_id and supersedes_id must be canonical lowercase UUIDv7",
        )));
    }
    let Some(parent) = office
        .accounting
        .get_usage(&office.info.org_id, &params.supersedes_id)?
    else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "usage record not found",
        )));
    };
    // Enforce actor and scope authorization BEFORE replay lookup, while deferring the
    // reached-budget guard until after exact-replay lookup under `budget_op_lock` (`Pass 10 #2/#3`).
    if let Err(e) = fill_correction_target_fields(office, &parent, &mut params)? {
        return Ok(Err(e));
    }
    if let Err(e) = check_correction_scope_auth(&viewer, &parent, &params) {
        return Ok(Err(e));
    }
    let _budget_guard = office.lock_budget_ops();
    let producer_id = format!("hq-corrections-{}", office.info.office_id);
    let recorded_at = format_rfc3339_ms(office.monotonic_now_ms());
    let corr = match build_attribution_correction(
        UsageBuildMeta {
            org_id: &office.info.org_id,
            origin_office_id: &office.info.office_id,
            sequence: 1,
            recorded_at: &recorded_at,
            recorded_by_kind: "employee",
            recorded_by_id: Some(caller.id.as_str().to_owned()),
            default_id: office.next_uuid_v7(),
        },
        &parent,
        &params,
    ) {
        Ok(r) => r,
        Err(e) => return Ok(Err(e)),
    };
    if let Some(existing) = office.accounting.find_by_source_event(
        &office.info.org_id,
        &office.info.office_id,
        &producer_id,
        &corr.source_event_id,
    )? {
        if existing.payload_sha256 == corr.payload_sha256 {
            super::super::budgets::op_reevaluate_for_record(office, &parent)?;
            super::super::budgets::op_reevaluate_for_record(office, &existing)?;
            return Ok(Ok(CostUsageCorrectResult { record: existing }));
        }
        return Ok(Err(ApiError::new(
            ErrorCode::Conflict,
            "operation_id reused with different correction payload",
        )));
    }
    if let Err(e) = check_reached_task_correction_guard_locked(office, &caller, &parent, &params)? {
        return Ok(Err(e));
    }
    if office
        .accounting
        .is_superseded(&office.info.org_id, &parent.id)?
    {
        return Ok(Err(ApiError::new(
            ErrorCode::Conflict,
            "usage record has already been superseded; correct the latest leaf record",
        )));
    }
    let (saved, _replayed) = match office.accounting.append_usage(&office.info.org_id, corr)? {
        Ok(v) => v,
        Err(e) => return Ok(Err(e)),
    };
    super::super::budgets::op_reevaluate_for_record(office, &parent)?;
    super::super::budgets::op_reevaluate_for_record(office, &saved)?;
    Ok(Ok(CostUsageCorrectResult { record: saved }))
}

fn fill_correction_target_fields(
    office: &Office,
    parent: &UsageRecordView,
    params: &mut CostUsageCorrectParams,
) -> Result<Result<(), ApiError>, OfficeError> {
    if params.scope_kind == UsageScopeKind::Task {
        if let Some(ref tref) = params.task_id {
            let inspected = match office.task_inspect(tref)? {
                Ok(v) => v,
                Err(e) => return Ok(Err(e)),
            };
            if params.project_id.is_some() && params.project_id != inspected.task.project_id {
                return Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "project_id does not match the target task's owning project",
                )));
            }
            if params
                .team_id
                .as_deref()
                .is_some_and(|t| t != inspected.task.team_id.as_str())
            {
                return Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "team_id does not match the target task's owning team",
                )));
            }
            params.task_id = Some(inspected.task.id);
            params.project_id = inspected.task.project_id;
            params.team_id = Some(inspected.task.team_id);
        }
    } else if params.scope_kind == UsageScopeKind::Project && params.project_id.is_none() {
        params.project_id = parent.project_id.clone();
    }
    Ok(Ok(()))
}

fn check_reached_task_correction_guard_locked(
    office: &Office,
    caller: &Employee,
    parent: &UsageRecordView,
    params: &CostUsageCorrectParams,
) -> Result<Result<(), ApiError>, OfficeError> {
    // `doc-92`: reducing spend on a reached task budget requires a person (owner or human PM).
    if caller.kind != EmployeeKind::Agent {
        return Ok(Ok(()));
    }
    let Some(old_task) = parent.task_id.as_deref() else {
        return Ok(Ok(()));
    };
    if params.task_id.as_deref() != Some(old_task)
        && super::super::budgets::op_is_task_reached(office, old_task)?
    {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "Only a person can raise, clear or extend a budget.",
        )));
    }
    Ok(Ok(()))
}

fn check_correction_scope_auth(
    viewer: &CostViewerScope,
    parent: &UsageRecordView,
    params: &CostUsageCorrectParams,
) -> Result<(), ApiError> {
    if !viewer.allows_project(parent.project_id.as_deref()) {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "caller is not authorized to correct usage from this source project",
        ));
    }
    match params.scope_kind {
        UsageScopeKind::Task | UsageScopeKind::Project
            if !viewer.allows_project(params.project_id.as_deref()) =>
        {
            Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "caller is not authorized to attribute usage to the target project",
            ))
        }
        UsageScopeKind::Team | UsageScopeKind::Organization
            if *viewer != CostViewerScope::Owner =>
        {
            Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "only the organization owner can attribute usage outside a project",
            ))
        }
        _ => Ok(()),
    }
}
