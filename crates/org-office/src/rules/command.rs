use org_proto::org::{
    MemberAddAgent, MemberAddAgentParams, MemberAddPerson, MemberAddPersonParams, MemberMove,
    MemberMoveParams, MemberOffboard, MemberOffboardParams, MemberRemove, MemberRemoveParams,
    MemberSetAgentStatus, MemberSetAgentStatusParams, MemberSetReportsTo, MemberSetReportsToParams,
    MemberSetRunMode, MemberSetRunModeParams, MemberUpdateAgent, MemberUpdateAgentParams,
    MemberUpdatePerson, MemberUpdatePersonParams, ProjectAddTeam, ProjectAddTeamParams,
    ProjectRemoveTeam, ProjectRemoveTeamParams, TeamCreate, TeamCreateParams, TeamDelete,
    TeamDeleteParams, TeamMove, TeamMoveParams, TeamRename, TeamRenameParams, TeamSetLead,
    TeamSetLeadParams, TeamSetTitle, TeamSetTitleParams,
};
use org_proto::{ApiError, ErrorCode, Request};

/// An organization command, read from a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrgCommand {
    TeamCreate(TeamCreateParams),
    TeamRename(TeamRenameParams),
    TeamMove(TeamMoveParams),
    TeamSetLead(TeamSetLeadParams),
    TeamDelete(TeamDeleteParams),
    TeamSetTitle(TeamSetTitleParams),
    MemberAddPerson(MemberAddPersonParams),
    MemberAddAgent(MemberAddAgentParams),
    MemberUpdateAgent(MemberUpdateAgentParams),
    MemberSetAgentStatus(MemberSetAgentStatusParams),
    MemberOffboard(MemberOffboardParams),
    MemberSetRunMode(MemberSetRunModeParams),
    MemberUpdatePerson(MemberUpdatePersonParams),
    MemberMove(MemberMoveParams),
    MemberRemove(MemberRemoveParams),
    MemberSetReportsTo(MemberSetReportsToParams),
    ProjectAddTeam(ProjectAddTeamParams),
    ProjectRemoveTeam(ProjectRemoveTeamParams),
}

/// A decoded organization command plus whether the caller asked for a dry-run check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedOrgCommand {
    pub command: OrgCommand,
    pub dry_run: bool,
}

impl OrgCommand {
    /// Whether `method` names an organization command known to this build.
    fn is_org_method(request: &Request) -> bool {
        use org_proto::Call;
        const METHODS: &[&str] = &[
            TeamCreate::METHOD,
            TeamRename::METHOD,
            TeamMove::METHOD,
            TeamSetLead::METHOD,
            TeamDelete::METHOD,
            TeamSetTitle::METHOD,
            MemberAddPerson::METHOD,
            MemberAddAgent::METHOD,
            MemberUpdateAgent::METHOD,
            MemberSetAgentStatus::METHOD,
            MemberOffboard::METHOD,
            MemberSetRunMode::METHOD,
            MemberUpdatePerson::METHOD,
            MemberMove::METHOD,
            MemberRemove::METHOD,
            MemberSetReportsTo::METHOD,
            ProjectAddTeam::METHOD,
            ProjectRemoveTeam::METHOD,
        ];
        METHODS.contains(&request.method.as_str())
    }

    /// Reads `request` as an organization command, also extracting optional `dry_run: bool`.
    /// When `dry_run` is `true`, `command_id` must be absent or empty (`invalid_request` otherwise).
    pub fn parse_request(request: &Request) -> Result<Option<ParsedOrgCommand>, ApiError> {
        if !Self::is_org_method(request) {
            return Ok(None);
        }
        let invalid = |msg: &str| ApiError::new(ErrorCode::InvalidRequest, msg);
        let dry_run = match request.params.as_object().and_then(|m| m.get("dry_run")) {
            None => false,
            Some(v) => v
                .as_bool()
                .ok_or_else(|| invalid("dry_run must be a boolean"))?,
        };
        if !dry_run {
            return Self::parse(request).map(|opt| {
                opt.map(|command| ParsedOrgCommand {
                    command,
                    dry_run: false,
                })
            });
        }
        let mut patched = request.clone();
        if let Some(obj) = patched.params.as_object_mut() {
            match obj.get("command_id") {
                None => {
                    obj.insert(
                        "command_id".into(),
                        serde_json::Value::String(String::new()),
                    );
                }
                Some(serde_json::Value::String(s)) if s.is_empty() => {}
                Some(_) => {
                    return Err(invalid("dry_run must not carry a command_id"));
                }
            }
        }
        Self::parse(&patched).map(|opt| {
            opt.map(|command| ParsedOrgCommand {
                command,
                dry_run: true,
            })
        })
    }

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
            TeamSetTitle => TeamSetTitle,
            MemberAddPerson => MemberAddPerson,
            MemberAddAgent => MemberAddAgent,
            MemberUpdateAgent => MemberUpdateAgent,
            MemberSetAgentStatus => MemberSetAgentStatus,
            MemberOffboard => MemberOffboard,
            MemberSetRunMode => MemberSetRunMode,
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
            Self::TeamSetTitle(p) => &p.command_id,
            Self::MemberAddPerson(p) => &p.command_id,
            Self::MemberAddAgent(p) => &p.command_id,
            Self::MemberUpdateAgent(p) => &p.command_id,
            Self::MemberSetAgentStatus(p) => &p.command_id,
            Self::MemberOffboard(p) => &p.command_id,
            Self::MemberSetRunMode(p) => &p.command_id,
            Self::MemberUpdatePerson(p) => &p.command_id,
            Self::MemberMove(p) => &p.command_id,
            Self::MemberRemove(p) => &p.command_id,
            Self::MemberSetReportsTo(p) => &p.command_id,
            Self::ProjectAddTeam(p) => &p.command_id,
            Self::ProjectRemoveTeam(p) => &p.command_id,
        }
    }
}
