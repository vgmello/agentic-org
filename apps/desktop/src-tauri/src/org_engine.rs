//! The mock office's HQ: applies organization commands to a structure with the contract's rules
//! (`crates/org-proto/src/org.rs`, `docs/design/teams-and-org-chart.md`).
//!
//! TEMPORARY: the real HQ is the authority and will replace this; the app only sends commands and
//! shows what comes back. Pure: no files, no clock. When several errors apply, the first of
//! `not_found`, `invalid_request`, `invalid_name`, `conflict`, then the command's own rule
//! (`owner_protected`, `cycle`, `not_a_member`, `not_empty`), `already_exists` is returned, as the
//! contract says.

use org_proto::org::{
    AgentStatus, CommandAccepted, CommandStatus, Member, MemberAddPersonParams, MemberKind,
    MemberMoveParams, MemberRemoveParams, MemberSetReportsToParams, MemberUpdatePersonParams,
    Membership, OrgChange, OrgChangedData, OrgSnapshot, Position, ProjectAddTeamParams,
    ProjectRemoveTeamParams, ProjectTeam, Team, TeamAddMemberParams, TeamCreateParams,
    TeamDeleteParams, TeamMoveParams, TeamRemoveMemberParams, TeamRenameParams, TeamSetLeadParams,
    TeamSetTitleParams,
};
use org_proto::{ApiError, ErrorCode, OrganizationName, OWNER_HANDLE};
use serde::{Deserialize, Serialize};

/// The commands the app sends for the org chart, as one value that carries its own method name.
/// Anything else is refused when the UI's JSON is read, not here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum OrgCommand {
    #[serde(rename = "team.create")]
    TeamCreate(TeamCreateParams),
    #[serde(rename = "team.rename")]
    TeamRename(TeamRenameParams),
    #[serde(rename = "team.move")]
    TeamMove(TeamMoveParams),
    #[serde(rename = "team.set_lead")]
    TeamSetLead(TeamSetLeadParams),
    #[serde(rename = "team.delete")]
    TeamDelete(TeamDeleteParams),
    #[serde(rename = "team.add_member")]
    TeamAddMember(TeamAddMemberParams),
    #[serde(rename = "team.remove_member")]
    TeamRemoveMember(TeamRemoveMemberParams),
    #[serde(rename = "team.set_title")]
    TeamSetTitle(TeamSetTitleParams),
    #[serde(rename = "member.add_person")]
    MemberAddPerson(MemberAddPersonParams),
    #[serde(rename = "member.update_person")]
    MemberUpdatePerson(MemberUpdatePersonParams),
    #[serde(rename = "member.remove")]
    MemberRemove(MemberRemoveParams),
    #[serde(rename = "member.set_reports_to")]
    MemberSetReportsTo(MemberSetReportsToParams),
    #[serde(rename = "member.move")]
    MemberMove(MemberMoveParams),
    #[serde(rename = "project.add_team")]
    ProjectAddTeam(ProjectAddTeamParams),
    #[serde(rename = "project.remove_team")]
    ProjectRemoveTeam(ProjectRemoveTeamParams),
}

impl OrgCommand {
    /// The client-made id that makes a retry safe.
    pub fn command_id(&self) -> &str {
        match self {
            Self::TeamCreate(p) => &p.command_id,
            Self::TeamRename(p) => &p.command_id,
            Self::TeamMove(p) => &p.command_id,
            Self::TeamSetLead(p) => &p.command_id,
            Self::TeamDelete(p) => &p.command_id,
            Self::TeamAddMember(p) => &p.command_id,
            Self::TeamRemoveMember(p) => &p.command_id,
            Self::TeamSetTitle(p) => &p.command_id,
            Self::MemberAddPerson(p) => &p.command_id,
            Self::MemberUpdatePerson(p) => &p.command_id,
            Self::MemberRemove(p) => &p.command_id,
            Self::MemberSetReportsTo(p) => &p.command_id,
            Self::MemberMove(p) => &p.command_id,
            Self::ProjectAddTeam(p) => &p.command_id,
            Self::ProjectRemoveTeam(p) => &p.command_id,
        }
    }
}

type Outcome<T = Vec<OrgChange>> = Result<T, ApiError>;

fn err<T>(code: ErrorCode, message: impl Into<String>) -> Outcome<T> {
    Err(ApiError::new(code, message))
}

/// A name as HQ stores it (trimmed), or `invalid_name`. Teams and people follow the same rule as
/// organizations: 1 to 64 characters, no control or invisible-formatting characters.
fn checked_name(raw: &str) -> Outcome<String> {
    OrganizationName::parse(raw)
        .map(OrganizationName::into_string)
        .map_err(ApiError::from)
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

/// Apply `command` to `s`. On success `s` has the changes and they are returned, in the order they
/// were made; on an error `s` is untouched.
pub fn apply(s: &mut OrgSnapshot, command: &OrgCommand) -> Outcome {
    let mut next = s.clone();
    let changes = apply_to(&mut next, command)?;
    *s = next;
    Ok(changes)
}

fn apply_to(s: &mut OrgSnapshot, command: &OrgCommand) -> Outcome {
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
            if s.teams[i].version != p.expected_version {
                return err(
                    ErrorCode::Conflict,
                    format!("{} changed since you looked.", s.teams[i].name),
                );
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
            if s.teams[i].version != p.expected_version {
                return err(
                    ErrorCode::Conflict,
                    format!("{} changed since you looked.", s.teams[i].name),
                );
            }
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
            s.teams[i].lead_member_id = p.lead_member_id.clone();
            s.teams[i].version += 1;
            Ok(vec![OrgChange::TeamUpserted {
                team: s.teams[i].clone(),
            }])
        }
        OrgCommand::TeamDelete(p) => {
            let i = team_ix(s, &p.team_id)?;
            if s.teams[i].version != p.expected_version {
                return err(
                    ErrorCode::Conflict,
                    format!("{} changed since you looked.", s.teams[i].name),
                );
            }
            if s.teams
                .iter()
                .any(|t| t.owner_team_id.as_deref() == Some(p.team_id.as_str()))
            {
                return err(
                    ErrorCode::NotEmpty,
                    format!("{} still has teams under it.", s.teams[i].name),
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
            for ms in s.memberships.iter().filter(|m| m.member_id == gone.id) {
                changes.push(OrgChange::MembershipRemoved {
                    membership_id: ms.id.clone(),
                });
            }
            s.memberships.retain(|m| m.member_id != gone.id);
            let led: Vec<String> = s
                .teams
                .iter()
                .filter(|t| t.lead_member_id.as_deref() == Some(gone.id.as_str()))
                .map(|t| t.id.clone())
                .collect();
            for team_id in led {
                changes.extend(clear_lead(s, &team_id, &gone.id));
            }
            // Whoever reported to them now reports to their own manager (the owner if they had none).
            for m in s
                .members
                .iter_mut()
                .filter(|m| m.reports_to_member_id.as_deref() == Some(gone.id.as_str()))
            {
                m.reports_to_member_id = gone.reports_to_member_id.clone();
                m.version += 1;
                changes.push(OrgChange::MemberUpserted { member: m.clone() });
            }
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
            if s.members[i].version != p.expected_version {
                return err(
                    ErrorCode::Conflict,
                    format!("{} changed since you looked.", s.members[i].name),
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
            if s.project_teams[i].version != p.expected_version {
                return err(ErrorCode::Conflict, "That link changed since you looked.");
            }
            s.project_teams.remove(i);
            Ok(vec![OrgChange::ProjectTeamRemoved {
                project_team_id: p.project_team_id.clone(),
            }])
        }
    }
}

/// A structure with its history of answers: HQ's side of "a retry with the same `command_id` gets
/// the same answer" and the position that moves with every commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hq {
    pub snapshot: OrgSnapshot,
    /// What each command id was answered, so a retry is answered again, not applied again.
    answers: std::collections::HashMap<String, Result<CommandAccepted, ApiError>>,
}

impl Hq {
    pub fn new(snapshot: OrgSnapshot) -> Self {
        Self {
            snapshot,
            answers: Default::default(),
        }
    }

    /// Answer a command. Returns the answer and, for a new commit, the `org.changed` data to
    /// send; a retry (same `command_id`) is answered the same and sends nothing.
    pub fn handle(
        &mut self,
        command: &OrgCommand,
    ) -> (Result<CommandAccepted, ApiError>, Option<OrgChangedData>) {
        let id = command.command_id().to_string();
        if let Some(answer) = self.answers.get(&id) {
            return (answer.clone(), None);
        }
        match apply(&mut self.snapshot, command) {
            Ok(changes) => {
                self.snapshot.position.seq += 1;
                let position: Position = self.snapshot.position;
                let accepted = CommandAccepted {
                    status: CommandStatus::Committed,
                    position: Some(position),
                };
                self.answers.insert(id.clone(), Ok(accepted));
                (
                    Ok(accepted),
                    Some(OrgChangedData {
                        position,
                        command_id: id,
                        changes,
                    }),
                )
            }
            Err(e) => {
                self.answers.insert(id, Err(e.clone()));
                (Err(e), None)
            }
        }
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
        params["command_id"] = json!(format!("c-{}", method));
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
        params["command_id"] = json!(format!("{method}-{n}"));
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
            json!({"method": "team.create", "params": {"command_id": "c", "team_id": "t", "name": "Ops"}}),
        );
        assert!(matches!(ok, Ok(OrgCommand::TeamCreate(_))));
        for bad in [
            json!({"method": "organization.delete", "params": {"command_id": "c"}}),
            json!({"method": "member.add_agent", "params": {"command_id": "c"}}),
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
            json!({"member_id": "p1", "name": "Sam Ortiz"}),
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
                json!({"member_id": format!("x{n}"), "name": name}),
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
                json!({"member_id": "p1", "name": "Someone"})
            )),
            ErrorCode::AlreadyExists
        );
        assert_eq!(
            code(run(
                &mut h,
                "member.add_person",
                7,
                json!({"member_id": "p9", "name": " "})
            )),
            ErrorCode::InvalidName
        );
        // Renaming to its own name in another case is fine; to someone else's is not.
        run(
            &mut h,
            "member.update_person",
            8,
            json!({"member_id": "p1", "expected_version": 1, "name": "SAM ORTIZ"}),
        )
        .unwrap();
        assert_eq!(
            code(run(
                &mut h,
                "member.update_person",
                9,
                json!({"member_id": "p1", "expected_version": 2, "name": "Avery Stone"})
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
            ("c-team.create", 2)
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
}
