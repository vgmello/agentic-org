//! Compatibility dispatcher composing the organization and employee use cases.
use super::OrgCommand;
use crate::employees::{
    hire_employee as member_add_agent, member_offboard, member_set_agent_status,
    member_set_run_mode, member_update_agent,
};
use crate::organization::{
    member_add_person, member_move, member_remove, member_set_reports_to, member_update_person,
    project_add_team, project_remove_team, team_add_member, team_create, team_delete, team_move,
    team_remove_member, team_rename, team_set_lead, team_set_title, OrgView,
};
use org_proto::org::OrgChange;
use org_proto::ApiError;
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
