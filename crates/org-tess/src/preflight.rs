//! Server-side target command resolution and live preflight validation for Chief of Staff
//! `suggestion.create` proposals (`TASK-80.4`).

mod hiring;
mod messaging;

use org_command_catalogue::{Command, MutationPolicy};
use org_office::organization_family::records::EmployeeKind;
use org_office::rules::OrgCommand;
use org_office::CallerAuthority;
use org_proto::{ApiError, ErrorCode, Request};
use serde_json::{json, Map, Value};

use self::hiring::preflight_hire;
use self::messaging::{
    caller_visible_conversations, is_messaging_target_call, preflight_messaging,
};
use super::{unavailable, DelegationAccess as Access, TessService};

/// Uniform refusal text for missing or cross-team hire requests (prevents existence oracles).
pub(super) const HIRE_NOT_VISIBLE: &str = "hire request not found or not visible to caller";

/// V1 allowlisted target RPC methods that Tess may propose for human confirmation.
pub(crate) const ALLOWED_SUGGESTION_CALLS: &[&str] = &[
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

/// Caller identity and live authority evaluated during `suggestion.create` preflight.
pub(super) struct PreflightContext<'a> {
    pub(super) tess_id: &'a str,
    pub(super) caller_id: &'a str,
    pub(super) authority: &'a CallerAuthority,
}

/// Verified preflight result, carrying the resolved wire RPC method persisted on `SuggestionView`.
pub(super) struct PreflightOutcome {
    pub(super) call: String,
    pub(super) note: Option<String>,
}

impl PreflightOutcome {
    pub(super) fn ok(call: &str, note: impl Into<String>) -> Self {
        Self {
            call: call.to_owned(),
            note: Some(note.into()),
        }
    }
}

pub(super) fn resolve_target_command(raw: &str) -> Result<&'static Command, ApiError> {
    let words: Vec<String> = raw.split_whitespace().map(str::to_owned).collect();
    if words.is_empty() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "`command` must name a target `<resource> <verb>` command",
        ));
    }
    let table = org_command_catalogue::runtime_table();
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
    ctx: &PreflightContext<'_>,
    call: &str,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreflightOutcome, ApiError> {
    enforce_delegation_access(svc, ctx.authority, call)?;
    preflight_attachments(svc, ctx, input)?;
    if call.starts_with("hr.hire") {
        return preflight_hire(svc, ctx, call, input, expected_revision);
    }
    if call.starts_with("tasks.") {
        return preflight_task(svc, call, input, expected_revision);
    }
    if is_messaging_target_call(call) {
        return preflight_messaging(svc, ctx, call, input);
    }
    if let Some(outcome) =
        preflight_org_command(svc, ctx.caller_id, call, input, expected_revision)?
    {
        enforce_delegation_access(svc, ctx.authority, &outcome.call)?;
        return Ok(outcome);
    }
    if matches!(
        call,
        "member.agent_update"
            | "member.agent_move"
            | "member.agent_set_status"
            | "member.agent_offboard"
    ) {
        return preflight_agent_target(svc, call, input, expected_revision);
    }
    if call == "team.set_max_members" {
        return preflight_team_target(svc, call, input, expected_revision);
    }
    Ok(PreflightOutcome::ok(call, "schema and authority verified"))
}

fn enforce_delegation_access(
    svc: &TessService,
    authority: &CallerAuthority,
    call: &str,
) -> Result<(), ApiError> {
    match svc.delegation_access(call) {
        Access::Never => Err(ApiError::new(
            ErrorCode::NotAuthorized,
            format!("`{call}` is not allowed through Chief of Staff delegation"),
        )),
        Access::CallerOwner if !authority.is_owner => Err(ApiError::new(
            ErrorCode::NotAuthorized,
            format!("caller is not authorized to perform `{call}`"),
        )),
        Access::CallerHires if !(authority.is_owner || authority.may_hire) => Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "caller does not have hiring authority",
        )),
        Access::Open | Access::CallerOwner | Access::CallerHires => Ok(()),
    }
}

fn has_non_empty_value(val: &Value) -> bool {
    match val {
        Value::Null => false,
        Value::String(s) => !s.trim().is_empty(),
        Value::Array(items) => !items.is_empty(),
        _ => true,
    }
}

fn collect_attachment_ids(input: &Map<String, Value>) -> Result<Vec<String>, ApiError> {
    let mut ids = Vec::new();
    for key in [
        "attachment_id",
        "attachment_ids",
        "add_attachment",
        "add_attachments",
    ] {
        let Some(val) = input.get(key) else {
            continue;
        };
        match val {
            Value::Null => {}
            Value::String(s) if !s.trim().is_empty() => ids.push(s.trim().to_owned()),
            Value::Array(items) => {
                for item in items {
                    let Some(s) = item.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
                        return Err(ApiError::new(
                            ErrorCode::InvalidRequest,
                            format!("`{key}` entries must be non-empty attachment IDs"),
                        ));
                    };
                    ids.push(s.to_owned());
                }
            }
            _ => {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    format!("`{key}` must be an attachment ID string or array of strings"),
                ));
            }
        }
    }
    Ok(ids)
}

/// Enforces `caller ∩ tess` attachment isolation on `suggestion.create` and `suggestion.apply`
/// (Grade-A Security P1-3): forbids proposing raw local file paths (`attach`) and verifies every
/// referenced attachment ID is readable by both caller and Tess and (when conversation-scoped)
/// belongs to a conversation visible to both.
fn preflight_attachments(
    svc: &TessService,
    ctx: &PreflightContext<'_>,
    input: &Map<String, Value>,
) -> Result<(), ApiError> {
    if input.get("attach").is_some_and(has_non_empty_value) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "local file paths (`attach`) cannot be proposed in suggestions; reference an authorized `attachment_id` instead",
        ));
    }
    let ids = collect_attachment_ids(input)?;
    if ids.is_empty() {
        return Ok(());
    }
    let mut visible_convs = None;
    for id in ids {
        svc.office()
            .attachment_inspect(
                ctx.caller_id,
                org_proto::attachments::AttachmentInspectParams {
                    attachment_id: id.clone(),
                },
            )
            .map_err(|e| {
                unavailable(
                    "tess_calls.attachment_inspect",
                    &e,
                    "unable to inspect attachment",
                )
            })??;
        svc.office()
            .attachment_inspect(
                ctx.tess_id,
                org_proto::attachments::AttachmentInspectParams {
                    attachment_id: id.clone(),
                },
            )
            .map_err(|e| {
                unavailable(
                    "tess_calls.attachment_inspect",
                    &e,
                    "unable to inspect attachment",
                )
            })??;
        let conv_id = svc.office().attachment_conversation_id(&id).map_err(|e| {
            unavailable(
                "tess_calls.attachment_conversation",
                &e,
                "unable to verify attachment conversation scope",
            )
        })?;
        if let Some(conv_id) = conv_id {
            let visible = match visible_convs.as_ref() {
                Some(list) => list,
                None => {
                    let list = caller_visible_conversations(svc, ctx.caller_id, ctx.tess_id)?;
                    visible_convs.insert(list)
                }
            };
            if !visible.iter().any(|c| c.id == conv_id) {
                return Err(ApiError::new(
                    ErrorCode::NotAuthorized,
                    "referenced attachment is outside caller and Chief of Staff visibility",
                ));
            }
        }
    }
    Ok(())
}

fn preflight_agent_target(
    svc: &TessService,
    call: &str,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreflightOutcome, ApiError> {
    let member_ref = input
        .get("member")
        .or_else(|| input.get("member_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ApiError::new(ErrorCode::InvalidRequest, "missing target agent member ID")
        })?;
    let snap = svc.office().org_snapshot().map_err(|e| {
        unavailable(
            "tess_calls.org_snapshot",
            &e,
            "unable to inspect organization",
        )
    })?;
    let resolved_id = snap
        .members
        .iter()
        .find(|m| {
            m.id == member_ref
                || m.handle.as_deref() == Some(member_ref)
                || (member_ref.eq_ignore_ascii_case("owner") && m.owner)
        })
        .map(|m| m.id.as_str())
        .unwrap_or(member_ref);
    let employee = svc
        .office()
        .load_employee(resolved_id)
        .map_err(|e| {
            unavailable(
                "tess_calls.load_employee",
                &e,
                "unable to inspect target employee",
            )
        })?
        .ok_or_else(|| {
            ApiError::new(
                ErrorCode::NotFound,
                format!("target member `{member_ref}` not found"),
            )
        })?;
    if employee.kind != EmployeeKind::Agent {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "target member is not an agent",
        ));
    }
    if employee.builtin_key.is_some()
        && matches!(
            call,
            "member.agent_move" | "member.agent_set_status" | "member.agent_offboard"
        )
    {
        let reason = match call {
            "member.agent_move" => "built-in agents cannot be moved out of the top-level team",
            "member.agent_set_status" => "built-in agents cannot be paused or resumed",
            _ => "built-in agents cannot be offboarded",
        };
        return Err(ApiError::new(ErrorCode::NotAuthorized, reason));
    }
    if let Some(expected) = expected_revision {
        if employee.meta.revision != expected {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                format!(
                    "target agent `{}` is at revision {}, not expected revision {expected}",
                    employee.id.as_str(),
                    employee.meta.revision
                ),
            ));
        }
    }
    if call == "member.agent_move" {
        if let Some(to_team) = input.get("to_team_id").and_then(Value::as_str) {
            if !snap.teams.iter().any(|t| t.id == to_team) {
                return Err(ApiError::new(
                    ErrorCode::NotFound,
                    format!("destination team `{to_team}` not found"),
                ));
            }
        }
    }
    Ok(PreflightOutcome::ok(call, "agent target verified"))
}

fn preflight_team_target(
    svc: &TessService,
    call: &str,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreflightOutcome, ApiError> {
    let team_id = input
        .get("team")
        .or_else(|| input.get("team_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::new(ErrorCode::InvalidRequest, "missing target team ID"))?;
    let snap = svc.office().org_snapshot().map_err(|e| {
        unavailable(
            "tess_calls.org_snapshot",
            &e,
            "unable to inspect organization",
        )
    })?;
    let team = snap.teams.iter().find(|t| t.id == team_id).ok_or_else(|| {
        ApiError::new(
            ErrorCode::NotFound,
            format!("target team `{team_id}` not found"),
        )
    })?;
    if let Some(expected) = expected_revision {
        if team.version != expected {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                format!(
                    "target team `{team_id}` is at revision {}, not expected revision {expected}",
                    team.version
                ),
            ));
        }
    }
    if let Some(cap) = input.get("max_members").and_then(Value::as_u64) {
        if !(1..=100).contains(&cap) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "`max_members` must be between 1 and 100",
            ));
        }
    }
    Ok(PreflightOutcome::ok(call, "team target verified"))
}

fn preflight_task(
    svc: &TessService,
    call: &str,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreflightOutcome, ApiError> {
    if call == "tasks.create" {
        if let Some(project) = input
            .get("project_id")
            .or_else(|| input.get("project"))
            .and_then(Value::as_str)
        {
            svc.office().inspect_project(project).map_err(|e| {
                unavailable(
                    "tess_calls.inspect_project",
                    &e,
                    "unable to inspect target project",
                )
            })??;
        }
        if let Some(team) = input
            .get("team_id")
            .or_else(|| input.get("team"))
            .and_then(Value::as_str)
        {
            let snap = svc.office().org_snapshot().map_err(|e| {
                unavailable(
                    "tess_calls.org_snapshot",
                    &e,
                    "unable to inspect target team",
                )
            })?;
            if !snap.teams.iter().any(|t| t.id == team || t.name == team) {
                return Err(ApiError::new(
                    ErrorCode::NotFound,
                    format!("target team `{team}` not found"),
                ));
            }
        }
        return Ok(PreflightOutcome::ok(call, "task creation target verified"));
    }
    let Some(task_ref) = input.get("task").and_then(Value::as_str) else {
        return Ok(PreflightOutcome::ok(call, "schema verified"));
    };
    let inspected = svc.office().task_inspect(task_ref).map_err(|e| {
        unavailable(
            "tess_calls.task_inspect",
            &e,
            "unable to inspect target task",
        )
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
    Ok(PreflightOutcome::ok(
        call,
        format!(
            "verified task `{}` at revision {}",
            inspected.task.key, inspected.task.revision
        ),
    ))
}

fn preflight_org_command(
    svc: &TessService,
    caller_id: &str,
    call: &str,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<Option<PreflightOutcome>, ApiError> {
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
        .map_err(|e| {
            unavailable(
                "tess_calls.check_org",
                &e,
                "unable to run organization preflight check",
            )
        })??;
    Ok(Some(PreflightOutcome::ok(
        wire_method,
        "organization dry-run preflight passed",
    )))
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
