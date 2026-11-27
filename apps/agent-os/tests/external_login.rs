//! Real-binary `agent-os login`, `whoami` and `logout` against a scripted office that checks
//! every Ed25519 signature (`TASK-137`): owner-only workspace state outside Git, walk-up
//! resolution, environment precedence, the context briefing of a sign-in and logout cleanup.

use std::io::BufReader;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

use base64::Engine as _;
use org_proto::directory::{Who, WhoResult};
use org_proto::external_auth::{
    challenge_message, AuthChallenge, AuthChallengeResult, AuthExchange, AuthExchangeResult,
    AuthLoginPoll, AuthLoginPollResult, AuthLoginRequest, AuthLoginRequestResult, AuthLogout,
    AuthLogoutResult, AuthWhoami, AuthWhoamiResult, ChallengePurpose, ExternalLoginState,
    SessionMode,
};
use org_proto::msg::{MsgBind, MsgBindResult};
use org_proto::processes::{ProcessEffective, ProcessEffectiveResult};
use org_proto::{frame, ApiError, ErrorCode, Hello, HelloResult, Request, Response};
use ring::signature::{UnparsedPublicKey, ED25519};
use serde_json::Value;

const OFFICE: &str = "0193a5c0-0000-7000-8000-000000000001";
const ADA: &str = "0193a5c0-0000-7000-8000-000000000051";
const SESSION: &str = "ext-0193a5c0-0000-7000-8000-000000000070";

#[derive(Default)]
struct Office {
    pubkey: Option<Vec<u8>>,
    pubkeys: Vec<Vec<u8>>,
    polls: u32,
    nonce: u32,
    tokens: Vec<String>,
    bound_with: Vec<String>,
    logged_out: bool,
    /// When set, challenges claim to come from this office (a socket that is not the
    /// signed-in office).
    challenge_office: Option<String>,
    exchanges: u32,
}

fn verify(office: &Office, purpose: ChallengePurpose, nonce: &str, sig: &str) -> bool {
    let msg = challenge_message(purpose, OFFICE, ADA, SESSION, nonce);
    let sig = base64::engine::general_purpose::STANDARD
        .decode(sig)
        .unwrap();
    UnparsedPublicKey::new(&ED25519, office.pubkey.as_ref().unwrap())
        .verify(&msg, &sig)
        .is_ok()
}

fn refuse(id: u64) -> Response {
    Response::error(id, ApiError::new(ErrorCode::NotAuthorized, "no"))
}

fn answer(req: &Request, office: &mut Office) -> Response {
    let id = req.id;
    if req.is::<Hello>() {
        let hello: HelloResult = serde_json::from_value(serde_json::json!({
            "api_version": 1, "server_version": "test", "capabilities": []
        }))
        .unwrap();
        Response::ok::<Hello>(id, &hello).unwrap()
    } else if req.is::<AuthLoginRequest>() {
        let p = req.params::<AuthLoginRequest>().unwrap();
        assert_eq!(p.member, "@ada");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&p.pubkey_ed25519_b64)
            .unwrap();
        office.pubkeys.push(decoded.clone());
        office.pubkey = Some(decoded);
        Response::ok::<AuthLoginRequest>(
            id,
            &AuthLoginRequestResult {
                request_id: "req-1".into(),
                member_id: ADA.into(),
                handle: Some("ada".into()),
                office_id: OFFICE.into(),
                approval_code: "ABCD-EFGH".into(),
                expires_at: "2099-01-01T00:00:00.000Z".into(),
                approve_command: "agentic-org agent approve ABCD-EFGH".into(),
                web_path: format!("/members/{ADA}"),
            },
        )
        .unwrap()
    } else if req.is::<AuthLoginPoll>() {
        office.polls += 1;
        let approved = office.polls > 1;
        Response::ok::<AuthLoginPoll>(
            id,
            &AuthLoginPollResult {
                state: if approved {
                    ExternalLoginState::Approved
                } else {
                    ExternalLoginState::Pending
                },
                member_id: ADA.into(),
                session_id: approved.then(|| SESSION.into()),
                lease_expires_at: approved.then(|| "2099-01-01T00:00:00.000Z".into()),
            },
        )
        .unwrap()
    } else if req.is::<AuthChallenge>() {
        office.nonce += 1;
        Response::ok::<AuthChallenge>(
            id,
            &AuthChallengeResult {
                member_id: ADA.into(),
                office_id: office
                    .challenge_office
                    .clone()
                    .unwrap_or_else(|| OFFICE.into()),
                nonce: format!("nonce{}", office.nonce),
                expires_at: "2099-01-01T00:00:00.000Z".into(),
            },
        )
        .unwrap()
    } else if req.is::<AuthExchange>() {
        office.exchanges += 1;
        let p = req.params::<AuthExchange>().unwrap();
        if !verify(
            office,
            ChallengePurpose::Exchange,
            &p.nonce,
            &p.signature_ed25519_b64,
        ) {
            return refuse(id);
        }
        let token = format!("fast-token-{}", office.tokens.len());
        office.tokens.push(token.clone());
        Response::ok::<AuthExchange>(
            id,
            &AuthExchangeResult {
                member_id: ADA.into(),
                office_id: OFFICE.into(),
                session_id: SESSION.into(),
                token: token.into(),
                expires_at: "2099-01-01T00:00:00.000Z".into(),
                lease_expires_at: "2099-01-01T00:00:00.000Z".into(),
            },
        )
        .unwrap()
    } else if req.is::<MsgBind>() {
        let p = req.params::<MsgBind>().unwrap();
        let token = p.session_token.unwrap_or_default();
        office.bound_with.push(token.clone());
        if !office.tokens.contains(&token) && token != "managed-env-token" {
            return refuse(id);
        }
        Response::ok::<MsgBind>(
            id,
            &MsgBindResult {
                member_id: Some(ADA.into()),
                office_id: OFFICE.into(),
            },
        )
        .unwrap()
    } else if req.is::<AuthWhoami>() {
        let managed = office.bound_with.last().map(String::as_str) == Some("managed-env-token");
        Response::ok::<AuthWhoami>(
            id,
            &AuthWhoamiResult {
                member_id: ADA.into(),
                handle: Some("ada".into()),
                office_id: OFFICE.into(),
                mode: if managed {
                    SessionMode::Managed
                } else {
                    SessionMode::External
                },
                session_id: (!managed).then(|| SESSION.into()),
                expires_at: None,
                lease_expires_at: None,
            },
        )
        .unwrap()
    } else if req.is::<AuthLogout>() {
        let p = req.params::<AuthLogout>().unwrap();
        if !verify(
            office,
            ChallengePurpose::Logout,
            &p.nonce,
            &p.signature_ed25519_b64,
        ) {
            return refuse(id);
        }
        office.logged_out = true;
        Response::ok::<AuthLogout>(
            id,
            &AuthLogoutResult {
                member_id: ADA.into(),
                session_id: SESSION.into(),
                revoked: true,
            },
        )
        .unwrap()
    } else if req.is::<Who>() {
        // The context briefing `login` answers once the workspace is bound.
        let who: WhoResult = serde_json::from_value(serde_json::json!({"members": [{
            "id": ADA, "name": "Ada", "handle": "ada", "kind": "agent", "teams": [],
            "presence": "online"
        }]}))
        .unwrap();
        Response::ok::<Who>(id, &who).unwrap()
    } else if req.is::<ProcessEffective>() {
        let effective: ProcessEffectiveResult = serde_json::from_value(serde_json::json!({
            "scope_chain": ["Organization"], "entries": [], "problems": [],
            "computed_at": "2026-11-26T00:00:00Z"
        }))
        .unwrap();
        Response::ok::<ProcessEffective>(id, &effective).unwrap()
    } else {
        panic!("unexpected method {}", req.method)
    }
}

fn serve(listener: UnixListener, office: Arc<Mutex<Office>>) {
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let office = Arc::clone(&office);
            thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                while let Ok(Some(line)) = frame::read_line(&mut reader) {
                    let req: Request = serde_json::from_slice(&line).unwrap();
                    let resp = answer(&req, &mut office.lock().unwrap());
                    if frame::write_message(&mut stream, &resp).is_err() {
                        return;
                    }
                }
            });
        }
    });
}

fn agent_os(cwd: &Path, state: &Path, home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_agent-os"));
    for name in [
        "AGENT_OS_SOCKET",
        "AGENT_OS_OFFICE",
        "AGENT_OS_RUNTIME_DIR",
        "ORG_OFFICE_HOME",
        "AGENT_OS_SESSION_TOKEN",
        "AGENT_OS_BINDING_FILE",
        "AGENT_OS_AGENT_ID",
        "ORG_DEV_ACCOUNT_BASE",
    ] {
        cmd.env_remove(name);
    }
    cmd.current_dir(cwd)
        .env("AGENT_OS_STATE_DIR", state)
        .env("HOME", home);
    cmd
}

fn envelope(out: &std::process::Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "{e}: {}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

fn mode_of(path: &Path) -> u32 {
    std::fs::symlink_metadata(path)
        .unwrap()
        .permissions()
        .mode()
        & 0o777
}

fn workspace_dir(state: &Path, repo: &Path) -> PathBuf {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(repo.as_os_str().as_encoded_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    state.join("workspaces").join(hex)
}

#[test]
fn login_whoami_and_logout_keep_owner_only_state_outside_git() {
    let tmp = tempfile::Builder::new().prefix("aosx").tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let sock = root.join("office.sock");
    let office = Arc::new(Mutex::new(Office::default()));
    serve(UnixListener::bind(&sock).unwrap(), Arc::clone(&office));
    let repo = root.join("repo");
    let sub = repo.join("src").join("deep");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::create_dir(repo.join(".git")).unwrap();
    let state = root.join("state");

    let out = agent_os(&sub, &state, &root)
        .args(["login", "--as", "@ada", "--socket"])
        .arg(&sock)
        .args(["--wait", "30"])
        .output()
        .unwrap();
    let env = envelope(&out);
    assert_eq!(out.status.code(), Some(0), "{env}");
    assert_eq!(env["data"]["mode"], "external");
    assert_eq!(env["data"]["session_id"], SESSION);
    assert_eq!(env["data"]["workspace_root"], repo.display().to_string());
    assert!(
        env["data"].get("boot").is_none() && env["data"].get("commands").is_none(),
        "legacy data.boot and split data.commands must be removed: {env}"
    );
    // The sign-in answers the exact `agent-os context` schema under `data.context`, with
    // `catalogue` inside it and external-mode rules.
    let login_ctx = &env["data"]["context"];
    assert_eq!(login_ctx["mode"], "external", "{env}");
    assert_eq!(login_ctx["identity"]["id"], ADA, "{env}");
    assert_eq!(
        login_ctx["rules"],
        org_agent_cli::EXTERNAL_RULES.trim_end(),
        "{env}"
    );
    assert!(login_ctx["catalogue"]
        .as_str()
        .unwrap()
        .lines()
        .any(|line| line.starts_with("msg: ") && line.contains("send")));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("agentic-org agent approve ABCD-EFGH"),
        "{stderr}"
    );
    assert!(stderr.contains(&format!("/members/{ADA}")), "{stderr}");

    // State lives outside the repository, owner-only, and never shows the key or token.
    let ws = workspace_dir(&state, &repo);
    assert_eq!(mode_of(&ws), 0o700);
    for name in ["binding.v1.json", "key.ed25519.pkcs8", "token.v1"] {
        assert_eq!(mode_of(&ws.join(name)), 0o600, "{name}");
    }
    assert_eq!(std::fs::read_dir(repo.join(".git")).unwrap().count(), 0);
    let token = office.lock().unwrap().tokens[0].clone();
    assert!(!String::from_utf8_lossy(&out.stdout).contains(&token));
    assert!(!stderr.contains(&token));

    // Any folder of the workspace resolves the binding (walk-up to the Git root).
    let out = agent_os(&sub, &state, &root)
        .arg("whoami")
        .output()
        .unwrap();
    let env = envelope(&out);
    assert_eq!(out.status.code(), Some(0), "{env}");
    assert_eq!(env["data"]["mode"], "external");
    assert_eq!(office.lock().unwrap().bound_with.last(), Some(&token));

    // The signed-in workspace is a bound shell: `--help` opens with the bound quickstart,
    // `context` matches `login`'s `data.context` schema verbatim, and legacy `boot` is gone.
    let help = |cwd: &Path| {
        let out = agent_os(cwd, &state, &root).arg("--help").output().unwrap();
        assert_eq!(out.status.code(), Some(0));
        String::from_utf8(out.stdout).unwrap()
    };
    assert!(
        help(&sub).starts_with("Agent quickstart: this shell is bound"),
        "{}",
        help(&sub)
    );
    assert!(help(&root).starts_with("Agent quickstart: this shell has no office binding"));
    let out = agent_os(&sub, &state, &root)
        .arg("context")
        .output()
        .unwrap();
    let ctx_env = envelope(&out);
    assert_eq!(out.status.code(), Some(0), "{ctx_env}");
    assert_eq!(
        ctx_env["data"], *login_ctx,
        "login data.context must match `agent-os context`"
    );
    let legacy = agent_os(&sub, &state, &root).arg("boot").output().unwrap();
    assert_eq!(legacy.status.code(), Some(2), "legacy `boot` must fail");

    // Environment bindings always win over the workspace.
    let out = agent_os(&sub, &state, &root)
        .arg("whoami")
        .env("AGENT_OS_SESSION_TOKEN", "managed-env-token")
        .env("AGENT_OS_SOCKET", &sock)
        .env("AGENT_OS_AGENT_ID", ADA)
        .output()
        .unwrap();
    let env = envelope(&out);
    assert_eq!(env["data"]["mode"], "managed", "{env}");
    // A managed binding that names an unreadable binding file fails closed: it never falls back
    // to acting as the workspace's agent.
    let out = agent_os(&sub, &state, &root)
        .arg("whoami")
        .env("AGENT_OS_BINDING_FILE", root.join("missing-binding"))
        .env("AGENT_OS_SOCKET", &sock)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(envelope(&out)["error"]["code"], "not_authorized");

    // Outside the workspace nothing is bound.
    let out = agent_os(&root, &state, &root)
        .arg("whoami")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(envelope(&out)["error"]["code"], "not_authorized");

    // Logout ends the lease (signed) and removes every local file.
    let out = agent_os(&repo, &state, &root)
        .arg("logout")
        .output()
        .unwrap();
    let env = envelope(&out);
    assert_eq!(out.status.code(), Some(0), "{env}");
    assert_eq!(env["data"]["revoked"], true);
    assert!(office.lock().unwrap().logged_out);
    assert!(!ws.exists());
    let out = agent_os(&repo, &state, &root)
        .arg("logout")
        .output()
        .unwrap();
    assert_eq!(envelope(&out)["error"]["code"], "not_found");
}

#[test]
fn a_symlinked_binding_is_never_followed() {
    let tmp = tempfile::Builder::new().prefix("aosl").tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let repo = root.join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let state = root.join("state");
    let ws = workspace_dir(&state, &repo);
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::set_permissions(&ws, std::fs::Permissions::from_mode(0o700)).unwrap();
    let planted = root.join("planted.json");
    std::fs::write(&planted, "{}").unwrap();
    std::os::unix::fs::symlink(&planted, ws.join("binding.v1.json")).unwrap();
    let out = agent_os(&repo, &state, &root)
        .arg("whoami")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(envelope(&out)["error"]["code"], "not_authorized");
}

/// A stand-in for the operator's signed-in `agentic-org`: records the words `exec` approves
/// with (one per line) and the office it named, then succeeds. The fake office approves on
/// its second poll whatever the operator does.
fn stub_operator(dir: &Path) -> PathBuf {
    let stub = dir.join("agentic-org-stub");
    std::fs::write(
        &stub,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\nprintf '%s\\n' \"$AGENTIC_ORG_OFFICE\" > \"$0.office\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    stub
}

struct ExecWorld {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    sock: PathBuf,
    state: PathBuf,
    stub: PathBuf,
    office: Arc<Mutex<Office>>,
}

fn exec_world(prefix: &str) -> ExecWorld {
    let tmp = tempfile::Builder::new().prefix(prefix).tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let sock = root.join("office.sock");
    let office = Arc::new(Mutex::new(Office::default()));
    serve(UnixListener::bind(&sock).unwrap(), Arc::clone(&office));
    let stub = stub_operator(&root);
    ExecWorld {
        state: root.join("state"),
        _tmp: tmp,
        root,
        sock,
        stub,
        office,
    }
}

impl ExecWorld {
    fn exec(&self, script: &str, script_args: &[&Path]) -> Command {
        let mut cmd = agent_os(&self.root, &self.state, &self.root);
        cmd.env("AGENTIC_ORG_BIN", &self.stub)
            .args(["exec", "--as", "@ada", "--socket"])
            .arg(&self.sock)
            .args(["--", "sh", "-c", script, "sh"])
            .args(script_args);
        cmd
    }

    /// The `exec/<id>` folders left under the state root (none once `exec` is done).
    fn exec_dirs(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.state.join("exec"))
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default()
    }

    fn assert_lease_ended_and_folder_removed(&self) {
        assert!(self.office.lock().unwrap().logged_out, "lease not ended");
        assert!(self.exec_dirs().is_empty(), "{:?}", self.exec_dirs());
    }
}

fn wait_for_file(path: &Path, child: &mut std::process::Child) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !path.exists() {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("exec ended early: {status:?}");
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{path:?} never appeared"
        );
        thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn wait_exit(child: &mut std::process::Child, secs: u64) -> std::process::ExitStatus {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            panic!("exec did not end within {secs}s");
        }
        thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn signal(target: &str, name: &str) {
    let status = Command::new("sh")
        .args(["-c", &format!("kill -{name} {target}")])
        .status()
        .unwrap();
    assert!(status.success(), "kill -{name} {target}");
}

/// B1 + B6: `exec` approves through `agentic-org` with named flags, hands the child a token
/// (env and an owner-only file), then ends the lease and removes the file when it exits.
#[test]
fn exec_runs_the_child_with_a_token_then_ends_the_lease_and_removes_its_folder() {
    let w = exec_world("aose");
    let out = w.root.join("seen");
    let output = w
        .exec(
            "printf '%s\\n%s\\n' \"$AGENT_OS_SESSION_TOKEN\" \"$AGENT_OS_BINDING_FILE\" > \"$1\"; \
             test -f \"$AGENT_OS_BINDING_FILE\" || exit 9; exit 7",
            &[&out],
        )
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(7),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let args = std::fs::read_to_string(w.stub.with_file_name("agentic-org-stub.args")).unwrap();
    assert_eq!(
        args.lines().collect::<Vec<_>>(),
        ["agent", "approve", "--code", "ABCD-EFGH", "--ttl", "86400s"]
    );
    let office_named =
        std::fs::read_to_string(w.stub.with_file_name("agentic-org-stub.office")).unwrap();
    assert_eq!(office_named.trim(), OFFICE);
    let seen = std::fs::read_to_string(&out).unwrap();
    let mut lines = seen.lines();
    let token = lines.next().unwrap();
    let file = PathBuf::from(lines.next().unwrap());
    assert_eq!(token, w.office.lock().unwrap().tokens[0]);
    assert!(file.starts_with(w.state.join("exec")), "{file:?}");
    assert!(!file.exists());
    w.assert_lease_ended_and_folder_removed();
}

/// B6: `SIGTERM` to the wrapper is forwarded to the child; the lease still ends and the token
/// folder is removed, and the wrapper exits as a shell would (128 + 15).
#[test]
fn a_terminated_exec_forwards_the_signal_ends_the_lease_and_cleans_up() {
    let w = exec_world("aost");
    let ready = w.root.join("ready");
    let mut child = w
        .exec("echo up > \"$1\"; exec sleep 30", &[&ready])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    wait_for_file(&ready, &mut child);
    assert_eq!(
        w.exec_dirs().len(),
        1,
        "the token folder exists while it runs"
    );
    signal(&child.id().to_string(), "TERM");
    let status = wait_exit(&mut child, 20);
    assert_eq!(status.code(), Some(143), "{status:?}");
    w.assert_lease_ended_and_folder_removed();
}

/// B6: Ctrl-C reaches the child (the terminal signals the whole foreground group); the wrapper
/// survives it, waits for the child, then ends the lease and cleans up. An interrupt sent to
/// the wrapper alone does not end it.
#[test]
fn ctrl_c_reaches_the_child_and_the_wrapper_still_cleans_up() {
    use std::os::unix::process::CommandExt as _;
    let w = exec_world("aosi");
    let ready = w.root.join("ready");
    let mut child = w
        .exec("echo up > \"$1\"; while :; do sleep 0.05; done", &[&ready])
        .stdout(std::process::Stdio::null())
        .process_group(0)
        .spawn()
        .unwrap();
    wait_for_file(&ready, &mut child);
    signal(&child.id().to_string(), "INT");
    thread::sleep(std::time::Duration::from_millis(400));
    assert!(
        child.try_wait().unwrap().is_none(),
        "an interrupt to the wrapper alone never ends it"
    );
    assert!(!w.office.lock().unwrap().logged_out);
    // Ctrl-C: the whole process group.
    signal(&format!("-{}", child.id()), "INT");
    let status = wait_exit(&mut child, 20);
    assert_eq!(status.code(), Some(130), "{status:?}");
    w.assert_lease_ended_and_folder_removed();
}

/// A socket that answers a refresh's challenge for another office gets no signature: the
/// workspace key is never a signing oracle for a nonce it did not sign in to.
#[test]
fn a_challenge_for_another_office_is_never_signed() {
    let tmp = tempfile::Builder::new().prefix("aoso").tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let sock = root.join("office.sock");
    let office = Arc::new(Mutex::new(Office::default()));
    serve(UnixListener::bind(&sock).unwrap(), Arc::clone(&office));
    let repo = root.join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let state = root.join("state");
    let out = agent_os(&repo, &state, &root)
        .args(["login", "--as", "@ada", "--socket"])
        .arg(&sock)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", envelope(&out));
    let exchanges = {
        let mut office = office.lock().unwrap();
        // The office "forgets" the token (a restart), and its socket now claims another office.
        office.tokens.clear();
        office.challenge_office = Some("0193a5c0-0000-7000-8000-00000000e011".into());
        office.exchanges
    };
    let out = agent_os(&repo, &state, &root)
        .arg("whoami")
        .output()
        .unwrap();
    let env = envelope(&out);
    assert_eq!(out.status.code(), Some(1), "{env}");
    assert_eq!(env["error"]["code"], "not_authorized", "{env}");
    assert!(
        env["error"]["message"]
            .as_str()
            .unwrap()
            .contains("another office"),
        "{env}"
    );
    assert_eq!(
        office.lock().unwrap().exchanges,
        exchanges,
        "nothing was signed or sent"
    );
}

#[test]
fn bare_login_auto_discovers_office_from_locator_instances_and_reuses_workspace_keypair() {
    let tmp = tempfile::Builder::new().prefix("aosd").tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let dev_base = root.join("base");
    let inst_root = root.join("inst");
    let rt = root.join("rt");
    let sock_dir = rt.join("o").join(&OFFICE[OFFICE.len() - 12..]);
    std::fs::create_dir_all(&sock_dir).unwrap();
    let sock = sock_dir.join("s.sock");
    let office = Arc::new(Mutex::new(Office::default()));
    serve(UnixListener::bind(&sock).unwrap(), Arc::clone(&office));

    let meta_dir = inst_root.join("office-state").join("offices").join(OFFICE);
    std::fs::create_dir_all(&meta_dir).unwrap();
    std::fs::write(
        meta_dir.join("office.json"),
        serde_json::json!({
            "v": 2,
            "office_id": OFFICE,
            "office_name": "HQ",
            "role": "hq",
            "org_id": "0193a5c0-0000-7000-8000-000000009001",
            "org_name": "Acme",
            "created_at": "2026-11-27T00:00:00.000Z"
        })
        .to_string(),
    )
    .unwrap();
    let locator_dir = dev_base.join("locator");
    std::fs::create_dir_all(&locator_dir).unwrap();
    std::fs::write(
        locator_dir.join("instances.json"),
        serde_json::json!({
            "version": 1,
            "user_temp": root.display().to_string(),
            "entries": [{
                "phase": "ready",
                "binding": {
                    "name": "default",
                    "id": "0193a5c0-0000-7000-8000-000000001001",
                    "root": inst_root.display().to_string(),
                    "identity": { "device": 1, "inode": 1 },
                    "temp_handle": "rt",
                    "temp_folder": rt.display().to_string(),
                }
            }]
        })
        .to_string(),
    )
    .unwrap();

    let repo = root.join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let state = root.join("state");

    // 1. Bare `agent-os login --as @ada` with NO `--socket` and NO `--office`.
    let out = agent_os(&repo, &state, &root)
        .env("ORG_DEV_ACCOUNT_BASE", &dev_base)
        .args(["login", "--as", "@ada", "--wait", "30"])
        .output()
        .unwrap();
    let env = envelope(&out);
    assert_eq!(out.status.code(), Some(0), "{env}");
    assert_eq!(env["data"]["office_id"], OFFICE);
    assert_eq!(env["data"]["context"]["mode"], "external");

    // 2. Re-login with `--office <UUID>` (NO `--socket`) in the same workspace reuses keypair.
    let out2 = agent_os(&repo, &state, &root)
        .env("ORG_DEV_ACCOUNT_BASE", &dev_base)
        .args(["login", "--as", "@ada", "--office", OFFICE, "--wait", "30"])
        .output()
        .unwrap();
    let env2 = envelope(&out2);
    assert_eq!(out2.status.code(), Some(0), "{env2}");
    let keys = office.lock().unwrap().pubkeys.clone();
    assert_eq!(keys.len(), 2);
    assert_eq!(
        keys[0], keys[1],
        "re-authenticating the same workspace must reuse key.ed25519.pkcs8"
    );
}
