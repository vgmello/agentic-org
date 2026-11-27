//! `agent-os exec --as @<handle> [--ttl 24h] [--takeover] -- <cmd...>` (`TASK-137`): a
//! zero-click external session minted with the operator's signed-in `agentic-org`, handed to
//! one child process, refreshed while it runs and ended when it exits.
//!
//! The wrapper never dies before its cleanup: from before the lease exists until it is ended
//! it holds [`WrapperSignals`]. Ctrl-C reaches the child through the terminal and the wrapper
//! waits for the child; `SIGTERM` or `SIGHUP` to the wrapper is forwarded to the child (killed
//! after [`TERMINATION_GRACE`]). Either way the lease is ended on the office and the private
//! token folder removed, then the wrapper exits as a shell would (the child's code, or 128 plus
//! the signal).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use org_command::Refusal;
use org_platform::process::{
    forward_signal, shell_exit_code, signal_exit_code, WrapperSignals, SIGINT,
};
use org_proto::external_auth::{AuthExchangeResult, DEFAULT_LEASE_TTL_SECS};
use org_proto::ErrorCode;
use zeroize::Zeroizing;

use super::client::{self, Target};
use super::login::{end_lease, request_login, wait_for_approval, EXEC_HELP};
use super::store::{self, WorkspaceBinding};
use super::{now_ms, print, signer_of, Flags, LifecycleError, Printed};
use crate::runtime::binding::{
    AGENT_ID_ENV, BINDING_FILE_ENV, OFFICE_ENV, RUNTIME_DIR_ENV, SESSION_TOKEN_ENV, SOCKET_ENV,
};
use crate::runtime::input::parse_ttl;
use crate::runtime::keys::try_new_uuid_v7;

/// Names the `agentic-org` executable `exec` approves through (default: next to `agent-os`,
/// then `PATH`).
pub const OPERATOR_BIN_ENV: &str = "AGENTIC_ORG_BIN";
/// The office `agentic-org` acts on for the approval.
const OPERATOR_OFFICE_ENV: &str = "AGENTIC_ORG_OFFICE";
/// Refresh the child's token this long before it expires.
const REFRESH_MARGIN_MS: u64 = 5 * 60 * 1000;
/// How often the refresher checks the token (and that the office still knows it).
const REFRESH_CHECK: Duration = Duration::from_secs(30);
/// How long the operator's approval may take to show on the office.
const APPROVAL_WAIT: Duration = Duration::from_secs(30);
/// How long a child is given to end after a forwarded termination request before it is killed.
const TERMINATION_GRACE: Duration = Duration::from_secs(10);
/// How often the wrapper looks at its child and at the signals it received.
const CHILD_POLL: Duration = Duration::from_millis(50);

pub(super) fn exec(args: &[String]) -> Printed {
    print(run_exec(args))
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

/// The `agentic-org` words `exec` approves `code` with: named flags only, so the line reads the
/// same whatever `agentic-org` adds after the command path (`--format`, `--dry-run`).
pub fn operator_approve_args(code: &str, ttl_secs: u64, takeover: bool) -> Vec<String> {
    let mut words: Vec<String> = ["agent", "approve", "--code", code, "--ttl"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    words.push(format!("{ttl_secs}s"));
    if takeover {
        words.push("--takeover".to_owned());
    }
    words
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
    let status = Command::new(&bin)
        .args(operator_approve_args(code, ttl_secs, takeover))
        .env(OPERATOR_OFFICE_ENV, office_id)
        .stdin(Stdio::inherit())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| {
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
            shell_exit_code(&status)
        ),
        "Sign in with `agentic-org sign-in` as a person who may manage this agent; pass --takeover to park a live managed turn.",
    ))
}

/// The private folder holding the child's refreshed token file, removed when dropped.
struct ExecDir {
    dir: PathBuf,
}

impl ExecDir {
    fn create() -> Result<Self, Refusal> {
        let id = try_new_uuid_v7().ok_or_else(|| {
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
        ExecDirRef {
            dir: self.dir.clone(),
        }
        .write(token)
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
            match client::exchange(&conn, &signer_of(&binding, &key)) {
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
        let suffix = try_new_uuid_v7().unwrap_or_else(|| "tmp".into());
        let tmp = self.dir.join(format!(".session-binding.{suffix}.tmp"));
        org_platform::fs::write_owner_only_file(&tmp, token.as_bytes())
            .and_then(|()| std::fs::rename(&tmp, self.dir.join("session-binding.v1")))
            .inspect_err(|_| {
                let _ = std::fs::remove_file(&tmp);
            })
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
    label.truncate(label.floor_char_boundary(max));
    label
}

/// The command `exec` runs and how it was asked to run it.
struct Plan<'a> {
    target: Target,
    member: &'a str,
    program: &'a str,
    args: &'a [String],
    ttl_secs: u64,
    takeover: bool,
}

fn plan(flags: &Flags) -> Result<Plan<'_>, LifecycleError> {
    let usage = |message: &str| {
        LifecycleError::usage(
            format!("Usage: {message}"),
            "agent-os exec --as @<handle> -- <command...>",
        )
    };
    let member = flags
        .value("as")
        .ok_or_else(|| usage("exec needs --as @<handle>."))?;
    let (program, args) = flags
        .rest
        .split_first()
        .ok_or_else(|| usage("exec needs a command after `--`."))?;
    let ttl_secs = match flags.value("ttl") {
        Some(text) => parse_ttl(text)?,
        None => DEFAULT_LEASE_TTL_SECS,
    };
    let target = Target::resolve(
        flags.value("socket").map(PathBuf::from),
        flags.value("office").map(str::to_owned),
    )?;
    Ok(Plan {
        target,
        member,
        program,
        args,
        ttl_secs,
        takeover: flags.switch("takeover"),
    })
}

fn run_exec(args: &[String]) -> Result<Printed, LifecycleError> {
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
    let plan = plan(&flags)?;
    // Held from before the lease exists until it is ended (module documentation).
    let signals = WrapperSignals::install().map_err(|e| {
        Refusal::new(
            ErrorCode::Unavailable,
            format!("cannot take over the wrapper's signals: {e}"),
            "Retry the command.",
        )
    })?;
    let (pkcs8, pubkey) = client::generate_key()?;
    let cwd_path = std::env::current_dir()
        .ok()
        .and_then(|d| d.canonicalize().ok())
        .map(|d| d.display().to_string());
    let request = request_login(
        &plan.target,
        plan.member,
        &pubkey,
        &label_for(plan.program),
        client::local_hostname(),
        cwd_path,
    )?;
    approve_with_operator(
        &request.approval_code,
        &request.office_id,
        plan.ttl_secs,
        plan.takeover,
    )?;
    let session_id = wait_for_approval(&plan.target, &request, APPROVAL_WAIT)?;
    let binding = WorkspaceBinding {
        version: 1,
        workspace_root: String::new(),
        office_id: request.office_id.clone(),
        socket: plan.target.socket.as_ref().map(|p| p.display().to_string()),
        runtime_dir: plan
            .target
            .runtime_dir
            .as_ref()
            .map(|p| p.display().to_string()),
        member_id: request.member_id.clone(),
        handle: request.handle.clone(),
        session_id,
        lease_expires_at: String::new(),
    };
    let key = Arc::new(pkcs8);
    // From here a lease exists: whatever happens, it is ended before the wrapper exits.
    let ran = run_leased(&plan, &binding, &key, &signals);
    if let Err(refusal) = end_lease(&binding, &key) {
        eprintln!(
            "agent-os exec: could not end the external session: {}",
            refusal.message
        );
    }
    drop(signals);
    Ok(Printed {
        stdout: String::new(),
        exit_code: ran?,
    })
}

/// The exit code for a wrapper told to stop before its child started, if it was.
fn stopped_before_start(signals: &WrapperSignals) -> Option<u8> {
    match signals.termination() {
        Some(signal) => Some(signal_exit_code(signal)),
        None => signals.interrupted().then(|| signal_exit_code(SIGINT)),
    }
}

/// Mints the first token, runs the child with it while a refresher keeps it fresh, and removes
/// the token folder (whatever happens). Answers the child's shell exit code.
fn run_leased(
    plan: &Plan<'_>,
    binding: &WorkspaceBinding,
    key: &Arc<Zeroizing<Vec<u8>>>,
    signals: &WrapperSignals,
) -> Result<u8, Refusal> {
    let token = client::exchange(&plan.target.connect()?, &signer_of(binding, key))?;
    let exec_dir = ExecDir::create()?;
    exec_dir.write_token(&token.token)?;
    if let Some(code) = stopped_before_start(signals) {
        return Ok(code);
    }
    let mut command = Command::new(plan.program);
    command
        .args(plan.args)
        .env(SESSION_TOKEN_ENV, token.token.as_str())
        .env(BINDING_FILE_ENV, exec_dir.token_file())
        .env(AGENT_ID_ENV, &binding.member_id)
        .env(OFFICE_ENV, &binding.office_id);
    match child_socket(&plan.target, &binding.office_id) {
        Some(socket) => command.env(SOCKET_ENV, socket),
        None => command.env_remove(SOCKET_ENV),
    };
    if let Some(dir) = &plan.target.runtime_dir {
        command.env(RUNTIME_DIR_ENV, dir);
    }
    let mut child = command.spawn().map_err(|e| {
        Refusal::new(
            ErrorCode::NotFound,
            format!("`{}` could not be started: {e}", plan.program),
            "Check the command after `--`.",
        )
    })?;
    let stop = Arc::new(AtomicBool::new(false));
    let refresher = spawn_refresher(
        plan.target.clone(),
        Arc::clone(key),
        binding.clone(),
        token,
        exec_dir.dir.clone(),
        Arc::clone(&stop),
    );
    let status = supervise(&mut child, signals);
    stop.store(true, Ordering::SeqCst);
    let _ = refresher.join();
    drop(exec_dir);
    let status = status.map_err(|e| {
        Refusal::new(
            ErrorCode::Unavailable,
            format!("lost track of `{}`: {e}", plan.program),
            "Check that the command ended.",
        )
    })?;
    Ok(shell_exit_code(&status))
}

/// Waits for `child`. A termination request to the wrapper is forwarded to it once, and after
/// [`TERMINATION_GRACE`] it is killed. An interrupt is the child's to handle (the terminal sent
/// it to the child as well).
fn supervise(child: &mut Child, signals: &WrapperSignals) -> std::io::Result<ExitStatus> {
    let mut forwarded: Option<Instant> = None;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if let Some(signal) = signals.termination() {
            match forwarded {
                None => {
                    let _ = forward_signal(child.id(), signal);
                    forwarded = Some(Instant::now());
                }
                Some(at) if at.elapsed() >= TERMINATION_GRACE => {
                    let _ = child.kill();
                    return child.wait();
                }
                Some(_) => {}
            }
        }
        std::thread::sleep(CHILD_POLL);
    }
}
