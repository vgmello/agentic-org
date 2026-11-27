//! `agent-os login`, `agent-os whoami` and `agent-os logout` (`TASK-137`).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use org_command::Refusal;
use org_proto::external_auth::{
    AuthLoginPoll, AuthLoginPollParams, AuthLoginPollResult, AuthLoginRequest,
    AuthLoginRequestParams, AuthLoginRequestResult, AuthLogout, AuthWhoami, ChallengePurpose,
    ExternalLoginState,
};
use org_proto::{Call, Empty, ErrorCode};
use serde_json::{json, Value};
use zeroize::Zeroizing;

use super::bind::default_label_for;
use super::client::{self, call, Target};
use super::store::{self, TokenFile, WorkspaceBinding, WorkspaceState};
use super::{failure, print, success, Flags, LifecycleError, LoginHooks, Printed};
use crate::runtime::transport::OfficeTransport;

pub const LOGIN_HELP: &str = "Usage: agent-os login --as @<handle> [--office <office-id> | --socket <path>] [--wait <seconds>] [--workspace <dir>] [--label <text>]
Use when: signing this workspace (a terminal, IDE or external agent harness) in as an organization agent you bring yourself.

Generates a workspace Ed25519 key, prints an approval code for a person to approve
(`agentic-org agent approve <code>` or the agent's page in the web app), waits for it (--wait,
default 120 seconds; the code itself stays valid for 10 minutes), and saves the binding outside
Git. Afterwards every agent-os command in this folder tree acts as the agent. Approving a new
sign-in supersedes the agent's previous external session. The answer carries `boot`: who the agent
is, the standing rules, the guides and the must-follow processes (`agent-os boot` reads them again).

Example:
  agent-os login --as @ada --office 0193a5c0-0000-7000-8000-000000000001
";

pub const WHOAMI_HELP: &str = "Usage: agent-os whoami
Use when: checking which agent, office and session this shell acts as (managed or external) and when it expires.

Example:
  agent-os whoami
";

pub const LOGOUT_HELP: &str = "Usage: agent-os logout [--workspace <dir>]
Use when: ending this workspace's external session on the office and removing its local binding, key and token.

Example:
  agent-os logout
";

pub const EXEC_HELP: &str = "Usage: agent-os exec --as @<handle> [--ttl <duration>] [--takeover] [--office <office-id> | --socket <path>] -- <command...>
Use when: running one external agent harness as an organization agent without a manual approval, using your signed-in `agentic-org`.

Mints an external session through `agentic-org agent approve`, runs <command...> with
AGENT_OS_SESSION_TOKEN, AGENT_OS_BINDING_FILE, AGENT_OS_SOCKET and AGENT_OS_AGENT_ID set (the
token is refreshed while it runs), and ends the session when the command exits. --takeover parks
a live managed turn instead of refusing.

Example:
  agent-os exec --as @ada --ttl 8h -- claude
";

fn help(text: &str) -> Printed {
    Printed {
        stdout: text.to_owned(),
        exit_code: 0,
    }
}

fn label_for(root: &Path) -> String {
    default_label_for(root)
}

fn workspace_root(flag: Option<&str>) -> Result<PathBuf, Refusal> {
    match flag {
        Some(dir) => PathBuf::from(dir).canonicalize().map_err(|e| {
            Refusal::new(
                ErrorCode::NotFound,
                format!("The workspace `{dir}` cannot be resolved: {e}"),
                "Pass an existing folder to --workspace.",
            )
        }),
        None => {
            let cwd = std::env::current_dir().map_err(|e| {
                Refusal::new(
                    ErrorCode::Unavailable,
                    format!("The working folder cannot be read: {e}"),
                    "Run agent-os from an existing folder.",
                )
            })?;
            store::default_workspace_root(&cwd)
        }
    }
}

/// Asks the office for a login of `member` with the workspace's public key. A socket that
/// answers for another office than `--office` named is refused.
pub(super) fn request_login(
    target: &Target,
    member: &str,
    pubkey: &str,
    label: &str,
) -> Result<AuthLoginRequestResult, Refusal> {
    let request = call::<AuthLoginRequest>(
        &target.connect()?,
        &AuthLoginRequestParams {
            member: member.to_owned(),
            pubkey_ed25519_b64: pubkey.to_owned(),
            workspace_label: label.to_owned(),
        },
    )?;
    if target
        .office_id
        .as_deref()
        .is_some_and(|office| office != request.office_id)
    {
        return Err(Refusal::new(
            ErrorCode::NotAuthorized,
            "The office socket answered for another office than --office named.",
            "Check --socket / --office and run the command again.",
        ));
    }
    Ok(request)
}

/// How long `agent-os login` waits for an approval by default (`doc-102` §1); the code itself
/// stays valid on the office for `APPROVAL_CODE_TTL_SECS`.
pub(super) const DEFAULT_LOGIN_WAIT_SECS: u64 = 120;
/// Between two polls of a pending login.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Polls until the login is approved (answering its lease session) or `wait` runs out. A poll
/// the office rate-limits is retried at the next interval.
pub(super) fn wait_for_approval(
    target: &Target,
    request: &AuthLoginRequestResult,
    wait: Duration,
) -> Result<String, Refusal> {
    let deadline = Instant::now() + wait;
    loop {
        // A fresh connection per poll keeps each one far below the office's call budget.
        let polled = call::<AuthLoginPoll>(
            &target.connect()?,
            &AuthLoginPollParams {
                request_id: request.request_id.clone(),
            },
        );
        match polled {
            Ok(polled) => {
                if let Some(session) = settled(polled)? {
                    return Ok(session);
                }
            }
            // The office's sign-in budget is spent for now: poll again at the next interval.
            Err(refusal) if refusal.code == ErrorCode::RateLimited => {}
            Err(refusal) => return Err(refusal),
        }
        if Instant::now() >= deadline {
            return Err(Refusal::new(
                ErrorCode::Timeout,
                format!(
                    "No one approved the sign-in within the wait (code {}).",
                    request.approval_code
                ),
                "Approve it and run `agent-os login` again, or pass a longer --wait (the code is valid for 10 minutes).",
            ));
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// The lease session of an approved login, `None` while it is pending, or why it never will be.
fn settled(polled: AuthLoginPollResult) -> Result<Option<String>, Refusal> {
    match polled.state {
        ExternalLoginState::Approved => polled.session_id.map(Some).ok_or_else(|| {
            Refusal::new(
                ErrorCode::Unavailable,
                "The office approved the sign-in without a session.",
                "Run `agent-os login` again.",
            )
        }),
        ExternalLoginState::Pending => Ok(None),
        ExternalLoginState::Locked => Err(Refusal::new(
            ErrorCode::NotAuthorized,
            "The sign-in was locked after too many wrong approval codes.",
            "Run `agent-os login` again for a new code.",
        )),
        _ => Err(Refusal::new(
            ErrorCode::Timeout,
            "The approval code expired before anyone approved it.",
            "Run `agent-os login` again for a new code.",
        )),
    }
}

pub(super) fn login(args: &[String], hooks: &LoginHooks) -> Printed {
    print(run_login(args, hooks))
}

fn run_login(args: &[String], hooks: &LoginHooks) -> Result<Printed, LifecycleError> {
    let flags = Flags::parse(
        args,
        &["as", "office", "socket", "wait", "workspace", "label"],
        &[],
        "agent-os login --as @<handle> [--office <id> | --socket <path>]",
    )?;
    if flags.help {
        return Ok(help(LOGIN_HELP));
    }
    let member = flags.value("as").ok_or_else(|| {
        LifecycleError::usage(
            "Usage: agent-os login needs --as @<handle>.",
            "Pass --as @<handle> of the agent to sign in as.",
        )
    })?;
    let wait = match flags.value("wait") {
        Some(text) => text.parse::<u64>().ok().filter(|s| *s > 0).ok_or_else(|| {
            LifecycleError::usage("Usage: --wait is a number of seconds.", "Pass --wait 300.")
        })?,
        None => DEFAULT_LOGIN_WAIT_SECS,
    };
    let target = Target::resolve(
        flags.value("socket").map(PathBuf::from),
        flags.value("office").map(str::to_owned),
    )?;
    let root = workspace_root(flags.value("workspace"))?;
    let state = WorkspaceState::for_root(&store::state_root()?, &root);
    let label = flags
        .value("label")
        .map_or_else(|| label_for(&root), str::to_owned);
    let (pkcs8, pubkey) = client::generate_key()?;
    let request = request_login(&target, member, &pubkey, &label)?;
    eprintln!(
        "To approve this sign-in as @{}, a person runs:\n  {}\nor approves it on the agent's page in the Agentic Org web app ({}).\nWaiting for approval (the code expires at {})...",
        request.handle.as_deref().unwrap_or(&request.member_id),
        request.approve_command,
        request.web_path,
        request.expires_at,
    );
    let session_id = wait_for_approval(&target, &request, Duration::from_secs(wait))?;
    let token = client::exchange(
        &target.connect()?,
        &client::Signer {
            pkcs8: &pkcs8,
            office_id: Some(&request.office_id),
            member_id: &request.member_id,
            session_id: &session_id,
        },
    )?;
    state.ensure()?;
    state.write_key(&pkcs8)?;
    let binding = WorkspaceBinding {
        version: 1,
        workspace_root: root.display().to_string(),
        office_id: request.office_id.clone(),
        socket: target.socket.as_ref().map(|p| p.display().to_string()),
        runtime_dir: target.runtime_dir.as_ref().map(|p| p.display().to_string()),
        member_id: request.member_id.clone(),
        handle: request.handle.clone(),
        session_id: session_id.clone(),
        lease_expires_at: token.lease_expires_at.clone(),
    };
    state.write_binding(&binding)?;
    state.write_token(&TokenFile {
        version: 1,
        session_id,
        token: token.token.clone(),
        expires_at: token.expires_at.clone(),
    })?;
    let bound = target.connect()?;
    client::bind(&bound, &request.member_id, &token.token)?;
    let who = call::<AuthWhoami>(&bound, &Empty {})?;
    let mut data = serde_json::to_value(&who).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut data {
        map.insert("workspace_root".into(), json!(binding.workspace_root));
        map.insert("commands".into(), json!((hooks.catalogue)()));
        map.insert("boot".into(), boot_briefing(&state, hooks));
    }
    Ok(success(&data))
}

/// The bound `agent-os boot` briefing of the workspace just signed in, read over its saved binding
/// (without the catalogue, which `commands` carries). The sign-in stands when the office cannot
/// answer it: then `next_action` says to read it with `agent-os boot`.
fn boot_briefing(state: &WorkspaceState, hooks: &LoginHooks) -> Value {
    let briefing = super::binding_of(state)
        .ok_or_else(super::not_signed_in)
        .and_then(|binding| (hooks.briefing)(&mut super::office_for(binding)));
    match briefing {
        Ok(mut boot) => {
            if let Value::Object(map) = &mut boot {
                map.remove("catalogue");
            }
            boot
        }
        Err(refusal) => json!({
            "mode": "unavailable",
            "next_action": format!(
                "Signed in, but the boot briefing could not be read ({}): run `agent-os boot` in this workspace before your first command.",
                refusal.message
            ),
        }),
    }
}

pub(super) fn whoami(args: &[String]) -> Printed {
    let flags = match Flags::parse(args, &[], &[], "agent-os whoami") {
        Ok(flags) => flags,
        Err(error) => return failure(&error),
    };
    if flags.help {
        return help(WHOAMI_HELP);
    }
    match super::env_office().call_method(AuthWhoami::METHOD, json!({})) {
        Ok(data) => success(&data),
        Err(refusal) => failure(&LifecycleError::Failed(refusal)),
    }
}

pub(super) fn logout(args: &[String]) -> Printed {
    print(run_logout(args))
}

fn run_logout(args: &[String]) -> Result<Printed, LifecycleError> {
    let flags = Flags::parse(
        args,
        &["workspace"],
        &[],
        "agent-os logout [--workspace <dir>]",
    )?;
    if flags.help {
        return Ok(help(LOGOUT_HELP));
    }
    let root = store::state_root()?;
    let found = match flags.value("workspace") {
        Some(dir) => {
            let dir = workspace_root(Some(dir))?;
            let state = WorkspaceState::for_root(&root, &dir);
            state.has_binding().then_some((dir, state))
        }
        None => std::env::current_dir()
            .ok()
            .and_then(|cwd| store::find_workspace(&root, &cwd)),
    };
    let (dir, state) = found.ok_or_else(|| {
        Refusal::new(
            ErrorCode::NotFound,
            "This folder is not signed in as an external agent.",
            "Run `agent-os logout` inside the signed-in workspace, or pass --workspace <dir>.",
        )
    })?;
    let binding = state.read_binding()?.ok_or_else(|| {
        Refusal::new(
            ErrorCode::NotFound,
            "The workspace binding is missing.",
            "Run `agent-os login` again.",
        )
    })?;
    let key = state.read_key()?;
    let (revoked, note) = match key {
        Some(key) => match end_lease(&binding, &key) {
            Ok(revoked) => (revoked, None),
            Err(refusal) => (false, Some(refusal.message)),
        },
        None => (false, Some("the workspace key is missing".to_owned())),
    };
    state.remove()?;
    Ok(success(&json!({
        "member_id": binding.member_id,
        "session_id": binding.session_id,
        "workspace_root": dir.display().to_string(),
        "revoked": revoked,
        "local_binding_removed": true,
        "note": note,
    })))
}

/// Signs a logout nonce and ends the lease on the office.
pub(super) fn end_lease(
    binding: &WorkspaceBinding,
    key: &Zeroizing<Vec<u8>>,
) -> Result<bool, Refusal> {
    let target = Target {
        socket: binding.socket.as_ref().map(PathBuf::from),
        office_id: Some(binding.office_id.clone()),
        runtime_dir: binding.runtime_dir.as_ref().map(PathBuf::from),
    };
    let conn = target.connect()?;
    let signed = client::signed_challenge(
        &conn,
        &super::signer_of(binding, key),
        ChallengePurpose::Logout,
    )?;
    Ok(call::<AuthLogout>(&conn, &signed)?.revoked)
}
