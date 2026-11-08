use org_proto::org::{
    MemberAddPerson, MemberAddPersonParams, MemberMove, MemberMoveParams, MemberRemove,
    MemberRemoveParams, MemberSetReportsTo, MemberSetReportsToParams, MemberUpdatePerson,
    MemberUpdatePersonParams, ProjectAddTeam, ProjectAddTeamParams, ProjectRemoveTeam,
    ProjectRemoveTeamParams, TeamAddMember, TeamAddMemberParams, TeamCreate, TeamCreateParams,
    TeamDelete, TeamDeleteParams, TeamMove, TeamMoveParams, TeamRemoveMember,
    TeamRemoveMemberParams, TeamRename, TeamRenameParams, TeamSetLead, TeamSetLeadParams,
    TeamSetTitle, TeamSetTitleParams,
};
use org_proto::{ApiError, ErrorCode, Request};

/// An organization command, read from a request. Agent commands join in a later change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrgCommand {
    TeamCreate(TeamCreateParams),
    TeamRename(TeamRenameParams),
    TeamMove(TeamMoveParams),
    TeamSetLead(TeamSetLeadParams),
    TeamDelete(TeamDeleteParams),
    TeamAddMember(TeamAddMemberParams),
    TeamRemoveMember(TeamRemoveMemberParams),
    TeamSetTitle(TeamSetTitleParams),
    MemberAddPerson(MemberAddPersonParams),
    MemberUpdatePerson(MemberUpdatePersonParams),
    MemberMove(MemberMoveParams),
    MemberRemove(MemberRemoveParams),
    MemberSetReportsTo(MemberSetReportsToParams),
    ProjectAddTeam(ProjectAddTeamParams),
    ProjectRemoveTeam(ProjectRemoveTeamParams),
}

impl OrgCommand {
    /// Reads `request` as an organization command: `Ok(None)` if its method is not one this
    /// build handles, `Err(invalid_request)` if the method is but its params are not valid.
    pub fn parse(request: &Request) -> Result<Option<Self>, ApiError> {
        macro_rules! read {
            ($($call:ident => $variant:ident),* $(,)?) => {
                $(if request.is::<$call>() {
                    return request
                        .params::<$call>()
                        .map(|params| Some(Self::$variant(params)))
                        .map_err(|_| {
                            ApiError::new(
                                ErrorCode::InvalidRequest,
                                format!("the params of `{}` are not valid", request.method),
                            )
                        });
                })*
            };
        }
        read!(
            TeamCreate => TeamCreate,
            TeamRename => TeamRename,
            TeamMove => TeamMove,
            TeamSetLead => TeamSetLead,
            TeamDelete => TeamDelete,
            TeamAddMember => TeamAddMember,
            TeamRemoveMember => TeamRemoveMember,
            TeamSetTitle => TeamSetTitle,
            MemberAddPerson => MemberAddPerson,
            MemberUpdatePerson => MemberUpdatePerson,
            MemberMove => MemberMove,
            MemberRemove => MemberRemove,
            MemberSetReportsTo => MemberSetReportsTo,
            ProjectAddTeam => ProjectAddTeam,
            ProjectRemoveTeam => ProjectRemoveTeam,
        );
        Ok(None)
    }

    /// The client-made id of this command, which makes a retry return the same answer.
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
            Self::MemberMove(p) => &p.command_id,
            Self::MemberRemove(p) => &p.command_id,
            Self::MemberSetReportsTo(p) => &p.command_id,
            Self::ProjectAddTeam(p) => &p.command_id,
            Self::ProjectRemoveTeam(p) => &p.command_id,
        }
    }
}
