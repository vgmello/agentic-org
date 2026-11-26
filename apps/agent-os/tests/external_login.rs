//! Real-binary `agent-os login`, `whoami` and `logout` against a scripted office that checks
//! every Ed25519 signature (`TASK-137`): owner-only workspace state outside Git, walk-up
//! resolution, environment precedence and logout cleanup.

use std::io::BufReader;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

use base64::Engine as _;
use org_proto::external_auth::{
    challenge_message, AuthChallenge, AuthChallengeResult, AuthExchange, AuthExchangeResult,
    AuthLoginPoll, AuthLoginPollResult, AuthLoginRequest, AuthLoginRequestResult, AuthLogout,
    AuthLogoutResult, AuthWhoami, AuthWhoamiResult, ChallengePurpose, ExternalLoginState,
    SessionMode,
};
use org_proto::msg::{MsgBind, MsgBindResult};
use org_proto::{frame, ApiError, ErrorCode, Hello, HelloResult, Request, Response};
use ring::signature::{UnparsedPublicKey, ED25519};
use serde_json::Value;

const OFFICE: &str = "0193a5c0-0000-7000-8000-000000000001";
const ADA: &str = "0193a5c0-0000-7000-8000-000000000051";
const SESSION: &str = "ext-0193a5c0-0000-7000-8000-000000000070";

#[derive(Default)]
struct Office {
    pubkey: Option<Vec<u8>>,
    polls: u32,
    nonce: u32,
    tokens: Vec<String>,
    bound_with: Vec<String>,
    logged_out: bool,
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
        office.pubkey = Some(
            base64::engine::general_purpose::STANDARD
                .decode(&p.pubkey_ed25519_b64)
                .unwrap(),
        );
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
                office_id: OFFICE.into(),
                nonce: format!("nonce{}", office.nonce),
                expires_at: "2099-01-01T00:00:00.000Z".into(),
            },
        )
        .unwrap()
    } else if req.is::<AuthExchange>() {
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
                token,
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
    assert!(env["data"]["commands"]
        .as_str()
        .unwrap()
        .contains("msg send"));
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
