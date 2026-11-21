//! Browser-approved terminal sign-in (`agentic-org sign-in`) and grant-bound office channel
//! execution (FC-173 / SIGN-40 / SES-8). Never asks for a password at the terminal.

use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use org_host_control::ops::{
    CliApprovalCreate, CliApprovalCreateParams, CliApprovalPoll, CliApprovalPollParams,
    CliApprovalPollResult, FdsReceived, FdsReceivedParams, LinkedOrg, OfficeOpenChannel,
    OfficeOpenChannelParams, SessionEnd, SessionGrant, SessionGrantParams, SessionParams,
};
use org_host_control::SessionHandle;
use org_proto::{
    frame, ApiError, ApiRange, ErrorCode, Hello, HelloParams, Principal, Request, Response,
};
use serde_json::Value;

use super::cli_context::{self, CliContext};
use super::office_command::Target;
use super::{connect_verified, not_running, registered, say};
use crate::human::Wire;

/// Help text for `agentic-org sign-in`.
pub const SIGN_IN_HELP: &str =
    "agentic-org sign-in [--name <instance>] [--office <office_id>] [-- <command...>]\n\n\
Prints a one-time approval link (expires in 10 minutes). Open it in a signed-in browser\n\
and approve to sign this terminal in without entering a password (SIGN-40 / SES-8).\n\
When `-- <command...>` is supplied, runs the organization/project CLI command over the\n\
approved grant-bound channel and ends the session on exit.";

/// Help text for `agentic-org context`.
pub const CONTEXT_HELP: &str = "agentic-org context <subcommand>\n\n\
Subcommands:\n  \
show                                          Show the saved instance and office context\n  \
use --office <office_id> [--name <instance>]  Save default instance and office selection\n  \
clear                                         Remove the saved CLI context";

/// An approved connection-bound `host.sock` session paired with an open office channel.
/// Dropping it sends `session.end` so grants are revoked immediately (`SES-8`).
pub struct ApprovedOfficeChannel {
    wire: Wire,
    session: SessionHandle,
    office_id: String,
    channel_id: String,
    reader: BufReader<UnixStream>,
    next_id: u64,
}

impl ApprovedOfficeChannel {
    pub fn office_id(&self) -> &str {
        &self.office_id
    }

    /// Sends one JSON-RPC call over the grant-bound host channel, minting a fresh
    /// `session.grant` per call.
    pub fn call_method(&mut self, method: &str, params: Value) -> Result<Value, ApiError> {
        let handle = SessionHandle::new(self.session.expose().into()).expect("a live handle");
        let granted = self
            .wire
            .call_answer::<SessionGrant>(
                &SessionGrantParams {
                    session: handle,
                    office_id: self.office_id.clone(),
                    channel_id: self.channel_id.clone(),
                },
                None,
            )
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "host grant transport unavailable"))?
            .map_err(|code| ApiError::new(code, "host refused office grant"))?;

        let rpc_id = self.next_id;
        self.next_id += 1;
        let principal = Principal {
            grant: granted.grant,
        };
        let request = Request {
            id: rpc_id,
            method: method.to_owned(),
            params,
            principal: Some(
                serde_json::to_value(&principal)
                    .map_err(|_| ApiError::new(ErrorCode::Unavailable, "could not encode grant"))?,
            ),
        };
        frame::write_message(self.reader.get_mut(), &request)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "could not write office request"))?;
        let reply: Response = frame::read_message(&mut self.reader)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "could not read office reply"))?
            .ok_or_else(|| {
                ApiError::new(
                    ErrorCode::Unavailable,
                    "office closed channel before replying",
                )
            })?;
        if reply.id != rpc_id {
            return Err(ApiError::new(
                ErrorCode::Unavailable,
                "office reply id mismatch",
            ));
        }
        match reply.outcome {
            org_proto::Outcome::Result(value) => Ok(value),
            org_proto::Outcome::Error(error) => Err(error),
        }
    }
}

impl Drop for ApprovedOfficeChannel {
    fn drop(&mut self) {
        let handle = SessionHandle::new(self.session.expose().into()).expect("a live handle");
        let _ = self
            .wire
            .call::<SessionEnd>(&SessionParams { session: handle }, None);
    }
}

/// Runs the browser-approved sign-in flow against `target`, printing the one-time approval
/// URL to `out` and polling until approved, declined, expired, or timed out.
pub(crate) fn approve_on_target(
    target: &Target<'_>,
    poll_interval: Duration,
    max_wait: Duration,
    out: &mut dyn Write,
) -> Result<(Wire, SessionHandle, String, Vec<LinkedOrg>), String> {
    let mut wire = connect_verified(target.socket, target.instance_id, |_stream: &UnixStream| {
        Ok(())
    })
    .map_err(|why| not_running(target.name, why))?;

    let created = match wire.call_answer::<CliApprovalCreate>(&CliApprovalCreateParams {}, None)? {
        Ok(created) => created,
        Err(ErrorCode::NotAuthorized) => {
            return Err("the host refused terminal sign-in approval".into())
        }
        Err(code) => return Err(format!("could not start terminal sign-in ({code:?})")),
    };
    say(
        out,
        format!(
            "Open this approval link in a signed-in browser (expires in 10 minutes):\n  {}",
            created.url.expose()
        ),
    )?;
    let _ = out.flush();

    let deadline = Instant::now() + max_wait;
    loop {
        match wire.call_answer::<CliApprovalPoll>(
            &CliApprovalPollParams {
                approval_id: created.approval_id.clone(),
            },
            None,
        )? {
            Ok(CliApprovalPollResult::Pending) => {
                if Instant::now() >= deadline {
                    return Err(
                        "approval link expired before it was approved; run the command again"
                            .into(),
                    );
                }
                std::thread::sleep(poll_interval);
            }
            Ok(CliApprovalPollResult::Approved {
                session,
                account,
                orgs,
                ..
            }) => return Ok((wire, session, account, orgs)),
            Ok(CliApprovalPollResult::Declined) => {
                return Err("terminal sign-in was declined in the browser".into())
            }
            Ok(CliApprovalPollResult::Expired) => {
                return Err("approval link expired; run the command again".into())
            }
            Err(ErrorCode::NotAuthorized) => {
                return Err("terminal sign-in approval is not authorized".into())
            }
            Err(code) => return Err(format!("terminal sign-in poll failed ({code:?})")),
        }
    }
}

/// Opens an authenticated host-mediated channel to `office_id` on an already-approved
/// `host.sock` session (`SES-8`).
pub(crate) fn open_approved_office(
    mut wire: Wire,
    session: SessionHandle,
    office_id: String,
) -> Result<ApprovedOfficeChannel, String> {
    let handle = SessionHandle::new(session.expose().into()).expect("a live handle");
    let opened = match wire.call_answer::<OfficeOpenChannel>(
        &OfficeOpenChannelParams {
            session: handle,
            office_id: office_id.clone(),
        },
        None,
    )? {
        Ok(opened) => opened,
        Err(ErrorCode::NotFound) => return Err(format!("office `{office_id}` not found")),
        Err(ErrorCode::NotAuthorized) => {
            return Err(format!("not authorized for office `{office_id}`"))
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
    let req =
        Request::new::<Hello>(1, &hello).map_err(|_| "could not encode office hello".to_owned())?;
    frame::write_message(reader.get_mut(), &req)
        .map_err(|_| "could not write office hello".to_owned())?;
    let reply: Response = frame::read_message(&mut reader)
        .map_err(|_| "could not read office hello".to_owned())?
        .ok_or_else(|| "office closed channel during hello".to_owned())?;
    let negotiated = reply
        .into_result::<Hello>()
        .map_err(|_| "office refused hello negotiation".to_owned())?;
    if negotiated.api_version != 2 {
        return Err("office negotiated unexpected protocol version".into());
    }
    Ok(ApprovedOfficeChannel {
        wire,
        session,
        office_id,
        channel_id: opened.channel_id,
        reader,
        next_id: 2,
    })
}

/// Interactive `agentic-org sign-in`: obtains browser approval without asking for a password,
/// optionally opens the selected office channel for a follow-up command, and saves context when
/// an office is selected or uniquely identified.
pub fn open_for_command(
    explicit_instance: Option<&str>,
    explicit_office: Option<&str>,
    out: &mut dyn Write,
) -> Result<ApprovedOfficeChannel, String> {
    let ctx_path = cli_context::default_context_path()?;
    let saved = cli_context::load_from(&ctx_path)?;
    let (instance_name, office_id) =
        cli_context::resolve_selection(explicit_instance, explicit_office, saved.as_ref())?;
    let (_, instance_id, socket) = registered(
        &crate::human::paths(instance_name.as_deref())?,
        instance_name.as_deref(),
    )?;
    if explicit_office.is_none() || explicit_instance.is_none() {
        if let Some(ctx) = &saved {
            if ctx.instance_id != instance_id {
                return Err(format!(
                    "saved CLI context points to instance {} but this instance is {}; update it with `agentic-org context use --office <office_id>`",
                    ctx.instance_id, instance_id
                ));
            }
        }
    }
    let (wire, session, _account, _orgs) = approve_on_target(
        &Target {
            socket: &socket,
            instance_id: &instance_id,
            name: instance_name.as_deref(),
        },
        Duration::from_millis(250),
        Duration::from_secs(10 * 60),
        out,
    )?;
    open_approved_office(wire, session, office_id)
}

/// Standalone `agentic-org sign-in` (without `-- <command>`): verifies browser approval, saves
/// instance+office context when known, prints signed-in confirmation, and ends the session.
pub fn sign_in_only(
    explicit_instance: Option<&str>,
    explicit_office: Option<&str>,
    out: &mut dyn Write,
) -> Result<(), String> {
    if let Some(office) = explicit_office {
        if !org_proto::is_uuid_v7(office) {
            return Err(format!(
                "office ID `{office}` must be a canonical UUIDv7; run `agentic-org office list` to inspect offices"
            ));
        }
    }
    let (_, instance_id, socket) =
        registered(&crate::human::paths(explicit_instance)?, explicit_instance)?;
    let (mut wire, session, account, orgs) = approve_on_target(
        &Target {
            socket: &socket,
            instance_id: &instance_id,
            name: explicit_instance,
        },
        Duration::from_millis(250),
        Duration::from_secs(10 * 60),
        out,
    )?;
    let handle = SessionHandle::new(session.expose().into()).expect("a live handle");
    let _ = wire.call::<SessionEnd>(&SessionParams { session: handle }, None);

    let chosen_office = explicit_office.map(str::to_owned).or_else(|| {
        let all: Vec<&str> = orgs
            .iter()
            .flat_map(|o| o.office_ids.iter().map(String::as_str))
            .collect();
        (all.len() == 1).then(|| all[0].to_owned())
    });
    if let Some(office_id) = chosen_office {
        let ctx_path = cli_context::default_context_path()?;
        let ctx = CliContext::new(
            instance_id,
            explicit_instance.map(str::to_owned),
            office_id.clone(),
        )?;
        cli_context::save_to(&ctx_path, &ctx)?;
        say(
            out,
            format!("Signed in as account {account}. Saved default office context: {office_id}"),
        )
    } else {
        say(
            out,
            format!(
                "Signed in as account {account}. Select an office with `agentic-org context use --office <office_id>`."
            ),
        )
    }
}

pub fn context_show(out: &mut dyn Write) -> Result<(), String> {
    let path = cli_context::default_context_path()?;
    match cli_context::load_from(&path)? {
        Some(ctx) => say(
            out,
            format!(
                "instance_id={}\tinstance_name={}\toffice_id={}",
                ctx.instance_id,
                ctx.instance_name.as_deref().unwrap_or("default"),
                ctx.office_id
            ),
        ),
        None => say(out, "No saved CLI context."),
    }
}

pub fn context_use(
    instance_name: Option<&str>,
    office_id: &str,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (_, instance_id, _) = registered(&crate::human::paths(instance_name)?, instance_name)?;
    let ctx = CliContext::new(
        instance_id,
        instance_name.map(str::to_owned),
        office_id.to_owned(),
    )?;
    let path = cli_context::default_context_path()?;
    cli_context::save_to(&path, &ctx)?;
    say(
        out,
        format!(
            "Saved CLI context: instance={} office={}",
            ctx.instance_name.as_deref().unwrap_or("default"),
            ctx.office_id
        ),
    )
}

pub fn context_clear(out: &mut dyn Write) -> Result<(), String> {
    let path = cli_context::default_context_path()?;
    if cli_context::clear_at(&path)? {
        say(out, "Cleared saved CLI context.")
    } else {
        say(out, "No saved CLI context.")
    }
}
