//! `agent-os exec --as @<handle> [--ttl 24h] [--takeover] -- <cmd...>` (`TASK-137`): a
//! zero-click external session minted with the operator's signed-in `agentic-org`, handed to
//! one child process, refreshed while it runs and ended when it exits.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use org_command::Refusal;
use org_proto::external_auth::{AuthExchangeResult, DEFAULT_LEASE_TTL_SECS};
use org_proto::ErrorCode;
use zeroize::Zeroizing;

use super::client::{self, Target};
use super::login::{end_lease, request_login, wait_for_approval, EXEC_HELP};
use super::store::{self, WorkspaceBinding};
use super::{failure, now_ms, Flags, Printed};
use crate::office::{
    AGENT_ID_ENV, BINDING_FILE_ENV, OFFICE_ENV, RUNTIME_DIR_ENV, SESSION_TOKEN_ENV, SOCKET_ENV,
};

/// Names the `agentic-org` executable `exec` approves through (default: next to `agent-os`,
/// then `PATH`).
pub const OPERATOR_BIN_ENV: &str = "AGENTIC_ORG_BIN";
/// The office `agentic-org` acts on for the approval.
const OPERATOR_OFFICE_ENV: &str = "AGENTIC_ORG_OFFICE";
/// Refresh the child's token this long before it expires.
const REFRESH_MARGIN_MS: u64 = 5 * 60 * 1000;
/// How often the refresher checks the token (and that the office still knows it).
const REFRESH_CHECK: Duration = Duration::from_secs(30);

pub(super) fn exec(args: &[String]) -> Printed {
    match run_exec(args) {
        Ok(printed) => printed,
        Err(refusal) => failure(&refusal),
    }
}

fn operator_binary() -> PathBuf {
    if let Some(bin) = std::env::var_os(OPERATOR_BIN_ENV).filter(|v| !v.is_empty()) {
        return PathBuf::from(bin);
    }
    std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name("agentic-org"))
        .filter(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from("agentic-org"))
}

/// Approves `code` with the operator's signed-in authority: `agentic-org agent approve`. Its
/// prompts and errors reach the terminal; its answer is not needed.
fn approve_with_operator(
    code: &str,
    office_id: &str,
    ttl_secs: u64,
    takeover: bool,
) -> Result<(), Refusal> {
    let bin = operator_binary();
    let mut command = Command::new(&bin);
    command
        .args(["agent", "approve", code, "--ttl", &format!("{ttl_secs}s")])
        .env(OPERATOR_OFFICE_ENV, office_id)
        .stdin(Stdio::inherit())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    if takeover {
        command.arg("--takeover");
    }
    let status = command.status().map_err(|e| {
        Refusal::new(
            ErrorCode::NotFound,
            format!("`{}` could not be started: {e}", bin.display()),
            "Install agentic-org next to agent-os or set AGENTIC_ORG_BIN.",
        )
    })?;
    if status.success() {
        return Ok(());
    }
    Err(Refusal::new(
        ErrorCode::NotAuthorized,
        format!(
            "agentic-org could not approve the session (exit {}).",
            status.code().unwrap_or(-1)
        ),
        "Sign in with `agentic-org sign-in` as a person who may manage this agent; pass --takeover to park a live managed turn.",
    ))
}

/// The private folder holding the child's refreshed token file.
struct ExecDir {
    dir: PathBuf,
}

impl ExecDir {
    fn create() -> Result<Self, Refusal> {
        let id = crate::keys::try_new_uuid_v7().ok_or_else(|| {
            Refusal::new(
                ErrorCode::Unavailable,
                "System entropy is unavailable.",
                "Retry the command.",
            )
        })?;
        let dir = store::state_root()?.join("exec").join(id);
        let state = store::WorkspaceState::at(dir.clone());
        state.ensure()?;
        Ok(Self { dir })
    }

    fn token_file(&self) -> PathBuf {
        self.dir.join("session-binding.v1")
    }

    fn write_token(&self, token: &str) -> Result<(), Refusal> {
        let tmp = self.dir.join(".session-binding.tmp");
        org_platform::fs::write_owner_only_file(&tmp, token.as_bytes())
            .and_then(|()| std::fs::rename(&tmp, self.token_file()))
            .map_err(|e| {
                Refusal::new(
                    ErrorCode::Unavailable,
                    format!("cannot write the session token file: {e}"),
                    "Check the agent-os state folder and retry.",
                )
            })
    }
}

impl Drop for ExecDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.token_file());
        let _ = std::fs::remove_dir(&self.dir);
    }
}

/// Keeps the child's token file fresh: refreshes before expiry, and when the office no longer
/// knows the token (an office restart).
fn spawn_refresher(
    target: Target,
    key: Arc<Zeroizing<Vec<u8>>>,
    binding: WorkspaceBinding,
    first: AuthExchangeResult,
    dir: PathBuf,
    stop: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let exec_dir = ExecDirRef { dir };
        let mut token = first.token;
        let mut expires_ms = org_proto::parse_rfc3339_ms(&first.expires_at).unwrap_or(0);
        let mut last_check = Instant::now();
        while !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(250));
            if last_check.elapsed() < REFRESH_CHECK {
                continue;
            }
            last_check = Instant::now();
            let Ok(conn) = target.connect() else { continue };
            let due = now_ms().saturating_add(REFRESH_MARGIN_MS) >= expires_ms;
            // A refused bind leaves the connection unbound, ready for the challenge.
            if !due && client::bind(&conn, &binding.member_id, &token).is_ok() {
                continue;
            }
            match client::exchange(&conn, &key, &binding.member_id, &binding.session_id) {
                Ok(fresh) => {
                    if exec_dir.write(&fresh.token).is_ok() {
                        token = fresh.token;
                        expires_ms = org_proto::parse_rfc3339_ms(&fresh.expires_at).unwrap_or(0);
                    }
                }
                Err(refusal) => {
                    eprintln!("agent-os exec: token refresh failed: {}", refusal.message)
                }
            }
        }
    })
}

/// The token file of an [`ExecDir`], for the refresher thread (which never removes it).
struct ExecDirRef {
    dir: PathBuf,
}

impl ExecDirRef {
    fn write(&self, token: &str) -> std::io::Result<()> {
        let tmp = self.dir.join(".session-binding.refresh.tmp");
        org_platform::fs::write_owner_only_file(&tmp, token.as_bytes())?;
        std::fs::rename(&tmp, self.dir.join("session-binding.v1"))
    }
}

fn child_socket(target: &Target, office_id: &str) -> Option<PathBuf> {
    target.socket.clone().or_else(|| {
        target
            .runtime_dir
            .as_deref()
            .and_then(|dir| org_proto::location::selected_office_socket_in(dir, office_id))
    })
}

fn label_for(program: &str) -> String {
    let name = Path::new(program)
        .file_name()
        .map_or_else(|| program.to_owned(), |n| n.to_string_lossy().into_owned());
    let mut label = format!("exec: {name}");
    let max = org_proto::external_auth::MAX_WORKSPACE_LABEL_BYTES;
    if label.len() > max {
        let mut cut = max;
        while !label.is_char_boundary(cut) {
            cut -= 1;
        }
        label.truncate(cut);
    }
    label
}

fn run_exec(args: &[String]) -> Result<Printed, Refusal> {
    let flags = Flags::parse(
        args,
        &["as", "ttl", "office", "socket"],
        &["takeover"],
        "agent-os exec --as @<handle> [--ttl 24h] [--takeover] -- <command...>",
    )?;
    if flags.help {
        return Ok(Printed {
            stdout: EXEC_HELP.to_owned(),
            exit_code: 0,
        });
    }
    let usage = |message: &str| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            format!("Usage: {message}"),
            "agent-os exec --as @<handle> -- <command...>",
        )
    };
    let member = flags
        .value("as")
        .ok_or_else(|| usage("exec needs --as @<handle>."))?;
    let program = flags
        .rest
        .first()
        .ok_or_else(|| usage("exec needs a command after `--`."))?;
    let ttl_secs = match flags.value("ttl") {
        Some(text) => client::parse_ttl(text)?,
        None => DEFAULT_LEASE_TTL_SECS,
    };
    let target = Target::resolve(
        flags.value("socket").map(PathBuf::from),
        flags.value("office").map(str::to_owned),
    )?;
    let (pkcs8, pubkey) = client::generate_key()?;
    let request = request_login(&target, member, &pubkey, &label_for(program))?;
    approve_with_operator(
        &request.approval_code,
        &request.office_id,
        ttl_secs,
        flags.switch("takeover"),
    )?;
    let session_id = wait_for_approval(&target, &request, Duration::from_secs(30))?;
    let token = client::exchange(&target.connect()?, &pkcs8, &request.member_id, &session_id)?;
    let binding = WorkspaceBinding {
        version: 1,
        workspace_root: String::new(),
        office_id: request.office_id.clone(),
        socket: target.socket.as_ref().map(|p| p.display().to_string()),
        runtime_dir: target.runtime_dir.as_ref().map(|p| p.display().to_string()),
        member_id: request.member_id.clone(),
        handle: request.handle.clone(),
        session_id,
        lease_expires_at: token.lease_expires_at.clone(),
    };
    let exec_dir = ExecDir::create()?;
    exec_dir.write_token(&token.token)?;
    let mut child = Command::new(program);
    child
        .args(&flags.rest[1..])
        .env(SESSION_TOKEN_ENV, &token.token)
        .env(BINDING_FILE_ENV, exec_dir.token_file())
        .env(AGENT_ID_ENV, &request.member_id)
        .env(OFFICE_ENV, &request.office_id);
    match child_socket(&target, &request.office_id) {
        Some(socket) => {
            child.env(SOCKET_ENV, socket);
        }
        None => {
            child.env_remove(SOCKET_ENV);
        }
    }
    if let Some(dir) = &target.runtime_dir {
        child.env(RUNTIME_DIR_ENV, dir);
    }
    let key = Arc::new(pkcs8);
    let stop = Arc::new(AtomicBool::new(false));
    let refresher = spawn_refresher(
        target,
        Arc::clone(&key),
        binding.clone(),
        token,
        exec_dir.dir.clone(),
        Arc::clone(&stop),
    );
    let status = child.status();
    stop.store(true, Ordering::SeqCst);
    let _ = refresher.join();
    if let Err(refusal) = end_lease(&binding, &key) {
        eprintln!(
            "agent-os exec: could not end the external session: {}",
            refusal.message
        );
    }
    drop(exec_dir);
    let status = status.map_err(|e| {
        Refusal::new(
            ErrorCode::NotFound,
            format!("`{program}` could not be started: {e}"),
            "Check the command after `--`.",
        )
    })?;
    let code = status.code().map_or(1, |c| u8::try_from(c).unwrap_or(1));
    Ok(Printed {
        stdout: String::new(),
        exit_code: code,
    })
}
