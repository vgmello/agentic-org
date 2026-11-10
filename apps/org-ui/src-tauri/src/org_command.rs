//! The organization commands the app sends, as one typed value: the seam between the UI's JSON and
//! whoever answers them (today the mock HQ in `org_engine`, later `org-client`). It does not depend on
//! the mock. Each variant carries `org_proto`'s own params type, and its `method` name is the contract's.

use org_proto::org::{
    AreaCreateParams, AreaRenameParams, AreaSetLeadParams, AreaSetTeamParams, MemberAddAgentParams,
    MemberAddPersonParams, MemberMoveParams, MemberOffboardParams, MemberRemoveParams,
    MemberSetAgentStatusParams, MemberSetReportsToParams, MemberSetRunModeParams,
    MemberUpdateAgentParams, MemberUpdatePersonParams, ProjectAddTeamParams, ProjectArchiveParams,
    ProjectCreateParams, ProjectRemoveTeamParams, ProjectUpdateParams, TeamAddMemberParams,
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
    #[serde(rename = "member.add_agent")]
    MemberAddAgent(MemberAddAgentParams),
    #[serde(rename = "member.update_agent")]
    MemberUpdateAgent(MemberUpdateAgentParams),
    #[serde(rename = "member.set_agent_status")]
    MemberSetAgentStatus(MemberSetAgentStatusParams),
    #[serde(rename = "member.set_run_mode")]
    MemberSetRunMode(MemberSetRunModeParams),
    #[serde(rename = "member.offboard")]
    MemberOffboard(MemberOffboardParams),
    #[serde(rename = "project.add_team")]
    ProjectAddTeam(ProjectAddTeamParams),
    #[serde(rename = "project.remove_team")]
    ProjectRemoveTeam(ProjectRemoveTeamParams),
    #[serde(rename = "project.create")]
    ProjectCreate(ProjectCreateParams),
    #[serde(rename = "project.update")]
    ProjectUpdate(ProjectUpdateParams),
    #[serde(rename = "project.archive")]
    ProjectArchive(ProjectArchiveParams),
    #[serde(rename = "area.create")]
    AreaCreate(AreaCreateParams),
    #[serde(rename = "area.rename")]
    AreaRename(AreaRenameParams),
    #[serde(rename = "area.set_team")]
    AreaSetTeam(AreaSetTeamParams),
    #[serde(rename = "area.set_lead")]
    AreaSetLead(AreaSetLeadParams),
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
            Self::MemberAddAgent(p) => &p.command_id,
            Self::MemberUpdateAgent(p) => &p.command_id,
            Self::MemberSetAgentStatus(p) => &p.command_id,
            Self::MemberSetRunMode(p) => &p.command_id,
            Self::MemberOffboard(p) => &p.command_id,
            Self::ProjectAddTeam(p) => &p.command_id,
            Self::ProjectRemoveTeam(p) => &p.command_id,
            Self::ProjectCreate(p) => &p.command_id,
            Self::ProjectUpdate(p) => &p.command_id,
            Self::ProjectArchive(p) => &p.command_id,
            Self::AreaCreate(p) => &p.command_id,
            Self::AreaRename(p) => &p.command_id,
            Self::AreaSetTeam(p) => &p.command_id,
            Self::AreaSetLead(p) => &p.command_id,
        }
    }
}

/// A command id for a test: the tag made into a ULID in its canonical form (26 uppercase Crockford characters, the first 0 to 7),
/// the same tag always giving the same id and different tags different ones.
#[cfg(test)]
pub fn test_command_id(tag: &str) -> String {
    const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut hash: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    for byte in tag.bytes() {
        hash ^= u128::from(byte);
        hash = hash.wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b);
    }
    let mut id = String::from("01ARZ3NDEK");
    for i in 0..16 {
        id.push(char::from(CROCKFORD[((hash >> (i * 5)) & 31) as usize]));
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use org_proto::org::{
        AreaCreate, AreaRename, AreaSetLead, AreaSetTeam, MemberAddAgent, MemberAddPerson,
        MemberMove, MemberOffboard, MemberRemove, MemberSetAgentStatus, MemberSetReportsTo,
        MemberSetRunMode, MemberUpdateAgent, MemberUpdatePerson, ProjectAddTeam, ProjectArchive,
        ProjectCreate, ProjectRemoveTeam, ProjectUpdate, TeamAddMember, TeamCreate, TeamDelete,
        TeamMove, TeamRemoveMember, TeamRename, TeamSetLead, TeamSetTitle,
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
            "member.add_agent",
            "member.update_agent",
            "member.set_agent_status",
            "member.set_run_mode",
            "member.offboard",
            "project.add_team",
            "project.remove_team",
            "project.create",
            "project.update",
            "project.archive",
            "area.create",
            "area.rename",
            "area.set_team",
            "area.set_lead",
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
            MemberAddAgent::METHOD,
            MemberUpdateAgent::METHOD,
            MemberSetAgentStatus::METHOD,
            MemberSetRunMode::METHOD,
            MemberOffboard::METHOD,
            ProjectAddTeam::METHOD,
            ProjectRemoveTeam::METHOD,
            ProjectCreate::METHOD,
            ProjectUpdate::METHOD,
            ProjectArchive::METHOD,
            AreaCreate::METHOD,
            AreaRename::METHOD,
            AreaSetTeam::METHOD,
            AreaSetLead::METHOD,
        ];
        assert_eq!(methods, contract);
        // The enum's own wire names: each parses with the contract's params for it (so a typo in a
        // `rename` can't pass), and writes back under the same name.
        let c = |extra: serde_json::Value| {
            let mut p = serde_json::json!({"command_id": "c"});
            p.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            p
        };
        let params = [
            c(serde_json::json!({"team_id": "t", "name": "n"})),
            c(serde_json::json!({"team_id": "t", "expected_version": 1, "name": "n"})),
            c(serde_json::json!({"team_id": "t", "expected_version": 1})),
            c(serde_json::json!({"team_id": "t", "expected_version": 1})),
            c(serde_json::json!({"team_id": "t", "expected_version": 1})),
            c(serde_json::json!({"membership_id": "m", "team_id": "t", "member_id": "p"})),
            c(serde_json::json!({"membership_id": "m", "expected_version": 1})),
            c(serde_json::json!({"membership_id": "m", "expected_version": 1})),
            c(serde_json::json!({"member_id": "p", "name": "n"})),
            c(serde_json::json!({"member_id": "p", "expected_version": 1})),
            c(serde_json::json!({"member_id": "p", "expected_version": 1})),
            c(serde_json::json!({"member_id": "p", "expected_version": 1})),
            c(
                serde_json::json!({"membership_id": "m", "expected_version": 1, "new_membership_id": "n", "to_team_id": "t"}),
            ),
            c(
                serde_json::json!({"member_id": "a", "name": "A", "office_id": "o", "profile": {"role": "r", "title": "t", "runtime": "claude_code", "messaging_name": "a", "status": "active"}}),
            ),
            c(serde_json::json!({"member_id": "a", "expected_version": 1, "role": "r"})),
            c(serde_json::json!({"member_id": "a", "expected_version": 1, "status": "paused"})),
            c(serde_json::json!({"member_id": "a", "expected_version": 1, "run_mode": "on_call"})),
            c(serde_json::json!({"member_id": "a", "expected_version": 1})),
            c(serde_json::json!({"project_team_id": "x", "project_id": "p", "team_id": "t"})),
            c(serde_json::json!({"project_team_id": "x", "expected_version": 1})),
            c(serde_json::json!({"project_id": "p", "name": "n", "general_area_id": "g"})),
            c(serde_json::json!({"project_id": "p", "expected_version": 1, "name": "n"})),
            c(serde_json::json!({"project_id": "p", "expected_version": 1, "archived": true})),
            c(serde_json::json!({"area_id": "a", "project_id": "p", "name": "n"})),
            c(serde_json::json!({"area_id": "a", "expected_version": 1, "name": "n"})),
            c(serde_json::json!({"area_id": "a", "expected_version": 1})),
            c(serde_json::json!({"area_id": "a", "expected_version": 1})),
        ];
        for (method, params) in methods.iter().zip(params) {
            let wire = serde_json::json!({"method": method, "params": params});
            let command: OrgCommand = serde_json::from_value(wire.clone())
                .unwrap_or_else(|e| panic!("{method} doesn't parse: {e}"));
            assert_eq!(serde_json::to_value(&command).unwrap()["method"], *method);
        }
        let unknown = serde_json::json!({"method": "team.explode", "params": {"command_id": "c"}});
        assert!(serde_json::from_value::<OrgCommand>(unknown).is_err());
    }
}
