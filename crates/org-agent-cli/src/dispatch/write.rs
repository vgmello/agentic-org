//! Team, person and project writes.

use org_command::{Answer, Request};
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use super::{
    derived_or_fresh_id, entity_id, opt_bool, opt_str, require_if_match, require_str, send_write,
    ConflictTarget,
};
use crate::transport::{OfficeTransport, Refusal};

pub(super) fn write_team_create<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let name = require_str(request, "name")?;
    let team_id = entity_id(request, "team_id", "team.create")?;
    let mut params = Map::new();
    params.insert("team_id".into(), json!(team_id));
    params.insert("name".into(), json!(name));
    if let Some(parent) = opt_str(request, "parent_team_id") {
        params.insert("owner_team_id".into(), json!(parent));
    }
    send_write(transport, request, "team.create", params, &team_id, None)
}

pub(super) fn write_team_update<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let team_id = require_str(request, "team")?;
    let expected = require_if_match(request)?;
    let name = opt_str(request, "name");
    let lead = opt_str(request, "lead_member_id");
    let clear_lead = opt_bool(request, "clear_lead").unwrap_or(false);
    let wants_lead = lead.is_some() || clear_lead;
    if name.is_some() && wants_lead {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "Rename and lead change cannot be combined in one team update.",
            "Run `team update --name ...` and `team update --lead-member-id ...` separately.",
        ));
    }
    if lead.is_some() && clear_lead {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "--lead-member-id and --clear-lead cannot both be set.",
            "Pass either --lead-member-id <id> or --clear-lead.",
        ));
    }
    let mut params = Map::new();
    params.insert("team_id".into(), json!(team_id));
    params.insert("expected_version".into(), json!(expected));
    let method = if let Some(new_name) = name {
        params.insert("name".into(), json!(new_name));
        "team.rename"
    } else if wants_lead {
        if let Some(lead_id) = lead {
            params.insert("lead_member_id".into(), json!(lead_id));
        }
        "team.set_lead"
    } else {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "Nothing to update on this team.",
            "Pass --name, --lead-member-id, or --clear-lead.",
        ));
    };
    send_write(
        transport,
        request,
        method,
        params,
        team_id,
        Some(ConflictTarget::Team(team_id)),
    )
}

pub(super) fn write_team_move<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let team_id = require_str(request, "team")?;
    let expected = require_if_match(request)?;
    let mut params = Map::new();
    params.insert("team_id".into(), json!(team_id));
    params.insert("expected_version".into(), json!(expected));
    let parent = require_str(request, "parent_team_id")?;
    params.insert("owner_team_id".into(), json!(parent));
    send_write(
        transport,
        request,
        "team.move",
        params,
        team_id,
        Some(ConflictTarget::Team(team_id)),
    )
}

pub(super) fn write_team_archive<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let team_id = require_str(request, "team")?;
    let expected = require_if_match(request)?;
    let mut params = Map::new();
    params.insert("team_id".into(), json!(team_id));
    params.insert("expected_version".into(), json!(expected));
    send_write(
        transport,
        request,
        "team.delete",
        params,
        team_id,
        Some(ConflictTarget::Team(team_id)),
    )
}

pub(super) fn write_person_add<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let name = require_str(request, "name")?;
    let member_id = entity_id(request, "member_id", "member.add_person")?;
    let mut params = Map::new();
    params.insert("member_id".into(), json!(member_id));
    params.insert("name".into(), json!(name));
    if let Some(handle) = opt_str(request, "handle") {
        params.insert("handle".into(), json!(handle));
    }
    if let Some(home) = opt_str(request, "home_team_id") {
        params.insert("home_team_id".into(), json!(home));
    }
    if let Some(contact) = opt_str(request, "contact") {
        params.insert("contact".into(), json!(contact));
    }
    send_write(
        transport,
        request,
        "member.add_person",
        params,
        &member_id,
        None,
    )
}

pub(super) fn person_update_params(
    request: &Request,
    member_id: &str,
    expected: u64,
) -> Result<(&'static str, Map<String, Value>), Refusal> {
    let name = opt_str(request, "name");
    let contact = opt_str(request, "contact");
    let clear_contact = opt_bool(request, "clear_contact").unwrap_or(false);
    let reports_to = opt_str(request, "reports_to_member_id");
    let clear_reports_to = opt_bool(request, "clear_reports_to").unwrap_or(false);
    let title_in_team = opt_str(request, "title_in_team");

    let wants_profile = name.is_some() || contact.is_some() || clear_contact;
    let wants_manager = reports_to.is_some() || clear_reports_to;
    let wants_title = title_in_team.is_some();
    match usize::from(wants_profile) + usize::from(wants_manager) + usize::from(wants_title) {
        0 => {
            return Err(Refusal::new(
                ErrorCode::InvalidRequest,
                "Nothing to update on this person.",
                "Pass --name/--contact/--clear-contact, --reports-to-member-id/--clear-reports-to, or --title-in-team.",
            ))
        }
        1 => {}
        _ => {
            return Err(Refusal::new(
                ErrorCode::InvalidRequest,
                "Profile, reporting manager, and team title updates must be run separately.",
                "Update one group of fields per `person update` call.",
            ))
        }
    }

    let mut params = Map::new();
    params.insert("expected_version".into(), json!(expected));
    if wants_profile {
        params.insert("member_id".into(), json!(member_id));
        if let Some(n) = name {
            params.insert("name".into(), json!(n));
        }
        if let Some(c) = contact {
            params.insert("contact".into(), json!(c));
        }
        if clear_contact {
            params.insert("clear_contact".into(), json!(true));
        }
        return Ok(("member.update_person", params));
    }
    if wants_manager {
        if reports_to.is_some() && clear_reports_to {
            return Err(Refusal::new(
                ErrorCode::InvalidRequest,
                "--reports-to-member-id and --clear-reports-to cannot both be set.",
                "Pass either --reports-to-member-id <id> or --clear-reports-to.",
            ));
        }
        params.insert("member_id".into(), json!(member_id));
        if let Some(mgr) = reports_to {
            params.insert("reports_to_member_id".into(), json!(mgr));
        }
        return Ok(("member.set_reports_to", params));
    }
    params.insert("membership_id".into(), json!(member_id));
    if let Some(title) = title_in_team {
        params.insert("title_in_team".into(), json!(title));
    }
    Ok(("team.set_title", params))
}

pub(super) fn write_person_update<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let member_id = require_str(request, "member")?;
    let expected = require_if_match(request)?;
    let (method, params) = person_update_params(request, member_id, expected)?;
    send_write(
        transport,
        request,
        method,
        params,
        member_id,
        Some(ConflictTarget::Member(member_id)),
    )
}

pub(super) fn write_person_move<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let member_id = require_str(request, "member")?;
    let to_team_id = require_str(request, "to_team_id")?;
    let expected = require_if_match(request)?;
    let new_membership_id = derived_or_fresh_id(request, "member.move.new_membership");
    let mut params = Map::new();
    params.insert("membership_id".into(), json!(member_id));
    params.insert("expected_version".into(), json!(expected));
    params.insert("new_membership_id".into(), json!(new_membership_id));
    params.insert("to_team_id".into(), json!(to_team_id));
    send_write(
        transport,
        request,
        "member.move",
        params,
        member_id,
        Some(ConflictTarget::Member(member_id)),
    )
}

pub(super) fn write_person_deactivate<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let member_id = require_str(request, "member")?;
    let expected = require_if_match(request)?;
    let mut params = Map::new();
    params.insert("member_id".into(), json!(member_id));
    params.insert("expected_version".into(), json!(expected));
    send_write(
        transport,
        request,
        "member.remove",
        params,
        member_id,
        Some(ConflictTarget::Member(member_id)),
    )
}

pub(super) fn write_project_create<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let name = require_str(request, "name")?;
    let project_id = entity_id(request, "project_id", "project.create.id")?;
    let mut params = Map::new();
    params.insert("project_id".into(), json!(project_id));
    params.insert("name".into(), json!(name));
    if let Some(slug) = opt_str(request, "slug") {
        params.insert("slug".into(), json!(slug));
    }
    if let Some(owner_team) = opt_str(request, "owner_team_id") {
        params.insert("owner_team_id".into(), json!(owner_team));
    } else {
        let create_team = entity_id(request, "create_team_id", "project.create.owner_team")?;
        params.insert("create_team_id".into(), json!(create_team));
    }
    if let Some(parent_team) = opt_str(request, "parent_team_id") {
        params.insert("parent_team_id".into(), json!(parent_team));
    }
    if let Some(contrib) = request.input.get("contributing_team_ids") {
        params.insert("contributing_team_ids".into(), contrib.clone());
    }
    send_write(
        transport,
        request,
        "project.create",
        params,
        &project_id,
        None,
    )
}
