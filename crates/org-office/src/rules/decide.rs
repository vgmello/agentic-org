use std::collections::HashSet;

use org_proto::org::{
    AgentStatus, Member, MemberAddPersonParams, MemberKind, MemberRemoveParams,
    MemberSetReportsToParams, MemberUpdatePersonParams, Membership, OrgChange,
    ProjectAddTeamParams, ProjectRemoveTeamParams, ProjectTeam, Team, TeamAddMemberParams,
    TeamCreateParams, TeamDeleteParams, TeamMoveParams, TeamRemoveMemberParams, TeamRenameParams,
    TeamSetLeadParams, TeamSetTitleParams, AGENT_TEXT_MAX_CHARS, OWNER_HANDLE,
};
use org_proto::{ApiError, ErrorCode, OrganizationName};

use super::command::OrgCommand;
use super::view::OrgView;
use super::{fail, ID_MAX_CHARS};

type Decision = Result<Vec<OrgChange>, ApiError>;

/// Decides one command against the current state: the resolved changes it makes, or the error
/// HQ answers with. `now` is HQ's clock (RFC 3339 UTC with milliseconds). Changes nothing.
pub fn decide(view: &dyn OrgView, now: &str, command: &OrgCommand) -> Decision {
    match command {
        OrgCommand::TeamCreate(p) => team_create(view, p),
        OrgCommand::TeamRename(p) => team_rename(view, p),
        OrgCommand::TeamMove(p) => team_move(view, p),
        OrgCommand::TeamSetLead(p) => team_set_lead(view, p),
        OrgCommand::TeamDelete(p) => team_delete(view, p),
        OrgCommand::TeamAddMember(p) => team_add_member(view, p),
        OrgCommand::TeamRemoveMember(p) => team_remove_member(view, p),
        OrgCommand::TeamSetTitle(p) => team_set_title(view, p),
        OrgCommand::MemberAddPerson(p) => member_add_person(view, now, p),
        OrgCommand::MemberUpdatePerson(p) => member_update_person(view, p),
        OrgCommand::MemberRemove(p) => member_remove(view, p),
        OrgCommand::MemberSetReportsTo(p) => member_set_reports_to(view, p),
        OrgCommand::ProjectAddTeam(p) => project_add_team(view, p),
        OrgCommand::ProjectRemoveTeam(p) => project_remove_team(view, p),
    }
}

/// The first entry of a new organization's log: its owner, a person who is in no team.
/// `member_id` is made by the office; the app sets the real name later with
/// `member.update_person`.
pub fn owner_bootstrap(member_id: &str, name: &str, now: &str) -> Decision {
    check_id(member_id)?;
    let name = parse_name(name)?;
    Ok(vec![OrgChange::MemberUpserted {
        member: Member {
            id: member_id.to_owned(),
            kind: MemberKind::Person,
            name: name.into_string(),
            contact: None,
            owner: true,
            office_id: None,
            agent: None,
            reports_to_member_id: None,
            created_at: Some(now.to_owned()),
            version: 1,
        },
    }])
}

// ---- teams ----------------------------------------------------------------------------

fn team_create(view: &dyn OrgView, p: &TeamCreateParams) -> Decision {
    if let Some(owner) = &p.owner_team_id {
        found(view.team(owner), "no such owner team")?;
    }
    check_id(&p.team_id)?;
    let name = parse_name(&p.name)?;
    if view.team(&p.team_id).is_some() {
        return Err(fail(ErrorCode::AlreadyExists, "that team id is taken"));
    }
    if view
        .team_named(p.owner_team_id.as_deref(), name.key())
        .is_some()
    {
        return Err(sibling_clash());
    }
    Ok(vec![OrgChange::TeamUpserted {
        team: Team {
            id: p.team_id.clone(),
            name: name.into_string(),
            owner_team_id: p.owner_team_id.clone(),
            lead_member_id: None,
            version: 1,
        },
    }])
}

fn team_rename(view: &dyn OrgView, p: &TeamRenameParams) -> Decision {
    let team = found(view.team(&p.team_id), "no such team")?;
    let name = parse_name(&p.name)?;
    check_version(team.version, p.expected_version)?;
    if view
        .team_named(team.owner_team_id.as_deref(), name.key())
        .is_some_and(|other| other.id != team.id)
    {
        return Err(sibling_clash());
    }
    Ok(vec![OrgChange::TeamUpserted {
        team: Team {
            name: name.into_string(),
            version: team.version + 1,
            ..team
        },
    }])
}

fn team_move(view: &dyn OrgView, p: &TeamMoveParams) -> Decision {
    let team = found(view.team(&p.team_id), "no such team")?;
    if let Some(owner) = &p.owner_team_id {
        found(view.team(owner), "no such owner team")?;
        if owns_or_is(view, &team.id, owner) {
            return Err(fail(
                ErrorCode::Cycle,
                "a team cannot be moved under itself or a team it owns",
            ));
        }
    }
    check_version(team.version, p.expected_version)?;
    let key = OrganizationName::parse(&team.name)
        .map(|n| n.key().to_owned())
        .unwrap_or_else(|_| team.name.to_lowercase());
    if view
        .team_named(p.owner_team_id.as_deref(), &key)
        .is_some_and(|other| other.id != team.id)
    {
        return Err(sibling_clash());
    }
    Ok(vec![OrgChange::TeamUpserted {
        team: Team {
            owner_team_id: p.owner_team_id.clone(),
            version: team.version + 1,
            ..team
        },
    }])
}

fn team_set_lead(view: &dyn OrgView, p: &TeamSetLeadParams) -> Decision {
    let team = found(view.team(&p.team_id), "no such team")?;
    if let Some(lead_id) = &p.lead_member_id {
        let lead = found(view.member(lead_id), "no such member")?;
        refuse_offboarded(&lead)?;
        if lead.owner {
            return Err(owner_protected("the owner cannot lead a team"));
        }
        if view.membership_of(&team.id, &lead.id).is_none() {
            return Err(fail(
                ErrorCode::NotAMember,
                "a team's lead must be a member of the team",
            ));
        }
    }
    check_version(team.version, p.expected_version)?;
    Ok(vec![OrgChange::TeamUpserted {
        team: Team {
            lead_member_id: p.lead_member_id.clone(),
            version: team.version + 1,
            ..team
        },
    }])
}

fn team_delete(view: &dyn OrgView, p: &TeamDeleteParams) -> Decision {
    let team = found(view.team(&p.team_id), "no such team")?;
    if !view.teams_owned_by(&team.id).is_empty() {
        return Err(fail(
            ErrorCode::NotEmpty,
            "the team owns other teams; move or delete them first",
        ));
    }
    check_version(team.version, p.expected_version)?;
    let mut changes: Vec<OrgChange> = view
        .memberships_of_team(&team.id)
        .into_iter()
        .map(|m| OrgChange::MembershipRemoved {
            membership_id: m.id,
        })
        .collect();
    changes.extend(
        view.project_teams_of_team(&team.id)
            .into_iter()
            .map(|link| OrgChange::ProjectTeamRemoved {
                project_team_id: link.id,
            }),
    );
    changes.push(OrgChange::TeamDeleted { team_id: team.id });
    Ok(changes)
}

fn team_add_member(view: &dyn OrgView, p: &TeamAddMemberParams) -> Decision {
    found(view.team(&p.team_id), "no such team")?;
    let member = found(view.member(&p.member_id), "no such member")?;
    check_id(&p.membership_id)?;
    refuse_offboarded(&member)?;
    let title = check_text(p.title_in_team.as_deref(), "title_in_team")?;
    if member.owner {
        return Err(owner_protected("the owner is not a member of any team"));
    }
    if view.membership(&p.membership_id).is_some() {
        return Err(fail(
            ErrorCode::AlreadyExists,
            "that membership id is taken",
        ));
    }
    if view.membership_of(&p.team_id, &p.member_id).is_some() {
        return Err(fail(
            ErrorCode::AlreadyExists,
            "the member is already in that team",
        ));
    }
    Ok(vec![OrgChange::MembershipUpserted {
        membership: Membership {
            id: p.membership_id.clone(),
            team_id: p.team_id.clone(),
            member_id: p.member_id.clone(),
            title_in_team: title,
            version: 1,
        },
    }])
}

fn team_remove_member(view: &dyn OrgView, p: &TeamRemoveMemberParams) -> Decision {
    let membership = found(view.membership(&p.membership_id), "no such membership")?;
    check_version(membership.version, p.expected_version)?;
    let mut changes = vec![OrgChange::MembershipRemoved {
        membership_id: membership.id.clone(),
    }];
    // Leaving a team ends leading it, in the same event.
    if let Some(team) = view.team(&membership.team_id) {
        if team.lead_member_id.as_deref() == Some(membership.member_id.as_str()) {
            changes.push(OrgChange::TeamUpserted {
                team: Team {
                    lead_member_id: None,
                    version: team.version + 1,
                    ..team
                },
            });
        }
    }
    Ok(changes)
}

fn team_set_title(view: &dyn OrgView, p: &TeamSetTitleParams) -> Decision {
    let membership = found(view.membership(&p.membership_id), "no such membership")?;
    let title = check_text(p.title_in_team.as_deref(), "title_in_team")?;
    check_version(membership.version, p.expected_version)?;
    Ok(vec![OrgChange::MembershipUpserted {
        membership: Membership {
            title_in_team: title,
            version: membership.version + 1,
            ..membership
        },
    }])
}

// ---- members --------------------------------------------------------------------------

fn member_add_person(view: &dyn OrgView, now: &str, p: &MemberAddPersonParams) -> Decision {
    check_id(&p.member_id)?;
    let contact = check_text(p.contact.as_deref(), "contact")?;
    let name = parse_name(&p.name)?;
    if view.member(&p.member_id).is_some() {
        return Err(fail(ErrorCode::AlreadyExists, "that member id is taken"));
    }
    check_handle_free(view, name.key(), None)?;
    Ok(vec![OrgChange::MemberUpserted {
        member: Member {
            id: p.member_id.clone(),
            kind: MemberKind::Person,
            name: name.into_string(),
            contact,
            owner: false,
            office_id: None,
            agent: None,
            reports_to_member_id: None,
            created_at: Some(now.to_owned()),
            version: 1,
        },
    }])
}

fn member_update_person(view: &dyn OrgView, p: &MemberUpdatePersonParams) -> Decision {
    let member = found(view.member(&p.member_id), "no such member")?;
    if member.kind != MemberKind::Person {
        return Err(fail(
            ErrorCode::InvalidRequest,
            "only people are edited here; agents are edited with member.update_agent",
        ));
    }
    if p.contact.is_some() && p.clear_contact {
        return Err(fail(
            ErrorCode::InvalidRequest,
            "give a contact or clear it, not both",
        ));
    }
    let contact = check_text(p.contact.as_deref(), "contact")?;
    let name = p.name.as_deref().map(parse_name).transpose()?;
    check_version(member.version, p.expected_version)?;
    if let Some(name) = &name {
        check_handle_free(view, name.key(), Some(&member.id))?;
    }
    Ok(vec![OrgChange::MemberUpserted {
        member: Member {
            name: name.map_or(member.name.clone(), OrganizationName::into_string),
            contact: if p.clear_contact {
                None
            } else {
                contact.or(member.contact.clone())
            },
            version: member.version + 1,
            ..member
        },
    }])
}

fn member_set_reports_to(view: &dyn OrgView, p: &MemberSetReportsToParams) -> Decision {
    let member = found(view.member(&p.member_id), "no such member")?;
    let manager = match &p.reports_to_member_id {
        Some(id) => Some(found(view.member(id), "no such manager")?),
        None => None,
    };
    refuse_offboarded(&member)?;
    if let Some(manager) = &manager {
        refuse_offboarded(manager)?;
    }
    if member.owner {
        return Err(owner_protected("the owner reports to no one"));
    }
    // Reporting to the owner is stored as reporting to no one, so each line has one form.
    let manager = manager.filter(|m| !m.owner);
    if let Some(manager) = &manager {
        if manager.id == member.id || reports_up_to(view, &manager.id, &member.id) {
            return Err(fail(
                ErrorCode::Cycle,
                "someone would end up reporting to themselves",
            ));
        }
    }
    check_version(member.version, p.expected_version)?;
    Ok(vec![OrgChange::MemberUpserted {
        member: Member {
            reports_to_member_id: manager.map(|m| m.id),
            version: member.version + 1,
            ..member
        },
    }])
}

fn member_remove(view: &dyn OrgView, p: &MemberRemoveParams) -> Decision {
    let member = found(view.member(&p.member_id), "no such member")?;
    refuse_offboarded(&member)?;
    if member.owner {
        return Err(owner_protected("the owner cannot be removed"));
    }
    check_version(member.version, p.expected_version)?;
    let mut changes = ending_relations(view, &member);
    changes.push(OrgChange::MemberRemoved {
        member_id: member.id,
    });
    Ok(changes)
}

/// What ends when a member leaves (removed, or later offboarded): its memberships, the teams
/// it led (they lose the lead), and its direct reports (they now report to its own manager, or
/// to the owner if it had none). Each affected team and member gets a new version.
pub(crate) fn ending_relations(view: &dyn OrgView, member: &Member) -> Vec<OrgChange> {
    let mut changes: Vec<OrgChange> = view
        .memberships_of_member(&member.id)
        .into_iter()
        .map(|m| OrgChange::MembershipRemoved {
            membership_id: m.id,
        })
        .collect();
    changes.extend(
        view.teams_led_by(&member.id)
            .into_iter()
            .map(|team| OrgChange::TeamUpserted {
                team: Team {
                    lead_member_id: None,
                    version: team.version + 1,
                    ..team
                },
            }),
    );
    changes.extend(view.reports_of(&member.id).into_iter().map(|report| {
        OrgChange::MemberUpserted {
            member: Member {
                reports_to_member_id: member.reports_to_member_id.clone(),
                version: report.version + 1,
                ..report
            },
        }
    }));
    changes
}

// ---- projects -------------------------------------------------------------------------

fn project_add_team(view: &dyn OrgView, p: &ProjectAddTeamParams) -> Decision {
    if !view.project_exists(&p.project_id) {
        return Err(fail(ErrorCode::NotFound, "no such project"));
    }
    found(view.team(&p.team_id), "no such team")?;
    check_id(&p.project_team_id)?;
    if view.project_team(&p.project_team_id).is_some() {
        return Err(fail(ErrorCode::AlreadyExists, "that link id is taken"));
    }
    if view.project_team_of(&p.project_id, &p.team_id).is_some() {
        return Err(fail(
            ErrorCode::AlreadyExists,
            "that team already works on the project",
        ));
    }
    Ok(vec![OrgChange::ProjectTeamAdded {
        project_team: ProjectTeam {
            id: p.project_team_id.clone(),
            project_id: p.project_id.clone(),
            team_id: p.team_id.clone(),
            version: 1,
        },
    }])
}

fn project_remove_team(view: &dyn OrgView, p: &ProjectRemoveTeamParams) -> Decision {
    let link = found(view.project_team(&p.project_team_id), "no such link")?;
    check_version(link.version, p.expected_version)?;
    Ok(vec![OrgChange::ProjectTeamRemoved {
        project_team_id: link.id,
    }])
}

// ---- shared checks --------------------------------------------------------------------

fn found<T>(value: Option<T>, what: &str) -> Result<T, ApiError> {
    value.ok_or_else(|| fail(ErrorCode::NotFound, what))
}

fn check_version(current: u64, expected: u64) -> Result<(), ApiError> {
    if current == expected {
        Ok(())
    } else {
        Err(fail(
            ErrorCode::Conflict,
            "it changed since you last looked; refresh and try again",
        ))
    }
}

fn parse_name(raw: &str) -> Result<OrganizationName, ApiError> {
    OrganizationName::parse(raw)
        .map_err(|reason| ApiError::new(ErrorCode::InvalidName, reason.message()))
}

fn sibling_clash() -> ApiError {
    fail(
        ErrorCode::AlreadyExists,
        "a team with that name already exists here",
    )
}

fn owner_protected(message: &str) -> ApiError {
    fail(ErrorCode::OwnerProtected, message)
}

/// A client-made id: 1 to [`ID_MAX_CHARS`] ASCII letters, digits, `-` and `_`.
fn check_id(id: &str) -> Result<(), ApiError> {
    let fine = !id.is_empty()
        && id.len() <= ID_MAX_CHARS
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if fine {
        Ok(())
    } else {
        Err(fail(
            ErrorCode::InvalidRequest,
            "an id is 1 to 64 letters, digits, '-' or '_'",
        ))
    }
}

/// Optional free text (a contact, a title in a team): trimmed, 1 to [`AGENT_TEXT_MAX_CHARS`]
/// characters, no control characters.
fn check_text(text: Option<&str>, what: &str) -> Result<Option<String>, ApiError> {
    let Some(text) = text else { return Ok(None) };
    let text = text.trim();
    if text.is_empty()
        || text.chars().count() > AGENT_TEXT_MAX_CHARS
        || text.chars().any(char::is_control)
    {
        return Err(fail(
            ErrorCode::InvalidRequest,
            &format!(
                "{what} must be 1 to {AGENT_TEXT_MAX_CHARS} characters with no control characters"
            ),
        ));
    }
    Ok(Some(text.to_owned()))
}

fn refuse_offboarded(member: &Member) -> Result<(), ApiError> {
    let offboarded = member
        .agent
        .as_ref()
        .is_some_and(|agent| agent.status == AgentStatus::Offboarded);
    if offboarded {
        Err(fail(ErrorCode::InvalidRequest, "that agent is offboarded"))
    } else {
        Ok(())
    }
}

/// Handles are compared ignoring case across every agent's `messaging_name`, every person's
/// `name` and the reserved word `owner` (see `org_proto::org`). `except` is the member being
/// renamed, which may keep or recapitalize its own.
fn check_handle_free(view: &dyn OrgView, key: &str, except: Option<&str>) -> Result<(), ApiError> {
    let taken_by_other = view
        .member_with_handle(key)
        .is_some_and(|holder| Some(holder.id.as_str()) != except);
    let reserved = key == OWNER_HANDLE && !is_owner(view, except);
    if taken_by_other || reserved {
        Err(fail(
            ErrorCode::AlreadyExists,
            "that name is already another member's handle",
        ))
    } else {
        Ok(())
    }
}

fn is_owner(view: &dyn OrgView, id: Option<&str>) -> bool {
    id.and_then(|id| view.member(id)).is_some_and(|m| m.owner)
}

/// Whether `candidate` is `team_id` itself or a team that `team_id` owns, directly or not.
fn owns_or_is(view: &dyn OrgView, team_id: &str, candidate: &str) -> bool {
    let mut seen = HashSet::new();
    let mut at = Some(candidate.to_owned());
    while let Some(id) = at {
        if id == team_id {
            return true;
        }
        if !seen.insert(id.clone()) {
            return false; // a loop already in the data: do not spin
        }
        at = view.team(&id).and_then(|team| team.owner_team_id);
    }
    false
}

/// Whether following `from`'s reporting line upwards reaches `target`.
fn reports_up_to(view: &dyn OrgView, from: &str, target: &str) -> bool {
    let mut seen = HashSet::new();
    let mut at = Some(from.to_owned());
    while let Some(id) = at {
        if id == target {
            return true;
        }
        if !seen.insert(id.clone()) {
            return false;
        }
        at = view.member(&id).and_then(|m| m.reports_to_member_id);
    }
    false
}
