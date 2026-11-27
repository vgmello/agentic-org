//! `person inspect|add|update|move|deactivate`.

use org_command::{Answer, Request};
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use super::{send_write, ConflictTarget};
use crate::runtime::card::inspect_member;
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{entity_id, opt_bool, opt_str, require_if_match, require_str};
use crate::runtime::transport::{OfficeTransport, Refusal};

/// `Use when:` guidance of each `person` command.
pub(super) const CUES: &[CommandCue] = &[
    ("person", "add", "onboarding a new human member into the organization and optional home team."),
    ("person", "deactivate", "deactivating a departing human member in the organization (requires `--if-match`)."),
    ("person", "inspect", "looking up one person's directory card, handle, presence, and team memberships."),
    ("person", "move", "moving a person to a different home team (requires `--if-match`)."),
    ("person", "update", "updating a person's display name, contact info, manager, or team title (requires `--if-match`)."),
];

/// Runs a `person` command.
pub(super) fn dispatch<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    Some(match request.command.verb {
        "inspect" => inspect_member(transport, request, "person"),
        "add" => write_person_add(transport, request),
        "update" => write_person_update(transport, request),
        "move" => write_person_move(transport, request),
        "deactivate" => write_person_deactivate(transport, request),
        _ => return None,
    })
}

fn write_person_add<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let name = require_str(request, "name")?;
    let member_id = entity_id(request, Some("member_id"), "member.add_person")?;
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

fn person_update_params(
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

fn write_person_update<T: OfficeTransport + ?Sized>(
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

fn write_person_move<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let member_id = require_str(request, "member")?;
    let to_team_id = require_str(request, "to_team_id")?;
    let expected = require_if_match(request)?;
    let new_membership_id = entity_id(request, None, "member.move.new_membership")?;
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

fn write_person_deactivate<T: OfficeTransport + ?Sized>(
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
