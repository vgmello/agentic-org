//! Direct RPC translators for organization, project, role, HR hiring, and agent apply commands.

use org_proto::hiring::HireProposalWire;
use org_proto::org::{CommandAccepted, CommandStatus};
use org_proto::ApiError;
use serde_json::{json, Map, Value};

use super::{
    insert_opt, invalid, opt_bool, opt_str, require_rev, require_str, unreadable, OutputShape,
    PreparedRpc,
};

pub(super) fn try_prepare(
    next_id: &mut impl FnMut() -> String,
    resource: &str,
    verb: &str,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Option<Result<PreparedRpc, ApiError>> {
    match (resource, verb) {
        ("team", "create") => Some(prep_team_create(next_id, input)),
        ("team", "update") => Some(prep_team_update(next_id, input, expected_revision)),
        ("team", "move") => Some(prep_team_move(next_id, input, expected_revision)),
        ("team", "archive") => Some(prep_team_archive(next_id, input, expected_revision)),
        ("team", "set-limit") => Some(prep_team_set_limit(next_id, input, expected_revision)),
        ("person", "add") => Some(prep_person_add(next_id, input)),
        ("person", "update") => Some(prep_person_update(next_id, input, expected_revision)),
        ("person", "move") => Some(prep_person_move(next_id, input, expected_revision)),
        ("person", "deactivate") => Some(prep_person_deactivate(next_id, input, expected_revision)),
        ("project", "create") => Some(prep_project_create(next_id, input)),
        ("project", "add-team") => Some(prep_project_add_team(next_id, input)),
        ("project", "remove-team") => {
            Some(prep_project_remove_team(next_id, input, expected_revision))
        }
        ("role", "create") => Some(prep_role_create(next_id, input)),
        ("role", "update") => Some(prep_role_update(next_id, input, expected_revision)),
        ("hr", "hire") => Some(prep_hr_hire(next_id, input, expected_revision)),
        ("agent", "update") => Some(prep_agent_update(next_id, input, expected_revision)),
        ("agent", "move") => Some(prep_agent_move(next_id, input, expected_revision)),
        ("agent", "pause") => Some(prep_agent_status(next_id, input, expected_revision, true)),
        ("agent", "resume") => Some(prep_agent_status(next_id, input, expected_revision, false)),
        ("agent", "offboard") => Some(prep_agent_offboard(next_id, input, expected_revision)),
        _ => None,
    }
}

pub(super) fn shape_org_accepted(
    method: &str,
    target_id: &str,
    raw: Value,
) -> Result<Value, ApiError> {
    let accepted: CommandAccepted = serde_json::from_value(raw).map_err(|_| unreadable(method))?;
    let status = match accepted.status {
        CommandStatus::Committed => "committed",
        CommandStatus::Checked => "checked",
        CommandStatus::Queued => "queued",
        _ => return Err(unreadable(method)),
    };
    let mut data = Map::new();
    data.insert("id".into(), json!(target_id));
    data.insert("status".into(), json!(status));
    if let Some(pos) = &accepted.position {
        data.insert(
            "position".into(),
            json!({ "epoch": pos.epoch, "seq": pos.seq }),
        );
    }
    Ok(Value::Object(data))
}

fn id_or_new(
    input: &Map<String, Value>,
    key: &str,
    next_id: &mut impl FnMut() -> String,
) -> String {
    opt_str(input, key)
        .map(str::to_owned)
        .unwrap_or_else(next_id)
}

fn prep_team_create(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
) -> Result<PreparedRpc, ApiError> {
    let name = require_str(input, "name")?;
    let team_id = id_or_new(input, "team_id", next_id);
    let mut params = Map::new();
    params.insert("command_id".into(), json!(next_id()));
    params.insert("team_id".into(), json!(team_id));
    params.insert("name".into(), json!(name));
    insert_opt(
        &mut params,
        "owner_team_id",
        opt_str(input, "parent_team_id"),
    );
    Ok(PreparedRpc {
        method: "team.create",
        params: Value::Object(params),
        shape: OutputShape::OrgAccepted { target_id: team_id },
    })
}

fn prep_team_update(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let team_id = require_str(input, "team")?;
    let expected = require_rev(expected_revision)?;
    let name = opt_str(input, "name");
    let lead = opt_str(input, "lead_member_id");
    let clear_lead = opt_bool(input, "clear_lead").unwrap_or(false);
    let wants_lead = lead.is_some() || clear_lead;
    if name.is_some() && wants_lead {
        return Err(invalid(
            "rename and lead change cannot be combined in one team update",
        ));
    }
    if lead.is_some() && clear_lead {
        return Err(invalid(
            "`lead_member_id` and `clear_lead` cannot both be set",
        ));
    }
    let mut params = Map::new();
    params.insert("command_id".into(), json!(next_id()));
    params.insert("team_id".into(), json!(team_id));
    params.insert("expected_version".into(), json!(expected));
    let method = if let Some(new_name) = name {
        params.insert("name".into(), json!(new_name));
        "team.rename"
    } else if wants_lead {
        insert_opt(&mut params, "lead_member_id", lead);
        "team.set_lead"
    } else {
        return Err(invalid(
            "team update requires `name`, `lead_member_id`, or `clear_lead`",
        ));
    };
    Ok(PreparedRpc {
        method,
        params: Value::Object(params),
        shape: OutputShape::OrgAccepted {
            target_id: team_id.to_owned(),
        },
    })
}

fn prep_team_move(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let team_id = require_str(input, "team")?;
    let parent = require_str(input, "parent_team_id")?;
    let expected = require_rev(expected_revision)?;
    Ok(PreparedRpc {
        method: "team.move",
        params: json!({
            "command_id": next_id(),
            "team_id": team_id,
            "expected_version": expected,
            "owner_team_id": parent,
        }),
        shape: OutputShape::OrgAccepted {
            target_id: team_id.to_owned(),
        },
    })
}

fn prep_team_archive(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let team_id = require_str(input, "team")?;
    let expected = require_rev(expected_revision)?;
    Ok(PreparedRpc {
        method: "team.delete",
        params: json!({
            "command_id": next_id(),
            "team_id": team_id,
            "expected_version": expected,
        }),
        shape: OutputShape::OrgAccepted {
            target_id: team_id.to_owned(),
        },
    })
}

fn prep_team_set_limit(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let team_id = require_str(input, "team")?;
    let max_members = input
        .get("max_members")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid("missing required integer `max_members`"))?;
    let expected = require_rev(expected_revision)?;
    Ok(PreparedRpc {
        method: "team.set_max_members",
        params: json!({
            "command_id": next_id(),
            "team_id": team_id,
            "expected_version": expected,
            "max_members": max_members,
        }),
        shape: OutputShape::Passthrough,
    })
}

fn prep_person_add(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
) -> Result<PreparedRpc, ApiError> {
    let name = require_str(input, "name")?;
    let member_id = id_or_new(input, "member_id", next_id);
    let mut params = Map::new();
    params.insert("command_id".into(), json!(next_id()));
    params.insert("member_id".into(), json!(member_id));
    params.insert("name".into(), json!(name));
    insert_opt(&mut params, "handle", opt_str(input, "handle"));
    insert_opt(&mut params, "home_team_id", opt_str(input, "home_team_id"));
    insert_opt(&mut params, "contact", opt_str(input, "contact"));
    Ok(PreparedRpc {
        method: "member.add_person",
        params: Value::Object(params),
        shape: OutputShape::OrgAccepted {
            target_id: member_id,
        },
    })
}

fn prep_person_update(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let member_id = require_str(input, "member")?;
    let expected = require_rev(expected_revision)?;
    let name = opt_str(input, "name");
    let contact = opt_str(input, "contact");
    let clear_contact = opt_bool(input, "clear_contact").unwrap_or(false);
    let reports_to = opt_str(input, "reports_to_member_id");
    let clear_reports_to = opt_bool(input, "clear_reports_to").unwrap_or(false);
    let title_in_team = opt_str(input, "title_in_team");

    let wants_profile = name.is_some() || contact.is_some() || clear_contact;
    let wants_manager = reports_to.is_some() || clear_reports_to;
    let wants_title = title_in_team.is_some();
    match usize::from(wants_profile) + usize::from(wants_manager) + usize::from(wants_title) {
        0 => return Err(invalid("nothing to update on this person")),
        1 => {}
        _ => {
            return Err(invalid(
                "profile, reporting manager, and team title updates must be run separately",
            ))
        }
    }

    let mut params = Map::new();
    params.insert("command_id".into(), json!(next_id()));
    params.insert("expected_version".into(), json!(expected));
    let method = if wants_profile {
        params.insert("member_id".into(), json!(member_id));
        insert_opt(&mut params, "name", name);
        insert_opt(&mut params, "contact", contact);
        if clear_contact {
            params.insert("clear_contact".into(), json!(true));
        }
        "member.update_person"
    } else if wants_manager {
        if reports_to.is_some() && clear_reports_to {
            return Err(invalid(
                "`reports_to_member_id` and `clear_reports_to` cannot both be set",
            ));
        }
        params.insert("member_id".into(), json!(member_id));
        insert_opt(&mut params, "reports_to_member_id", reports_to);
        "member.set_reports_to"
    } else {
        params.insert("membership_id".into(), json!(member_id));
        insert_opt(&mut params, "title_in_team", title_in_team);
        "team.set_title"
    };
    Ok(PreparedRpc {
        method,
        params: Value::Object(params),
        shape: OutputShape::OrgAccepted {
            target_id: member_id.to_owned(),
        },
    })
}

fn prep_person_move(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let member_id = require_str(input, "member")?;
    let to_team_id = require_str(input, "to_team_id")?;
    let expected = require_rev(expected_revision)?;
    Ok(PreparedRpc {
        method: "member.move",
        params: json!({
            "command_id": next_id(),
            "membership_id": member_id,
            "expected_version": expected,
            "new_membership_id": next_id(),
            "to_team_id": to_team_id,
        }),
        shape: OutputShape::OrgAccepted {
            target_id: member_id.to_owned(),
        },
    })
}

fn prep_person_deactivate(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let member_id = require_str(input, "member")?;
    let expected = require_rev(expected_revision)?;
    Ok(PreparedRpc {
        method: "member.remove",
        params: json!({
            "command_id": next_id(),
            "member_id": member_id,
            "expected_version": expected,
        }),
        shape: OutputShape::OrgAccepted {
            target_id: member_id.to_owned(),
        },
    })
}

fn prep_project_create(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
) -> Result<PreparedRpc, ApiError> {
    let name = require_str(input, "name")?;
    let project_id = id_or_new(input, "project_id", next_id);
    let mut params = Map::new();
    params.insert("command_id".into(), json!(next_id()));
    params.insert("project_id".into(), json!(project_id));
    params.insert("name".into(), json!(name));
    insert_opt(&mut params, "slug", opt_str(input, "slug"));
    if let Some(owner_team) = opt_str(input, "owner_team_id") {
        params.insert("owner_team_id".into(), json!(owner_team));
    } else {
        let create_team = id_or_new(input, "create_team_id", next_id);
        params.insert("create_team_id".into(), json!(create_team));
    }
    insert_opt(
        &mut params,
        "parent_team_id",
        opt_str(input, "parent_team_id"),
    );
    if let Some(contrib) = input.get("contributing_team_ids") {
        params.insert("contributing_team_ids".into(), contrib.clone());
    }
    Ok(PreparedRpc {
        method: "project.create",
        params: Value::Object(params),
        shape: OutputShape::OrgAccepted {
            target_id: project_id,
        },
    })
}

fn prep_project_add_team(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
) -> Result<PreparedRpc, ApiError> {
    let project_id = require_str(input, "project_id")?;
    let team_id = require_str(input, "team_id")?;
    let project_team_id = id_or_new(input, "project_team_id", next_id);
    Ok(PreparedRpc {
        method: "project.add_team",
        params: json!({
            "command_id": next_id(),
            "project_team_id": project_team_id,
            "project_id": project_id,
            "team_id": team_id,
        }),
        shape: OutputShape::OrgAccepted {
            target_id: project_team_id,
        },
    })
}

fn prep_project_remove_team(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let project_team_id = require_str(input, "project_team_id")?;
    let expected = require_rev(expected_revision)?;
    let mut params = Map::new();
    params.insert("command_id".into(), json!(next_id()));
    params.insert("project_team_id".into(), json!(project_team_id));
    params.insert("expected_version".into(), json!(expected));
    insert_opt(
        &mut params,
        "areas_to_team_id",
        opt_str(input, "areas_to_team_id"),
    );
    Ok(PreparedRpc {
        method: "project.remove_team",
        params: Value::Object(params),
        shape: OutputShape::OrgAccepted {
            target_id: project_team_id.to_owned(),
        },
    })
}

fn prep_role_create(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
) -> Result<PreparedRpc, ApiError> {
    let name = require_str(input, "name")?;
    let role_id = id_or_new(input, "role_id", next_id);
    let mut params = Map::new();
    params.insert("command_id".into(), json!(next_id()));
    params.insert("role_id".into(), json!(role_id));
    params.insert("name".into(), json!(name));
    insert_opt(&mut params, "key", opt_str(input, "key"));
    insert_opt(&mut params, "body", opt_str(input, "body"));
    Ok(PreparedRpc {
        method: "role.create",
        params: Value::Object(params),
        shape: OutputShape::Passthrough,
    })
}

fn prep_role_update(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let role_id = require_str(input, "role")?;
    let expected = require_rev(expected_revision)?;
    let mut params = Map::new();
    params.insert("command_id".into(), json!(next_id()));
    params.insert("role_id".into(), json!(role_id));
    params.insert("expected_version".into(), json!(expected));
    insert_opt(&mut params, "name", opt_str(input, "name"));
    insert_opt(&mut params, "key", opt_str(input, "key"));
    if let Some(clear_key) = opt_bool(input, "clear_key") {
        params.insert("clear_key".into(), json!(clear_key));
    }
    insert_opt(&mut params, "body", opt_str(input, "body"));
    Ok(PreparedRpc {
        method: "role.update",
        params: Value::Object(params),
        shape: OutputShape::Passthrough,
    })
}

fn prep_hr_hire(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let is_draft = opt_bool(input, "draft").unwrap_or(false);
    let is_withdraw = opt_bool(input, "withdraw").unwrap_or(false);
    if let Some(existing_id) = opt_str(input, "id") {
        if is_draft {
            return Err(invalid("`draft` cannot be combined with `id`"));
        }
        let expected = require_rev(expected_revision)?;
        if is_withdraw {
            let closed_reason = opt_str(input, "closed_reason")
                .or_else(|| opt_str(input, "reason"))
                .ok_or_else(|| invalid("missing required `closed_reason`"))?;
            return Ok(PreparedRpc {
                method: "hr.hire.withdraw",
                params: json!({
                    "command_id": next_id(),
                    "request_id": existing_id,
                    "expected_version": expected,
                    "closed_reason": closed_reason,
                }),
                shape: OutputShape::Passthrough,
            });
        }
        let employee_id = id_or_new(input, "employee_id", next_id);
        return Ok(PreparedRpc {
            method: "hr.hire.confirm",
            params: json!({
                "command_id": next_id(),
                "request_id": existing_id,
                "employee_id": employee_id,
                "expected_version": expected,
            }),
            shape: OutputShape::Passthrough,
        });
    }
    if is_withdraw {
        return Err(invalid("`withdraw` requires `id`"));
    }
    let team = require_str(input, "team")?;
    let reason = require_str(input, "reason")?;
    let proposal = build_hire_proposal(input)?;
    let request_id = id_or_new(input, "request_id", next_id);
    let mut params = Map::new();
    params.insert("command_id".into(), json!(next_id()));
    params.insert("request_id".into(), json!(request_id));
    params.insert("home_team_id".into(), json!(team));
    insert_opt(&mut params, "role_id", opt_str(input, "role_id"));
    params.insert(
        "proposal".into(),
        serde_json::to_value(&proposal).map_err(|_| invalid("invalid hire proposal"))?,
    );
    params.insert("reason".into(), json!(reason));
    let method = if is_draft {
        "hr.hire.draft"
    } else {
        params.insert(
            "employee_id".into(),
            json!(id_or_new(input, "employee_id", next_id)),
        );
        "hr.hire"
    };
    Ok(PreparedRpc {
        method,
        params: Value::Object(params),
        shape: OutputShape::Passthrough,
    })
}

fn build_hire_proposal(input: &Map<String, Value>) -> Result<HireProposalWire, ApiError> {
    Ok(HireProposalWire {
        name: require_str(input, "name")?.to_owned(),
        handle: require_str(input, "handle")?.to_owned(),
        title: opt_str(input, "title").map(str::to_owned),
        focus: opt_str(input, "focus").map(str::to_owned),
        role_text: opt_str(input, "role_text").map(str::to_owned),
        title_in_team: opt_str(input, "title_in_team").map(str::to_owned),
        reports_to_employee_id: opt_str(input, "reports_to_member_id").map(str::to_owned),
        office_id: opt_str(input, "office_id").map(str::to_owned),
        model_profile_id: opt_str(input, "model_profile_id").map(str::to_owned),
        model_id: opt_str(input, "model_id").map(str::to_owned),
        permission_preset: opt_str(input, "permission_preset").map(str::to_owned),
        effort: opt_str(input, "effort").map(str::to_owned),
        tools_config: Vec::new(),
        system_prompt_markdown: opt_str(input, "system_prompt_markdown").map(str::to_owned),
        body: opt_str(input, "body").map(str::to_owned),
    })
}

fn prep_agent_update(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let member_id = require_str(input, "member")?;
    let expected = require_rev(expected_revision)?;
    let mut params = Map::new();
    params.insert("command_id".into(), json!(next_id()));
    params.insert("member_id".into(), json!(member_id));
    params.insert("expected_version".into(), json!(expected));
    for field in [
        "name",
        "role_id",
        "reports_to_member_id",
        "title",
        "focus",
        "role_text",
        "title_in_team",
        "body",
    ] {
        insert_opt(&mut params, field, opt_str(input, field));
    }
    for flag in [
        "clear_role_id",
        "clear_reports_to",
        "clear_title",
        "clear_focus",
        "clear_role_text",
        "clear_title_in_team",
    ] {
        if let Some(val) = opt_bool(input, flag) {
            params.insert(flag.into(), json!(val));
        }
    }
    Ok(PreparedRpc {
        method: "member.agent_update",
        params: Value::Object(params),
        shape: OutputShape::Passthrough,
    })
}

fn prep_agent_move(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let member_id = require_str(input, "member")?;
    let to_team_id = require_str(input, "to_team_id")?;
    let expected = require_rev(expected_revision)?;
    Ok(PreparedRpc {
        method: "member.agent_move",
        params: json!({
            "command_id": next_id(),
            "member_id": member_id,
            "expected_version": expected,
            "to_team_id": to_team_id,
        }),
        shape: OutputShape::Passthrough,
    })
}

fn prep_agent_status(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
    paused: bool,
) -> Result<PreparedRpc, ApiError> {
    let member_id = require_str(input, "member")?;
    let expected = require_rev(expected_revision)?;
    Ok(PreparedRpc {
        method: "member.agent_set_status",
        params: json!({
            "command_id": next_id(),
            "member_id": member_id,
            "expected_version": expected,
            "paused": paused,
        }),
        shape: OutputShape::Passthrough,
    })
}

fn prep_agent_offboard(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let member_id = require_str(input, "member")?;
    let expected = require_rev(expected_revision)?;
    Ok(PreparedRpc {
        method: "member.agent_offboard",
        params: json!({
            "command_id": next_id(),
            "member_id": member_id,
            "expected_version": expected,
        }),
        shape: OutputShape::Passthrough,
    })
}
