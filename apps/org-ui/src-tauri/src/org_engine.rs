//! The mock office's HQ: applies organization commands to a structure with the contract's rules
//! (`crates/org-proto/src/org.rs`, `docs/design/teams-and-org-chart.md`).
//!
//! TEMPORARY: the real HQ is the authority and will replace this; the app only sends commands and
//! shows what comes back. Pure: no files, no clock. When several errors apply, the first of
//! `not_found`, `invalid_request`, `invalid_name`, then the command's own rule (`owner_protected`, `cycle`,
//! `not_a_member`, `not_empty`), `conflict`, `already_exists` is returned: the domain codes come before a stale
//! version, so a permanent refusal is never hidden behind a refresh (org.rs and the real rules).

use crate::org_agent_rules as rules;
use crate::org_command::OrgCommand;
use org_proto::org::{
    AgentProfile, AgentStatus, CommandAccepted, CommandStatus, Member, MemberKind, Membership,
    OrgChange, OrgChangedData, OrgSnapshot, Position, ProjectTeam, Team,
};
use org_proto::{
    ApiError, ErrorCode, NameError, OrganizationName, AGENT_TEXT_MAX_CHARS, OWNER_HANDLE,
};
use serde::{Deserialize, Serialize};

mod projects;

type Outcome<T = Vec<OrgChange>> = Result<T, ApiError>;

fn err<T>(code: ErrorCode, message: impl Into<String>) -> Outcome<T> {
    Err(ApiError::new(code, message))
}

/// A name as HQ stores it (trimmed), or `invalid_name`. Teams and people follow the same rule as
/// organizations: 1 to 64 characters, no control or invisible-formatting characters.
fn checked_name(raw: &str) -> Outcome<String> {
    OrganizationName::parse(raw)
        .map(OrganizationName::into_string)
        .map_err(|e| match e {
            // The only message that names an organization; a team or a person is being named here.
            NameError::Empty => ApiError::new(ErrorCode::InvalidName, "Give it a name."),
            other => ApiError::from(other),
        })
}

fn same(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

fn is_offboarded(m: &Member) -> bool {
    m.agent
        .as_ref()
        .is_some_and(|a| a.status == AgentStatus::Offboarded)
}

/// Whether `handle` is another member's handle: an agent's `messaging_name`, a person's name or
/// the owner's reserved `owner`, ignoring case. `except` is the member being renamed.
fn handle_taken(s: &OrgSnapshot, handle: &str, except: Option<&str>) -> bool {
    same(handle, OWNER_HANDLE)
        || s.members
            .iter()
            .filter(|m| Some(m.id.as_str()) != except && !m.owner)
            .any(|m| match &m.agent {
                Some(a) => same(&a.messaging_name, handle),
                None => same(&m.name, handle),
            })
}

/// Every team below `team_id`, directly or through others.
fn teams_below(s: &OrgSnapshot, team_id: &str) -> Vec<String> {
    let mut below: Vec<String> = Vec::new();
    let mut stack = vec![team_id.to_string()];
    while let Some(id) = stack.pop() {
        for t in s
            .teams
            .iter()
            .filter(|t| t.owner_team_id.as_deref() == Some(id.as_str()))
        {
            if !below.contains(&t.id) {
                below.push(t.id.clone());
                stack.push(t.id.clone());
            }
        }
    }
    below
}

/// Everyone who reports to `member_id`, directly or through others.
fn reports_below(s: &OrgSnapshot, member_id: &str) -> Vec<String> {
    let mut below: Vec<String> = Vec::new();
    let mut stack = vec![member_id.to_string()];
    while let Some(id) = stack.pop() {
        for m in s
            .members
            .iter()
            .filter(|m| !m.owner && m.reports_to_member_id.as_deref() == Some(id.as_str()))
        {
            if m.id != member_id && !below.contains(&m.id) {
                below.push(m.id.clone());
                stack.push(m.id.clone());
            }
        }
    }
    below
}

fn team_ix(s: &OrgSnapshot, id: &str) -> Outcome<usize> {
    match s.teams.iter().position(|t| t.id == id) {
        Some(i) => Ok(i),
        None => err(
            ErrorCode::NotFound,
            "That team isn't in the organization any more.",
        ),
    }
}

fn member_ix(s: &OrgSnapshot, id: &str) -> Outcome<usize> {
    match s.members.iter().position(|m| m.id == id) {
        Some(i) => Ok(i),
        None => err(
            ErrorCode::NotFound,
            "That person isn't in the organization any more.",
        ),
    }
}

fn membership_ix(s: &OrgSnapshot, id: &str) -> Outcome<usize> {
    match s.memberships.iter().position(|m| m.id == id) {
        Some(i) => Ok(i),
        None => err(ErrorCode::NotFound, "That membership isn't there any more."),
    }
}

fn sibling_taken(s: &OrgSnapshot, parent: Option<&str>, name: &str, except: Option<&str>) -> bool {
    s.teams.iter().any(|t| {
        Some(t.id.as_str()) != except && t.owner_team_id.as_deref() == parent && same(&t.name, name)
    })
}

/// A team whose lead was `member` loses the lead: its new version, to send as `team_upserted`.
fn clear_lead(s: &mut OrgSnapshot, team_id: &str, member_id: &str) -> Option<OrgChange> {
    let t = s.teams.iter_mut().find(|t| t.id == team_id)?;
    if t.lead_member_id.as_deref() != Some(member_id) {
        return None;
    }
    t.lead_member_id = None;
    t.version += 1;
    Some(OrgChange::TeamUpserted { team: t.clone() })
}

/// What happens to a member's place in the organization when they leave it (removed, or an agent offboarded):
/// their memberships end, the teams they led lose the lead, and whoever reported to them reports to their own
/// manager (the owner if they had none). Each affected team and member gets a new version.
fn detach(s: &mut OrgSnapshot, id: &str, manager: Option<String>, changes: &mut Vec<OrgChange>) {
    for ms in s.memberships.iter().filter(|m| m.member_id == id) {
        changes.push(OrgChange::MembershipRemoved {
            membership_id: ms.id.clone(),
        });
    }
    s.memberships.retain(|m| m.member_id != id);
    let led: Vec<String> = s
        .teams
        .iter()
        .filter(|t| t.lead_member_id.as_deref() == Some(id))
        .map(|t| t.id.clone())
        .collect();
    for team_id in led {
        changes.extend(clear_lead(s, &team_id, id));
    }
    for m in s
        .members
        .iter_mut()
        .filter(|m| m.reports_to_member_id.as_deref() == Some(id))
    {
        m.reports_to_member_id = manager.clone();
        m.version += 1;
        changes.push(OrgChange::MemberUpserted { member: m.clone() });
    }
}

/// What HQ knows besides the structure: its clock (RFC 3339 UTC with milliseconds) and which offices exist.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub now: String,
    pub offices: Vec<String>,
}

#[cfg(test)]
impl Ctx {
    /// The clock and the one office the shared cases and the sample organization use.
    pub fn for_tests() -> Self {
        Self {
            now: "2026-11-08T12:00:00.000Z".into(),
            offices: vec!["office-1".into()],
        }
    }
}

/// Apply `command` to `s`. On success `s` has the changes and they are returned, in the order they
/// were made; on an error `s` is untouched.
pub fn apply_with(s: &mut OrgSnapshot, command: &OrgCommand, ctx: &Ctx) -> Outcome {
    let mut next = s.clone();
    let changes = apply_to(&mut next, command, ctx)?;
    *s = next;
    Ok(changes)
}

#[cfg(test)]
pub fn apply(s: &mut OrgSnapshot, command: &OrgCommand) -> Outcome {
    apply_with(s, command, &Ctx::for_tests())
}

/// The longest id the mock accepts. The real ones are ULIDs (26 characters); the sample uses short
/// readable ids, so only the shape is checked here: not empty, not long, no control characters.
const ID_MAX_CHARS: usize = 64;

fn bad_id(id: &str) -> bool {
    id.is_empty() || id.chars().count() > ID_MAX_CHARS || id.chars().any(char::is_control)
}

fn bad_text(text: &Option<String>) -> bool {
    text.as_deref().is_some_and(|t| {
        t.trim().is_empty()
            || t.chars().count() > AGENT_TEXT_MAX_CHARS
            || t.chars().any(char::is_control)
    })
}

/// The ids a command carries, for `well_formed`.
fn ids_of(command: &OrgCommand) -> Vec<&str> {
    fn opt(o: &Option<String>) -> Option<&str> {
        o.as_deref()
    }
    let mut ids: Vec<&str> = vec![command.command_id()];
    match command {
        OrgCommand::TeamCreate(p) => ids.extend(
            [Some(p.team_id.as_str()), opt(&p.owner_team_id)]
                .into_iter()
                .flatten(),
        ),
        OrgCommand::TeamRename(p) => ids.push(&p.team_id),
        OrgCommand::TeamMove(p) => ids.extend(
            [Some(p.team_id.as_str()), opt(&p.owner_team_id)]
                .into_iter()
                .flatten(),
        ),
        OrgCommand::TeamSetLead(p) => ids.extend(
            [Some(p.team_id.as_str()), opt(&p.lead_member_id)]
                .into_iter()
                .flatten(),
        ),
        OrgCommand::TeamDelete(p) => ids.push(&p.team_id),
        OrgCommand::TeamAddMember(p) => {
            ids.extend([&p.membership_id, &p.team_id, &p.member_id].map(String::as_str))
        }
        OrgCommand::TeamRemoveMember(p) => ids.push(&p.membership_id),
        OrgCommand::TeamSetTitle(p) => ids.push(&p.membership_id),
        OrgCommand::MemberAddPerson(p) => ids.push(&p.member_id),
        OrgCommand::MemberUpdatePerson(p) => ids.push(&p.member_id),
        OrgCommand::MemberRemove(p) => ids.push(&p.member_id),
        OrgCommand::MemberSetReportsTo(p) => ids.extend(
            [Some(p.member_id.as_str()), opt(&p.reports_to_member_id)]
                .into_iter()
                .flatten(),
        ),
        OrgCommand::MemberMove(p) => {
            ids.extend([&p.membership_id, &p.new_membership_id, &p.to_team_id].map(String::as_str))
        }
        OrgCommand::MemberAddAgent(p) => {
            ids.extend([&p.member_id, &p.office_id].map(String::as_str))
        }
        OrgCommand::MemberUpdateAgent(p) => ids.push(&p.member_id),
        OrgCommand::MemberSetAgentStatus(p) => ids.push(&p.member_id),
        OrgCommand::MemberSetRunMode(p) => ids.push(&p.member_id),
        OrgCommand::MemberOffboard(p) => ids.push(&p.member_id),
        OrgCommand::ProjectAddTeam(p) => {
            ids.extend([&p.project_team_id, &p.project_id, &p.team_id].map(String::as_str))
        }
        OrgCommand::ProjectRemoveTeam(p) => ids.extend(
            [Some(p.project_team_id.as_str()), opt(&p.areas_to_team_id)]
                .into_iter()
                .flatten(),
        ),
        OrgCommand::ProjectCreate(p) => {
            ids.extend([p.project_id.as_str(), p.general_area_id.as_str()]);
            ids.extend(
                p.teams
                    .iter()
                    .flat_map(|t| [t.project_team_id.as_str(), t.team_id.as_str()]),
            );
        }
        OrgCommand::ProjectUpdate(p) => ids.push(&p.project_id),
        OrgCommand::ProjectArchive(p) => ids.push(&p.project_id),
        OrgCommand::AreaCreate(p) => ids.extend(
            [
                Some(p.area_id.as_str()),
                Some(p.project_id.as_str()),
                opt(&p.team_id),
                opt(&p.lead_member_id),
            ]
            .into_iter()
            .flatten(),
        ),
        OrgCommand::AreaRename(p) => ids.push(&p.area_id),
        OrgCommand::AreaSetTeam(p) => ids.extend(
            [Some(p.area_id.as_str()), opt(&p.team_id)]
                .into_iter()
                .flatten(),
        ),
        OrgCommand::AreaSetLead(p) => ids.extend(
            [Some(p.area_id.as_str()), opt(&p.lead_member_id)]
                .into_iter()
                .flatten(),
        ),
    }
    ids
}

/// The id of a new member (`member.add_person`, `member.add_agent`) is exactly 26 ASCII letters or digits (ULID-shaped):
/// anything else is `invalid_request`. The sample's readable ids (`sam`) are display fixtures, not ids to send.
fn check_new_member_id(id: &str) -> Outcome<()> {
    if id.len() == 26 && id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        Ok(())
    } else {
        err(
            ErrorCode::InvalidRequest,
            "A new member's id is 26 letters or digits.",
        )
    }
}

/// An id that is not an id (empty, long, with control characters) can never name anything, so it is
/// refused before the organization is looked at.
fn well_formed(command: &OrgCommand) -> Outcome<()> {
    // A command id is a ULID in its canonical form (HQ keeps each answer under it); the other ids are only shaped.
    if org_proto::ulid_time_ms(command.command_id()).is_none() {
        return err(
            ErrorCode::InvalidRequest,
            "The command id isn't a valid ULID.",
        );
    }
    if ids_of(command).into_iter().any(bad_id) {
        return err(
            ErrorCode::InvalidRequest,
            "An id in that request isn't valid.",
        );
    }
    Ok(())
}

/// A title or contact that is empty (clear it instead), too long or has control characters: refused
/// in the command's own `invalid_request` step, after what it names has been found.
fn check_text(text: &Option<String>) -> Outcome<()> {
    if bad_text(text) {
        return err(
            ErrorCode::InvalidRequest,
            "A title or contact can't be empty, too long or contain control characters.",
        );
    }
    Ok(())
}

/// Run the command, then restore the area invariants its changes may have broken (design §2.5): a team that left a
/// project, a lead who left a team or the organization.
fn apply_to(s: &mut OrgSnapshot, command: &OrgCommand, ctx: &Ctx) -> Outcome {
    let mut changes = apply_command(s, command, ctx)?;
    projects::reconcile_areas(s, &mut changes);
    Ok(changes)
}

fn apply_command(s: &mut OrgSnapshot, command: &OrgCommand, ctx: &Ctx) -> Outcome {
    well_formed(command)?;
    match command {
        OrgCommand::TeamCreate(p) => {
            if let Some(owner) = &p.owner_team_id {
                team_ix(s, owner)?;
            }
            let name = checked_name(&p.name)?;
            if s.teams.iter().any(|t| t.id == p.team_id) {
                return err(ErrorCode::AlreadyExists, "That team id is taken.");
            }
            if sibling_taken(s, p.owner_team_id.as_deref(), &name, None) {
                return err(
                    ErrorCode::AlreadyExists,
                    format!("There's already a team called {name} here."),
                );
            }
            let team = Team {
                id: p.team_id.clone(),
                name,
                owner_team_id: p.owner_team_id.clone(),
                lead_member_id: None,
                version: 1,
            };
            s.teams.push(team.clone());
            Ok(vec![OrgChange::TeamUpserted { team }])
        }
        OrgCommand::TeamRename(p) => {
            let i = team_ix(s, &p.team_id)?;
            let name = checked_name(&p.name)?;
            if s.teams[i].version != p.expected_version {
                return err(
                    ErrorCode::Conflict,
                    format!("{} changed since you looked.", s.teams[i].name),
                );
            }
            if sibling_taken(
                s,
                s.teams[i].owner_team_id.as_deref(),
                &name,
                Some(&p.team_id),
            ) {
                return err(
                    ErrorCode::AlreadyExists,
                    format!("There's already a team called {name} here."),
                );
            }
            s.teams[i].name = name;
            s.teams[i].version += 1;
            Ok(vec![OrgChange::TeamUpserted {
                team: s.teams[i].clone(),
            }])
        }
        OrgCommand::TeamMove(p) => {
            let i = team_ix(s, &p.team_id)?;
            if let Some(owner) = &p.owner_team_id {
                team_ix(s, owner)?;
            }
            if let Some(owner) = &p.owner_team_id {
                if *owner == p.team_id || teams_below(s, &p.team_id).contains(owner) {
                    return err(
                        ErrorCode::Cycle,
                        format!(
                            "{} can't go under itself or a team below it.",
                            s.teams[i].name
                        ),
                    );
                }
            }
            if s.teams[i].version != p.expected_version {
                return err(
                    ErrorCode::Conflict,
                    format!("{} changed since you looked.", s.teams[i].name),
                );
            }
            if sibling_taken(
                s,
                p.owner_team_id.as_deref(),
                &s.teams[i].name,
                Some(&p.team_id),
            ) {
                return err(
                    ErrorCode::AlreadyExists,
                    format!("There's already a team called {} there.", s.teams[i].name),
                );
            }
            s.teams[i].owner_team_id = p.owner_team_id.clone();
            s.teams[i].version += 1;
            Ok(vec![OrgChange::TeamUpserted {
                team: s.teams[i].clone(),
            }])
        }
        OrgCommand::TeamSetLead(p) => {
            let i = team_ix(s, &p.team_id)?;
            let lead = match &p.lead_member_id {
                Some(id) => {
                    let m = &s.members[member_ix(s, id)?];
                    if is_offboarded(m) {
                        return err(
                            ErrorCode::InvalidRequest,
                            format!("{} has left the organization.", m.name),
                        );
                    }
                    Some(m.clone())
                }
                None => None,
            };
            if let Some(m) = &lead {
                if m.owner {
                    return err(ErrorCode::OwnerProtected, "The owner can't lead a team.");
                }
                if !s
                    .memberships
                    .iter()
                    .any(|x| x.team_id == p.team_id && x.member_id == m.id)
                {
                    return err(
                        ErrorCode::NotAMember,
                        format!("{} isn't in {}.", m.name, s.teams[i].name),
                    );
                }
            }
            if s.teams[i].version != p.expected_version {
                return err(
                    ErrorCode::Conflict,
                    format!("{} changed since you looked.", s.teams[i].name),
                );
            }
            s.teams[i].lead_member_id = p.lead_member_id.clone();
            s.teams[i].version += 1;
            Ok(vec![OrgChange::TeamUpserted {
                team: s.teams[i].clone(),
            }])
        }
        OrgCommand::TeamDelete(p) => {
            let i = team_ix(s, &p.team_id)?;
            if s.teams
                .iter()
                .any(|t| t.owner_team_id.as_deref() == Some(p.team_id.as_str()))
            {
                return err(
                    ErrorCode::NotEmpty,
                    format!("{} still has teams under it.", s.teams[i].name),
                );
            }
            if s.teams[i].version != p.expected_version {
                return err(
                    ErrorCode::Conflict,
                    format!("{} changed since you looked.", s.teams[i].name),
                );
            }
            let mut changes = vec![OrgChange::TeamDeleted {
                team_id: p.team_id.clone(),
            }];
            for m in s.memberships.iter().filter(|m| m.team_id == p.team_id) {
                changes.push(OrgChange::MembershipRemoved {
                    membership_id: m.id.clone(),
                });
            }
            for pt in s.project_teams.iter().filter(|x| x.team_id == p.team_id) {
                changes.push(OrgChange::ProjectTeamRemoved {
                    project_team_id: pt.id.clone(),
                });
            }
            s.teams.remove(i);
            s.memberships.retain(|m| m.team_id != p.team_id);
            s.project_teams.retain(|x| x.team_id != p.team_id);
            Ok(changes)
        }
        OrgCommand::TeamAddMember(p) => {
            let i = team_ix(s, &p.team_id)?;
            let m = s.members[member_ix(s, &p.member_id)?].clone();
            check_text(&p.title_in_team)?;
            if is_offboarded(&m) {
                return err(
                    ErrorCode::InvalidRequest,
                    format!("{} has left the organization.", m.name),
                );
            }
            if m.owner {
                return err(
                    ErrorCode::OwnerProtected,
                    "The owner isn't on a team: they are in every team already.",
                );
            }
            if s.memberships.iter().any(|x| {
                x.id == p.membership_id || (x.team_id == p.team_id && x.member_id == p.member_id)
            }) {
                return err(
                    ErrorCode::AlreadyExists,
                    format!("{} is already in {}.", m.name, s.teams[i].name),
                );
            }
            let membership = Membership {
                id: p.membership_id.clone(),
                team_id: p.team_id.clone(),
                member_id: p.member_id.clone(),
                title_in_team: p.title_in_team.clone(),
                version: 1,
            };
            s.memberships.push(membership.clone());
            Ok(vec![OrgChange::MembershipUpserted { membership }])
        }
        OrgCommand::TeamRemoveMember(p) => {
            let i = membership_ix(s, &p.membership_id)?;
            if s.memberships[i].version != p.expected_version {
                return err(
                    ErrorCode::Conflict,
                    "That membership changed since you looked.",
                );
            }
            let gone = s.memberships.remove(i);
            let mut changes = vec![OrgChange::MembershipRemoved {
                membership_id: gone.id,
            }];
            changes.extend(clear_lead(s, &gone.team_id, &gone.member_id));
            Ok(changes)
        }
        OrgCommand::TeamSetTitle(p) => {
            let i = membership_ix(s, &p.membership_id)?;
            check_text(&p.title_in_team)?;
            if s.memberships[i].version != p.expected_version {
                return err(
                    ErrorCode::Conflict,
                    "That membership changed since you looked.",
                );
            }
            s.memberships[i].title_in_team = p.title_in_team.clone();
            s.memberships[i].version += 1;
            Ok(vec![OrgChange::MembershipUpserted {
                membership: s.memberships[i].clone(),
            }])
        }
        OrgCommand::MemberAddPerson(p) => {
            check_text(&p.contact)?;
            check_new_member_id(&p.member_id)?;
            let name = checked_name(&p.name)?;
            if s.members.iter().any(|m| m.id == p.member_id) {
                return err(ErrorCode::AlreadyExists, "That person id is taken.");
            }
            if handle_taken(s, &name, None) {
                return err(
                    ErrorCode::AlreadyExists,
                    format!("{name} is already taken as a name here."),
                );
            }
            let member = Member {
                id: p.member_id.clone(),
                kind: MemberKind::Person,
                name,
                contact: p.contact.clone(),
                owner: false,
                office_id: None,
                agent: None,
                reports_to_member_id: None,
                created_at: None,
                version: 1,
            };
            s.members.push(member.clone());
            Ok(vec![OrgChange::MemberUpserted { member }])
        }
        OrgCommand::MemberUpdatePerson(p) => {
            let i = member_ix(s, &p.member_id)?;
            check_text(&p.contact)?;
            if s.members[i].kind != MemberKind::Person || (p.contact.is_some() && p.clear_contact) {
                return err(
                    ErrorCode::InvalidRequest,
                    "That change doesn't apply to this member.",
                );
            }
            let name = p.name.as_deref().map(checked_name).transpose()?;
            if s.members[i].version != p.expected_version {
                return err(
                    ErrorCode::Conflict,
                    format!("{} changed since you looked.", s.members[i].name),
                );
            }
            if let Some(name) = &name {
                if !s.members[i].owner && handle_taken(s, name, Some(&p.member_id)) {
                    return err(
                        ErrorCode::AlreadyExists,
                        format!("{name} is already taken as a name here."),
                    );
                }
                s.members[i].name = name.clone();
            }
            if p.contact.is_some() {
                s.members[i].contact = p.contact.clone();
            }
            if p.clear_contact {
                s.members[i].contact = None;
            }
            s.members[i].version += 1;
            Ok(vec![OrgChange::MemberUpserted {
                member: s.members[i].clone(),
            }])
        }
        OrgCommand::MemberRemove(p) => {
            let i = member_ix(s, &p.member_id)?;
            if is_offboarded(&s.members[i]) {
                return err(
                    ErrorCode::InvalidRequest,
                    "An agent who has left stays on record.",
                );
            }
            if s.members[i].owner {
                return err(ErrorCode::OwnerProtected, "The owner can't be removed.");
            }
            if s.members[i].version != p.expected_version {
                return err(
                    ErrorCode::Conflict,
                    format!("{} changed since you looked.", s.members[i].name),
                );
            }
            let gone = s.members.remove(i);
            let mut changes = vec![OrgChange::MemberRemoved {
                member_id: gone.id.clone(),
            }];
            detach(s, &gone.id, gone.reports_to_member_id.clone(), &mut changes);
            Ok(changes)
        }
        OrgCommand::MemberSetReportsTo(p) => {
            let i = member_ix(s, &p.member_id)?;
            let boss = match &p.reports_to_member_id {
                Some(id) => Some(s.members[member_ix(s, id)?].clone()),
                None => None,
            };
            if is_offboarded(&s.members[i]) || boss.as_ref().is_some_and(is_offboarded) {
                return err(
                    ErrorCode::InvalidRequest,
                    "Someone who has left can't be given a manager or be one.",
                );
            }
            if s.members[i].owner {
                return err(ErrorCode::OwnerProtected, "The owner reports to no one.");
            }
            if let Some(boss) = &boss {
                if boss.id == p.member_id || reports_below(s, &p.member_id).contains(&boss.id) {
                    return err(
                        ErrorCode::Cycle,
                        format!(
                            "{} reports to {}, directly or through others.",
                            boss.name, s.members[i].name
                        ),
                    );
                }
            }
            if s.members[i].version != p.expected_version {
                return err(
                    ErrorCode::Conflict,
                    format!("{} changed since you looked.", s.members[i].name),
                );
            }
            // Reporting to the owner is stored as empty, so each line has one form.
            s.members[i].reports_to_member_id = boss.filter(|b| !b.owner).map(|b| b.id);
            s.members[i].version += 1;
            Ok(vec![OrgChange::MemberUpserted {
                member: s.members[i].clone(),
            }])
        }
        OrgCommand::MemberMove(p) => {
            let i = membership_ix(s, &p.membership_id)?;
            let to = team_ix(s, &p.to_team_id)?;
            check_text(&p.title_in_team)?;
            if s.memberships[i].version != p.expected_version {
                return err(
                    ErrorCode::Conflict,
                    "That membership changed since you looked.",
                );
            }
            let old = s.memberships[i].clone();
            if s.memberships.iter().any(|x| {
                x.id == p.new_membership_id
                    || (x.team_id == p.to_team_id && x.member_id == old.member_id)
            }) {
                return err(
                    ErrorCode::AlreadyExists,
                    format!("They are already in {}.", s.teams[to].name),
                );
            }
            s.memberships.remove(i);
            let membership = Membership {
                id: p.new_membership_id.clone(),
                team_id: p.to_team_id.clone(),
                member_id: old.member_id.clone(),
                title_in_team: p.title_in_team.clone(),
                version: 1,
            };
            s.memberships.push(membership.clone());
            let mut changes = vec![
                OrgChange::MembershipRemoved {
                    membership_id: old.id.clone(),
                },
                OrgChange::MembershipUpserted { membership },
            ];
            changes.extend(clear_lead(s, &old.team_id, &old.member_id));
            Ok(changes)
        }
        OrgCommand::MemberAddAgent(p) => {
            if !ctx.offices.contains(&p.office_id) {
                return err(
                    ErrorCode::NotFound,
                    "That office isn't in the organization.",
                );
            }
            check_new_member_id(&p.member_id)?;
            let profile = rules::new_profile(&p.profile)?;
            let name = checked_name(&p.name)?;
            if s.members.iter().any(|m| m.id == p.member_id) {
                return err(ErrorCode::AlreadyExists, "That agent id is taken.");
            }
            if handle_taken(s, &profile.messaging_name, None) {
                return err(
                    ErrorCode::AlreadyExists,
                    format!(
                        "{} is already taken as a name here.",
                        profile.messaging_name
                    ),
                );
            }
            let member = Member {
                id: p.member_id.clone(),
                kind: MemberKind::Agent,
                name,
                contact: None,
                owner: false,
                office_id: Some(p.office_id.clone()),
                agent: Some(profile),
                reports_to_member_id: None,
                created_at: Some(ctx.now.clone()),
                version: 1,
            };
            s.members.push(member.clone());
            Ok(vec![OrgChange::MemberUpserted { member }])
        }
        OrgCommand::MemberUpdateAgent(p) => {
            let i = member_ix(s, &p.member_id)?;
            let Some(current) = s.members[i]
                .agent
                .clone()
                .filter(|_| s.members[i].kind == MemberKind::Agent)
            else {
                return err(
                    ErrorCode::InvalidRequest,
                    "That member isn't an agent: people are edited another way.",
                );
            };
            if is_offboarded(&s.members[i]) {
                return err(
                    ErrorCode::InvalidRequest,
                    "An agent who has left isn't edited: its record is history.",
                );
            }
            let nothing = p.name.is_none()
                && p.role.is_none()
                && p.title.is_none()
                && p.focus.is_none()
                && p.runtime.is_none()
                && p.model.is_none()
                && p.home.is_none()
                && p.messaging_name.is_none()
                && !p.clear_focus
                && !p.clear_model
                && !p.clear_home;
            if nothing
                || (p.focus.is_some() && p.clear_focus)
                || (p.model.is_some() && p.clear_model)
                || (p.home.is_some() && p.clear_home)
            {
                return err(
                    ErrorCode::InvalidRequest,
                    "Give a value to change, and not both a value and its clear flag.",
                );
            }
            let mut next: AgentProfile = current;
            if let Some(role) = &p.role {
                next.role = rules::text(role, "The role")?;
            }
            if let Some(title) = &p.title {
                next.title = rules::text(title, "The title")?;
            }
            if let Some(focus) = &p.focus {
                next.focus = Some(rules::text(focus, "The focus")?);
            }
            if p.clear_focus {
                next.focus = None;
            }
            if let Some(rt) = &p.runtime {
                next.runtime = rules::runtime(rt)?;
            }
            if let Some(model) = &p.model {
                next.model = Some(rules::text(model, "The model")?);
            }
            if p.clear_model {
                next.model = None;
            }
            if let Some(home) = &p.home {
                next.home = Some(rules::home(home)?);
            }
            if p.clear_home {
                next.home = None;
            }
            let name = p.name.as_deref().map(checked_name).transpose()?;
            if let Some(mn) = &p.messaging_name {
                next.messaging_name = rules::messaging_name(mn)?;
            }
            if s.members[i].version != p.expected_version {
                return err(
                    ErrorCode::Conflict,
                    format!("{} changed since you looked.", s.members[i].name),
                );
            }
            if p.messaging_name.is_some()
                && handle_taken(s, &next.messaging_name, Some(&p.member_id))
            {
                return err(
                    ErrorCode::AlreadyExists,
                    format!("{} is already taken as a name here.", next.messaging_name),
                );
            }
            if let Some(name) = name {
                s.members[i].name = name;
            }
            s.members[i].agent = Some(next);
            s.members[i].version += 1;
            Ok(vec![OrgChange::MemberUpserted {
                member: s.members[i].clone(),
            }])
        }
        OrgCommand::MemberSetAgentStatus(p) => {
            let i = member_ix(s, &p.member_id)?;
            if s.members[i].kind != MemberKind::Agent || s.members[i].agent.is_none() {
                return err(ErrorCode::InvalidRequest, "That member isn't an agent.");
            }
            if is_offboarded(&s.members[i]) {
                return err(
                    ErrorCode::InvalidRequest,
                    "An agent who has left can't be paused or resumed.",
                );
            }
            if !matches!(p.status, AgentStatus::Active | AgentStatus::Paused) {
                return err(ErrorCode::InvalidRequest, "An agent is active or paused.");
            }
            if s.members[i].version != p.expected_version {
                return err(
                    ErrorCode::Conflict,
                    format!("{} changed since you looked.", s.members[i].name),
                );
            }
            if let Some(a) = s.members[i].agent.as_mut() {
                a.status = p.status;
            }
            s.members[i].version += 1;
            Ok(vec![OrgChange::MemberUpserted {
                member: s.members[i].clone(),
            }])
        }
        OrgCommand::MemberSetRunMode(p) => {
            let i = member_ix(s, &p.member_id)?;
            if s.members[i].kind != MemberKind::Agent || s.members[i].agent.is_none() {
                return err(ErrorCode::InvalidRequest, "That member isn't an agent.");
            }
            if is_offboarded(&s.members[i]) {
                return err(
                    ErrorCode::InvalidRequest,
                    "An agent who has left isn't run.",
                );
            }
            let (run_mode, schedule) = rules::run_mode(p.run_mode, p.schedule.as_ref())?;
            if s.members[i].version != p.expected_version {
                return err(
                    ErrorCode::Conflict,
                    format!("{} changed since you looked.", s.members[i].name),
                );
            }
            if let Some(a) = s.members[i].agent.as_mut() {
                a.run_mode = run_mode;
                a.schedule = schedule;
            }
            s.members[i].version += 1;
            Ok(vec![OrgChange::MemberUpserted {
                member: s.members[i].clone(),
            }])
        }
        OrgCommand::MemberOffboard(p) => {
            let i = member_ix(s, &p.member_id)?;
            if s.members[i].kind != MemberKind::Agent || s.members[i].agent.is_none() {
                return err(
                    ErrorCode::InvalidRequest,
                    "That member isn't an agent: people are removed instead.",
                );
            }
            if is_offboarded(&s.members[i]) {
                return err(ErrorCode::InvalidRequest, "That agent has already left.");
            }
            if s.members[i].version != p.expected_version {
                return err(
                    ErrorCode::Conflict,
                    format!("{} changed since you looked.", s.members[i].name),
                );
            }
            // Offboarding is a state, not a deletion: the record and its handle stay for good.
            if let Some(a) = s.members[i].agent.as_mut() {
                a.status = AgentStatus::Offboarded;
                a.offboarded_at = Some(ctx.now.clone());
            }
            s.members[i].version += 1;
            let id = s.members[i].id.clone();
            let manager = s.members[i].reports_to_member_id.clone();
            let mut changes = vec![OrgChange::MemberUpserted {
                member: s.members[i].clone(),
            }];
            detach(s, &id, manager, &mut changes);
            Ok(changes)
        }
        OrgCommand::ProjectAddTeam(p) => {
            team_ix(s, &p.team_id)?;
            if !s.projects.iter().any(|x| x.id == p.project_id) {
                return err(ErrorCode::NotFound, "That project isn't there any more.");
            }
            if s.project_teams.iter().any(|x| {
                x.id == p.project_team_id
                    || (x.project_id == p.project_id && x.team_id == p.team_id)
            }) {
                return err(
                    ErrorCode::AlreadyExists,
                    "That team already works on this project.",
                );
            }
            let project_team = ProjectTeam {
                id: p.project_team_id.clone(),
                project_id: p.project_id.clone(),
                team_id: p.team_id.clone(),
                version: 1,
            };
            s.project_teams.push(project_team.clone());
            Ok(vec![OrgChange::ProjectTeamAdded { project_team }])
        }
        OrgCommand::ProjectRemoveTeam(p) => {
            let Some(i) = s
                .project_teams
                .iter()
                .position(|x| x.id == p.project_team_id)
            else {
                return err(ErrorCode::NotFound, "That link isn't there any more.");
            };
            // The leaving team's areas go to `areas_to_team_id` when the project keeps other teams (required then), or
            // are left team-less when it was the last one (then it may not be given).
            let link = s.project_teams[i].clone();
            let owns_areas = s.areas.iter().any(|a| {
                a.project_id == link.project_id
                    && !a.deleted
                    && a.team_id.as_deref() == Some(link.team_id.as_str())
            });
            let remaining: Vec<String> = s
                .project_teams
                .iter()
                .filter(|x| x.project_id == link.project_id && x.id != link.id)
                .map(|x| x.team_id.clone())
                .collect();
            match &p.areas_to_team_id {
                Some(target) if remaining.is_empty() || !remaining.contains(target) => {
                    return err(
                        ErrorCode::InvalidRequest,
                        "Areas can only go to another team that still works on the project.",
                    );
                }
                None if owns_areas && !remaining.is_empty() => {
                    return err(
                        ErrorCode::InvalidRequest,
                        "Say which of the project's other teams takes over this team's areas.",
                    );
                }
                _ => {}
            }
            if s.project_teams[i].version != p.expected_version {
                return err(ErrorCode::Conflict, "That link changed since you looked.");
            }
            s.project_teams.remove(i);
            let mut changes = vec![OrgChange::ProjectTeamRemoved {
                project_team_id: p.project_team_id.clone(),
            }];
            if let Some(target) = &p.areas_to_team_id {
                let in_target: Vec<String> = s
                    .memberships
                    .iter()
                    .filter(|m| m.team_id == *target)
                    .map(|m| m.member_id.clone())
                    .collect();
                for a in s.areas.iter_mut().filter(|a| {
                    a.project_id == link.project_id
                        && !a.deleted
                        && a.team_id.as_deref() == Some(link.team_id.as_str())
                }) {
                    a.team_id = Some(target.clone());
                    // A lead who isn't in the team taking over stops leading, in the same change.
                    if a.lead_member_id
                        .as_ref()
                        .is_some_and(|l| !in_target.contains(l))
                    {
                        a.lead_member_id = None;
                    }
                    a.version += 1;
                    changes.push(OrgChange::AreaUpserted { area: a.clone() });
                }
            }
            Ok(changes)
        }
        OrgCommand::ProjectCreate(p) => projects::project_create(s, p, ctx),
        OrgCommand::ProjectUpdate(p) => projects::project_update(s, p),
        OrgCommand::ProjectArchive(p) => projects::project_archive(s, p),
        OrgCommand::AreaCreate(p) => projects::area_create(s, p),
        OrgCommand::AreaRename(p) => projects::area_rename(s, p),
        OrgCommand::AreaSetTeam(p) => projects::area_set_team(s, p),
        OrgCommand::AreaSetLead(p) => projects::area_set_lead(s, p),
    }
}

/// What HQ answered a command id, and (a hash of) the command it answered, so the same id sent for a
/// different command is refused instead of getting the first one's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Answer {
    command: String,
    result: Result<CommandAccepted, ApiError>,
}

/// How many answers HQ keeps: a retry only needs the recent ones, and the file must not grow for ever.
const ANSWERS_KEPT: usize = 1000;

/// The most a command may weigh once written as JSON. Real commands are a few hundred bytes.
const COMMAND_MAX_BYTES: usize = 16 * 1024;

/// FNV-1a over the command's JSON: small and the same on every launch (a `DefaultHasher` is not).
fn fingerprint(command: &OrgCommand) -> Option<String> {
    let json = serde_json::to_string(command).ok()?;
    if json.len() > COMMAND_MAX_BYTES {
        return None;
    }
    let hash = json.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    Some(format!("{hash:016x}:{}", json.len()))
}

/// Whether `text` is what `fingerprint` writes: sixteen hex digits, a colon and a length.
fn is_fingerprint(text: &str) -> bool {
    text.split_once(':').is_some_and(|(hash, len)| {
        hash.len() == 16
            && hash.bytes().all(|b| b.is_ascii_hexdigit())
            && !len.is_empty()
            && len.bytes().all(|b| b.is_ascii_digit())
    })
}

/// A structure with its history of answers: HQ's side of "a retry with the same `command_id` gets
/// the same answer" and the position that moves with every commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hq {
    pub snapshot: OrgSnapshot,
    /// What each command id was answered, so a retry is answered again, not applied again.
    answers: std::collections::HashMap<String, Answer>,
    /// The ids in `answers`, oldest first: the oldest is forgotten past `ANSWERS_KEPT`.
    #[serde(default)]
    order: std::collections::VecDeque<String>,
}

impl Hq {
    pub fn new(snapshot: OrgSnapshot) -> Self {
        Self {
            snapshot,
            answers: Default::default(),
            order: Default::default(),
        }
    }

    /// Make a structure read from a file consistent: keep only answers kept in the current form (a
    /// fingerprint, not a command's text), list them in `order`, and honour the cap. Older files
    /// written by earlier builds had neither.
    pub fn tidy(&mut self) {
        projects::backfill_general_areas(&mut self.snapshot);
        self.answers.retain(|_, a| is_fingerprint(&a.command));
        let known: std::collections::HashSet<String> = self.answers.keys().cloned().collect();
        self.order.retain(|id| known.contains(id));
        let listed: std::collections::HashSet<String> = self.order.iter().cloned().collect();
        let mut missing: Vec<String> = known.difference(&listed).cloned().collect();
        missing.sort();
        self.order.extend(missing);
        while self.order.len() > ANSWERS_KEPT {
            if let Some(oldest) = self.order.pop_front() {
                self.answers.remove(&oldest);
            }
        }
    }

    /// Answer a command. Returns the answer and, for a new commit, the `org.changed` data to
    /// send; a retry (same `command_id`, same command) is answered the same and sends nothing.
    #[cfg(test)]
    pub fn handle(
        &mut self,
        command: &OrgCommand,
    ) -> (Result<CommandAccepted, ApiError>, Option<OrgChangedData>) {
        self.handle_with(command, &Ctx::for_tests())
    }

    pub fn handle_with(
        &mut self,
        command: &OrgCommand,
        ctx: &Ctx,
    ) -> (Result<CommandAccepted, ApiError>, Option<OrgChangedData>) {
        let id = command.command_id().to_string();
        // A request that can't be read as a command is refused and never remembered.
        let Some(fingerprint) = fingerprint(command).filter(|_| !bad_id(&id)) else {
            return (
                err(ErrorCode::InvalidRequest, "That request isn't valid."),
                None,
            );
        };
        if let Some(answer) = self.answers.get(&id) {
            if answer.command == fingerprint {
                return (answer.result.clone(), None);
            }
            return (
                err(
                    ErrorCode::InvalidRequest,
                    "That command id was already used for a different command.",
                ),
                None,
            );
        }
        let (result, changed) = match apply_with(&mut self.snapshot, command, ctx) {
            Ok(changes) => {
                self.snapshot.position.seq += 1;
                let position: Position = self.snapshot.position;
                let accepted = CommandAccepted {
                    status: CommandStatus::Committed,
                    position: Some(position),
                };
                let data = OrgChangedData {
                    position,
                    command_id: id.clone(),
                    changes,
                };
                (Ok(accepted), Some(data))
            }
            Err(e) => (Err(e), None),
        };
        self.answers.insert(
            id.clone(),
            Answer {
                command: fingerprint,
                result: result.clone(),
            },
        );
        self.order.push_back(id);
        while self.order.len() > ANSWERS_KEPT {
            if let Some(oldest) = self.order.pop_front() {
                self.answers.remove(&oldest);
            }
        }
        (result, changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::org_mock::sample_org;
    use serde_json::json;

    fn hq() -> Hq {
        Hq::new(sample_org("office-1"))
    }

    fn cmd(method: &str, params: serde_json::Value) -> OrgCommand {
        let mut params = params;
        params["command_id"] = json!(crate::org_command::test_command_id(&format!("c-{method}")));
        serde_json::from_value(json!({ "method": method, "params": params })).unwrap()
    }

    /// Run a command with a fresh id each time (the helper above reuses one per method).
    fn run(
        hq: &mut Hq,
        method: &str,
        n: u32,
        params: serde_json::Value,
    ) -> Result<Vec<OrgChange>, ApiError> {
        let mut params = params;
        params["command_id"] = json!(crate::org_command::test_command_id(&format!(
            "{method}-{n}"
        )));
        let command: OrgCommand =
            serde_json::from_value(json!({ "method": method, "params": params })).unwrap();
        apply(&mut hq.snapshot, &command)
    }

    fn code<T: std::fmt::Debug>(r: Result<T, ApiError>) -> ErrorCode {
        r.unwrap_err().code
    }

    fn team<'a>(s: &'a OrgSnapshot, id: &str) -> &'a Team {
        s.teams.iter().find(|t| t.id == id).unwrap()
    }
    fn member<'a>(s: &'a OrgSnapshot, id: &str) -> &'a Member {
        s.members.iter().find(|m| m.id == id).unwrap()
    }

    #[test]
    fn commands_read_from_the_wire_by_method_and_unknown_methods_are_refused() {
        let ok: Result<OrgCommand, _> = serde_json::from_value(
            json!({"method": "team.create", "params": {"command_id": crate::org_command::test_command_id("c"), "team_id": "t", "name": "Ops"}}),
        );
        assert!(matches!(ok, Ok(OrgCommand::TeamCreate(_))));
        for bad in [
            json!({"method": "organization.delete", "params": {"command_id": crate::org_command::test_command_id("c")}}),
            json!({"method": "member.add_agent", "params": {"command_id": crate::org_command::test_command_id("c")}}),
            json!({"method": "team.create", "params": {"team_id": "t", "name": "Ops"}}),
            json!({"params": {}}),
        ] {
            assert!(serde_json::from_value::<OrgCommand>(bad).is_err());
        }
    }

    #[test]
    fn creating_a_team_adds_it_and_names_clash_among_siblings_only() {
        let mut h = hq();
        let c = run(
            &mut h,
            "team.create",
            1,
            json!({"team_id": "ops", "name": "  Ops  "}),
        )
        .unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(team(&h.snapshot, "ops").name, "Ops");
        assert_eq!(
            code(run(
                &mut h,
                "team.create",
                2,
                json!({"team_id": "ops2", "name": "ENGINEERING"})
            )),
            ErrorCode::AlreadyExists
        );
        // Under another team the same name is fine.
        run(
            &mut h,
            "team.create",
            3,
            json!({"team_id": "ops3", "name": "Design", "owner_team_id": "eng"}),
        )
        .unwrap();
        assert_eq!(
            code(run(
                &mut h,
                "team.create",
                4,
                json!({"team_id": "ops", "name": "Other"})
            )),
            ErrorCode::AlreadyExists
        );
        assert_eq!(
            code(run(
                &mut h,
                "team.create",
                5,
                json!({"team_id": "x", "name": "   "})
            )),
            ErrorCode::InvalidName
        );
        assert_eq!(
            code(run(
                &mut h,
                "team.create",
                6,
                json!({"team_id": "x", "name": "A", "owner_team_id": "ghost"})
            )),
            ErrorCode::NotFound
        );
    }

    #[test]
    fn errors_come_in_the_contracts_order() {
        let mut h = hq();
        // not_found beats invalid_name; invalid_name beats conflict; conflict beats already_exists.
        assert_eq!(
            code(run(
                &mut h,
                "team.rename",
                1,
                json!({"team_id": "ghost", "expected_version": 9, "name": ""})
            )),
            ErrorCode::NotFound
        );
        assert_eq!(
            code(run(
                &mut h,
                "team.rename",
                2,
                json!({"team_id": "web", "expected_version": 9, "name": ""})
            )),
            ErrorCode::InvalidName
        );
        assert_eq!(
            code(run(
                &mut h,
                "team.rename",
                3,
                json!({"team_id": "web", "expected_version": 9, "name": "Platform"})
            )),
            ErrorCode::Conflict
        );
        assert_eq!(
            code(run(
                &mut h,
                "team.rename",
                4,
                json!({"team_id": "web", "expected_version": 1, "name": "platform"})
            )),
            ErrorCode::AlreadyExists
        );
        run(
            &mut h,
            "team.rename",
            5,
            json!({"team_id": "web", "expected_version": 1, "name": "Frontend"}),
        )
        .unwrap();
        assert_eq!(team(&h.snapshot, "web").version, 2);
    }

    #[test]
    fn moving_a_team_refuses_loops_and_sibling_clashes() {
        let mut h = hq();
        assert_eq!(
            code(run(
                &mut h,
                "team.move",
                1,
                json!({"team_id": "eng", "expected_version": 1, "owner_team_id": "web"})
            )),
            ErrorCode::Cycle
        );
        assert_eq!(
            code(run(
                &mut h,
                "team.move",
                2,
                json!({"team_id": "eng", "expected_version": 1, "owner_team_id": "eng"})
            )),
            ErrorCode::Cycle
        );
        run(
            &mut h,
            "team.create",
            3,
            json!({"team_id": "web2", "name": "Web", "owner_team_id": "design"}),
        )
        .unwrap();
        // Web is under Engineering; moving it under Design clashes with the new Web there.
        assert_eq!(
            code(run(
                &mut h,
                "team.move",
                4,
                json!({"team_id": "web", "expected_version": 1, "owner_team_id": "design"})
            )),
            ErrorCode::AlreadyExists
        );
        run(
            &mut h,
            "team.move",
            5,
            json!({"team_id": "web", "expected_version": 1, "owner_team_id": "people"}),
        )
        .unwrap();
        assert_eq!(
            team(&h.snapshot, "web").owner_team_id.as_deref(),
            Some("people")
        );
        run(
            &mut h,
            "team.move",
            6,
            json!({"team_id": "web", "expected_version": 2}),
        )
        .unwrap();
        assert_eq!(
            team(&h.snapshot, "web").owner_team_id,
            None,
            "to the top level"
        );
    }

    #[test]
    fn a_lead_is_a_member_of_the_team_and_never_the_owner() {
        let mut h = hq();
        assert_eq!(
            code(run(
                &mut h,
                "team.set_lead",
                1,
                json!({"team_id": "web", "expected_version": 1, "lead_member_id": "juno"})
            )),
            ErrorCode::NotAMember
        );
        assert_eq!(
            code(run(
                &mut h,
                "team.set_lead",
                2,
                json!({"team_id": "web", "expected_version": 1, "lead_member_id": "you"})
            )),
            ErrorCode::OwnerProtected
        );
        assert_eq!(
            code(run(
                &mut h,
                "team.set_lead",
                3,
                json!({"team_id": "web", "expected_version": 1, "lead_member_id": "ghost"})
            )),
            ErrorCode::NotFound
        );
        run(
            &mut h,
            "team.set_lead",
            4,
            json!({"team_id": "web", "expected_version": 1, "lead_member_id": "mira"}),
        )
        .unwrap();
        assert_eq!(
            team(&h.snapshot, "web").lead_member_id.as_deref(),
            Some("mira")
        );
        run(
            &mut h,
            "team.set_lead",
            5,
            json!({"team_id": "web", "expected_version": 2}),
        )
        .unwrap();
        assert_eq!(team(&h.snapshot, "web").lead_member_id, None);
    }

    #[test]
    fn deleting_a_team_needs_it_to_own_no_team_and_takes_its_memberships_and_links() {
        let mut h = hq();
        assert_eq!(
            code(run(
                &mut h,
                "team.delete",
                1,
                json!({"team_id": "eng", "expected_version": 1})
            )),
            ErrorCode::NotEmpty
        );
        assert_eq!(
            code(run(
                &mut h,
                "team.delete",
                2,
                json!({"team_id": "design", "expected_version": 5})
            )),
            ErrorCode::Conflict
        );
        let changes = run(
            &mut h,
            "team.delete",
            3,
            json!({"team_id": "design", "expected_version": 1}),
        )
        .unwrap();
        assert!(matches!(&changes[0], OrgChange::TeamDeleted { team_id } if team_id == "design"));
        let removed_memberships = changes
            .iter()
            .filter(|c| matches!(c, OrgChange::MembershipRemoved { .. }))
            .count();
        assert_eq!(removed_memberships, 3, "Juno, Pax and Noor");
        assert!(changes.iter().any(|c| matches!(c, OrgChange::ProjectTeamRemoved { project_team_id } if project_team_id == "pt-3")));
        assert!(h.snapshot.memberships.iter().all(|m| m.team_id != "design"));
        assert!(h
            .snapshot
            .project_teams
            .iter()
            .all(|p| p.team_id != "design"));
        // Beacon still has Web: only a project with no team left is unassigned.
        assert!(h
            .snapshot
            .project_teams
            .iter()
            .any(|p| p.project_id == "beacon"));
    }

    #[test]
    fn adding_and_removing_members_and_clearing_a_departing_lead() {
        let mut h = hq();
        assert_eq!(
            code(run(
                &mut h,
                "team.add_member",
                1,
                json!({"membership_id": "m1", "team_id": "web", "member_id": "you"})
            )),
            ErrorCode::OwnerProtected
        );
        assert_eq!(
            code(run(
                &mut h,
                "team.add_member",
                2,
                json!({"membership_id": "m1", "team_id": "web", "member_id": "mira"})
            )),
            ErrorCode::AlreadyExists
        );
        assert_eq!(
            code(run(
                &mut h,
                "team.add_member",
                3,
                json!({"membership_id": "ms-1", "team_id": "web", "member_id": "pax"})
            )),
            ErrorCode::AlreadyExists,
            "the id is taken"
        );
        run(&mut h, "team.add_member", 4, json!({"membership_id": "m1", "team_id": "web", "member_id": "pax", "title_in_team": "Designer"})).unwrap();
        run(
            &mut h,
            "team.set_title",
            5,
            json!({"membership_id": "m1", "expected_version": 1, "title_in_team": "Lead designer"}),
        )
        .unwrap();
        let ms = h
            .snapshot
            .memberships
            .iter()
            .find(|m| m.id == "m1")
            .unwrap();
        assert_eq!(
            (ms.version, ms.title_in_team.as_deref()),
            (2, Some("Lead designer"))
        );
        // Noor leads Web: removing her membership clears the lead and raises the team's version.
        let changes = run(
            &mut h,
            "team.remove_member",
            6,
            json!({"membership_id": "ms-2", "expected_version": 1}),
        )
        .unwrap();
        assert!(
            matches!(&changes[1], OrgChange::TeamUpserted { team } if team.id == "web" && team.lead_member_id.is_none() && team.version == 2)
        );
        assert_eq!(
            code(run(
                &mut h,
                "team.remove_member",
                7,
                json!({"membership_id": "ms-2", "expected_version": 1})
            )),
            ErrorCode::NotFound
        );
        assert_eq!(
            code(run(
                &mut h,
                "team.remove_member",
                8,
                json!({"membership_id": "m1", "expected_version": 1})
            )),
            ErrorCode::Conflict
        );
    }

    #[test]
    fn people_share_one_handle_namespace_with_agents_and_the_owner() {
        let mut h = hq();
        run(
            &mut h,
            "member.add_person",
            1,
            json!({"member_id": "01JC00000000000000000000P1", "name": "Sam Ortiz"}),
        )
        .unwrap();
        for (n, name) in [
            (2, "sam ortiz"),
            (3, "SAM"),
            (4, "Owner"),
            (5, "Hana Weiss"),
        ] {
            let r = run(
                &mut h,
                "member.add_person",
                n,
                json!({"member_id": format!("01JC{n:0>22}"), "name": name}),
            );
            if name == "SAM" {
                assert_eq!(
                    code(r),
                    ErrorCode::AlreadyExists,
                    "an agent's messaging name is a handle too"
                );
            } else {
                assert_eq!(code(r), ErrorCode::AlreadyExists, "{name}");
            }
        }
        assert_eq!(
            code(run(
                &mut h,
                "member.add_person",
                6,
                json!({"member_id": "01JC00000000000000000000P1", "name": "Someone"})
            )),
            ErrorCode::AlreadyExists
        );
        assert_eq!(
            code(run(
                &mut h,
                "member.add_person",
                7,
                json!({"member_id": "01JC00000000000000000000P9", "name": " "})
            )),
            ErrorCode::InvalidName
        );
        // Renaming to its own name in another case is fine; to someone else's is not.
        run(
            &mut h,
            "member.update_person",
            8,
            json!({"member_id": "01JC00000000000000000000P1", "expected_version": 1, "name": "SAM ORTIZ"}),
        )
        .unwrap();
        assert_eq!(
            code(run(
                &mut h,
                "member.update_person",
                9,
                json!({"member_id": "01JC00000000000000000000P1", "expected_version": 2, "name": "Avery Stone"})
            )),
            ErrorCode::AlreadyExists
        );
    }

    #[test]
    fn updating_a_person_changes_only_what_is_given() {
        let mut h = hq();
        assert_eq!(
            code(run(
                &mut h,
                "member.update_person",
                1,
                json!({"member_id": "sam", "expected_version": 1, "name": "X"})
            )),
            ErrorCode::InvalidRequest,
            "agents are edited another way"
        );
        assert_eq!(
            code(run(
                &mut h,
                "member.update_person",
                2,
                json!({"member_id": "avery", "expected_version": 1, "contact": "a@b", "clear_contact": true})
            )),
            ErrorCode::InvalidRequest
        );
        run(
            &mut h,
            "member.update_person",
            3,
            json!({"member_id": "avery", "expected_version": 1, "contact": "new@example.com"}),
        )
        .unwrap();
        assert_eq!(
            member(&h.snapshot, "avery").contact.as_deref(),
            Some("new@example.com")
        );
        assert_eq!(member(&h.snapshot, "avery").name, "Avery Stone");
        run(
            &mut h,
            "member.update_person",
            4,
            json!({"member_id": "avery", "expected_version": 2, "clear_contact": true}),
        )
        .unwrap();
        assert_eq!(member(&h.snapshot, "avery").contact, None);
        assert_eq!(
            code(run(
                &mut h,
                "member.update_person",
                5,
                json!({"member_id": "avery", "expected_version": 2, "name": "Z"})
            )),
            ErrorCode::Conflict
        );
    }

    #[test]
    fn removing_a_member_cascades_in_one_set_of_changes() {
        let mut h = hq();
        assert_eq!(
            code(run(
                &mut h,
                "member.remove",
                1,
                json!({"member_id": "you", "expected_version": 1})
            )),
            ErrorCode::OwnerProtected
        );
        assert_eq!(
            code(run(
                &mut h,
                "member.remove",
                2,
                json!({"member_id": "sam", "expected_version": 4})
            )),
            ErrorCode::Conflict
        );
        // Sam leads Engineering, is in it, and Noor, Kai and Mira report to Sam, who reports to Avery.
        let changes = run(
            &mut h,
            "member.remove",
            3,
            json!({"member_id": "sam", "expected_version": 1}),
        )
        .unwrap();
        assert!(
            matches!(&changes[0], OrgChange::MemberRemoved { member_id } if member_id == "sam")
        );
        assert!(changes.iter().any(|c| matches!(c, OrgChange::MembershipRemoved { membership_id } if membership_id == "ms-1")));
        assert!(changes.iter().any(|c| matches!(c, OrgChange::TeamUpserted { team } if team.id == "eng" && team.lead_member_id.is_none() && team.version == 2)));
        for id in ["noor", "kai", "mira"] {
            let m = member(&h.snapshot, id);
            assert_eq!(
                (m.reports_to_member_id.as_deref(), m.version),
                (Some("avery"), 2),
                "{id} now reports to Sam's manager"
            );
        }
        assert!(h.snapshot.members.iter().all(|m| m.id != "sam"));
        // Quinn reports to the owner (empty); removing someone with no manager sends reporters to the owner.
        run(
            &mut h,
            "member.remove",
            4,
            json!({"member_id": "avery", "expected_version": 1}),
        )
        .unwrap();
        assert_eq!(member(&h.snapshot, "hana").reports_to_member_id, None);
    }

    #[test]
    fn reporting_lines_refuse_loops_and_the_owner_is_stored_as_empty() {
        let mut h = hq();
        // Avery -> Hana -> Leo: Avery under Leo would be a loop.
        assert_eq!(
            code(run(
                &mut h,
                "member.set_reports_to",
                1,
                json!({"member_id": "avery", "expected_version": 1, "reports_to_member_id": "leo"})
            )),
            ErrorCode::Cycle
        );
        assert_eq!(
            code(run(
                &mut h,
                "member.set_reports_to",
                2,
                json!({"member_id": "avery", "expected_version": 1, "reports_to_member_id": "avery"})
            )),
            ErrorCode::Cycle
        );
        assert_eq!(
            code(run(
                &mut h,
                "member.set_reports_to",
                3,
                json!({"member_id": "you", "expected_version": 1, "reports_to_member_id": "avery"})
            )),
            ErrorCode::OwnerProtected
        );
        assert_eq!(
            code(run(
                &mut h,
                "member.set_reports_to",
                4,
                json!({"member_id": "leo", "expected_version": 1, "reports_to_member_id": "ghost"})
            )),
            ErrorCode::NotFound
        );
        run(
            &mut h,
            "member.set_reports_to",
            5,
            json!({"member_id": "leo", "expected_version": 1, "reports_to_member_id": "you"}),
        )
        .unwrap();
        assert_eq!(
            member(&h.snapshot, "leo").reports_to_member_id,
            None,
            "the owner's own id is stored as empty"
        );
        run(
            &mut h,
            "member.set_reports_to",
            6,
            json!({"member_id": "leo", "expected_version": 2, "reports_to_member_id": "juno"}),
        )
        .unwrap();
        assert_eq!(
            member(&h.snapshot, "leo").reports_to_member_id.as_deref(),
            Some("juno")
        );
    }

    #[test]
    fn an_offboarded_agent_cannot_be_given_a_manager_a_team_or_lead() {
        let mut h = hq();
        let quinn = h
            .snapshot
            .members
            .iter_mut()
            .find(|m| m.id == "quinn")
            .unwrap();
        quinn.agent.as_mut().unwrap().status = AgentStatus::Offboarded;
        assert_eq!(
            code(run(
                &mut h,
                "member.set_reports_to",
                1,
                json!({"member_id": "quinn", "expected_version": 1})
            )),
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            code(run(
                &mut h,
                "member.set_reports_to",
                2,
                json!({"member_id": "leo", "expected_version": 1, "reports_to_member_id": "quinn"})
            )),
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            code(run(
                &mut h,
                "team.add_member",
                3,
                json!({"membership_id": "m", "team_id": "web", "member_id": "quinn"})
            )),
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            code(run(
                &mut h,
                "team.set_lead",
                4,
                json!({"team_id": "web", "expected_version": 1, "lead_member_id": "quinn"})
            )),
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            code(run(
                &mut h,
                "member.remove",
                5,
                json!({"member_id": "quinn", "expected_version": 1})
            )),
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn moving_a_member_replaces_the_membership_and_clears_a_lead_they_leave() {
        let mut h = hq();
        // Noor leads Web (ms-2): move her to Platform.
        assert_eq!(
            code(run(
                &mut h,
                "member.move",
                1,
                json!({"membership_id": "ms-2", "expected_version": 1, "new_membership_id": "n1", "to_team_id": "web"})
            )),
            ErrorCode::AlreadyExists,
            "already there"
        );
        assert_eq!(
            code(run(
                &mut h,
                "member.move",
                2,
                json!({"membership_id": "ms-2", "expected_version": 1, "new_membership_id": "ms-1", "to_team_id": "platform"})
            )),
            ErrorCode::AlreadyExists,
            "new id taken"
        );
        assert_eq!(
            code(run(
                &mut h,
                "member.move",
                3,
                json!({"membership_id": "ms-2", "expected_version": 1, "new_membership_id": "n1", "to_team_id": "ghost"})
            )),
            ErrorCode::NotFound
        );
        let changes = run(&mut h, "member.move", 4, json!({"membership_id": "ms-2", "expected_version": 1, "new_membership_id": "n1", "to_team_id": "platform", "title_in_team": "Engineer"})).unwrap();
        assert!(
            matches!(&changes[0], OrgChange::MembershipRemoved { membership_id } if membership_id == "ms-2")
        );
        assert!(
            matches!(&changes[1], OrgChange::MembershipUpserted { membership } if membership.id == "n1" && membership.title_in_team.as_deref() == Some("Engineer"))
        );
        assert!(
            matches!(&changes[2], OrgChange::TeamUpserted { team } if team.id == "web" && team.lead_member_id.is_none())
        );
        assert!(h
            .snapshot
            .memberships
            .iter()
            .any(|m| m.id == "n1" && m.team_id == "platform" && m.member_id == "noor"));
    }

    #[test]
    fn projects_gain_and_lose_teams() {
        let mut h = hq();
        assert_eq!(
            code(run(
                &mut h,
                "project.add_team",
                1,
                json!({"project_team_id": "pt-9", "project_id": "ghost", "team_id": "web"})
            )),
            ErrorCode::NotFound
        );
        assert_eq!(
            code(run(
                &mut h,
                "project.add_team",
                2,
                json!({"project_team_id": "pt-9", "project_id": "beacon", "team_id": "web"})
            )),
            ErrorCode::AlreadyExists
        );
        run(
            &mut h,
            "project.add_team",
            3,
            json!({"project_team_id": "pt-9", "project_id": "scratch", "team_id": "web"}),
        )
        .unwrap();
        assert_eq!(
            code(run(
                &mut h,
                "project.remove_team",
                4,
                json!({"project_team_id": "pt-9", "expected_version": 3})
            )),
            ErrorCode::Conflict
        );
        run(
            &mut h,
            "project.remove_team",
            5,
            json!({"project_team_id": "pt-9", "expected_version": 1}),
        )
        .unwrap();
        assert_eq!(
            code(run(
                &mut h,
                "project.remove_team",
                6,
                json!({"project_team_id": "pt-9", "expected_version": 1})
            )),
            ErrorCode::NotFound
        );
    }

    #[test]
    fn a_refused_command_changes_nothing() {
        let mut h = hq();
        let before = h.snapshot.clone();
        // Fails half way: the team exists, the member does not.
        let _ = run(
            &mut h,
            "member.move",
            1,
            json!({"membership_id": "ms-2", "expected_version": 1, "new_membership_id": "n1", "to_team_id": "ghost"}),
        );
        let _ = run(
            &mut h,
            "member.remove",
            2,
            json!({"member_id": "you", "expected_version": 1}),
        );
        assert_eq!(h.snapshot, before);
    }

    #[test]
    fn a_retry_gets_the_same_answer_and_is_not_applied_twice() {
        let mut h = hq();
        let c = cmd("team.create", json!({"team_id": "ops", "name": "Ops"}));
        let (first, changed) = h.handle(&c);
        let first = first.unwrap();
        assert_eq!(first.status, CommandStatus::Committed);
        assert_eq!(first.position, Some(Position { epoch: 1, seq: 2 }));
        let data = changed.unwrap();
        assert_eq!(
            (data.command_id.as_str(), data.position.seq),
            (
                crate::org_command::test_command_id("c-team.create").as_str(),
                2
            )
        );
        let (again, changed) = h.handle(&c);
        assert_eq!(again.unwrap(), first);
        assert!(changed.is_none(), "a retry sends no event");
        assert_eq!(h.snapshot.teams.iter().filter(|t| t.id == "ops").count(), 1);
        assert_eq!(h.snapshot.position.seq, 2);
        // A refusal is also remembered.
        let bad = cmd(
            "team.rename",
            json!({"team_id": "ghost", "expected_version": 1, "name": "X"}),
        );
        let e1 = h.handle(&bad).0.unwrap_err();
        let e2 = h.handle(&bad).0.unwrap_err();
        assert_eq!(
            (e1.code, e1.message.clone()),
            (ErrorCode::NotFound, e2.message)
        );
        assert_eq!(
            h.snapshot.position.seq, 2,
            "a refusal doesn't move the position"
        );
    }

    #[test]
    fn the_state_survives_a_round_trip_through_json_with_its_answers() {
        let mut h = hq();
        let c = cmd("team.create", json!({"team_id": "ops", "name": "Ops"}));
        let _ = h.handle(&c);
        let stored: Hq = serde_json::from_str(&serde_json::to_string(&h).unwrap()).unwrap();
        assert_eq!(stored, h);
        let mut stored = stored;
        let (again, changed) = stored.handle(&c);
        assert!(
            again.is_ok() && changed.is_none(),
            "still answered from memory after a restart"
        );
    }

    /// The cases shared with the TypeScript reducer (`src/lib/org/vectors.test.ts`): both engines
    /// must give the same answer to each.
    #[test]
    fn the_shared_cases_agree_with_the_contract() {
        let vectors: serde_json::Value =
            serde_json::from_str(include_str!("../../test-vectors/org-commands.json")).unwrap();
        let build = |step: &serde_json::Value, n: usize| -> OrgCommand {
            let mut params = step["params"].clone();
            params["command_id"] = json!(format!("01ARZ3NDEKTSV4RRFFQ69G5F{n:02}"));
            serde_json::from_value(json!({"method": step["method"], "params": params})).unwrap()
        };
        let cases = vectors["cases"].as_array().unwrap();
        assert_eq!(
            cases.len(),
            252,
            "a case was lost or added: update this count"
        );
        for case in cases {
            let mut h = hq();
            for (i, step) in case["setup"].as_array().into_iter().flatten().enumerate() {
                apply(&mut h.snapshot, &build(step, i))
                    .unwrap_or_else(|e| panic!("setup of {}: {e}", case["name"]));
            }
            let before = h.snapshot.clone();
            let outcome = apply(&mut h.snapshot, &build(&case["command"], 99));
            if outcome.is_err() {
                assert_eq!(
                    h.snapshot, before,
                    "{} changed the state while refused",
                    case["name"]
                );
            }
            match case["expect"].as_str().unwrap() {
                "ok" => {
                    assert!(outcome.is_ok(), "{}: {:?}", case["name"], outcome);
                    // What the command left behind: entities found by id in the snapshot's lists (design §4.8).
                    let state = serde_json::to_value(&h.snapshot).unwrap();
                    for check in case["after"].as_array().into_iter().flatten() {
                        let list = check["in"].as_str().unwrap();
                        let found = state[list]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .find(|x| x["id"] == check["id"]);
                        if check["absent"] == json!(true) {
                            assert!(
                                found.is_none(),
                                "{}: {list} {} should be gone",
                                case["name"],
                                check["id"]
                            );
                            continue;
                        }
                        let found = found.unwrap_or_else(|| {
                            panic!("{}: no {list} {}", case["name"], check["id"])
                        });
                        for (k, v) in check["has"].as_object().into_iter().flatten() {
                            assert_eq!(
                                &found[k], v,
                                "{}: {list} {} {k}",
                                case["name"], check["id"]
                            );
                        }
                        for k in check["lacks"].as_array().into_iter().flatten() {
                            assert!(
                                found.get(k.as_str().unwrap()).is_none(),
                                "{}: {list} {} should lack {k}",
                                case["name"],
                                check["id"]
                            );
                        }
                    }
                }
                code => {
                    let got = serde_json::to_value(outcome.unwrap_err().code).unwrap();
                    assert_eq!(got, json!(code), "{}", case["name"]);
                }
            }
        }
    }

    #[test]
    fn the_same_command_id_for_a_different_command_is_refused_and_ids_must_be_ids() {
        let mut h = hq();
        let a = cmd("team.create", json!({"team_id": "ops", "name": "Ops"}));
        assert!(h.handle(&a).0.is_ok());
        // Same command_id, another command: not answered with the first one's answer.
        let mut other = json!({"team_id": "qa", "name": "QA"});
        other["command_id"] = json!(crate::org_command::test_command_id("c-team.create"));
        let b: OrgCommand =
            serde_json::from_value(json!({"method": "team.create", "params": other})).unwrap();
        assert_eq!(h.handle(&b).0.unwrap_err().code, ErrorCode::InvalidRequest);
        assert!(h.snapshot.teams.iter().all(|t| t.id != "qa"));
        // An empty, over-long or control-character command id is never recorded.
        for id in ["", &"x".repeat(65), "a\u{7}b"] {
            let c: OrgCommand = serde_json::from_value(json!({"method": "team.create", "params": {"command_id": id, "team_id": "z", "name": "Z"}})).unwrap();
            assert_eq!(
                h.handle(&c).0.unwrap_err().code,
                ErrorCode::InvalidRequest,
                "{id:?}"
            );
        }
        assert!(h.snapshot.teams.iter().all(|t| t.id != "z"));
        // Contact and title can't be blank, long, or hold control characters.
        for contact in [" ", "x".repeat(201).as_str(), "a\nb"] {
            let r = run(
                &mut h,
                "member.add_person",
                70,
                json!({"member_id": "01JC00000000000000000000P9", "name": "Pat", "contact": contact}),
            );
            assert_eq!(code(r), ErrorCode::InvalidRequest, "{contact:?}");
        }
    }

    #[test]
    fn a_refusal_is_remembered_even_when_the_same_command_would_now_succeed() {
        let mut h = hq();
        // Under a team that doesn't exist yet: refused.
        let c = cmd(
            "team.create",
            json!({"team_id": "ops", "name": "Ops", "owner_team_id": "later"}),
        );
        assert_eq!(h.handle(&c).0.unwrap_err().code, ErrorCode::NotFound);
        // The team appears; the same command, sent again, would succeed now, but its answer stands.
        run_handled(
            &mut h,
            "team.create",
            7,
            json!({"team_id": "later", "name": "Later"}),
        );
        assert_eq!(h.handle(&c).0.unwrap_err().code, ErrorCode::NotFound);
        assert!(h.snapshot.teams.iter().all(|t| t.id != "ops"));
        // A new command id for it is a new command.
        let again = cmd(
            "team.create",
            json!({"team_id": "ops", "name": "Ops", "owner_team_id": "later"}),
        );
        let mut again = serde_json::to_value(again).unwrap();
        again["params"]["command_id"] = json!(crate::org_command::test_command_id("fresh"));
        let again: OrgCommand = serde_json::from_value(again).unwrap();
        assert!(h.handle(&again).0.is_ok());
    }

    fn run_handled(h: &mut Hq, method: &str, n: u32, params: serde_json::Value) {
        let mut params = params;
        params["command_id"] = json!(crate::org_command::test_command_id(&format!(
            "h-{method}-{n}"
        )));
        let c: OrgCommand =
            serde_json::from_value(json!({"method": method, "params": params})).unwrap();
        assert!(h.handle(&c).0.is_ok());
    }

    #[test]
    fn only_the_recent_answers_are_kept_and_an_oversize_command_is_refused_unrecorded() {
        let mut h = hq();
        let first = cmd("team.create", json!({"team_id": "t0", "name": "T0"}));
        assert!(h.handle(&first).0.is_ok());
        for n in 1..=ANSWERS_KEPT {
            run_handled(
                &mut h,
                "team.create",
                n as u32,
                json!({"team_id": format!("t{n}"), "name": format!("T{n}")}),
            );
        }
        // The first answer was forgotten: its retry is judged afresh (the team now exists).
        assert_eq!(
            h.handle(&first).0.unwrap_err().code,
            ErrorCode::AlreadyExists
        );
        // The history is bounded in memory and in the file.
        assert!(serde_json::to_string(&h).unwrap().len() < 1_000_000);
        // A command too big to be a command is refused and leaves no trace.
        let size_before = serde_json::to_string(&h).unwrap().len();
        let huge = cmd(
            "team.create",
            json!({"team_id": "big", "name": "x".repeat(20_000)}),
        );
        assert_eq!(
            h.handle(&huge).0.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(serde_json::to_string(&h).unwrap().len(), size_before);
    }

    #[test]
    fn an_organization_saved_before_areas_gets_each_projects_general_area_on_load() {
        let mut h = hq();
        h.snapshot.areas.clear();
        h.tidy();
        let ids: Vec<&str> = h.snapshot.areas.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "atlas-general",
                "beacon-general",
                "onboarding-general",
                "scratch-general"
            ]
        );
        let beacon = h
            .snapshot
            .areas
            .iter()
            .find(|a| a.id == "beacon-general")
            .unwrap();
        assert_eq!(beacon.team_id.as_deref(), Some("web"));
        assert!(beacon.general && beacon.version == 1);
        // Loading again adds nothing.
        h.tidy();
        assert_eq!(h.snapshot.areas.len(), 4);
    }

    #[test]
    fn an_older_file_is_tidied_on_load_only_current_answers_survive_in_order() {
        let mut h = hq();
        let c = cmd("team.create", json!({"team_id": "ops", "name": "Ops"}));
        assert!(h.handle(&c).0.is_ok());
        // A file written by an earlier build: the answer holds the command's text, and no `order`.
        let mut legacy: serde_json::Value = serde_json::to_value(&h).unwrap();
        legacy["answers"]["old"] = json!({"command": "{\"method\":\"team.create\"}", "result": {"Err": {"code": "not_found", "message": "x"}}});
        legacy.as_object_mut().unwrap().remove("order");
        let mut loaded: Hq = serde_json::from_value(legacy).unwrap();
        loaded.tidy();
        // The current-form answer is kept and listed; the legacy one is dropped.
        assert_eq!(
            loaded.handle(&c).0.unwrap().status,
            CommandStatus::Committed
        );
        assert_eq!(
            loaded
                .snapshot
                .teams
                .iter()
                .filter(|t| t.id == "ops")
                .count(),
            1
        );
        assert!(serde_json::to_string(&loaded).unwrap().contains(&format!(
            r#""order":["{}"]"#,
            crate::org_command::test_command_id("c-team.create")
        )));
        assert!(!serde_json::to_string(&loaded)
            .unwrap()
            .contains("team.create\\\"}"));
    }

    fn profile_of<'a>(s: &'a OrgSnapshot, id: &str) -> &'a AgentProfile {
        member(s, id).agent.as_ref().unwrap()
    }

    #[test]
    fn an_added_agent_is_stored_trimmed_in_its_office_with_the_clock_and_no_manager() {
        let mut h = hq();
        let changes = run(
            &mut h,
            "member.add_agent",
            1,
            json!({"member_id": "01JC0000000000000000000CAM", "name": "  Cameron  ", "office_id": "office-1",
                   "profile": {"role": "  QA engineer ", "title": "QA", "focus": " tests ", "runtime": "codex",
                               "model": " gpt ", "messaging_name": "cam", "status": "active"}}),
        )
        .unwrap();
        assert!(
            matches!(&changes[..], [OrgChange::MemberUpserted { member }] if member.id == "01JC0000000000000000000CAM")
        );
        let cam = member(&h.snapshot, "01JC0000000000000000000CAM");
        assert_eq!(
            (cam.name.as_str(), cam.kind, cam.version),
            ("Cameron", MemberKind::Agent, 1)
        );
        assert_eq!(cam.office_id.as_deref(), Some("office-1"));
        assert_eq!(cam.created_at.as_deref(), Some("2026-11-08T12:00:00.000Z"));
        assert_eq!(cam.reports_to_member_id, None);
        let a = profile_of(&h.snapshot, "01JC0000000000000000000CAM");
        assert_eq!(
            (a.role.as_str(), a.focus.as_deref(), a.model.as_deref()),
            ("QA engineer", Some("tests"), Some("gpt"))
        );
    }

    #[test]
    fn offboarding_keeps_the_record_and_its_handle_and_ends_everything_else_in_one_set_of_changes()
    {
        let mut h = hq();
        let changes = run(
            &mut h,
            "member.offboard",
            1,
            json!({"member_id": "sam", "expected_version": 1}),
        )
        .unwrap();
        // First the agent itself, with its state and date.
        match &changes[0] {
            OrgChange::MemberUpserted { member } => {
                assert_eq!(member.version, 2);
                let a = member.agent.as_ref().unwrap();
                assert_eq!(a.status, AgentStatus::Offboarded);
                assert_eq!(a.offboarded_at.as_deref(), Some("2026-11-08T12:00:00.000Z"));
            }
            other => panic!("{other:?}"),
        }
        assert!(changes.iter().any(|c| matches!(c, OrgChange::MembershipRemoved { membership_id } if membership_id == "ms-1")));
        assert!(changes.iter().any(|c| matches!(c, OrgChange::TeamUpserted { team } if team.id == "eng" && team.lead_member_id.is_none() && team.version == 2)));
        for id in ["noor", "kai", "mira"] {
            let m = member(&h.snapshot, id);
            assert_eq!(
                (m.reports_to_member_id.as_deref(), m.version),
                (Some("avery"), 2),
                "{id}"
            );
        }
        // The record stays (history), and nothing left refers to it as a team member or lead.
        assert_eq!(
            member(&h.snapshot, "sam").agent.as_ref().unwrap().status,
            AgentStatus::Offboarded
        );
        assert!(h.snapshot.memberships.iter().all(|m| m.member_id != "sam"));
    }

    #[test]
    fn updating_an_agent_changes_only_what_is_given_and_trims() {
        let mut h = hq();
        run(&mut h, "member.update_agent", 1, json!({"member_id": "sam", "expected_version": 1, "role": " staff ", "model": " m ", "home": "/a/b"})).unwrap();
        let a = profile_of(&h.snapshot, "sam");
        assert_eq!(
            (a.role.as_str(), a.model.as_deref(), a.home.as_deref()),
            ("staff", Some("m"), Some("/a/b"))
        );
        assert_eq!(a.title, "software engineer", "untouched");
        assert_eq!(member(&h.snapshot, "sam").version, 2);
        run(&mut h, "member.update_agent", 2, json!({"member_id": "sam", "expected_version": 2, "clear_model": true, "clear_home": true})).unwrap();
        let a = profile_of(&h.snapshot, "sam");
        assert_eq!((a.model.as_deref(), a.home.as_deref()), (None, None));
    }

    #[test]
    fn pausing_and_the_run_mode_change_the_profile_and_the_version() {
        let mut h = hq();
        run(
            &mut h,
            "member.set_agent_status",
            1,
            json!({"member_id": "sam", "expected_version": 1, "status": "paused"}),
        )
        .unwrap();
        assert_eq!(profile_of(&h.snapshot, "sam").status, AgentStatus::Paused);
        run(&mut h, "member.set_run_mode", 2, json!({"member_id": "sam", "expected_version": 2, "run_mode": "scheduled", "schedule": {"spec": " daily ", "time_zone": "UTC"}})).unwrap();
        let a = profile_of(&h.snapshot, "sam");
        assert_eq!(a.schedule.as_ref().map(|s| s.spec.as_str()), Some("daily"));
        run(
            &mut h,
            "member.set_run_mode",
            3,
            json!({"member_id": "sam", "expected_version": 3, "run_mode": "on_call"}),
        )
        .unwrap();
        assert_eq!(
            profile_of(&h.snapshot, "sam").schedule,
            None,
            "a schedule only goes with scheduled"
        );
        assert_eq!(member(&h.snapshot, "sam").version, 4);
    }
}
