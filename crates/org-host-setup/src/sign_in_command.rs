//! Browser-approved terminal sign-in (`agentic-org sign-in`) and grant-bound office channel
//! execution (FC-173 / SIGN-40 / SES-8). Never asks for a password at the terminal.

use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use org_host_control::ops::{
    FdsReceived, FdsReceivedParams, LinkedOrg, OfficeOpenChannel, OfficeOpenChannelParams,
    SessionEnd, SessionGrant, SessionGrantParams, SessionParams,
};
use org_host_control::SessionHandle;
use org_proto::{frame, ApiError, ErrorCode, Principal, Request, Response};
use serde::Serialize;
use serde_json::Value;

use super::cli_context::{self, CliContext};
use super::cli_session::{approve_on_wire, Target};
use super::project_command::office_hello;
use super::wire::registered;
use org_host::cli_error::{refused_by, HostCliError};
use org_host::human::Wire;

/// Help text for `agentic-org sign-in`.
pub const SIGN_IN_HELP: &str =
    "agentic-org sign-in [--name <instance>] [--office <office_id>] [-- <command...>]\n\
Use when: authenticating from a terminal via a browser approval link without typing a password\n\
(especially on Linux, containers, or HTTPS instances), or running one command over a grant-bound\n\
office channel (`-- <command...>`).\n\n\
Prints a one-time approval link (expires in 10 minutes). Open it in a signed-in browser\n\
and approve to sign this terminal in without entering a password (SIGN-40 / SES-8).\n\
When `-- <command...>` is supplied, runs the organization/project CLI command over the\n\
approved grant-bound channel and ends the session on exit.";

/// Help text for `agentic-org context`.
pub const CONTEXT_HELP: &str = "agentic-org context <subcommand>\n\
Use when: viewing, setting, or clearing the default instance and office targeted by operator commands.\n\n\
Subcommands:\n  \
show                                          Show the saved instance and office context\n      \
Use when: checking which default instance and office ID future CLI commands will use.\n  \
use --office <office_id> [--name <instance>]  Save default instance and office selection\n      \
Use when: pinning a default office so you do not pass --office/--name on every command.\n  \
clear                                         Remove the saved CLI context\n      \
Use when: resetting saved defaults before switching to another instance or office.";

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

/// What standalone `agentic-org sign-in` did: the approved account, and the default context
/// it saved when one office was known (the one named, or the account's only office).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SignedIn {
    pub account: String,
    pub saved_context: Option<CliContext>,
}

/// What `agentic-org context clear` found: a saved context it removed, or none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextCleared {
    Cleared,
    NothingSaved,
}

/// Runs the browser-approved sign-in flow against `target`, printing the one-time approval
/// URL to `out` and polling until approved, declined, expired, or timed out.
pub(crate) fn approve_on_target(
    target: &Target<'_>,
    poll_interval: Duration,
    max_wait: Duration,
    out: &mut dyn Write,
) -> Result<(Wire, SessionHandle, String, Vec<LinkedOrg>), HostCliError> {
    let wire = super::cli_session::connect_local_peer(target)?;
    approve_on_wire(wire, poll_interval, max_wait, out)
}

/// Opens an authenticated host-mediated channel to `office_id` on an already-approved
/// `host.sock` session (`SES-8`).
pub(crate) fn open_approved_office(
    mut wire: Wire,
    session: SessionHandle,
    office_id: String,
) -> Result<ApprovedOfficeChannel, HostCliError> {
    let handle = SessionHandle::new(session.expose().into()).expect("a live handle");
    let opened = match wire.call_answer::<OfficeOpenChannel>(
        &OfficeOpenChannelParams {
            session: handle,
            office_id: office_id.clone(),
        },
        None,
    )? {
        Ok(opened) => opened,
        Err(code @ ErrorCode::NotFound) => {
            return Err(HostCliError::refused(
                code,
                format!("office `{office_id}` not found"),
            ))
        }
        Err(code @ ErrorCode::NotAuthorized) => {
            return Err(HostCliError::refused(
                code,
                format!("not authorized for office `{office_id}`"),
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
    Ok(ApprovedOfficeChannel {
        wire,
        session,
        office_id,
        channel_id: opened.channel_id,
        reader,
        next_id: 2,
    })
}

/// Opens an office channel for a human/operator CLI command (`agentic-org task|msg|hr|...` or
/// `agentic-org sign-in -- <cmd>`).
///
/// Authentication tries `session.local` first (instant on local authless HTTP instances per
/// ADR-0034) and falls back to browser-approved sign-in when authentication is enabled, its
/// approval link written to `prompt_out`.
/// When no `--office` and no saved context are present, auto-selects the host's HQ or sole
/// office; when multiple offices exist without a unique HQ, refuses with an actionable hint.
pub fn open_for_command(
    explicit_instance: Option<&str>,
    explicit_office: Option<&str>,
    prompt_out: &mut dyn Write,
) -> Result<ApprovedOfficeChannel, HostCliError> {
    let ctx_path = cli_context::default_context_path()?;
    let saved = cli_context::load_from(&ctx_path)?;
    let (instance_name, selected_office) =
        match cli_context::resolve_selection(explicit_instance, explicit_office, saved.as_ref()) {
            Ok((inst, office)) => (inst, Some(office)),
            Err(_) if explicit_office.is_none() => (
                explicit_instance
                    .map(str::to_owned)
                    .or_else(|| saved.as_ref().and_then(|s| s.instance_name.clone())),
                None,
            ),
            Err(err) => return Err(HostCliError::usage(err)),
        };
    let (_, instance_id, socket) = registered(
        &org_host::human::paths(instance_name.as_deref())?,
        instance_name.as_deref(),
    )?;
    if selected_office.is_some() && (explicit_office.is_none() || explicit_instance.is_none()) {
        if let Some(ctx) = &saved {
            if ctx.instance_id != instance_id {
                return Err(HostCliError::runtime(format!(
                    "saved CLI context points to instance {} but this instance is {}",
                    ctx.instance_id, instance_id
                ))
                .with_help("update it with `agentic-org context use --office <office_id>`"));
            }
        }
    }
    let target = Target {
        socket: &socket,
        instance_id: &instance_id,
        name: instance_name.as_deref(),
    };
    let (mut wire, session) = super::cli_session::connect_cli_session(
        &target,
        None,
        &mut org_host::human::password,
        prompt_out,
    )?;
    let office_id = match selected_office {
        Some(id) => id,
        None => match resolve_default_office(&mut wire, &session) {
            Ok(id) => id,
            Err(err) => {
                super::cli_session::end_session(&mut wire, &session);
                return Err(err);
            }
        },
    };
    open_approved_office(wire, session, office_id)
}

fn resolve_default_office(
    wire: &mut Wire,
    session: &SessionHandle,
) -> Result<String, HostCliError> {
    use org_host_control::ops::{OfficeList, OfficeListParams};
    let handle = SessionHandle::new(session.expose().into()).expect("a live handle");
    let listed = match wire.call_answer::<OfficeList>(
        &OfficeListParams {
            session: handle,
            org_id: None,
        },
        None,
    )? {
        Ok(listed) => listed,
        Err(code) => return Err(refused_by("the office lookup", code)),
    };
    if listed.offices.is_empty() {
        return Err(
            HostCliError::refused(None, "no offices found on this host").with_help(
                "create an organization with `agentic-org org create` or join as a satellite with \
             `agentic-org office join`",
            ),
        );
    }
    let hqs: Vec<&str> = listed
        .offices
        .iter()
        .filter(|o| o.role == org_proto::OfficeRole::Hq)
        .map(|o| o.id.as_str())
        .collect();
    if hqs.len() == 1 {
        return Ok(hqs[0].to_owned());
    }
    if listed.offices.len() == 1 {
        return Ok(listed.offices[0].id.clone());
    }
    Err(HostCliError::usage("multiple offices found").with_help(
        "pass --office <office_id> [--name <instance>] or save a default with \
         `agentic-org context use --office <office_id>`",
    ))
}

/// Standalone `agentic-org sign-in` (without `-- <command>`): browser approval (its link
/// written to `prompt_out`), then the instance and office context saved when one office is
/// known, and the session ended.
pub fn sign_in_only(
    explicit_instance: Option<&str>,
    explicit_office: Option<&str>,
    prompt_out: &mut dyn Write,
) -> Result<SignedIn, HostCliError> {
    if let Some(office) = explicit_office {
        if !org_proto::is_uuid_v7(office) {
            return Err(office_id_usage(office));
        }
    }
    let (_, instance_id, socket) = registered(
        &org_host::human::paths(explicit_instance)?,
        explicit_instance,
    )?;
    let (mut wire, session, account, orgs) = approve_on_target(
        &Target {
            socket: &socket,
            instance_id: &instance_id,
            name: explicit_instance,
        },
        Duration::from_millis(250),
        Duration::from_secs(10 * 60),
        prompt_out,
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
    let saved_context = match chosen_office {
        Some(office_id) => {
            let ctx_path = cli_context::default_context_path()?;
            let ctx =
                CliContext::new(instance_id, explicit_instance.map(str::to_owned), office_id)?;
            cli_context::save_to(&ctx_path, &ctx)?;
            Some(ctx)
        }
        None => None,
    };
    Ok(SignedIn {
        account,
        saved_context,
    })
}

fn office_id_usage(office: &str) -> HostCliError {
    HostCliError::usage(format!("office ID `{office}` must be a canonical UUIDv7"))
        .with_help("run `agentic-org office list` to inspect offices")
}

/// `agentic-org context show`: the saved context, if any.
pub fn context_show() -> Result<Option<CliContext>, HostCliError> {
    let path = cli_context::default_context_path()?;
    Ok(cli_context::load_from(&path)?)
}

/// `agentic-org context use`: saves and returns the default instance and office.
pub fn context_use(
    instance_name: Option<&str>,
    office_id: &str,
) -> Result<CliContext, HostCliError> {
    if !org_proto::is_uuid_v7(office_id) {
        return Err(office_id_usage(office_id));
    }
    let (_, instance_id, _) = registered(&org_host::human::paths(instance_name)?, instance_name)?;
    let ctx = CliContext::new(
        instance_id,
        instance_name.map(str::to_owned),
        office_id.to_owned(),
    )?;
    let path = cli_context::default_context_path()?;
    cli_context::save_to(&path, &ctx)?;
    Ok(ctx)
}

/// `agentic-org context clear`.
pub fn context_clear() -> Result<ContextCleared, HostCliError> {
    let path = cli_context::default_context_path()?;
    Ok(if cli_context::clear_at(&path)? {
        ContextCleared::Cleared
    } else {
        ContextCleared::NothingSaved
    })
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT_HELP, SIGN_IN_HELP};

    #[test]
    fn sign_in_and_context_help_include_use_when_guidance() {
        assert!(SIGN_IN_HELP.contains("Use when:"), "{SIGN_IN_HELP}");
        assert!(
            CONTEXT_HELP.matches("Use when:").count() >= 4,
            "expected command + 3 subcommand Use when cues:\n{CONTEXT_HELP}"
        );
    }
}
