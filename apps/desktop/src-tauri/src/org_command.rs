//! The organization commands the app sends, as one typed value: the seam between the UI's JSON and
//! whoever answers them (today the mock HQ in `org_engine`, later `org-client`). It does not depend on
//! the mock. Each variant carries `org_proto`'s own params type, and its `method` name is the contract's.

use org_proto::org::{
    MemberAddPersonParams, MemberMoveParams, MemberRemoveParams, MemberSetReportsToParams,
    MemberUpdatePersonParams, ProjectAddTeamParams, ProjectRemoveTeamParams, TeamAddMemberParams,
    TeamCreateParams, TeamDeleteParams, TeamMoveParams, TeamRemoveMemberParams, TeamRenameParams,
    TeamSetLeadParams, TeamSetTitleParams,
};
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

#[cfg(test)]
mod tests {
    use super::*;
    use org_proto::org::{
        MemberAddPerson, MemberMove, MemberRemove, MemberSetReportsTo, MemberUpdatePerson,
        ProjectAddTeam, ProjectRemoveTeam, TeamAddMember, TeamCreate, TeamDelete, TeamMove,
        TeamRemoveMember, TeamRename, TeamSetLead, TeamSetTitle,
    };
    use org_proto::Call;

    /// Each variant's wire name is the contract's method name: a typo in either is caught here.
    #[test]
    fn the_method_names_are_the_contracts() {
        let methods = [
            "team.create",
            "team.rename",
            "team.move",
            "team.set_lead",
            "team.delete",
            "team.add_member",
            "team.remove_member",
            "team.set_title",
            "member.add_person",
            "member.update_person",
            "member.remove",
            "member.set_reports_to",
            "member.move",
            "project.add_team",
            "project.remove_team",
        ];
        let contract = [
            TeamCreate::METHOD,
            TeamRename::METHOD,
            TeamMove::METHOD,
            TeamSetLead::METHOD,
            TeamDelete::METHOD,
            TeamAddMember::METHOD,
            TeamRemoveMember::METHOD,
            TeamSetTitle::METHOD,
            MemberAddPerson::METHOD,
            MemberUpdatePerson::METHOD,
            MemberRemove::METHOD,
            MemberSetReportsTo::METHOD,
            MemberMove::METHOD,
            ProjectAddTeam::METHOD,
            ProjectRemoveTeam::METHOD,
        ];
        assert_eq!(methods, contract);
        // And the enum accepts exactly these names (a params object of the right shape for each is
        // exercised by the engine's tests; here an unknown name is refused).
        let unknown = serde_json::json!({"method": "team.explode", "params": {"command_id": "c"}});
        assert!(serde_json::from_value::<OrgCommand>(unknown).is_err());
    }
}
