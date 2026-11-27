//! External ("bring your own agent") workspace sign-in for `agent-os` (`TASK-137`, `doc-102`).
//!
//! - `agent-os login --as @<handle>`: generates a workspace Ed25519 key, asks the office for an
//!   approval code, waits until a person approves it (`agentic-org agent approve <code>`), then
//!   saves the binding, the key and a fast token outside Git ([`store`]).
//! - `agent-os whoami`, `agent-os logout`, and `agent-os exec --as @<handle> -- <cmd...>` (a
//!   one-shot session minted with the operator's signed-in `agentic-org`).
//! - Every other `agent-os` command finds its binding in this order: `AGENT_OS_*` environment
//!   variables (managed runs, `exec`), then the nearest signed-in workspace from the working
//!   folder up to its Git root ([`workspace_binding`]). A workspace token past its expiry, or one
//!   the office no longer knows (an office restart), is refreshed by one challenge-response.

pub mod bind;
pub mod client;
mod exec;
mod login;
pub mod store;

use std::path::{Path, PathBuf};

use org_client::Client;
use org_command::{Refusal, EXIT_USAGE};
use org_proto::ErrorCode;
use serde_json::Value;
use zeroize::Zeroizing;

pub use exec::operator_approve_args;

use crate::runtime::binding::{env_requests_binding, Binding};
use crate::runtime::office::BoundOffice;
use crate::runtime::transport::OfficeTransport;

/// How long before its expiry a workspace token is refreshed.
const TOKEN_REFRESH_MARGIN_MS: u64 = 60_000;

/// The top-level `agent-os` verbs of the external lifecycle.
const LIFECYCLE_VERBS: &[&str] = &["login", "whoami", "logout", "exec"];

/// The external lifecycle verbs, which sit outside the command table, as `agent-os --help` closes
/// with them.
pub(crate) const SIGN_IN_HELP: &str = "
Workspace sign-in (bring your own agent):
  login   Sign this workspace in as an agent once a person approves (agent-os login --as @<handle>)
  whoami  Show the agent, office, mode (managed|external), session and expiry this shell acts as
  logout  End this workspace's external session and remove its local binding
  exec    Run one command as an agent with a session your signed-in agentic-org approves
";

/// What `agent-os login` answers from the rest of `agent-os`, given by the composition root so
/// this slice names no other: the bound `agent-os context` briefing (with `catalogue` inside it).
pub(crate) struct LoginHooks {
    /// The bound context briefing over a transport (`data.context` of the answer).
    pub(crate) briefing: fn(&mut dyn OfficeTransport) -> Result<Value, Refusal>,
}

/// Milliseconds since the Unix epoch (`u64::MAX` when the clock is unreadable, so every token
/// looks stale and is refreshed).
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(u64::MAX, |d| {
            u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
        })
}

/// Why a lifecycle command did not complete. The variant, never the message, decides the exit
/// code: a command line that cannot be read is [`EXIT_USAGE`] (2), any refusal 1.
#[derive(Debug)]
pub(crate) enum LifecycleError {
    /// The command line cannot be read.
    Usage(Refusal),
    /// The command was refused or failed.
    Failed(Refusal),
}

impl LifecycleError {
    /// A command line that cannot be read: its `Usage: ...` message and the line it takes.
    pub(crate) fn usage(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::Usage(Refusal::new(ErrorCode::InvalidRequest, message, hint))
    }

    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => EXIT_USAGE,
            Self::Failed(_) => 1,
        }
    }

    pub(crate) fn refusal(&self) -> &Refusal {
        match self {
            Self::Usage(refusal) | Self::Failed(refusal) => refusal,
        }
    }
}

impl From<Refusal> for LifecycleError {
    fn from(refusal: Refusal) -> Self {
        Self::Failed(refusal)
    }
}

/// One printed answer: stdout text and exit code.
pub(crate) struct Printed {
    pub(crate) stdout: String,
    pub(crate) exit_code: u8,
}

/// The success envelope (`ok`, `data`, `meta`, `error`, in that order), one line, escaped as
/// every other envelope is ([`org_command::success_envelope_json`]).
pub(crate) fn success(data: &Value) -> Printed {
    Printed {
        stdout: format!("{}\n", org_command::success_envelope_json(data)),
        exit_code: 0,
    }
}

/// The failure envelope of `error`, with its exit code ([`LifecycleError::exit_code`]).
pub(crate) fn failure(error: &LifecycleError) -> Printed {
    Printed {
        stdout: format!(
            "{}\n",
            org_command::failure_envelope_json(error.refusal(), false)
        ),
        exit_code: error.exit_code(),
    }
}

/// What a lifecycle verb prints for its outcome.
pub(crate) fn print(outcome: Result<Printed, LifecycleError>) -> Printed {
    outcome.unwrap_or_else(|error| failure(&error))
}

/// Runs `agent-os [--json] login|whoami|logout|exec`; `None` for any other command line.
pub(crate) fn run_lifecycle(args: &[String], hooks: &LoginHooks) -> Option<(String, u8)> {
    let (leading_json, tail) = match args {
        [flag, rest @ ..] if flag == "--json" => (true, rest),
        _ => (false, args),
    };
    let verb = tail.first()?.as_str();
    if !LIFECYCLE_VERBS.contains(&verb) {
        return None;
    }
    let rest = &tail[1..];
    let printed = match verb {
        "login" => login::login(rest, leading_json, hooks),
        "whoami" => login::whoami(rest, leading_json),
        "logout" => login::logout(rest, leading_json),
        _ => exec::exec(rest, leading_json),
    };
    Some((printed.stdout, printed.exit_code))
}

/// Named flags of a lifecycle verb, `--name value` or `--name=value`; `--` ends them.
pub(crate) struct Flags {
    values: Vec<(String, String)>,
    switches: Vec<String>,
    pub(crate) rest: Vec<String>,
    pub(crate) help: bool,
}

impl Flags {
    fn parse_named(
        &mut self,
        args: &[String],
        at: &mut usize,
        body: &str,
        valued: &[&str],
        switches: &[&str],
        usage: &str,
    ) -> Result<(), LifecycleError> {
        let bad = |message: String| LifecycleError::usage(format!("Usage: {message}"), usage);
        let (name, inline) = match body.split_once('=') {
            Some((name, value)) => (name, Some(value.to_owned())),
            None => (body, None),
        };
        if switches.contains(&name) && inline.is_none() {
            if name == "json" && self.switch("json") {
                return Err(bad("--json is given twice.".to_owned()));
            }
            self.switches.push(name.to_owned());
            return Ok(());
        }
        if name == "json" && inline.is_some() {
            return Err(bad(
                "--json is a yes-or-no flag for JSON output and takes no value.".to_owned(),
            ));
        }
        if !valued.contains(&name) {
            return Err(bad(format!("--{name} is not a flag of this command.")));
        }
        let value = match inline {
            Some(value) => value,
            None => {
                let value = args
                    .get(*at)
                    .ok_or_else(|| bad(format!("--{name} needs a value.")))?;
                *at += 1;
                value.clone()
            }
        };
        if self.values.iter().any(|(n, _)| n == name) {
            return Err(bad(format!("--{name} is given twice.")));
        }
        self.values.push((name.to_owned(), value));
        Ok(())
    }

    pub(crate) fn parse(
        args: &[String],
        valued: &[&str],
        switches: &[&str],
        usage: &str,
    ) -> Result<Self, LifecycleError> {
        let mut out = Self {
            values: Vec::new(),
            switches: Vec::new(),
            rest: Vec::new(),
            help: false,
        };
        let mut at = 0;
        while let Some(arg) = args.get(at) {
            at += 1;
            if arg == "--" {
                out.rest = args[at..].to_vec();
                break;
            }
            if arg == "--help" || arg == "-h" {
                out.help = true;
                continue;
            }
            let Some(body) = arg.strip_prefix("--").filter(|b| !b.is_empty()) else {
                return Err(LifecycleError::usage(
                    format!("Usage: `{arg}` is a bare word; every value is a named flag."),
                    usage,
                ));
            };
            out.parse_named(args, &mut at, body, valued, switches, usage)?;
        }
        Ok(out)
    }

    pub(crate) fn value(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    pub(crate) fn switch(&self, name: &str) -> bool {
        self.switches.iter().any(|n| n == name)
    }

    pub(crate) fn check_leading_json(
        &self,
        leading_json: bool,
        usage: &str,
    ) -> Result<(), LifecycleError> {
        if leading_json && self.switch("json") {
            return Err(LifecycleError::usage(
                "Usage: --json is given twice.",
                usage,
            ));
        }
        Ok(())
    }
}

/// The binding of this process: `AGENT_OS_*` environment variables when they carry a token or
/// name one ([`env_requests_binding`]; they always win, so a managed session whose token cannot
/// be read fails closed instead of acting as a workspace's agent), else the signed-in workspace
/// of the working folder, else the (tokenless) environment binding, whose refusal says how to
/// get one.
pub(crate) fn resolve_binding() -> Binding {
    let env = Binding::from_env();
    if env.session_token.is_some() || env_requests_binding() {
        return env;
    }
    std::env::current_dir()
        .ok()
        .and_then(|cwd| workspace_binding(&cwd))
        .unwrap_or(env)
}

/// The office of `binding`, a signed-in workspace's connected by challenge-response
/// ([`connect_workspace`]).
pub(crate) fn office_for(binding: Binding) -> BoundOffice {
    BoundOffice::new(binding).with_workspace_connector(connect_workspace)
}

/// The office of this process ([`resolve_binding`]): bound when the environment names a session
/// token or a binding file ([`env_requests_binding`]), or a signed-in workspace, even when its
/// token cannot be read yet, so `context` never drops to offline.
pub(crate) fn env_office() -> BoundOffice {
    office_for(resolve_binding()).with_binding_requested(env_requests_binding())
}

/// The binding of the nearest signed-in workspace from `cwd` up to its Git root. Reads only
/// local files; the token is refreshed when the connection is made.
pub(crate) fn workspace_binding(cwd: &Path) -> Option<Binding> {
    let root = store::state_root().ok()?;
    let (_, state) = store::find_workspace(&root, cwd)?;
    binding_of(&state)
}

/// The binding of the signed-in workspace whose state is `state`; `None` when it has none.
pub(crate) fn binding_of(state: &store::WorkspaceState) -> Option<Binding> {
    let binding = state.read_binding().ok()??;
    let token = state
        .read_token()
        .ok()
        .flatten()
        .filter(|t| t.session_id == binding.session_id)
        .map(|t| t.token.to_string());
    Some(Binding {
        socket: binding.socket.map(PathBuf::from),
        office_id: Some(binding.office_id),
        runtime_dir: binding.runtime_dir.map(PathBuf::from),
        session_token: token,
        agent_id: Some(binding.member_id),
        external_state: Some(state.dir().to_path_buf()),
    })
}

fn target_of(binding: &store::WorkspaceBinding) -> client::Target {
    client::Target {
        socket: binding.socket.as_ref().map(PathBuf::from),
        office_id: Some(binding.office_id.clone()),
        runtime_dir: binding.runtime_dir.as_ref().map(PathBuf::from),
    }
}

fn not_signed_in() -> Refusal {
    Refusal::new(
        ErrorCode::NotAuthorized,
        "This workspace is not signed in.",
        "Run `agent-os login --as @<handle>` in the workspace.",
    )
}

/// The signer of a signed-in workspace: its key, office, agent and lease session.
pub(crate) fn signer_of<'a>(
    binding: &'a store::WorkspaceBinding,
    pkcs8: &'a [u8],
) -> client::Signer<'a> {
    client::Signer {
        pkcs8,
        office_id: Some(&binding.office_id),
        member_id: &binding.member_id,
        session_id: &binding.session_id,
    }
}

/// Refreshes the workspace's fast token on the unbound `client` and saves it.
fn refresh_token(
    client: &Client,
    state: &store::WorkspaceState,
    binding: &store::WorkspaceBinding,
) -> Result<Zeroizing<String>, Refusal> {
    let key = state.read_key()?.ok_or_else(not_signed_in)?;
    let fresh = client::exchange(client, &signer_of(binding, &key))?;
    state.write_token(&store::TokenFile {
        version: 1,
        session_id: fresh.session_id,
        token: fresh.token.clone(),
        expires_at: fresh.expires_at,
    })?;
    Ok(fresh.token)
}

/// Connects a workspace binding and binds it, refreshing its token first when it is stale and
/// once more when the office refuses it (an office restart drops every RAM token).
pub(crate) fn connect_workspace(binding: &mut Binding) -> Result<Client, Refusal> {
    let dir = binding.external_state.clone().ok_or_else(not_signed_in)?;
    let state = store::WorkspaceState::at(dir);
    let workspace = state.read_binding()?.ok_or_else(not_signed_in)?;
    let client = target_of(&workspace).connect()?;
    let now = now_ms();
    let stored = state.read_token()?.filter(|t| {
        t.session_id == workspace.session_id && !t.is_stale(now, TOKEN_REFRESH_MARGIN_MS)
    });
    let (mut token, mut fresh) = match stored {
        Some(t) => (t.token.clone(), false),
        None => (refresh_token(&client, &state, &workspace)?, true),
    };
    loop {
        match client::bind(&client, &workspace.member_id, &token) {
            Ok(()) => {
                binding.session_token = Some(token.to_string());
                return Ok(client);
            }
            Err(refusal) if !fresh && refusal.code == ErrorCode::NotAuthorized => {
                token = refresh_token(&client, &state, &workspace)?;
                fresh = true;
            }
            Err(refusal) => return Err(refusal),
        }
    }
}

#[cfg(test)]
mod tests;
