//! Row decoders: one row of each organization table back into its record.

use org_store::{EngineError, Id};
use rusqlite::{Connection, Row};

use super::{all, foreign, id, meta, number, one_of, optional_id, text};
use crate::organization::{
    ports::RecordRef,
    records::{
        AgentExecutionConfig, BuiltinKey, Employee, EmployeeKind, EmployeeStatus, HireProposal,
        HireRequest, HireState, ModelProfile, Office, OfficeCode, OfficeRole, Organization,
        PermissionPreset, ProviderAuthMode, ProviderConnection, ReasoningEffort, Repository,
        RepositoryKind, Role, Team, TeamAllocation, TeamCounter, TeamStatus, WatchingModelSetting,
    },
};

/// A `default_dod_json` column: a JSON array of step texts, or null (`TASK-88`).
pub(in crate::store::sqlite) fn default_dod(
    row: &Row<'_>,
    col: usize,
) -> rusqlite::Result<Option<Vec<String>>> {
    let json: Option<String> = row.get(col)?;
    json.map(|json| serde_json::from_str(&json))
        .transpose()
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(col, rusqlite::types::Type::Text, e.into())
        })
}

pub(super) fn organization_row(row: &Row<'_>) -> rusqlite::Result<Organization> {
    Ok(Organization {
        id: id(row, 0)?,
        slug: row.get(1)?,
        name: row.get(2)?,
        organization_repository_id: id(row, 3)?,
        state_branch: row.get(4)?,
        default_dod: default_dod(row, 5)?,
        meta: meta(row, 6)?,
    })
}

pub(super) fn repository_row(row: &Row<'_>) -> rusqlite::Result<Repository> {
    const KINDS: [RepositoryKind; 2] = [RepositoryKind::Remote, RepositoryKind::Local];
    Ok(Repository {
        id: id(row, 0)?,
        key: row.get(1)?,
        kind: one_of(row, 2, &KINDS, RepositoryKind::as_str)?,
        remote_url: row.get(3)?,
        meta: meta(row, 4)?,
    })
}

pub(super) fn office_row(row: &Row<'_>) -> rusqlite::Result<Office> {
    const ROLES: [OfficeRole; 2] = [OfficeRole::Hq, OfficeRole::Satellite];
    Ok(Office {
        id: id(row, 0)?,
        slug: row.get(1)?,
        name: row.get(2)?,
        role: one_of(row, 3, &ROLES, OfficeRole::as_str)?,
        meta: meta(row, 4)?,
    })
}

pub(super) fn office_code_row(row: &Row<'_>) -> rusqlite::Result<OfficeCode> {
    Ok(OfficeCode {
        code: row.get(0)?,
        office_id: id(row, 1)?,
        issued_at: row.get(2)?,
        retired_at: row.get(3)?,
    })
}

pub(super) fn team_row(row: &Row<'_>) -> rusqlite::Result<Team> {
    const STATUSES: [TeamStatus; 2] = [TeamStatus::Active, TeamStatus::Archived];
    Ok(Team {
        id: id(row, 0)?,
        name: row.get(1)?,
        number: number(row, 2)?,
        parent_team_id: optional_id(row, 3)?,
        lead_employee_id: optional_id(row, 4)?,
        status: one_of(row, 5, &STATUSES, TeamStatus::as_str)?,
        max_members: number(row, 6)?,
        meta: meta(row, 7)?,
    })
}

pub(super) fn employee_row(row: &Row<'_>) -> rusqlite::Result<Employee> {
    const KINDS: [EmployeeKind; 2] = [EmployeeKind::Person, EmployeeKind::Agent];
    const STATUSES: [EmployeeStatus; 3] = [
        EmployeeStatus::Active,
        EmployeeStatus::Paused,
        EmployeeStatus::Inactive,
    ];
    Ok(Employee {
        id: id(row, 0)?,
        handle: row.get(1)?,
        kind: one_of(row, 2, &KINDS, EmployeeKind::as_str)?,
        name: row.get(3)?,
        home_team_id: id(row, 4)?,
        role_id: optional_id(row, 5)?,
        reports_to_employee_id: optional_id(row, 6)?,
        status: one_of(row, 7, &STATUSES, EmployeeStatus::as_str)?,
        contact: row.get(8)?,
        title: row.get(9)?,
        focus: row.get(10)?,
        role_text: row.get(11)?,
        title_in_team: row.get(12)?,
        offboarded_at: row.get(13)?,
        may_hire: row.get(14)?,
        builtin_key: optional_builtin_key(row, 15)?,
        body: row.get(16)?,
        meta: meta(row, 17)?,
    })
}

fn optional_builtin_key(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<BuiltinKey>> {
    let stored: Option<String> = row.get(column)?;
    match stored.as_deref() {
        None => Ok(None),
        Some(text) => BuiltinKey::parse(text)
            .map(Some)
            .ok_or_else(|| foreign(column, "not a known value")),
    }
}

pub(super) fn role_row(row: &Row<'_>) -> rusqlite::Result<Role> {
    Ok(Role {
        id: id(row, 0)?,
        key: row.get(1)?,
        name: row.get(2)?,
        body: row.get(3)?,
        meta: meta(row, 4)?,
    })
}

pub(super) fn team_counter_row(row: &Row<'_>) -> rusqlite::Result<TeamCounter> {
    Ok(TeamCounter {
        organization_id: id(row, 0)?,
        last_number: number(row, 1)?,
    })
}

pub(super) fn team_allocation_row(row: &Row<'_>) -> rusqlite::Result<TeamAllocation> {
    Ok(TeamAllocation {
        organization_id: id(row, 0)?,
        number: number(row, 1)?,
        team_id: id(row, 2)?,
        allocated_at: row.get(3)?,
    })
}

pub(super) fn record_ref_row(row: &Row<'_>) -> rusqlite::Result<RecordRef> {
    Ok(RecordRef {
        kind: row.get(0)?,
        id: id(row, 1)?,
        is_deleted: row.get(2)?,
    })
}

pub(super) fn hire_request_row(row: &Row<'_>) -> rusqlite::Result<HireRequest> {
    let state_text: String = row.get(2)?;
    let state = HireState::parse(&state_text).ok_or_else(|| foreign(2, "not a hire state"))?;
    let proposal_json: String = row.get(6)?;
    let proposal: HireProposal =
        serde_json::from_str(&proposal_json).map_err(|_| foreign(6, "invalid proposal_json"))?;
    Ok(HireRequest {
        id: id(row, 0)?,
        revision: number(row, 1)?,
        state,
        requested_by_employee_id: id(row, 3)?,
        home_team_id: id(row, 4)?,
        role_id: optional_id(row, 5)?,
        proposal,
        reason: row.get(7)?,
        confirmed_at: row.get(8)?,
        closed_reason: row.get(9)?,
        employee_id: optional_id(row, 10)?,
        failed_step: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

pub(super) fn agent_execution_row(row: &Row<'_>) -> rusqlite::Result<AgentExecutionConfig> {
    let tools_json: String = row.get(4)?;
    let tools_config: Vec<String> =
        serde_json::from_str(&tools_json).map_err(|_| foreign(4, "invalid tools_config_json"))?;
    let preset_text: String = row.get(5)?;
    let permission_preset = PermissionPreset::parse(&preset_text)
        .ok_or_else(|| foreign(5, "not a permission preset"))?;
    let effort_text: Option<String> = row.get(7)?;
    let effort = effort_text
        .map(|text| ReasoningEffort::parse(&text).ok_or_else(|| foreign(7, "not an effort")))
        .transpose()?;
    Ok(AgentExecutionConfig {
        employee_id: id(row, 0)?,
        model_profile_id: id(row, 1)?,
        model_id: row.get(2)?,
        system_prompt_markdown: row.get(3)?,
        tools_config,
        permission_preset,
        effort,
        enabled: row.get(6)?,
        meta: meta(row, 8)?,
    })
}

pub(super) fn watching_model_row(row: &Row<'_>) -> rusqlite::Result<WatchingModelSetting> {
    Ok(WatchingModelSetting {
        model_profile_id: optional_id(row, 0)?,
        meta: meta(row, 1)?,
    })
}

pub(super) fn provider_connection_row(row: &Row<'_>) -> rusqlite::Result<ProviderConnection> {
    let auth_text: String = row.get(4)?;
    let auth_mode =
        ProviderAuthMode::parse(&auth_text).ok_or_else(|| foreign(4, "not an auth mode"))?;
    Ok(ProviderConnection {
        id: id(row, 0)?,
        key: row.get(1)?,
        name: row.get(2)?,
        provider_type: row.get(3)?,
        auth_mode,
        subscription_identity: row.get(5)?,
        runtime_driver: row.get(6)?,
        endpoint_url: row.get(7)?,
        enabled: row.get(8)?,
        archived: row.get(9)?,
        max_concurrent_turns: number(row, 10)?,
        meta: meta(row, 11)?,
    })
}

pub(super) fn model_profile_base_row(row: &Row<'_>) -> rusqlite::Result<ModelProfile> {
    Ok(ModelProfile {
        id: id(row, 0)?,
        key: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        is_auto_generated: row.get(4)?,
        generated_for_connection_id: optional_id(row, 5)?,
        archived: row.get(6)?,
        connection_ids: Vec::new(),
        meta: meta(row, 7)?,
    })
}

pub(super) fn profile_connections(
    db: &Connection,
    scope: &str,
    profile_id: &Id,
) -> Result<Vec<Id>, EngineError> {
    let sql = "SELECT connection_id FROM model_profile_connections \
               WHERE organization_id = ?1 AND model_profile_id = ?2 ORDER BY priority";
    all(db, sql, vec![text(scope), text(profile_id.as_str())], |r| {
        id(r, 0)
    })
}
