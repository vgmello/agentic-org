//! `agentic-org project list|inspect|create` (implementation plan B3, FC-030, FC-038):
//! native session calls over `host.sock` -> `office.open_channel` -> `session.grant` ->
//! HQ office (`project.list`, `project.inspect`, `project.create`).
use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;

use org_host_control::ops::{
    FdsReceived, FdsReceivedParams, OfficeList, OfficeListParams, OfficeOpenChannel,
    OfficeOpenChannelParams, SessionEnd, SessionGrant, SessionGrantParams, SessionParams,
};
use org_host_control::SessionHandle;
use org_proto::org::{ProjectCreate, ProjectCreateParams};
use org_proto::projects::{
    ProjectInspect, ProjectInspectParams, ProjectList, ProjectListParams, ProjectRecord,
};
use org_proto::{
    frame, ApiError, ApiRange, Call, ErrorCode, Hello, HelloParams, OfficeRole, Principal, Request,
    Response,
};

use super::office_command::{connect_and_sign_in, AskPassword, Target};
use super::{registered, say};
use crate::human::Wire;

/// Help text for `agentic-org project`.
pub const HELP: &str = "agentic-org project <subcommand>\n\n\
Subcommands:\n  \
list --login <login> [--org <org_id>] [--include-archived] [--name <instance>] [--json]\n      \
List projects in an organization\n  \
inspect <project> --login <login> [--org <org_id>] [--name <instance>] [--json]\n      \
Inspect one project by ID, P-<number> key, or slug\n  \
create <name> --login <login> [--org <org_id>] [--slug <slug>] [--owner-team <team_id>] [--parent-team <team_id>] [--name <instance>] [--json]\n      \
Create a project (creates a dedicated owning team automatically unless --owner-team is given)\n\n\
Asks for the account password at the terminal. On systems that cannot identify the host's \
program (Linux, containers), terminal password sign-in is refused until browser-approved \
CLI sign-in is available.";

/// Parameters for `agentic-org project create`.
#[derive(Debug, Clone, Default)]
pub struct ProjectCreateInput<'a> {
    pub org_id: Option<&'a str>,
    pub name: &'a str,
    pub slug: Option<&'a str>,
    pub owner_team_id: Option<&'a str>,
    pub parent_team_id: Option<&'a str>,
    pub json: bool,
}

pub fn project_list(
    instance_name: Option<&str>,
    login: &str,
    org_id: Option<&str>,
    include_archived: bool,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (_, id, socket) = registered(&crate::human::paths(instance_name)?, instance_name)?;
    list(
        &Target {
            socket: &socket,
            instance_id: &id,
            name: instance_name,
        },
        login,
        org_id,
        include_archived,
        json,
        &mut crate::human::password,
        out,
    )
}

pub fn project_inspect(
    instance_name: Option<&str>,
    login: &str,
    org_id: Option<&str>,
    project: &str,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (_, id, socket) = registered(&crate::human::paths(instance_name)?, instance_name)?;
    inspect(
        &Target {
            socket: &socket,
            instance_id: &id,
            name: instance_name,
        },
        login,
        org_id,
        project,
        json,
        &mut crate::human::password,
        out,
    )
}

pub fn project_create(
    instance_name: Option<&str>,
    login: &str,
    input: &ProjectCreateInput<'_>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (_, id, socket) = registered(&crate::human::paths(instance_name)?, instance_name)?;
    create(
        &Target {
            socket: &socket,
            instance_id: &id,
            name: instance_name,
        },
        login,
        input,
        &mut crate::human::password,
        out,
    )
}

struct OfficeSession {
    office_id: String,
    channel_id: String,
    reader: BufReader<UnixStream>,
    next_id: u64,
}

impl OfficeSession {
    fn open(
        wire: &mut Wire,
        session: &SessionHandle,
        org_id: Option<&str>,
    ) -> Result<Self, String> {
        let handle = || SessionHandle::new(session.expose().into()).expect("a live handle");
        let listed = match wire.call_answer::<OfficeList>(
            &OfficeListParams {
                session: handle(),
                org_id: org_id.map(str::to_owned),
            },
            None,
        )? {
            Ok(listed) => listed,
            Err(ErrorCode::NotFound) => return Err("organization not found".into()),
            Err(ErrorCode::NotAuthorized) => {
                return Err("not authorized to access this organization".into())
            }
            Err(code) => return Err(format!("the service refused office lookup ({code:?})")),
        };
        if listed.offices.is_empty() {
            return Err("no offices found for this account".into());
        }
        if org_id.is_none() {
            let distinct_orgs: std::collections::BTreeSet<&str> = listed
                .offices
                .iter()
                .map(|o| o.organization_id.as_str())
                .collect();
            if distinct_orgs.len() > 1 {
                return Err("multiple organizations found; pass --org <org_id>".into());
            }
        }
        let office_id = listed
            .offices
            .iter()
            .find(|o| o.role == OfficeRole::Hq)
            .or_else(|| listed.offices.first())
            .map(|o| o.id.clone())
            .ok_or_else(|| "no HQ office found".to_owned())?;
        let opened = match wire.call_answer::<OfficeOpenChannel>(
            &OfficeOpenChannelParams {
                session: handle(),
                office_id: office_id.clone(),
            },
            None,
        )? {
            Ok(opened) => opened,
            Err(ErrorCode::NotFound) => return Err("office not found".into()),
            Err(ErrorCode::NotAuthorized) => {
                return Err("not authorized to open office channel".into())
            }
            Err(code) => return Err(format!("could not open office channel ({code:?})")),
        };
        let stream = wire.receive_office_channel()?;
        wire.call::<FdsReceived>(
            &FdsReceivedParams {
                of: opened.channel_id.clone(),
            },
            None,
        )?;
        let mut reader = BufReader::new(stream);
        let mut hello = HelloParams::default();
        hello.api = Some(ApiRange { min: 2, max: 2 });
        hello.client = Some(format!("agentic-org {}", env!("CARGO_PKG_VERSION")));
        let hello_req = Request::new::<Hello>(1, &hello)
            .map_err(|_| "could not encode office hello".to_owned())?;
        frame::write_message(reader.get_mut(), &hello_req)
            .map_err(|_| "could not write office hello".to_owned())?;
        let hello_reply: Response = frame::read_message(&mut reader)
            .map_err(|_| "could not read office hello".to_owned())?
            .ok_or_else(|| "office closed channel during hello".to_owned())?;
        let negotiated = hello_reply
            .into_result::<Hello>()
            .map_err(|_| "office refused hello negotiation".to_owned())?;
        if negotiated.api_version != 2 {
            return Err("office negotiated unexpected protocol version".into());
        }
        Ok(Self {
            office_id,
            channel_id: opened.channel_id,
            reader,
            next_id: 2,
        })
    }

    fn call<C: Call>(
        &mut self,
        wire: &mut Wire,
        session: &SessionHandle,
        params: &C::Params,
    ) -> Result<Result<C::Result, ApiError>, String> {
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
            Err(ErrorCode::NotAuthorized) => return Err("not authorized for this office".into()),
            Err(code) => return Err(format!("could not obtain office grant ({code:?})")),
        };
        let rpc_id = self.next_id;
        self.next_id += 1;
        let principal = Principal {
            grant: granted.grant,
        };
        let req = Request::new::<C>(rpc_id, params)
            .and_then(|r| r.with_principal(&principal))
            .map_err(|_| "could not encode office request".to_owned())?;
        frame::write_message(self.reader.get_mut(), &req)
            .map_err(|_| "could not send office request".to_owned())?;
        let reply: Response = frame::read_message(&mut self.reader)
            .map_err(|_| "could not read office reply".to_owned())?
            .ok_or_else(|| "office closed channel before replying".to_owned())?;
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

fn end_session(wire: &mut Wire, session: &SessionHandle) {
    let handle = SessionHandle::new(session.expose().into()).expect("a live handle");
    let _ = wire.call::<SessionEnd>(&SessionParams { session: handle }, None);
}

fn format_project_line(project: &ProjectRecord) -> String {
    let contributing = if project.contributing_team_ids.is_empty() {
        "-".to_owned()
    } else {
        project.contributing_team_ids.join(",")
    };
    let status = if project.archived {
        "archived"
    } else {
        "active"
    };
    format!(
        "{}\t{}\t{}\towner_team={}\tcontributing={}\tstatus={}\tname={}",
        project.key,
        project.slug,
        project.id,
        project.owner_team_id,
        contributing,
        status,
        project.name
    )
}

pub(crate) fn list(
    target: &Target<'_>,
    login: &str,
    org_id: Option<&str>,
    include_archived: bool,
    json: bool,
    password: AskPassword<'_>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (mut wire, session) = connect_and_sign_in(target, login, password)?;
    let answer = (|| {
        let mut office = OfficeSession::open(&mut wire, &session, org_id)?;
        office.call::<ProjectList>(&mut wire, &session, &ProjectListParams { include_archived })
    })();
    end_session(&mut wire, &session);
    let listed = match answer? {
        Ok(listed) => listed,
        Err(error) => {
            return Err(format!(
                "the office refused project list ({:?}): {}",
                error.code, error.message
            ))
        }
    };
    if json {
        let encoded = serde_json::to_string_pretty(&listed)
            .map_err(|_| "could not encode project list".to_owned())?;
        return say(out, encoded);
    }
    if listed.projects.is_empty() {
        return say(out, "No projects.");
    }
    for project in &listed.projects {
        say(out, format_project_line(project))?;
    }
    Ok(())
}

pub(crate) fn inspect(
    target: &Target<'_>,
    login: &str,
    org_id: Option<&str>,
    project_ref: &str,
    json: bool,
    password: AskPassword<'_>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (mut wire, session) = connect_and_sign_in(target, login, password)?;
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
    let project = match answer? {
        Ok(project) => project,
        Err(error) if error.code == ErrorCode::NotFound => return Err("project not found".into()),
        Err(error) => {
            return Err(format!(
                "the office refused project inspect ({:?}): {}",
                error.code, error.message
            ))
        }
    };
    if json {
        let encoded = serde_json::to_string_pretty(&project)
            .map_err(|_| "could not encode project".to_owned())?;
        return say(out, encoded);
    }
    say(out, format_project_line(&project))
}

pub(crate) fn create(
    target: &Target<'_>,
    login: &str,
    input: &ProjectCreateInput<'_>,
    password: AskPassword<'_>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let project_id =
        crate::ids::uuid_v7().map_err(|e| format!("could not mint project id: {e}"))?;
    let command_id =
        crate::ids::uuid_v7().map_err(|e| format!("could not mint command id: {e}"))?;
    let create_team_id = if input.owner_team_id.is_none() {
        Some(crate::ids::uuid_v7().map_err(|e| format!("could not mint team id: {e}"))?)
    } else {
        None
    };
    let (mut wire, session) = connect_and_sign_in(target, login, password)?;
    let answer = (|| -> Result<Result<ProjectRecord, ApiError>, String> {
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
        match office.call::<ProjectCreate>(&mut wire, &session, &params)? {
            Ok(_) => {}
            Err(error) => return Ok(Err(error)),
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
    let project = match answer? {
        Ok(project) => project,
        Err(error) => {
            return Err(format!(
                "the office refused project create ({:?}): {}",
                error.code, error.message
            ))
        }
    };
    if input.json {
        let encoded = serde_json::to_string_pretty(&project)
            .map_err(|_| "could not encode created project".to_owned())?;
        return say(out, encoded);
    }
    say(out, format_project_line(&project))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_project_line_renders_key_slug_owner_and_contributing_teams() {
        let project = ProjectRecord {
            id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071f1".into(),
            number: 1,
            key: "P-1".into(),
            slug: "billing".into(),
            name: "Billing".into(),
            owner_team_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071f2".into(),
            contributing_team_ids: vec!["0190a5d2-7c3e-7a01-8b2c-3d4e5f6071f3".into()],
            archived: false,
            revision: 1,
            created_at: None,
            updated_at: None,
        };
        let line = format_project_line(&project);
        assert!(line.starts_with("P-1\tbilling\t0190a5d2-"), "{line}");
        assert!(
            line.contains("owner_team=0190a5d2-7c3e-7a01-8b2c-3d4e5f6071f2"),
            "{line}"
        );
        assert!(
            line.contains("contributing=0190a5d2-7c3e-7a01-8b2c-3d4e5f6071f3"),
            "{line}"
        );
        assert!(line.contains("status=active\tname=Billing"), "{line}");
    }
}
