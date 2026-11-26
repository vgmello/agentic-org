//! `agentic-org project list|inspect|create` (implementation plan B3, FC-030, FC-038):
//! native session calls over `host.sock` -> `office.open_channel` -> `session.grant` ->
//! HQ office (`project.list`, `project.inspect`, `project.create`).
use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;

use org_host_control::ops::{
    FdsReceived, FdsReceivedParams, OfficeList, OfficeListParams, OfficeOpenChannel,
    OfficeOpenChannelParams, SessionGrant, SessionGrantParams,
};
use org_host_control::SessionHandle;
use org_proto::org::{ProjectCreate, ProjectCreateParams};
use org_proto::projects::{
    ProjectInspect, ProjectInspectParams, ProjectList, ProjectListParams, ProjectListResult,
    ProjectRecord,
};
use org_proto::{
    frame, ApiError, ApiRange, Call, ErrorCode, Hello, HelloParams, OfficeRole, Principal, Request,
    Response,
};

use super::cli_error::{office_refused, refused_by, HostCliError};
use super::cli_session::{connect_cli_session, end_session, AskPassword, Target};
use super::wire::registered;
use crate::human::Wire;

/// Help text for `agentic-org project`.
pub const HELP: &str = "agentic-org project <subcommand>\n\
Use when: listing, inspecting, or creating organization projects from the operator CLI.\n\n\
Subcommands:\n  \
list [--org <org_id>] [--include-archived] [--login <login>] [--name <instance>] [--json]\n      \
List projects in an organization\n      \
Use when: discovering active (or archived with --include-archived) projects, keys, slugs, and owning teams.\n  \
inspect <project> [--org <org_id>] [--login <login>] [--name <instance>] [--json]\n      \
Inspect one project by ID, P-<number> key, or slug\n      \
Use when: checking one project's owning team, contributing teams, and status.\n  \
create <name> [--org <org_id>] [--slug <slug>] [--owner-team <team_id>] [--parent-team <team_id>] [--login <login>] [--name <instance>] [--json]\n      \
Create a project (creates a dedicated owning team automatically unless --owner-team is given)\n      \
Use when: starting a new project with an accountable owning team.\n\n\
When --login is omitted, uses the local authless session if authentication is disabled,\n\
or prints a browser approval link to stderr when authentication is enabled.";

/// Parameters for `agentic-org project create`.
#[derive(Debug, Clone, Default)]
pub struct ProjectCreateInput<'a> {
    pub org_id: Option<&'a str>,
    pub name: &'a str,
    pub slug: Option<&'a str>,
    pub owner_team_id: Option<&'a str>,
    pub parent_team_id: Option<&'a str>,
}

/// `agentic-org project list` for registered instance `instance_name`.
pub fn project_list(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    include_archived: bool,
) -> Result<ProjectListResult, HostCliError> {
    let (_, id, socket) = registered(&crate::human::paths(instance_name)?, instance_name)?;
    let mut err_out = std::io::stderr().lock();
    list(
        &Target {
            socket: &socket,
            instance_id: &id,
            name: instance_name,
        },
        login,
        org_id,
        include_archived,
        &mut crate::human::password,
        &mut err_out,
    )
}

/// `agentic-org project inspect` for registered instance `instance_name`.
pub fn project_inspect(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    project: &str,
) -> Result<ProjectRecord, HostCliError> {
    let (_, id, socket) = registered(&crate::human::paths(instance_name)?, instance_name)?;
    let mut err_out = std::io::stderr().lock();
    inspect(
        &Target {
            socket: &socket,
            instance_id: &id,
            name: instance_name,
        },
        login,
        org_id,
        project,
        &mut crate::human::password,
        &mut err_out,
    )
}

/// `agentic-org project create` for registered instance `instance_name`: the created project.
pub fn project_create(
    instance_name: Option<&str>,
    login: Option<&str>,
    input: &ProjectCreateInput<'_>,
) -> Result<ProjectRecord, HostCliError> {
    let (_, id, socket) = registered(&crate::human::paths(instance_name)?, instance_name)?;
    let mut err_out = std::io::stderr().lock();
    create(
        &Target {
            socket: &socket,
            instance_id: &id,
            name: instance_name,
        },
        login,
        input,
        &mut crate::human::password,
        &mut err_out,
    )
}

pub(super) struct OfficeSession {
    office_id: String,
    channel_id: String,
    reader: BufReader<UnixStream>,
    next_id: u64,
}

impl OfficeSession {
    pub(super) fn open(
        wire: &mut Wire,
        session: &SessionHandle,
        org_id: Option<&str>,
    ) -> Result<Self, HostCliError> {
        Self::open_selected(wire, session, org_id, None, false)
    }

    pub(super) fn open_hq(
        wire: &mut Wire,
        session: &SessionHandle,
        org_id: Option<&str>,
    ) -> Result<Self, HostCliError> {
        Self::open_selected(wire, session, org_id, None, true)
    }

    pub(super) fn open_for_office(
        wire: &mut Wire,
        session: &SessionHandle,
        org_id: Option<&str>,
        office_id: Option<&str>,
    ) -> Result<Self, HostCliError> {
        Self::open_selected(wire, session, org_id, office_id, false)
    }

    fn open_selected(
        wire: &mut Wire,
        session: &SessionHandle,
        org_id: Option<&str>,
        explicit_office_id: Option<&str>,
        require_hq: bool,
    ) -> Result<Self, HostCliError> {
        let handle = || SessionHandle::new(session.expose().into()).expect("a live handle");
        let listed = match wire.call_answer::<OfficeList>(
            &OfficeListParams {
                session: handle(),
                org_id: org_id.map(str::to_owned),
            },
            None,
        )? {
            Ok(listed) => listed,
            Err(code @ ErrorCode::NotFound) => {
                return Err(HostCliError::refused(code, "organization not found"))
            }
            Err(code @ ErrorCode::NotAuthorized) => {
                return Err(HostCliError::refused(
                    code,
                    "not authorized to access this organization",
                ))
            }
            Err(code) => return Err(refused_by("the office lookup", code)),
        };
        let office_id = select_office(&listed.offices, org_id, explicit_office_id, require_hq)?;
        let opened = match wire.call_answer::<OfficeOpenChannel>(
            &OfficeOpenChannelParams {
                session: handle(),
                office_id: office_id.clone(),
            },
            None,
        )? {
            Ok(opened) => opened,
            Err(code @ ErrorCode::NotFound) => {
                return Err(HostCliError::refused(code, "office not found"))
            }
            Err(code @ ErrorCode::NotAuthorized) => {
                return Err(HostCliError::refused(
                    code,
                    "not authorized to open office channel",
                ))
            }
            Err(code) => return Err(refused_by("opening the office channel", code)),
        };
        let stream = wire.receive_office_channel()?;
        wire.call::<FdsReceived>(
            &FdsReceivedParams {
                of: opened.channel_id.clone(),
            },
            None,
        )?;
        let mut reader = BufReader::new(stream);
        office_hello(&mut reader)?;
        Ok(Self {
            office_id,
            channel_id: opened.channel_id,
            reader,
            next_id: 2,
        })
    }

    pub(super) fn call<C: Call>(
        &mut self,
        wire: &mut Wire,
        session: &SessionHandle,
        params: &C::Params,
    ) -> Result<Result<C::Result, ApiError>, HostCliError> {
        let handle = SessionHandle::new(session.expose().into()).expect("a live handle");
        let granted = match wire.call_answer::<SessionGrant>(
            &SessionGrantParams {
                session: handle,
                office_id: self.office_id.clone(),
                channel_id: self.channel_id.clone(),
            },
            None,
        )? {
            Ok(granted) => granted,
            Err(code @ ErrorCode::NotAuthorized) => {
                return Err(HostCliError::refused(
                    code,
                    "not authorized for this office",
                ))
            }
            Err(code) => return Err(refused_by("the office grant", code)),
        };
        let rpc_id = self.next_id;
        self.next_id += 1;
        let principal = Principal {
            grant: granted.grant,
        };
        let req = Request::new::<C>(rpc_id, params)
            .and_then(|r| r.with_principal(&principal))
            .map_err(|_| "could not encode office request")?;
        frame::write_message(self.reader.get_mut(), &req)
            .map_err(|_| "could not send office request")?;
        let reply: Response = frame::read_message(&mut self.reader)
            .map_err(|_| "could not read office reply")?
            .ok_or("office closed channel before replying")?;
        if reply.id != rpc_id {
            return Err("office reply id mismatch".into());
        }
        match reply.into_result::<C>() {
            Ok(value) => Ok(Ok(value)),
            Err(org_proto::CallError::Api(error)) => Ok(Err(error)),
            Err(org_proto::CallError::Decode(_)) => Err("could not decode office reply".into()),
        }
    }
}

/// Signs in to the registered instance `instance_name` (password with `login`, else the
/// local session or browser approval, its link on standard error), opens a channel to its
/// HQ office (`require_hq`) or to the selected office, sends one `C` call and ends the
/// session. The office's refusal is the inner error.
pub(super) fn call_office<C: Call>(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    office_id: Option<&str>,
    require_hq: bool,
    params: &C::Params,
) -> Result<Result<C::Result, ApiError>, HostCliError> {
    let (_, id, socket) = registered(&crate::human::paths(instance_name)?, instance_name)?;
    let target = Target {
        socket: &socket,
        instance_id: &id,
        name: instance_name,
    };
    let mut err_out = std::io::stderr().lock();
    let (mut wire, session) =
        connect_cli_session(&target, login, &mut crate::human::password, &mut err_out)?;
    let answer = (|| {
        let mut office = if require_hq {
            OfficeSession::open_hq(&mut wire, &session, org_id)?
        } else {
            OfficeSession::open_for_office(&mut wire, &session, org_id, office_id)?
        };
        office.call::<C>(&mut wire, &session, params)
    })();
    end_session(&mut wire, &session);
    answer
}

/// The office a command acts on, among those the account sees: the one named, HQ, or the
/// only one. Choosing among several organizations or offices is the person's (`Usage`).
fn select_office(
    offices: &[org_proto::OfficeInfo],
    org_id: Option<&str>,
    explicit_office_id: Option<&str>,
    require_hq: bool,
) -> Result<String, HostCliError> {
    if offices.is_empty() {
        return Err(HostCliError::refused(
            None,
            "no offices found for this account",
        ));
    }
    if org_id.is_none() && explicit_office_id.is_none() {
        let distinct_orgs: std::collections::BTreeSet<&str> =
            offices.iter().map(|o| o.organization_id.as_str()).collect();
        if distinct_orgs.len() > 1 {
            return Err(HostCliError::usage(
                "multiple organizations found; pass --org <org_id>",
            ));
        }
    }
    let hq = offices.iter().find(|o| o.role == OfficeRole::Hq);
    if let Some(requested) = explicit_office_id {
        offices
            .iter()
            .find(|o| o.id == requested)
            .map(|o| o.id.clone())
            .ok_or_else(|| HostCliError::refused(None, format!("office `{requested}` not found")))
    } else if require_hq {
        hq.map(|o| o.id.clone()).ok_or_else(|| {
            HostCliError::refused(
                None,
                "no HQ office found on this host; run this command on the HQ instance",
            )
        })
    } else if let Some(hq) = hq {
        Ok(hq.id.clone())
    } else if offices.len() == 1 {
        Ok(offices[0].id.clone())
    } else {
        Err(HostCliError::usage(
            "multiple offices found; pass --office <office_id>",
        ))
    }
}

/// Negotiates protocol version 2 on a freshly opened office channel (request ID 1).
pub(super) fn office_hello(reader: &mut BufReader<UnixStream>) -> Result<(), HostCliError> {
    let mut hello = HelloParams::default();
    hello.api = Some(ApiRange { min: 2, max: 2 });
    hello.client = Some(format!("agentic-org {}", env!("CARGO_PKG_VERSION")));
    let request = Request::new::<Hello>(1, &hello).map_err(|_| "could not encode office hello")?;
    frame::write_message(reader.get_mut(), &request).map_err(|_| "could not write office hello")?;
    let reply: Response = frame::read_message(reader)
        .map_err(|_| "could not read office hello")?
        .ok_or("office closed channel during hello")?;
    let negotiated = reply
        .into_result::<Hello>()
        .map_err(|_| "office refused hello negotiation")?;
    if negotiated.api_version != 2 {
        return Err("office negotiated unexpected protocol version".into());
    }
    Ok(())
}

pub(crate) fn list(
    target: &Target<'_>,
    login: Option<&str>,
    org_id: Option<&str>,
    include_archived: bool,
    password: AskPassword<'_>,
    prompt_out: &mut dyn Write,
) -> Result<ProjectListResult, HostCliError> {
    let (mut wire, session) = connect_cli_session(target, login, password, prompt_out)?;
    let answer = (|| {
        let mut office = OfficeSession::open(&mut wire, &session, org_id)?;
        office.call::<ProjectList>(&mut wire, &session, &ProjectListParams { include_archived })
    })();
    end_session(&mut wire, &session);
    answer?.map_err(|error| office_refused("project list", &error))
}

pub(crate) fn inspect(
    target: &Target<'_>,
    login: Option<&str>,
    org_id: Option<&str>,
    project_ref: &str,
    password: AskPassword<'_>,
    prompt_out: &mut dyn Write,
) -> Result<ProjectRecord, HostCliError> {
    let (mut wire, session) = connect_cli_session(target, login, password, prompt_out)?;
    let answer = (|| {
        let mut office = OfficeSession::open(&mut wire, &session, org_id)?;
        office.call::<ProjectInspect>(
            &mut wire,
            &session,
            &ProjectInspectParams {
                project: project_ref.to_owned(),
            },
        )
    })();
    end_session(&mut wire, &session);
    answer?.map_err(|error| match error.code {
        ErrorCode::NotFound => HostCliError::refused(error.code, "project not found"),
        _ => office_refused("project inspect", &error),
    })
}

pub(crate) fn create(
    target: &Target<'_>,
    login: Option<&str>,
    input: &ProjectCreateInput<'_>,
    password: AskPassword<'_>,
    prompt_out: &mut dyn Write,
) -> Result<ProjectRecord, HostCliError> {
    let project_id =
        crate::ids::uuid_v7().map_err(|e| format!("could not mint project id: {e}"))?;
    let command_id =
        crate::ids::uuid_v7().map_err(|e| format!("could not mint command id: {e}"))?;
    let create_team_id = if input.owner_team_id.is_none() {
        Some(crate::ids::uuid_v7().map_err(|e| format!("could not mint team id: {e}"))?)
    } else {
        None
    };
    let (mut wire, session) = connect_cli_session(target, login, password, prompt_out)?;
    let answer = (|| -> Result<Result<ProjectRecord, ApiError>, HostCliError> {
        let mut office = OfficeSession::open(&mut wire, &session, input.org_id)?;
        let params = ProjectCreateParams {
            project_id: project_id.clone(),
            name: input.name.to_owned(),
            key: None,
            description: None,
            teams: Vec::new(),
            general_area_id: String::new(),
            general_area_name: None,
            repo: None,
            integration_branch: None,
            slug: input.slug.map(str::to_owned),
            owner_team_id: input.owner_team_id.map(str::to_owned),
            create_team_id,
            parent_team_id: input.parent_team_id.map(str::to_owned),
            contributing_team_ids: Vec::new(),
            command_id,
        };
        if let Err(error) = office.call::<ProjectCreate>(&mut wire, &session, &params)? {
            return Ok(Err(error));
        }
        office.call::<ProjectInspect>(
            &mut wire,
            &session,
            &ProjectInspectParams {
                project: project_id.clone(),
            },
        )
    })();
    end_session(&mut wire, &session);
    answer?.map_err(|error| office_refused("project create", &error))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn office(id: &str, org: &str, role: OfficeRole) -> org_proto::OfficeInfo {
        org_proto::OfficeInfo {
            id: id.into(),
            organization_id: org.into(),
            name: "Office".into(),
            role,
            host: "This Computer".into(),
            endpoint: None,
            status: org_proto::OfficeStatus::Active,
            status_reason: None,
            next_restart_at: None,
        }
    }

    #[test]
    fn the_office_is_the_named_one_hq_or_the_only_one_and_ambiguity_is_usage() {
        use super::super::HostCliErrorKind::{Refused, Usage};
        let hq = office("hq", "org", OfficeRole::Hq);
        let sat = office("sat", "org", OfficeRole::Satellite);
        let both = [hq.clone(), sat.clone()];
        assert_eq!(select_office(&both, None, None, false).unwrap(), "hq");
        assert_eq!(
            select_office(&both, None, Some("sat"), false).unwrap(),
            "sat"
        );
        assert_eq!(select_office(&both, None, None, true).unwrap(), "hq");
        assert_eq!(
            select_office(std::slice::from_ref(&sat), None, None, false).unwrap(),
            "sat"
        );
        let kind = |offices: &[org_proto::OfficeInfo], explicit, hq_only| {
            select_office(offices, None, explicit, hq_only)
                .unwrap_err()
                .kind
        };
        assert_eq!(kind(&[], None, false), Refused);
        assert_eq!(kind(&both, Some("missing"), false), Refused);
        assert_eq!(kind(std::slice::from_ref(&sat), None, true), Refused);
        let two_sats = [sat.clone(), office("sat2", "org", OfficeRole::Satellite)];
        assert_eq!(kind(&two_sats, None, false), Usage);
        let two_orgs = [hq, office("other", "org2", OfficeRole::Hq)];
        assert_eq!(kind(&two_orgs, None, false), Usage);
        assert_eq!(
            select_office(&two_orgs, Some("org2"), None, false).unwrap(),
            "hq"
        );
    }

    #[test]
    fn help_includes_use_when_guidance_for_each_subcommand() {
        assert!(
            HELP.matches("Use when:").count() >= 4,
            "expected command + 3 subcommand Use when cues:\n{HELP}"
        );
        // Browser approval is the default; `--login` is optional on every subcommand.
        assert!(!HELP.contains("--login <login> ["), "{HELP}");
        assert!(HELP.contains("browser approval link"), "{HELP}");
    }
}
