use std::collections::HashSet;

use org_proto::org::{
    AgentProfile, AgentRuntime, AgentStatus, Member, MemberAddAgentParams, MemberAddPersonParams,
    MemberKind, MemberMoveParams, MemberOffboardParams, MemberRemoveParams,
    MemberSetAgentStatusParams, MemberSetReportsToParams, MemberSetRunModeParams,
    MemberUpdateAgentParams, MemberUpdatePersonParams, Membership, OrgChange, ProjectAddTeamParams,
    ProjectRemoveTeamParams, ProjectTeam, RunMode, Schedule, Team, TeamAddMemberParams,
    TeamCreateParams, TeamDeleteParams, TeamMoveParams, TeamRemoveMemberParams, TeamRenameParams,
    TeamSetLeadParams, TeamSetTitleParams, AGENT_HOME_MAX_BYTES, AGENT_MESSAGING_NAME_MAX_CHARS,
    AGENT_TEXT_MAX_CHARS, AGENT_TIME_ZONE_MAX_CHARS, OFFICE_HANDLE, OWNER_HANDLE,
};
use org_proto::{ApiError, ErrorCode, NameError, OrganizationName};

use super::command::OrgCommand;
use super::fail;
use super::view::OrgView;

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
        OrgCommand::MemberAddAgent(p) => member_add_agent(view, now, p),
        OrgCommand::MemberUpdateAgent(p) => member_update_agent(view, p),
        OrgCommand::MemberSetAgentStatus(p) => member_set_agent_status(view, p),
        OrgCommand::MemberOffboard(p) => member_offboard(view, now, p),
        OrgCommand::MemberSetRunMode(p) => member_set_run_mode(view, p),
        OrgCommand::MemberUpdatePerson(p) => member_update_person(view, p),
        OrgCommand::MemberMove(p) => member_move(view, p),
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
    check_member_id(member_id)?;
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
    if view
        .team_named(p.owner_team_id.as_deref(), &name_key(&team.name))
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
    Ok(leaving(view, &membership))
}

/// Ending `membership`; leaving a team ends leading it, in the same event.
fn leaving(view: &dyn OrgView, membership: &Membership) -> Vec<OrgChange> {
    let mut changes = vec![OrgChange::MembershipRemoved {
        membership_id: membership.id.clone(),
    }];
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
    changes
}

fn member_move(view: &dyn OrgView, p: &MemberMoveParams) -> Decision {
    let membership = found(view.membership(&p.membership_id), "no such membership")?;
    found(view.team(&p.to_team_id), "no such team")?;
    check_id(&p.new_membership_id)?;
    let title = check_text(p.title_in_team.as_deref(), "title_in_team")?;
    check_version(membership.version, p.expected_version)?;
    if view.membership(&p.new_membership_id).is_some() {
        return Err(fail(
            ErrorCode::AlreadyExists,
            "that membership id is taken",
        ));
    }
    // This also refuses a move to the team the member is already in.
    if view
        .membership_of(&p.to_team_id, &membership.member_id)
        .is_some()
    {
        return Err(fail(
            ErrorCode::AlreadyExists,
            "the member is already in that team",
        ));
    }
    let mut changes = leaving(view, &membership);
    changes.insert(
        1,
        OrgChange::MembershipUpserted {
            membership: Membership {
                id: p.new_membership_id.clone(),
                team_id: p.to_team_id.clone(),
                member_id: membership.member_id,
                title_in_team: title,
                version: 1,
            },
        },
    );
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
    check_member_id(&p.member_id)?;
    let contact = check_text(p.contact.as_deref(), "contact")?;
    let name = parse_handle(&p.name)?;
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

// ---- agents ---------------------------------------------------------------------------

fn member_add_agent(view: &dyn OrgView, now: &str, p: &MemberAddAgentParams) -> Decision {
    // The office id is a client-made id like the others: a bad one is invalid_request, before
    // the lookup that would say not_found.
    check_id(&p.office_id)?;
    if !view.office_exists(&p.office_id) {
        return Err(fail(ErrorCode::NotFound, "no such office"));
    }
    check_member_id(&p.member_id)?;
    let profile = &p.profile;
    let role = required_text(&profile.role, "role")?;
    let title = required_text(&profile.title, "title")?;
    let focus = check_text(profile.focus.as_deref(), "focus")?;
    let runtime = check_runtime(&profile.runtime)?;
    let model = check_text(profile.model.as_deref(), "model")?;
    let home = profile.home.as_deref().map(check_home).transpose()?;
    if profile.offboarded_at.is_some() {
        return Err(fail(
            ErrorCode::InvalidRequest,
            "offboarded_at is set by member.offboard only",
        ));
    }
    if !matches!(profile.status, AgentStatus::Active | AgentStatus::Paused) {
        return Err(fail(
            ErrorCode::InvalidRequest,
            "a new agent is active or paused; offboarding is member.offboard",
        ));
    }
    let (run_mode, schedule) = check_run_mode(profile.run_mode, profile.schedule.as_ref())?;
    let name = parse_name(&p.name)?;
    let handle = parse_messaging_name(&profile.messaging_name)?;
    if view.member(&p.member_id).is_some() {
        return Err(fail(ErrorCode::AlreadyExists, "that member id is taken"));
    }
    check_handle_free(view, &handle.to_lowercase(), None)?;
    Ok(vec![OrgChange::MemberUpserted {
        member: Member {
            id: p.member_id.clone(),
            kind: MemberKind::Agent,
            name: name.into_string(),
            contact: None,
            owner: false,
            office_id: Some(p.office_id.clone()),
            agent: Some(AgentProfile {
                role,
                title,
                focus,
                runtime,
                model,
                run_mode,
                schedule,
                home,
                messaging_name: handle,
                status: profile.status,
                offboarded_at: None,
            }),
            reports_to_member_id: None,
            created_at: Some(now.to_owned()),
            version: 1,
        },
    }])
}

/// The agent `member_id` names, which must be one that is still a member: a person, or an
/// offboarded agent (whose record is history), is `invalid_request`.
fn live_agent(view: &dyn OrgView, member_id: &str) -> Result<(Member, AgentProfile), ApiError> {
    let member = found(view.member(member_id), "no such member")?;
    let Some(profile) = member
        .agent
        .clone()
        .filter(|_| member.kind == MemberKind::Agent)
    else {
        return Err(fail(
            ErrorCode::InvalidRequest,
            "that member is a person; agents only here",
        ));
    };
    refuse_offboarded(&member)?;
    Ok((member, profile))
}

fn member_update_agent(view: &dyn OrgView, p: &MemberUpdateAgentParams) -> Decision {
    let (member, mut profile) = live_agent(view, &p.member_id)?;
    let gives_nothing = p.name.is_none()
        && p.role.is_none()
        && p.title.is_none()
        && p.focus.is_none()
        && !p.clear_focus
        && p.runtime.is_none()
        && p.model.is_none()
        && !p.clear_model
        && p.home.is_none()
        && !p.clear_home
        && p.messaging_name.is_none();
    if gives_nothing {
        return Err(fail(ErrorCode::InvalidRequest, "give a field to change"));
    }
    for (given, clear, what) in [
        (p.focus.is_some(), p.clear_focus, "focus"),
        (p.model.is_some(), p.clear_model, "model"),
        (p.home.is_some(), p.clear_home, "home"),
    ] {
        if given && clear {
            return Err(fail(
                ErrorCode::InvalidRequest,
                &format!("give {what} or clear it, not both"),
            ));
        }
    }
    if let Some(role) = &p.role {
        profile.role = required_text(role, "role")?;
    }
    if let Some(title) = &p.title {
        profile.title = required_text(title, "title")?;
    }
    if p.clear_focus {
        profile.focus = None;
    } else if p.focus.is_some() {
        profile.focus = check_text(p.focus.as_deref(), "focus")?;
    }
    if let Some(runtime) = &p.runtime {
        profile.runtime = check_runtime(runtime)?;
    }
    if p.clear_model {
        profile.model = None;
    } else if p.model.is_some() {
        profile.model = check_text(p.model.as_deref(), "model")?;
    }
    if p.clear_home {
        profile.home = None;
    } else if let Some(home) = &p.home {
        profile.home = Some(check_home(home)?);
    }
    let name = p.name.as_deref().map(parse_name).transpose()?;
    let handle = p
        .messaging_name
        .as_deref()
        .map(parse_messaging_name)
        .transpose()?;
    check_version(member.version, p.expected_version)?;
    if let Some(handle) = &handle {
        check_handle_free(view, &handle.to_lowercase(), Some(&member.id))?;
        profile.messaging_name = handle.clone();
    }
    Ok(vec![OrgChange::MemberUpserted {
        member: Member {
            name: name.map_or(member.name.clone(), OrganizationName::into_string),
            agent: Some(profile),
            version: member.version + 1,
            ..member
        },
    }])
}

fn member_set_agent_status(view: &dyn OrgView, p: &MemberSetAgentStatusParams) -> Decision {
    let (member, mut profile) = live_agent(view, &p.member_id)?;
    if !matches!(p.status, AgentStatus::Active | AgentStatus::Paused) {
        return Err(fail(
            ErrorCode::InvalidRequest,
            "an agent is active or paused; offboarding is member.offboard",
        ));
    }
    check_version(member.version, p.expected_version)?;
    profile.status = p.status;
    Ok(vec![OrgChange::MemberUpserted {
        member: Member {
            agent: Some(profile),
            version: member.version + 1,
            ..member
        },
    }])
}

fn member_offboard(view: &dyn OrgView, now: &str, p: &MemberOffboardParams) -> Decision {
    let (member, mut profile) = live_agent(view, &p.member_id)?;
    check_version(member.version, p.expected_version)?;
    // Its relations end as for a removal, except that an already offboarded agent's record is
    // history and keeps who it reported to; the record and its handle stay, as history.
    let mut changes = ending_relations(view, &member, false);
    profile.status = AgentStatus::Offboarded;
    profile.offboarded_at = Some(now.to_owned());
    changes.push(OrgChange::MemberUpserted {
        member: Member {
            agent: Some(profile),
            version: member.version + 1,
            ..member
        },
    });
    Ok(changes)
}

fn member_set_run_mode(view: &dyn OrgView, p: &MemberSetRunModeParams) -> Decision {
    let (member, mut profile) = live_agent(view, &p.member_id)?;
    let (run_mode, schedule) = check_run_mode(p.run_mode, p.schedule.as_ref())?;
    check_version(member.version, p.expected_version)?;
    profile.run_mode = run_mode;
    profile.schedule = schedule;
    Ok(vec![OrgChange::MemberUpserted {
        member: Member {
            agent: Some(profile),
            version: member.version + 1,
            ..member
        },
    }])
}

/// A required profile text (`role`, `title`): trimmed, 1 to [`AGENT_TEXT_MAX_CHARS`] characters,
/// no control characters.
fn required_text(raw: &str, what: &str) -> Result<String, ApiError> {
    Ok(check_text(Some(raw), what)?.expect("a given text is returned"))
}

/// A runtime a writer may send: a named one, or `Other` with a name that is not a named runtime
/// in another spelling (ignoring case, with `-`, `_` and spaces alike). `Unknown` is refused.
fn check_runtime(runtime: &AgentRuntime) -> Result<AgentRuntime, ApiError> {
    match runtime {
        AgentRuntime::Unknown => Err(fail(
            ErrorCode::InvalidRequest,
            "the runtime must be one this office knows",
        )),
        AgentRuntime::Other(name) => {
            let name = required_text(name, "runtime")?;
            let spelled: String = name
                .chars()
                .map(|c| if c == '-' || c == ' ' { '_' } else { c })
                .collect::<String>()
                .to_lowercase();
            if matches!(spelled.as_str(), "claude_code" | "codex" | "antigravity") {
                Err(fail(
                    ErrorCode::InvalidRequest,
                    "that runtime has a name of its own; use it",
                ))
            } else {
                Ok(AgentRuntime::Other(name))
            }
        }
        known => Ok(known.clone()),
    }
}

/// A home: an absolute path of at most [`AGENT_HOME_MAX_BYTES`] bytes with no control character
/// and no `..` component. It is stored as given and never opened by the rules.
fn check_home(home: &str) -> Result<String, ApiError> {
    if !home.starts_with('/')
        || home.len() > AGENT_HOME_MAX_BYTES
        || home.chars().any(char::is_control)
        || home.split('/').any(|part| part == "..")
    {
        return Err(fail(
            ErrorCode::InvalidRequest,
            "home must be an absolute path with no `..`, at most 1024 bytes",
        ));
    }
    Ok(home.to_owned())
}

/// The run mode and schedule together: `Unknown` is refused, `scheduled` needs a schedule and
/// any other mode (or none) must have none.
fn check_run_mode(
    run_mode: Option<RunMode>,
    schedule: Option<&Schedule>,
) -> Result<(Option<RunMode>, Option<Schedule>), ApiError> {
    match (run_mode, schedule) {
        (Some(RunMode::Unknown), _) => Err(fail(
            ErrorCode::InvalidRequest,
            "the run mode must be one this office knows",
        )),
        (Some(RunMode::Scheduled), Some(schedule)) => {
            Ok((Some(RunMode::Scheduled), Some(check_schedule(schedule)?)))
        }
        (Some(RunMode::Scheduled), None) => Err(fail(
            ErrorCode::InvalidRequest,
            "a scheduled agent needs a schedule",
        )),
        (mode, None) => Ok((mode, None)),
        (_, Some(_)) => Err(fail(
            ErrorCode::InvalidRequest,
            "only a scheduled agent has a schedule",
        )),
    }
}

/// A schedule: the spec is text (trimmed, 1 to [`AGENT_TEXT_MAX_CHARS`], no control characters)
/// and the time zone is `/`-separated names of ASCII letters, digits, `_`, `+` and `-`, so it
/// can never name a path.
fn check_schedule(schedule: &Schedule) -> Result<Schedule, ApiError> {
    let spec = required_text(&schedule.spec, "schedule.spec")?;
    let zone = schedule.time_zone.as_str();
    let zone_ok = !zone.is_empty()
        && zone.len() <= AGENT_TIME_ZONE_MAX_CHARS
        && zone.split('/').all(|name| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'-'))
        });
    if !zone_ok {
        return Err(fail(
            ErrorCode::InvalidRequest,
            "schedule.time_zone must be an IANA name like Europe/Lisbon",
        ));
    }
    Ok(Schedule {
        spec,
        time_zone: zone.to_owned(),
    })
}

/// An agent's `messaging_name`, its handle: 1 to [`AGENT_MESSAGING_NAME_MAX_CHARS`] ASCII
/// letters, digits, `-`, `_` and `.`, starting with a letter or digit, and not shaped like an
/// address (a member id, `run:`). Not trimmed: a name that breaks the format is `invalid_name`.
fn parse_messaging_name(raw: &str) -> Result<String, ApiError> {
    let bytes = raw.as_bytes();
    let format_ok = !bytes.is_empty()
        && bytes.len() <= AGENT_MESSAGING_NAME_MAX_CHARS
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if !format_ok {
        return Err(ApiError::new(
            ErrorCode::InvalidName,
            "a handle is 1 to 64 letters, digits, `-`, `_` and `.`, starting with a letter or digit",
        ));
    }
    if org_proto::is_shaped_like_an_address(raw) {
        return Err(ApiError::new(
            ErrorCode::InvalidName,
            "a handle can't look like a member id",
        ));
    }
    Ok(raw.to_owned())
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
    // The owner's display name is not a handle: it never clashes and has no address shape to avoid.
    let name = match (p.name.as_deref(), member.owner) {
        (Some(raw), true) => Some(parse_name(raw)?),
        (Some(raw), false) => Some(parse_handle(raw)?),
        (None, _) => None,
    };
    check_version(member.version, p.expected_version)?;
    if let Some(name) = name.as_ref().filter(|_| !member.owner) {
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
        if reports_up_to(view, &manager.id, &member.id) {
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
    let mut changes = ending_relations(view, &member, true);
    changes.push(OrgChange::MemberRemoved {
        member_id: member.id,
    });
    Ok(changes)
}

/// What ends when a member leaves (removed, or later offboarded): its memberships, the teams
/// it led (they lose the lead), and its direct reports (they now report to its own manager, or
/// to the owner if it had none). Each affected team and member gets a new version.
pub(crate) fn ending_relations(
    view: &dyn OrgView,
    member: &Member,
    repoint_offboarded: bool,
) -> Vec<OrgChange> {
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
    changes.extend(
        view.reports_of(&member.id)
            .into_iter()
            // A record that is history (an offboarded agent) is only re-pointed when its
            // manager is deleted outright, never when the manager is offboarded.
            .filter(|report| repoint_offboarded || !is_offboarded(report))
            .map(|report| OrgChange::MemberUpserted {
                member: Member {
                    reports_to_member_id: member.reports_to_member_id.clone(),
                    version: report.version + 1,
                    ..report
                },
            }),
    );
    changes
}

/// Whether `member` is an agent that was offboarded: a record that is history.
pub(crate) fn is_offboarded(member: &Member) -> bool {
    member
        .agent
        .as_ref()
        .is_some_and(|agent| agent.status == AgentStatus::Offboarded)
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

/// The key sibling names are compared by (ignoring case and edges) for a name already stored.
/// A stored name always parses; the fallback only keeps a damaged row from panicking.
pub fn name_key(stored: &str) -> String {
    OrganizationName::parse(stored)
        .map(|n| n.key().to_owned())
        .unwrap_or_else(|_| stored.trim().to_lowercase())
}

/// A team's name, the owner's display name, or any other name that is not a handle. (A person's
/// name is a handle: `parse_handle`.) `NameError::Empty` speaks of "the organization", so the empty
/// case gets its own words here; the other messages are about "the name" in general.
fn parse_name(raw: &str) -> Result<OrganizationName, ApiError> {
    OrganizationName::parse(raw).map_err(|reason| {
        let message = match reason {
            NameError::Empty => "Give it a name.".to_owned(),
            other => other.message(),
        };
        ApiError::new(ErrorCode::InvalidName, message)
    })
}

/// A person's name, which is also the handle messaging addresses them by: a name, and one that
/// does not look like what messaging already addresses by (a member id, a run), so a handle never
/// shadows either.
fn parse_handle(raw: &str) -> Result<OrganizationName, ApiError> {
    let name = parse_name(raw)?;
    if org_proto::is_shaped_like_an_address(name.as_str()) {
        return Err(ApiError::new(
            ErrorCode::InvalidName,
            "a name can't look like a member id or start with `run:`",
        ));
    }
    Ok(name)
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

/// A client-made id: the contract's `org_proto::is_valid_id`.
fn check_id(id: &str) -> Result<(), ApiError> {
    if org_proto::is_valid_id(id) {
        Ok(())
    } else {
        Err(fail(
            ErrorCode::InvalidRequest,
            "an id is 1 to 64 letters, digits, '-' or '_'",
        ))
    }
}

/// A member id: exactly 26 ASCII letters and digits, the shape of a ULID. No handle has this
/// shape (`parse_handle` refuses it), so an id and a handle never name different members.
fn check_member_id(id: &str) -> Result<(), ApiError> {
    if org_proto::is_valid_member_id(id) {
        Ok(())
    } else {
        Err(fail(
            ErrorCode::InvalidRequest,
            "a member id is exactly 26 letters and digits",
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
    if is_offboarded(member) {
        Err(fail(ErrorCode::InvalidRequest, "that agent is offboarded"))
    } else {
        Ok(())
    }
}

/// Handles are compared ignoring case across every agent's `messaging_name`, every person's
/// `name` and the reserved word `owner`; the owner's display name is not a handle. `except` is
/// the member being renamed, which may keep or recapitalize its own.
fn check_handle_free(view: &dyn OrgView, key: &str, except: Option<&str>) -> Result<(), ApiError> {
    // The view never returns the owner (its display name is not a handle); the filter keeps a
    // view that forgets that from refusing names the owner merely displays.
    let taken_by_other = view
        .member_with_handle(key)
        .is_some_and(|holder| !holder.owner && Some(holder.id.as_str()) != except);
    if taken_by_other || key == OWNER_HANDLE || key == OFFICE_HANDLE {
        Err(fail(
            ErrorCode::AlreadyExists,
            "that name is already another member's handle",
        ))
    } else {
        Ok(())
    }
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
