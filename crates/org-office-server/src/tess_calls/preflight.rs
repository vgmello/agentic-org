//! Server-side target command resolution and live preflight validation for Chief of Staff
//! `suggestion.create` proposals (`TASK-80.4`).

use org_agent_cli::{Command, MutationPolicy};
use org_office::rules::OrgCommand;
use org_office::CallerAuthority;
use org_proto::{ApiError, ErrorCode, Request};
use serde_json::{json, Map, Value};

use super::TessService;

/// V1 allowlisted target RPC methods that Tess may propose for human confirmation.
const ALLOWED_SUGGESTION_CALLS: &[&str] = &[
    "chat.ask",
    "chat.channel_create",
    "chat.channel_join",
    "chat.channel_leave",
    "chat.mark_read",
    "chat.send",
    "docs.create",
    "docs.move",
    "docs.update",
    "hr.hire",
    "hr.hire.confirm",
    "hr.hire.draft",
    "hr.hire.withdraw",
    "member.add_agent",
    "member.add_person",
    "member.agent_move",
    "member.agent_offboard",
    "member.agent_set_status",
    "member.agent_update",
    "member.move",
    "member.offboard",
    "member.remove",
    "member.set_agent_status",
    "member.set_reports_to",
    "member.update_agent",
    "member.update_person",
    "msg.accept",
    "msg.defer",
    "msg.done",
    "msg.unsnooze",
    "msg.withdraw",
    "process.write",
    "project.add_team",
    "project.create",
    "project.remove_team",
    "repo.attach",
    "repo.fetch",
    "repo.register",
    "role.create",
    "role.update",
    "tasks.add_dependency",
    "tasks.assign",
    "tasks.checklist.add",
    "tasks.checklist.check",
    "tasks.checklist.edit",
    "tasks.checklist.move",
    "tasks.checklist.remove",
    "tasks.comment",
    "tasks.create",
    "tasks.remove_dependency",
    "tasks.transition",
    "tasks.unwatch",
    "tasks.update",
    "tasks.watch",
    "team.create",
    "team.delete",
    "team.move",
    "team.rename",
    "team.set_lead",
    "team.set_max_members",
    "team.set_title",
    "worktree.prepare",
];

/// Owner-only mutations that a non-owner caller cannot have proposed on their behalf.
const OWNER_ONLY_CALLS: &[&str] = &[
    "member.add_agent",
    "member.add_person",
    "member.agent_move",
    "member.agent_offboard",
    "member.agent_set_status",
    "member.agent_update",
    "member.move",
    "member.offboard",
    "member.remove",
    "member.set_agent_status",
    "member.set_reports_to",
    "member.update_agent",
    "member.update_person",
    "process.write",
    "project.add_team",
    "project.create",
    "project.remove_team",
    "repo.attach",
    "repo.register",
    "role.create",
    "role.update",
    "team.create",
    "team.delete",
    "team.move",
    "team.rename",
    "team.set_lead",
    "team.set_max_members",
    "team.set_title",
];

pub(super) fn resolve_target_command(raw: &str) -> Result<&'static Command, ApiError> {
    let words: Vec<String> = raw.split_whitespace().map(str::to_owned).collect();
    if words.is_empty() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "`command` must name a target `<resource> <verb>` command",
        ));
    }
    let table = org_agent_cli::runtime_table();
    let expanded = table.expand(&words);
    let [resource, verb] = expanded.as_ref() else {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("`{raw}` is not a valid `<resource> <verb>` command"),
        ));
    };
    let row = table.find(resource, verb).ok_or_else(|| {
        ApiError::new(
            ErrorCode::InvalidRequest,
            format!("unknown target command `{resource} {verb}`"),
        )
    })?;
    match table.mutation_policy(row) {
        MutationPolicy::ForbiddenForAgent => Err(ApiError::new(
            ErrorCode::NotAuthorized,
            format!(
                "`{}` is forbidden for agents and cannot be proposed",
                row.name()
            ),
        )),
        MutationPolicy::ReadOnly | MutationPolicy::AutoSafe => Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!(
                "`{}` does not require confirmation and can be run directly",
                row.name()
            ),
        )),
        MutationPolicy::ConfirmRequired => {
            if !ALLOWED_SUGGESTION_CALLS.contains(&row.call) {
                return Err(ApiError::new(
                    ErrorCode::NotAuthorized,
                    format!("`{}` is not eligible for suggestion proposals", row.name()),
                ));
            }
            Ok(row)
        }
    }
}

pub(super) fn preflight_target(
    svc: &TessService,
    caller_id: &str,
    authority: &CallerAuthority,
    call: &str,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<Option<String>, ApiError> {
    if OWNER_ONLY_CALLS.contains(&call) && !authority.is_owner {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            format!("caller is not authorized to perform `{call}`"),
        ));
    }
    if call.starts_with("hr.hire") {
        return preflight_hire(svc, caller_id, authority, call, input);
    }
    if call.starts_with("tasks.") {
        return preflight_task(svc, call, input, expected_revision);
    }
    if let Some(note) = preflight_org_command(svc, caller_id, call, input, expected_revision)? {
        return Ok(Some(note));
    }
    if matches!(
        call,
        "member.agent_update"
            | "member.agent_move"
            | "member.agent_set_status"
            | "member.agent_offboard"
    ) {
        return preflight_agent_target(svc, input);
    }
    if call == "team.set_max_members" {
        return preflight_team_target(svc, input);
    }
    Ok(Some("schema and authority verified".into()))
}

fn preflight_agent_target(
    svc: &TessService,
    input: &Map<String, Value>,
) -> Result<Option<String>, ApiError> {
    let member_id = input
        .get("member")
        .or_else(|| input.get("member_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ApiError::new(ErrorCode::InvalidRequest, "missing target agent member ID")
        })?;
    let snap = svc.office().org_snapshot().map_err(|err| {
        crate::logged("tess_calls.org_snapshot", &err);
        ApiError::new(ErrorCode::Unavailable, "unable to inspect organization")
    })?;
    let exists = snap
        .members
        .iter()
        .any(|m| m.id == member_id || m.handle.as_deref() == Some(member_id));
    if !exists {
        return Err(ApiError::new(
            ErrorCode::NotFound,
            format!("target member `{member_id}` not found"),
        ));
    }
    Ok(Some("agent target verified".into()))
}

fn preflight_team_target(
    svc: &TessService,
    input: &Map<String, Value>,
) -> Result<Option<String>, ApiError> {
    let team_id = input
        .get("team")
        .or_else(|| input.get("team_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::new(ErrorCode::InvalidRequest, "missing target team ID"))?;
    let snap = svc.office().org_snapshot().map_err(|err| {
        crate::logged("tess_calls.org_snapshot", &err);
        ApiError::new(ErrorCode::Unavailable, "unable to inspect organization")
    })?;
    if !snap.teams.iter().any(|t| t.id == team_id) {
        return Err(ApiError::new(
            ErrorCode::NotFound,
            format!("target team `{team_id}` not found"),
        ));
    }
    Ok(Some("team target verified".into()))
}

fn preflight_hire(
    svc: &TessService,
    caller_id: &str,
    authority: &CallerAuthority,
    call: &str,
    input: &Map<String, Value>,
) -> Result<Option<String>, ApiError> {
    if !(authority.is_owner || authority.may_hire) {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "caller does not have hiring authority",
        ));
    }
    match call {
        "hr.hire" | "hr.hire.draft" => {
            if let Some(req_id) = input
                .get("id")
                .or_else(|| input.get("request"))
                .or_else(|| input.get("request_id"))
                .and_then(Value::as_str)
            {
                if input.get("team").is_none() && input.get("home_team_id").is_none() {
                    return preflight_existing_hire(svc, caller_id, authority, req_id);
                }
            }
            let team = input
                .get("team")
                .or_else(|| input.get("home_team_id"))
                .and_then(Value::as_str)
                .ok_or_else(|| ApiError::new(ErrorCode::InvalidRequest, "missing target team"))?;
            check_caller_hire_team(svc, caller_id, authority, team)?;
            Ok(Some("hiring scope verified".into()))
        }
        "hr.hire.confirm" | "hr.hire.withdraw" => {
            let req_id = input
                .get("request")
                .or_else(|| input.get("request_id"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ApiError::new(ErrorCode::InvalidRequest, "missing hire request ID")
                })?;
            preflight_existing_hire(svc, caller_id, authority, req_id)
        }
        _ => Ok(Some("hiring authority verified".into())),
    }
}

fn preflight_existing_hire(
    svc: &TessService,
    caller_id: &str,
    authority: &CallerAuthority,
    req_id: &str,
) -> Result<Option<String>, ApiError> {
    let scope = svc
        .office()
        .delegated_hire_scope(req_id)
        .map_err(|err| {
            crate::logged("tess_calls.hire_scope", &err);
            ApiError::new(ErrorCode::Unavailable, "unable to inspect hire request")
        })?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "hire request not found"))?;
    check_caller_hire_team(svc, caller_id, authority, &scope.home_team_id)?;
    Ok(Some("hire request verified".into()))
}

fn check_caller_hire_team(
    svc: &TessService,
    caller_id: &str,
    authority: &CallerAuthority,
    team_id: &str,
) -> Result<(), ApiError> {
    if authority.is_owner {
        return Ok(());
    }
    let ok = svc
        .office()
        .caller_may_hire_into(caller_id, team_id)
        .map_err(|err| {
            crate::logged("tess_calls.may_hire_into", &err);
            ApiError::new(ErrorCode::Unavailable, "unable to check hiring scope")
        })?;
    if !ok {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "caller may not hire into the requested team",
        ));
    }
    Ok(())
}

fn preflight_task(
    svc: &TessService,
    call: &str,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<Option<String>, ApiError> {
    if call == "tasks.create" {
        if let Some(project) = input.get("project").and_then(Value::as_str) {
            svc.office().inspect_project(project).map_err(|err| {
                crate::logged("tess_calls.inspect_project", &err);
                ApiError::new(ErrorCode::Unavailable, "unable to inspect target project")
            })??;
        }
        if let Some(team) = input.get("team").and_then(Value::as_str) {
            let snap = svc.office().org_snapshot().map_err(|err| {
                crate::logged("tess_calls.org_snapshot", &err);
                ApiError::new(ErrorCode::Unavailable, "unable to inspect target team")
            })?;
            if !snap.teams.iter().any(|t| t.id == team || t.name == team) {
                return Err(ApiError::new(
                    ErrorCode::NotFound,
                    format!("target team `{team}` not found"),
                ));
            }
        }
        return Ok(Some("task creation target verified".into()));
    }
    let Some(task_ref) = input.get("task").and_then(Value::as_str) else {
        return Ok(Some("schema verified".into()));
    };
    let inspected = svc.office().task_inspect(task_ref).map_err(|err| {
        crate::logged("tess_calls.task_inspect", &err);
        ApiError::new(ErrorCode::Unavailable, "unable to inspect target task")
    })??;
    if let Some(expected) = expected_revision {
        if inspected.task.revision != expected {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                format!(
                    "target task `{}` is at revision {}, not expected revision {expected}",
                    inspected.task.key, inspected.task.revision
                ),
            ));
        }
    }
    Ok(Some(format!(
        "verified task `{}` at revision {}",
        inspected.task.key, inspected.task.revision
    )))
}

fn preflight_org_command(
    svc: &TessService,
    caller_id: &str,
    call: &str,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<Option<String>, ApiError> {
    let (wire_method, params) = translate_org_params(svc, call, input, expected_revision)?;
    let synthetic = Request {
        id: 0,
        method: wire_method.to_owned(),
        principal: None,
        params: Value::Object(params),
    };
    let parsed = match OrgCommand::parse_request(&synthetic) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => return Ok(None),
        Err(err) => return Err(err),
    };
    svc.office()
        .check_org(caller_id, parsed.command)
        .map_err(|err| {
            crate::logged("tess_calls.check_org", &err);
            ApiError::new(
                ErrorCode::Unavailable,
                "unable to run organization preflight check",
            )
        })??;
    Ok(Some("organization dry-run preflight passed".into()))
}

fn translate_org_params<'a>(
    svc: &TessService,
    call: &'a str,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<(&'a str, Map<String, Value>), ApiError> {
    let mut params = input.clone();
    params.remove("command_id");
    params.insert("dry_run".into(), json!(true));
    if let Some(rev) = expected_revision {
        params
            .entry("expected_version")
            .or_insert_with(|| json!(rev));
    }
    let mut wire_method = call;
    match call {
        "team.create" => {
            params
                .entry("team_id")
                .or_insert_with(|| json!(svc.next_uuid_v7()));
            rename_key(&mut params, "parent_team_id", "owner_team_id");
        }
        "team.rename" => wire_method = translate_team_update(&mut params)?,
        "team.move" => {
            rename_key(&mut params, "team", "team_id");
            rename_key(&mut params, "parent_team_id", "owner_team_id");
        }
        "team.delete" => rename_key(&mut params, "team", "team_id"),
        "project.create" => {
            params
                .entry("project_id")
                .or_insert_with(|| json!(svc.next_uuid_v7()));
            if !params.contains_key("owner_team_id") {
                params
                    .entry("create_team_id")
                    .or_insert_with(|| json!(svc.next_uuid_v7()));
            }
        }
        "project.add_team" => {
            params
                .entry("project_team_id")
                .or_insert_with(|| json!(svc.next_uuid_v7()));
        }
        "member.add_person" | "member.add_agent" => {
            params
                .entry("member_id")
                .or_insert_with(|| json!(svc.next_uuid_v7()));
        }
        "member.update_person"
        | "member.update_agent"
        | "member.move"
        | "member.set_reports_to"
        | "member.set_agent_status"
        | "member.offboard"
        | "member.remove" => rename_key(&mut params, "member", "member_id"),
        _ => {}
    }
    Ok((wire_method, params))
}

fn translate_team_update(params: &mut Map<String, Value>) -> Result<&'static str, ApiError> {
    rename_key(params, "team", "team_id");
    let has_name = params.contains_key("name");
    let has_lead = params.contains_key("lead_member_id");
    let clear_lead = params
        .remove("clear_lead")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if has_name && (has_lead || clear_lead) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "rename and lead change cannot be combined in one team update",
        ));
    }
    if has_lead && clear_lead {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "`lead_member_id` and `clear_lead` cannot both be set",
        ));
    }
    if has_name {
        Ok("team.rename")
    } else if has_lead || clear_lead {
        Ok("team.set_lead")
    } else {
        Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "team update requires `name`, `lead_member_id`, or `clear_lead`",
        ))
    }
}

fn rename_key(params: &mut Map<String, Value>, from: &str, to: &str) {
    if let Some(val) = params.remove(from) {
        params.insert(to.into(), val);
    }
}
