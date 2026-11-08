//! An in-memory organization state for tests: implements both [`OrgView`] and [`OrgWrite`].

use std::collections::{BTreeMap, BTreeSet};

use org_proto::org::{AgentProfile, Member, MemberKind, Membership, ProjectTeam, Team};
use org_proto::OrganizationName;

use super::view::{OrgView, OrgWrite};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct MemState {
    pub teams: BTreeMap<String, Team>,
    pub members: BTreeMap<String, Member>,
    pub memberships: BTreeMap<String, Membership>,
    pub project_teams: BTreeMap<String, ProjectTeam>,
    pub projects: BTreeSet<String>,
}

fn key(name: &str) -> String {
    OrganizationName::parse(name)
        .map(|n| n.key().to_owned())
        .unwrap_or_else(|_| name.to_lowercase())
}

fn handle_of(member: &Member) -> String {
    match (&member.kind, &member.agent) {
        (MemberKind::Agent, Some(AgentProfile { messaging_name, .. })) => {
            messaging_name.to_lowercase()
        }
        _ => key(&member.name),
    }
}

impl OrgView for MemState {
    fn team(&self, id: &str) -> Option<Team> {
        self.teams.get(id).cloned()
    }
    fn team_named(&self, owner_team_id: Option<&str>, name_key: &str) -> Option<Team> {
        self.teams
            .values()
            .find(|t| t.owner_team_id.as_deref() == owner_team_id && key(&t.name) == name_key)
            .cloned()
    }
    fn teams_owned_by(&self, team_id: &str) -> Vec<Team> {
        self.teams
            .values()
            .filter(|t| t.owner_team_id.as_deref() == Some(team_id))
            .cloned()
            .collect()
    }
    fn teams_led_by(&self, member_id: &str) -> Vec<Team> {
        self.teams
            .values()
            .filter(|t| t.lead_member_id.as_deref() == Some(member_id))
            .cloned()
            .collect()
    }
    fn member(&self, id: &str) -> Option<Member> {
        self.members.get(id).cloned()
    }
    fn member_with_handle(&self, handle_key: &str) -> Option<Member> {
        self.members
            .values()
            .find(|m| handle_of(m) == handle_key)
            .cloned()
    }
    fn reports_of(&self, member_id: &str) -> Vec<Member> {
        self.members
            .values()
            .filter(|m| m.reports_to_member_id.as_deref() == Some(member_id))
            .cloned()
            .collect()
    }
    fn membership(&self, id: &str) -> Option<Membership> {
        self.memberships.get(id).cloned()
    }
    fn membership_of(&self, team_id: &str, member_id: &str) -> Option<Membership> {
        self.memberships
            .values()
            .find(|m| m.team_id == team_id && m.member_id == member_id)
            .cloned()
    }
    fn memberships_of_team(&self, team_id: &str) -> Vec<Membership> {
        self.memberships
            .values()
            .filter(|m| m.team_id == team_id)
            .cloned()
            .collect()
    }
    fn memberships_of_member(&self, member_id: &str) -> Vec<Membership> {
        self.memberships
            .values()
            .filter(|m| m.member_id == member_id)
            .cloned()
            .collect()
    }
    fn project_exists(&self, project_id: &str) -> bool {
        self.projects.contains(project_id)
    }
    fn project_team(&self, id: &str) -> Option<ProjectTeam> {
        self.project_teams.get(id).cloned()
    }
    fn project_team_of(&self, project_id: &str, team_id: &str) -> Option<ProjectTeam> {
        self.project_teams
            .values()
            .find(|l| l.project_id == project_id && l.team_id == team_id)
            .cloned()
    }
    fn project_teams_of_team(&self, team_id: &str) -> Vec<ProjectTeam> {
        self.project_teams
            .values()
            .filter(|l| l.team_id == team_id)
            .cloned()
            .collect()
    }
}

impl OrgWrite for MemState {
    fn put_team(&mut self, team: &Team) {
        self.teams.insert(team.id.clone(), team.clone());
    }
    fn delete_team(&mut self, id: &str) {
        self.teams.remove(id);
    }
    fn put_member(&mut self, member: &Member) {
        self.members.insert(member.id.clone(), member.clone());
    }
    fn delete_member(&mut self, id: &str) {
        self.members.remove(id);
    }
    fn put_membership(&mut self, membership: &Membership) {
        self.memberships
            .insert(membership.id.clone(), membership.clone());
    }
    fn delete_membership(&mut self, id: &str) {
        self.memberships.remove(id);
    }
    fn put_project_team(&mut self, project_team: &ProjectTeam) {
        self.project_teams
            .insert(project_team.id.clone(), project_team.clone());
    }
    fn delete_project_team(&mut self, id: &str) {
        self.project_teams.remove(id);
    }
}
