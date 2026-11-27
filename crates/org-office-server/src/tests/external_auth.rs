//! External (BYOA) agents end to end over the real office socket, a real `SessionManager` and
//! the real `agent-os` library (`TASK-137`): login, operator approval, the 1-RTT Ed25519
//! exchange, hot-path bound commands, the collaboration allowlist, held managed wakes, token
//! loss and refresh, nonce replay, supersede, revoke and offboarding.

use std::sync::Arc;
use std::time::Duration;

use org_agent_cli::external_auth::client::{self, Target};
use org_agent_cli::external_auth::store::{TokenFile, WorkspaceBinding, WorkspaceState};
use org_agent_cli::Binding;
use org_client::Client;
use org_office::{EngineChoice, Office, TestOffice};
use org_proto::external_auth::{
    AuthExchange, AuthLoginPoll, AuthLoginPollParams, AuthLoginRequest, AuthLoginRequestParams,
    AuthLoginRequestResult, AuthWhoami, ChallengePurpose, ExternalLoginState, ExternalSessionView,
    MemberExternalApprove, MemberExternalApproveParams, MemberExternalPending,
    MemberExternalPendingParams, MemberExternalPendingResult, MemberExternalRenew,
    MemberExternalRenewParams, MemberExternalRevoke, MemberExternalRevokeParams,
    MemberExternalSessions, MemberExternalSessionsParams, SessionMode, WorkspaceFamiliarity,
    MAX_LEASE_TTL_SECS,
};
use org_proto::hiring::{AgentOffboard, AgentOffboardParams};
use org_proto::msg::{MsgBind, MsgBindParams};
use org_proto::{Call, Empty, ErrorCode, Outcome, Request, Response};
use serde_json::{json, Value};
use zeroize::Zeroizing;

use super::a2a_messaging::{hire_agent_at, setup_provider_on_hq, wait_until, LiveOfficeNode};

struct World {
    office: Arc<Office>,
    node: LiveOfficeNode,
    owner: String,
    ada: String,
    ben: String,
}

fn world() -> World {
    let office = Arc::new(Office::for_test_with_real_ca(TestOffice {
        engine: EngineChoice::Sqlite,
    }));
    let owner = office.owner_member_id().to_owned();
    let team_id = office.org_snapshot().unwrap().teams[0].id.clone();
    let hq_id = office.info().office_id.clone();
    let (_conn, profile) = setup_provider_on_hq(&office, &owner);
    let ada = hire_agent_at(&office, &owner, &team_id, &hq_id, &profile, "Ada", "ada");
    let ben = hire_agent_at(&office, &owner, &team_id, &hq_id, &profile, "Ben", "ben");
    let node = LiveOfficeNode::start(Arc::clone(&office), false, false);
    World {
        office,
        node,
        owner,
        ada,
        ben,
    }
}

fn target(w: &World) -> Target {
    Target {
        socket: Some(w.node.socket.clone()),
        office_id: None,
        runtime_dir: None,
    }
}

fn grant_call<C: Call>(w: &World, params: &C::Params) -> Response {
    let request = Request::new::<C>(7, params).unwrap();
    w.node.shared.answer_for_test(&request, &w.owner, true)
}

fn ok_value(response: Response) -> Value {
    match response.outcome {
        Outcome::Result(v) => v,
        Outcome::Error(e) => panic!("refused: {e:?}"),
    }
}

fn error_code(response: &Response) -> ErrorCode {
    match &response.outcome {
        Outcome::Error(e) => e.code,
        Outcome::Result(v) => panic!("expected a refusal, got {v}"),
    }
}

fn login(w: &World, member: &str) -> (Zeroizing<Vec<u8>>, AuthLoginRequestResult) {
    let (pkcs8, pubkey) = client::generate_key().unwrap();
    let request = client::call::<AuthLoginRequest>(
        &target(w).connect().unwrap(),
        &AuthLoginRequestParams {
            member: member.into(),
            pubkey_ed25519_b64: pubkey,
            workspace_label: "laptop:~/src/app".into(),
            hostname: None,
            workspace_path: None,
        },
    )
    .unwrap();
    (pkcs8, request)
}

fn approve(w: &World, code: &str) -> ExternalSessionView {
    let view = ok_value(grant_call::<MemberExternalApprove>(
        w,
        &MemberExternalApproveParams {
            command_id: None,
            code: code.into(),
            ttl_secs: None,
            takeover: false,
        },
    ));
    serde_json::from_value(view).unwrap()
}

fn signed_in(w: &World, member: &str) -> (Zeroizing<Vec<u8>>, String, String) {
    let (pkcs8, request) = login(w, member);
    let view = approve(w, &request.approval_code);
    let polled = client::call::<AuthLoginPoll>(
        &target(w).connect().unwrap(),
        &AuthLoginPollParams {
            request_id: request.request_id,
        },
    )
    .unwrap();
    assert_eq!(polled.state, ExternalLoginState::Approved);
    let token = client::exchange(
        &target(w).connect().unwrap(),
        &signer(w, &pkcs8, &request.member_id, &view.session_id),
    )
    .unwrap();
    (pkcs8, view.session_id, token.token.to_string())
}

fn signer<'a>(
    w: &'a World,
    pkcs8: &'a [u8],
    member: &'a str,
    session: &'a str,
) -> client::Signer<'a> {
    client::Signer {
        pkcs8,
        office_id: Some(&w.office.info().office_id),
        member_id: member,
        session_id: session,
    }
}

fn bound_client(w: &World, member: &str, token: &str) -> Client {
    let conn = target(w).connect().unwrap();
    client::bind(&conn, member, token).unwrap();
    conn
}

fn agent_os(w: &World, binding: Binding, args: &[&str]) -> (u8, Value) {
    let _ = w;
    let owned: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
    let out = org_agent_cli::execute(&owned, binding);
    let json = serde_json::from_str(&out.stdout)
        .unwrap_or_else(|e| panic!("{args:?}: {e}: {}", out.stdout));
    (out.exit_code, json)
}

fn token_binding(w: &World, member: &str, token: &str) -> Binding {
    Binding {
        socket: Some(w.node.socket.clone()),
        office_id: None,
        runtime_dir: None,
        session_token: Some(token.to_owned()),
        agent_id: Some(member.to_owned()),
        external_state: None,
    }
}

#[test]
fn an_office_socket_agent_can_never_approve_list_or_revoke_external_sessions() {
    let w = world();
    let (_, request) = login(&w, "@ada");
    // Ben is a managed agent bound on the office socket with a valid run token.
    let run = w
        .node
        .sessions
        .bindings()
        .issue(
            &w.ben,
            "run-ben",
            "sess-ben",
            std::process::id() as libc::pid_t,
            None,
        )
        .unwrap();
    let conn = target(&w).connect().unwrap();
    conn.call::<MsgBind>(&MsgBindParams::member_with_token(
        w.ben.clone(),
        run.expose_token().to_owned(),
    ))
    .unwrap();
    for (method, params) in [
        (
            MemberExternalApprove::METHOD,
            json!({"code": request.approval_code}),
        ),
        (
            MemberExternalSessions::METHOD,
            json!({"employee_id": "@ada"}),
        ),
        (MemberExternalRevoke::METHOD, json!({"employee_id": "@ada"})),
    ] {
        let refused = conn.call_raw(method, params).unwrap_err();
        assert!(
            matches!(&refused, org_client::ClientError::Api(e) if e.code == ErrorCode::NotAuthorized),
            "{method}: {refused:?}"
        );
    }
    // Nothing was spent: the owner still approves with the same code.
    approve(&w, &request.approval_code);
    w.node.sessions.bindings().revoke(&w.ben, None);
}

#[test]
fn wrong_codes_lock_a_sign_in_after_three_attempts() {
    let w = world();
    let (_, request) = login(&w, "ada");
    let (lookup, secret) = request.approval_code.split_once('-').unwrap();
    let flip = |i: usize| {
        let mut chars: Vec<char> = secret.chars().collect();
        chars[i] = if chars[i] == 'Z' { 'Y' } else { 'Z' };
        format!("{lookup}-{}", chars.into_iter().collect::<String>())
    };
    for i in 0..3 {
        let response = grant_call::<MemberExternalApprove>(
            &w,
            &MemberExternalApproveParams {
                command_id: None,
                code: flip(i),
                ttl_secs: None,
                takeover: false,
            },
        );
        assert_eq!(error_code(&response), ErrorCode::NotAuthorized);
    }
    let late = grant_call::<MemberExternalApprove>(
        &w,
        &MemberExternalApproveParams {
            command_id: None,
            code: request.approval_code.clone(),
            ttl_secs: None,
            takeover: false,
        },
    );
    assert_eq!(error_code(&late), ErrorCode::Conflict);
    let polled = client::call::<AuthLoginPoll>(
        &target(&w).connect().unwrap(),
        &AuthLoginPollParams {
            request_id: request.request_id,
        },
    )
    .unwrap();
    assert_eq!(polled.state, ExternalLoginState::Locked);
}

#[test]
fn external_agent_lifecycle_over_the_real_office_socket() {
    let w = world();
    let (pkcs8, session, token) = signed_in(&w, "@ada");

    // Hot path: `agent-os` binds with the fast token and runs collaboration commands.
    let (code, who) = agent_os(&w, token_binding(&w, &w.ada, &token), &["who"]);
    assert_eq!(code, 0, "{who}");
    let (code, tasks) = agent_os(&w, token_binding(&w, &w.ada, &token), &["task", "list"]);
    assert_eq!(code, 0, "{tasks}");
    let conn = bound_client(&w, &w.ada, &token);
    let me = conn.call::<AuthWhoami>(&Empty {}).unwrap();
    assert_eq!(me.mode, SessionMode::External);
    assert_eq!(me.session_id.as_deref(), Some(session.as_str()));
    assert_eq!(me.handle.as_deref(), Some("ada"));

    // Only collaboration calls: host administration and session internals are refused.
    let (code, grants) = agent_os(&w, token_binding(&w, &w.ada, &token), &["grant", "list"]);
    assert_ne!(code, 0);
    assert_eq!(grants["error"]["code"], "not_authorized", "{grants}");
    let (code, repos) = agent_os(&w, token_binding(&w, &w.ada, &token), &["repo", "list"]);
    assert_ne!(code, 0, "{repos}");
    // A bound connection cannot start another sign-in.
    let refused = conn
        .call_raw(
            AuthLoginRequest::METHOD,
            json!({"member": "ada", "pubkey_ed25519_b64": "x", "workspace_label": "y"}),
        )
        .unwrap_err();
    assert!(
        matches!(&refused, org_client::ClientError::Api(e) if e.code == ErrorCode::InvalidRequest)
    );

    // While the lease is active, a message to Ada wakes no managed run; it waits for her.
    let run = w
        .node
        .sessions
        .bindings()
        .issue(
            &w.ben,
            "run-ben",
            "sess-ben",
            std::process::id() as libc::pid_t,
            None,
        )
        .unwrap();
    let ben = token_binding(&w, &w.ben, run.expose_token());
    let (code, sent) = agent_os(&w, ben, &["msg", "send", "--to", "ada", "--body", "ping"]);
    assert_eq!(code, 0, "{sent}");
    w.node.sessions.bindings().revoke(&w.ben, None);
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        w.node.recorded.count_for("ada"),
        0,
        "no competing managed run"
    );
    let (_, inbox) = agent_os(&w, token_binding(&w, &w.ada, &token), &["msg", "inbox"]);
    assert!(
        inbox["meta"]["waiting_messages"].as_u64().unwrap_or(0) >= 1,
        "{inbox}"
    );

    // Nonce replay is refused.
    let signed = client::signed_challenge(
        &target(&w).connect().unwrap(),
        &signer(&w, &pkcs8, &w.ada, &session),
        ChallengePurpose::Exchange,
    )
    .unwrap();
    let fresh = target(&w).connect().unwrap();
    client::call::<AuthExchange>(&fresh, &signed).unwrap();
    let replay = client::call::<AuthExchange>(&fresh, &signed).unwrap_err();
    assert_eq!(replay.code, ErrorCode::NotAuthorized);

    // The office forgets every RAM token (as on a restart): the bound connection ends, and a
    // signed-in workspace refreshes by challenge-response on its next command.
    w.node.sessions.bindings().revoke_external(&w.ada, None);
    assert!(conn.call_raw("who", json!({})).is_err());
    let state_dir = tempfile::tempdir().unwrap();
    let state = WorkspaceState::at(state_dir.path().join("ws"));
    state.ensure().unwrap();
    state.write_key(&pkcs8).unwrap();
    state
        .write_binding(&WorkspaceBinding {
            version: 1,
            workspace_root: "/src/app".into(),
            office_id: w.office.info().office_id.clone(),
            socket: Some(w.node.socket.display().to_string()),
            runtime_dir: None,
            member_id: w.ada.clone(),
            handle: Some("ada".into()),
            session_id: session.clone(),
            lease_expires_at: "2099-01-01T00:00:00.000Z".into(),
        })
        .unwrap();
    state
        .write_token(&TokenFile {
            version: 1,
            session_id: session.clone(),
            token: token.clone().into(),
            expires_at: "2099-01-01T00:00:00.000Z".into(),
        })
        .unwrap();
    let workspace = Binding {
        socket: Some(w.node.socket.clone()),
        office_id: Some(w.office.info().office_id.clone()),
        runtime_dir: None,
        session_token: Some(token.clone()),
        agent_id: Some(w.ada.clone()),
        external_state: Some(state.dir().to_path_buf()),
    };
    let (code, who) = agent_os(&w, workspace.clone(), &["who"]);
    assert_eq!(code, 0, "refresh after token loss: {who}");
    let refreshed = state.read_token().unwrap().unwrap().token.to_string();
    assert_ne!(refreshed, token);

    // A second approval supersedes the lease: the old key and token stop working at once.
    let live = bound_client(&w, &w.ada, &refreshed);
    let (_, second_session, second_token) = signed_in(&w, "ada");
    assert_ne!(second_session, session);
    assert!(
        live.call_raw("who", json!({})).is_err(),
        "superseded connection closed"
    );
    assert!(client::bind(&target(&w).connect().unwrap(), &w.ada, &refreshed).is_err());
    let (code, gone) = agent_os(&w, workspace, &["who"]);
    assert_ne!(code, 0, "{gone}");

    // The operator lists and revokes it; the connection closes and the token is refused.
    let listed = ok_value(grant_call::<MemberExternalSessions>(
        &w,
        &MemberExternalSessionsParams {
            employee_id: "@ada".into(),
            limit: None,
        },
    ));
    assert_eq!(listed["active"]["session_id"], second_session.as_str());
    let live = bound_client(&w, &w.ada, &second_token);
    let revoked = ok_value(grant_call::<MemberExternalRevoke>(
        &w,
        &MemberExternalRevokeParams {
            command_id: None,
            employee_id: "ada".into(),
            session_id: None,
        },
    ));
    assert_eq!(revoked["revoked"], true);
    assert!(live.call_raw("who", json!({})).is_err());
    assert!(client::bind(&target(&w).connect().unwrap(), &w.ada, &second_token).is_err());

    // Revoking the lease releases the held wake from Ben's earlier ping; wait for that managed
    // turn to settle before opening a fresh non-takeover lease for the offboarding check.
    wait_until(
        Duration::from_secs(5),
        "held wake settles after revoke",
        || {
            w.node.recorded.count_for("ada") >= 1
                && !matches!(
                    w.node
                        .sessions
                        .inspect_session(&w.ada, None)
                        .ok()
                        .map(|s| s.session.state),
                    Some(
                        org_proto::sessions::SessionState::Starting
                            | org_proto::sessions::SessionState::Running
                            | org_proto::sessions::SessionState::WaitingApproval
                    )
                )
        },
    );

    // Offboarding ends a fresh lease durably and closes its connections.
    let (_, third_session, third_token) = signed_in(&w, "ada");
    let live = bound_client(&w, &w.ada, &third_token);
    let version = w
        .office
        .org_snapshot()
        .unwrap()
        .members
        .into_iter()
        .find(|m| m.id == w.ada)
        .unwrap()
        .version;
    ok_value(grant_call::<AgentOffboard>(
        &w,
        &AgentOffboardParams {
            command_id: w.office.next_uuid_v7(),
            member_id: w.ada.clone(),
            expected_version: version,
        },
    ));
    assert!(live.call_raw("who", json!({})).is_err());
    let lease = w
        .node
        .sessions
        .store()
        .read_external_lease(&w.ada)
        .unwrap()
        .unwrap();
    assert_eq!(lease.session_id, third_session);
    assert!(lease.revoked_at.is_some());
    wait_until(Duration::from_secs(2), "no external token survives", || {
        !w.node.sessions.bindings().has_active_external(&w.ada, 0)
    });
}

/// B4b: the approval's closing step spares a connection already bound with the new lease (a
/// fast-polling `agent-os login` that exchanged and bound first) and closes the lease it
/// superseded; an operator revocation and a logout close only their own lease's connections.
#[test]
fn an_approval_never_closes_a_connection_already_bound_with_the_new_lease() {
    let w = world();
    let (_, old_session, old_token) = signed_in(&w, "ada");
    let old_conn = bound_client(&w, &w.ada, &old_token);
    // The second sign-in is approved through the port alone: the server's closing step has
    // not run yet when the workspace polls, exchanges and binds.
    let (pkcs8, request) = login(&w, "ada");
    let view = w
        .node
        .sessions
        .external_approve(
            &w.owner,
            &MemberExternalApproveParams {
                command_id: None,
                code: request.approval_code.clone(),
                ttl_secs: None,
                takeover: false,
            },
        )
        .unwrap();
    assert_ne!(view.session_id, old_session);
    let token = client::exchange(
        &target(&w).connect().unwrap(),
        &signer(&w, &pkcs8, &w.ada, &view.session_id),
    )
    .unwrap();
    let new_conn = bound_client(&w, &w.ada, &token.token);
    crate::serve::close_after_approval(&w.node.shared, &view);
    assert!(
        new_conn.call_raw("who", json!({})).is_ok(),
        "the new lease's connection keeps going"
    );
    assert!(old_conn.call_raw("who", json!({})).is_err());
    // Revoking the new lease closes its connection.
    let revoked = ok_value(grant_call::<MemberExternalRevoke>(
        &w,
        &MemberExternalRevokeParams {
            command_id: None,
            employee_id: "ada".into(),
            session_id: Some(view.session_id.clone()),
        },
    ));
    assert_eq!(revoked["revoked"], true);
    assert!(new_conn.call_raw("who", json!({})).is_err());
}

/// B5a: a caller that opens a fresh connection for every sign-in call never meets the
/// per-connection budget; the office-wide budget stops it, per kind of call.
#[test]
fn fresh_connections_cannot_flood_the_sign_in_calls() {
    let w = world();
    let mut answers = Vec::new();
    for _ in 0..12 {
        let (_, pubkey) = client::generate_key().unwrap();
        answers.push(client::call::<AuthLoginRequest>(
            &target(&w).connect().unwrap(),
            &AuthLoginRequestParams {
                member: "@ben".into(),
                pubkey_ed25519_b64: pubkey,
                workspace_label: "flood".into(),
                hostname: None,
                workspace_path: None,
            },
        ));
    }
    let codes: Vec<ErrorCode> = answers
        .iter()
        .map(|a| a.as_ref().err().map_or(ErrorCode::Unknown, |r| r.code))
        .collect();
    let accepted = answers.iter().filter(|a| a.is_ok()).count();
    assert_eq!(accepted, 3, "the per-agent pending cap: {codes:?}");
    assert!(
        codes[8..].iter().all(|c| *c == ErrorCode::RateLimited),
        "the office-wide budget: {codes:?}"
    );
    // Polls have their own budget: the waiting logins still learn where they stand.
    let pending = answers[0].as_ref().unwrap();
    let polled = client::call::<AuthLoginPoll>(
        &target(&w).connect().unwrap(),
        &AuthLoginPollParams {
            request_id: pending.request_id.clone(),
        },
    )
    .unwrap();
    assert_eq!(polled.state, ExternalLoginState::Pending);
    // A flood of polls from several callers on fresh connections is held the same way.
    let limited = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                for _ in 0..400 {
                    if limited.load(std::sync::atomic::Ordering::SeqCst) {
                        return;
                    }
                    let answer = client::call::<AuthLoginPoll>(
                        &target(&w).connect().unwrap(),
                        &AuthLoginPollParams {
                            request_id: "no-such-request".into(),
                        },
                    );
                    if answer.is_err_and(|r| r.code == ErrorCode::RateLimited) {
                        limited.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                }
            });
        }
    });
    let limited = limited.into_inner();
    assert!(limited, "polls are budgeted office-wide");
}

/// B3: a bound external agent's commands write nothing to disk (the history keeps lifecycle
/// events alone) and stay within the hot-path budget over the real office socket (`doc-7` #18):
/// the external binding's own share of a call (against the same call on a managed binding,
/// sampled in turn) is at most 1 ms at the median and 2 ms at the 95th percentile in every
/// build, and the whole call is within that budget in an optimized build. (An unoptimized test
/// build spends more than 1 ms per call in the directory lookup every bound call makes, managed
/// or external alike.)
#[test]
fn bound_external_commands_do_no_disk_io_and_stay_within_the_hot_path_budget() {
    let w = world();
    let (_, _session, token) = signed_in(&w, "ada");
    let external = bound_client(&w, &w.ada, &token);
    let run = w
        .node
        .sessions
        .bindings()
        .issue(
            &w.ben,
            "run-ben",
            "sess-ben",
            std::process::id() as libc::pid_t,
            None,
        )
        .unwrap();
    let managed = bound_client(&w, &w.ben, run.expose_token());
    let history = w
        .node
        .sessions
        .store()
        .external_history_path(&w.ada)
        .unwrap();
    let before = std::fs::read(&history).unwrap();
    for _ in 0..30 {
        external.call_raw(HOT_PATH_METHOD, json!({})).unwrap();
        managed.call_raw(HOT_PATH_METHOD, json!({})).unwrap();
    }
    let timed = |conn: &Client| {
        let started = std::time::Instant::now();
        conn.call_raw(HOT_PATH_METHOD, json!({})).unwrap();
        started.elapsed()
    };
    let (mut ext, mut man): (Vec<Duration>, Vec<Duration>) = (0..300)
        .map(|_| (timed(&external), timed(&managed)))
        .unzip();
    ext.sort();
    man.sort();
    let at = |samples: &[Duration], pct: usize| samples[samples.len() * pct / 100];
    let (median, p95) = (at(&ext, 50), at(&ext, 95));
    let share_median = median.saturating_sub(at(&man, 50));
    let share_p95 = p95.saturating_sub(at(&man, 95));
    eprintln!(
        "external hot path over the socket: median {median:?} (external share {share_median:?}), p95 {p95:?} (external share {share_p95:?})"
    );
    assert!(share_median <= Duration::from_millis(1), "{share_median:?}");
    assert!(share_p95 <= Duration::from_millis(2), "{share_p95:?}");
    if !cfg!(debug_assertions) {
        assert!(median <= Duration::from_millis(1), "median {median:?}");
        assert!(p95 <= Duration::from_millis(2), "p95 {p95:?}");
    }
    assert_eq!(
        std::fs::read(&history).unwrap(),
        before,
        "no history line per command"
    );
    assert!(
        w.node
            .sessions
            .bindings()
            .external_accepted_commands(&w.ada)
            .len()
            >= 330,
        "every command is still counted in RAM"
    );
    w.node.sessions.bindings().revoke(&w.ben, None);
}

/// The collaboration call the hot-path test repeats.
const HOT_PATH_METHOD: &str = "msg.inbox";

/// An [`org_agent_cli::OfficeTransport`] backed directly by the real office server's
/// grant-bound dispatch for `actor`.
struct GrantTransport<'a> {
    world: &'a World,
    actor: String,
    next_id: u64,
}

impl<'a> GrantTransport<'a> {
    fn new(world: &'a World, actor: &str) -> Self {
        Self {
            world,
            actor: actor.to_owned(),
            next_id: 100,
        }
    }
}

impl org_agent_cli::OfficeTransport for GrantTransport<'_> {
    fn call_method(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, org_agent_cli::Refusal> {
        let id = self.next_id;
        self.next_id += 1;
        let req = Request {
            id,
            method: method.to_owned(),
            params,
            principal: None,
        };
        let resp = self
            .world
            .node
            .shared
            .answer_for_test(&req, &self.actor, true);
        match resp.outcome {
            Outcome::Result(value) => Ok(value),
            Outcome::Error(err) => Err(org_agent_cli::api_refusal(err)),
        }
    }
}

fn init_real_git_repo(dir: &std::path::Path) {
    std::fs::create_dir_all(dir).unwrap();
    let run = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    };
    run(&["init", "-q"]);
    std::fs::write(dir.join("README.md"), "# Workspace\n").unwrap();
    run(&["add", "README.md"]);
    run(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "commit",
        "-q",
        "-m",
        "initial commit",
    ]);
}

fn assert_clean_git_tree(dir: &std::path::Path) {
    let out = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git status failed");
    assert!(
        out.stdout.is_empty(),
        "expected zero git working-tree diff, got: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

fn binding_from_workspace_state(state: &WorkspaceState) -> Binding {
    let saved = state.read_binding().unwrap().expect("saved binding");
    Binding {
        socket: saved.socket.map(std::path::PathBuf::from),
        office_id: Some(saved.office_id),
        runtime_dir: saved.runtime_dir.map(std::path::PathBuf::from),
        // Intentionally None: forces the real ADR-0036 Ed25519 challenge-exchange flow.
        session_token: None,
        agent_id: Some(saved.member_id),
        external_state: Some(state.dir().to_path_buf()),
    }
}

fn mode_of(path: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::symlink_metadata(path)
        .unwrap()
        .permissions()
        .mode()
        & 0o777
}

#[test]
fn one_step_agent_bind_authenticates_agent_os_immediately_with_zero_git_diff() {
    use org_agent_cli::external_auth::bind::{bind_workspace, WorkspaceBindRequest};

    let w = world();
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    init_real_git_repo(&repo);
    let state_root = tmp.path().join("state");

    let mut transport = GrantTransport::new(&w, &w.owner);
    let bound = bind_workspace(
        &mut transport,
        &WorkspaceBindRequest {
            member: "@ada",
            workspace_dir: &repo,
            label: None,
            ttl_secs: None,
            takeover: false,
            command_id: None,
            state_root: Some(&state_root),
        },
    )
    .unwrap();
    assert_eq!(bound.office_id, w.office.info().office_id);
    assert_eq!(bound.session.employee_id, w.ada);
    assert_eq!(bound.session.handle.as_deref(), Some("ada"));
    assert_eq!(bound.session.approved_by, w.owner);
    assert!(bound.session.active);

    // Credentials live strictly outside Git with 0700 / 0600 modes and ZERO git working-tree diff.
    let canonical = repo.canonicalize().unwrap();
    let state = WorkspaceState::for_root(&state_root, &canonical);
    assert_eq!(mode_of(state.dir()), 0o700);
    assert_eq!(mode_of(&state.dir().join("binding.v1.json")), 0o600);
    assert_eq!(mode_of(&state.dir().join("key.ed25519.pkcs8")), 0o600);
    assert!(state.read_token().unwrap().is_none());
    assert_clean_git_tree(&repo);

    // Subsequent `agent-os` commands authenticate immediately via Ed25519 challenge-exchange!
    let ws_binding = binding_from_workspace_state(&state);
    let (code, who) = agent_os(&w, ws_binding.clone(), &["who"]);
    assert_eq!(code, 0, "immediate agent-os who failed: {who}");
    let (code, tasks) = agent_os(&w, ws_binding, &["task", "list"]);
    assert_eq!(code, 0, "immediate agent-os task list failed: {tasks}");

    // The challenge-exchange wrote an owner-only cached fast token and bound as External.
    let cached = state.read_token().unwrap().expect("cached fast token");
    assert_eq!(cached.session_id, bound.session.session_id);
    assert_eq!(mode_of(&state.dir().join("token.v1")), 0o600);
    let conn = bound_client(&w, &w.ada, &cached.token);
    let me = conn.call::<AuthWhoami>(&Empty {}).unwrap();
    assert_eq!(me.mode, SessionMode::External);
    assert_eq!(me.member_id, w.ada);
    assert_eq!(
        me.session_id.as_deref(),
        Some(bound.session.session_id.as_str())
    );
    assert_clean_git_tree(&repo);
}

#[test]
fn one_step_hr_hire_with_bind_workspace_hires_and_authenticates_immediately() {
    use org_agent_cli::external_auth::bind::{bind_workspace, WorkspaceBindRequest};
    use org_agent_cli::external_auth::store::find_workspace;
    use org_agent_cli::TransportOffice;

    let w = world();
    let team_id = w.office.org_snapshot().unwrap().teams[0].id.clone();
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("grace-repo");
    init_real_git_repo(&repo);
    let nested = repo.join("src").join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    let state_root = tmp.path().join("state");

    // Step 1 + 2 on the same operator grant transport: hire `@grace` and bind `<repo>`.
    let mut transport = GrantTransport::new(&w, &w.owner);
    let hire_args: Vec<String> = [
        "hr",
        "hire",
        "--team",
        &team_id,
        "--name",
        "Grace",
        "--handle",
        "grace",
        "--reason",
        "BYOA one-step hire and bind",
        "--format",
        "json",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let hire_out = org_agent_cli::execute_with_office(
        "agentic-org",
        &hire_args,
        &mut TransportOffice(&mut transport),
    );
    assert_eq!(hire_out.exit_code, 0, "{}", hire_out.stdout);
    let hire_env: Value = serde_json::from_str(&hire_out.stdout).unwrap();
    assert_eq!(hire_env["data"]["state"], "hired", "{hire_env}");
    let grace_id = hire_env["data"]["employee_id"].as_str().unwrap().to_owned();

    let bound = bind_workspace(
        &mut transport,
        &WorkspaceBindRequest {
            member: &grace_id,
            workspace_dir: &repo,
            label: None,
            ttl_secs: None,
            takeover: false,
            command_id: None,
            state_root: Some(&state_root),
        },
    )
    .unwrap();
    assert_eq!(bound.session.employee_id, grace_id);
    assert_eq!(bound.session.handle.as_deref(), Some("grace"));

    // Walk-up discovery from a nested subdirectory resolves the repo binding immediately.
    let (found_root, state) = find_workspace(&state_root, &nested).expect("walk-up binding");
    assert_eq!(found_root, repo.canonicalize().unwrap());
    let (code, who) = agent_os(&w, binding_from_workspace_state(&state), &["who"]);
    assert_eq!(
        code, 0,
        "immediate agent-os auth for newly hired agent: {who}"
    );
    let cached = state.read_token().unwrap().expect("exchanged fast token");
    let conn = bound_client(&w, &grace_id, &cached.token);
    let me = conn.call::<AuthWhoami>(&Empty {}).unwrap();
    assert_eq!(me.mode, SessionMode::External);
    assert_eq!(me.member_id, grace_id);
    assert_eq!(me.handle.as_deref(), Some("grace"));
    assert_clean_git_tree(&repo);
}

#[test]
fn one_step_bind_enforces_single_active_lease_supersession_and_managed_takeover() {
    use org_agent_cli::external_auth::bind::{bind_workspace, WorkspaceBindRequest};

    let w = world();
    let tmp = tempfile::tempdir().unwrap();
    let ws1 = tmp.path().join("ws1");
    let ws2 = tmp.path().join("ws2");
    let ws_ben = tmp.path().join("ws-ben");
    for d in [&ws1, &ws2, &ws_ben] {
        std::fs::create_dir(d).unwrap();
    }
    let state_root = tmp.path().join("state");
    let mut transport = GrantTransport::new(&w, &w.owner);

    // Bind Workspace 1 -> authenticates and opens a live bound connection.
    let first = bind_workspace(
        &mut transport,
        &WorkspaceBindRequest {
            member: "@ada",
            workspace_dir: &ws1,
            label: Some("ws1"),
            ttl_secs: None,
            takeover: false,
            command_id: None,
            state_root: Some(&state_root),
        },
    )
    .unwrap();
    let state1 = WorkspaceState::for_root(&state_root, &ws1.canonicalize().unwrap());
    let (code, _) = agent_os(&w, binding_from_workspace_state(&state1), &["who"]);
    assert_eq!(code, 0);
    let ws1_token = state1.read_token().unwrap().unwrap().token.to_string();
    let live1 = bound_client(&w, &w.ada, &ws1_token);

    // Re-binding Workspace 2 supersedes Workspace 1 immediately.
    let second = bind_workspace(
        &mut transport,
        &WorkspaceBindRequest {
            member: "@ada",
            workspace_dir: &ws2,
            label: Some("ws2"),
            ttl_secs: None,
            takeover: false,
            command_id: None,
            state_root: Some(&state_root),
        },
    )
    .unwrap();
    assert_ne!(second.session.session_id, first.session.session_id);
    assert_eq!(
        second.session.superseded_session_id.as_deref(),
        Some(first.session.session_id.as_str())
    );
    assert!(
        live1.call_raw("who", json!({})).is_err(),
        "superseded workspace connection must close immediately"
    );
    let (code, refused) = agent_os(&w, binding_from_workspace_state(&state1), &["who"]);
    assert_ne!(code, 0, "superseded workspace must fail closed: {refused}");

    let state2 = WorkspaceState::for_root(&state_root, &ws2.canonicalize().unwrap());
    let (code, who2) = agent_os(&w, binding_from_workspace_state(&state2), &["who"]);
    assert_eq!(
        code, 0,
        "replacement workspace must work immediately: {who2}"
    );

    // Active managed turn on `@ben` refuses bind without `--takeover` (repeated attempts never
    // leak pending logins) and succeeds WITH `--takeover`.
    let _managed = w
        .node
        .sessions
        .bindings()
        .issue(
            &w.ben,
            "run-ben-live",
            "sess-ben-live",
            std::process::id() as libc::pid_t,
            None,
        )
        .unwrap();
    let state_ben = WorkspaceState::for_root(&state_root, &ws_ben.canonicalize().unwrap());
    for attempt in 0..4 {
        let err = bind_workspace(
            &mut transport,
            &WorkspaceBindRequest {
                member: "@ben",
                workspace_dir: &ws_ben,
                label: None,
                ttl_secs: None,
                takeover: false,
                command_id: None,
                state_root: Some(&state_root),
            },
        )
        .unwrap_err();
        assert_eq!(
            err.code,
            ErrorCode::Conflict,
            "attempt {attempt} must fail with Conflict (no pending-login leak): {err:?}"
        );
        assert!(!state_ben.dir().exists());
    }
    let taken = bind_workspace(
        &mut transport,
        &WorkspaceBindRequest {
            member: "@ben",
            workspace_dir: &ws_ben,
            label: None,
            ttl_secs: None,
            takeover: true,
            command_id: None,
            state_root: Some(&state_root),
        },
    )
    .unwrap();
    assert!(taken.session.took_over_managed);
    assert!(!w.node.sessions.bindings().has_active(&w.ben));
    let (code, who_ben) = agent_os(&w, binding_from_workspace_state(&state_ben), &["who"]);
    assert_eq!(code, 0, "bound workspace after takeover failed: {who_ben}");
}

#[test]
fn one_step_bind_refuses_unauthorized_callers_and_chief_of_staff_closed() {
    use org_agent_cli::external_auth::bind::{bind_workspace, WorkspaceBindRequest};
    use org_proto::external_auth::MemberExternalBind;
    use org_proto::org::MemberAddPersonParams;

    let w = world();
    let team_id = w.office.org_snapshot().unwrap().teams[0].id.clone();
    let alice_id = w.office.next_uuid_v7();
    w.office
        .submit_org(
            &w.owner,
            org_office::rules::OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: w.office.next_uuid_v7(),
                member_id: alice_id.clone(),
                name: "Alice".into(),
                handle: Some("alice".into()),
                home_team_id: Some(team_id),
                contact: None,
            }),
        )
        .unwrap()
        .unwrap();
    // Newly added people start with `may_hire = true`; revoke it so Alice has no authority over Ada.
    let alice_version = w
        .office
        .org_snapshot()
        .unwrap()
        .members
        .into_iter()
        .find(|m| m.id == alice_id)
        .unwrap()
        .version;
    ok_value(grant_call::<org_proto::hiring::MemberSetMayHire>(
        &w,
        &org_proto::hiring::MemberSetMayHireParams {
            command_id: w.office.next_uuid_v7(),
            member_id: alice_id.clone(),
            expected_version: alice_version,
            may_hire: false,
        },
    ));

    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let state_root = tmp.path().join("state");
    let state = WorkspaceState::for_root(&state_root, &ws.canonicalize().unwrap());

    // 1. Unauthorized active person (not owner, may_hire=false, not manager/lead) fails closed.
    let mut alice_transport = GrantTransport::new(&w, &alice_id);
    for _ in 0..4 {
        let err = bind_workspace(
            &mut alice_transport,
            &WorkspaceBindRequest {
                member: "@ada",
                workspace_dir: &ws,
                label: None,
                ttl_secs: None,
                takeover: false,
                command_id: None,
                state_root: Some(&state_root),
            },
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotAuthorized, "{err:?}");
        assert!(!state.dir().exists());
    }

    // 2. Managed agent bound on the office socket can never call `member.external_bind`.
    let run = w
        .node
        .sessions
        .bindings()
        .issue(
            &w.ben,
            "run-ben",
            "sess-ben",
            std::process::id() as libc::pid_t,
            None,
        )
        .unwrap();
    let managed_conn = target(&w).connect().unwrap();
    managed_conn
        .call::<MsgBind>(&MsgBindParams::member_with_token(
            w.ben.clone(),
            run.expose_token().to_owned(),
        ))
        .unwrap();
    let (_, pubkey) = client::generate_key().unwrap();
    let refused = managed_conn
        .call_raw(
            MemberExternalBind::METHOD,
            json!({
                "employee_id": "@ada",
                "pubkey_ed25519_b64": pubkey,
                "workspace_label": "stolen"
            }),
        )
        .unwrap_err();
    assert!(
        matches!(&refused, org_client::ClientError::Api(e) if e.code == ErrorCode::NotAuthorized),
        "managed agent must be refused: {refused:?}"
    );
    w.node.sessions.bindings().revoke(&w.ben, None);

    // 3. External agent bound via fast token on the office socket can never call `member.external_bind`.
    let (_, _, ada_token) = signed_in(&w, "@ada");
    let ext_conn = bound_client(&w, &w.ada, &ada_token);
    let refused = ext_conn
        .call_raw(
            MemberExternalBind::METHOD,
            json!({
                "employee_id": "@ben",
                "pubkey_ed25519_b64": pubkey,
                "workspace_label": "lateral"
            }),
        )
        .unwrap_err();
    assert!(
        matches!(&refused, org_client::ClientError::Api(e) if e.code == ErrorCode::NotAuthorized),
        "external agent must be refused: {refused:?}"
    );

    // 4. Built-in Chief of Staff (even with a live managed turn!) and built-in HR agent can
    // never be bound externally even by owner, and both return `NotAuthorized` (never `Conflict`).
    let tess = w.office.chief_of_staff().unwrap().expect("Chief of Staff");
    let _tess_turn = w
        .node
        .sessions
        .bindings()
        .issue(
            tess.id.as_str(),
            "run-tess-live",
            "sess-tess-live",
            std::process::id() as libc::pid_t,
            None,
        )
        .unwrap();
    let mut owner_transport = GrantTransport::new(&w, &w.owner);
    let err = bind_workspace(
        &mut owner_transport,
        &WorkspaceBindRequest {
            member: tess.id.as_str(),
            workspace_dir: &ws,
            label: None,
            ttl_secs: None,
            takeover: false,
            command_id: None,
            state_root: Some(&state_root),
        },
    )
    .unwrap_err();
    assert_eq!(
        err.code,
        ErrorCode::NotAuthorized,
        "Chief of Staff with live turn must fail NotAuthorized, never Conflict: {err:?}"
    );
    assert!(!state.dir().exists());
    w.node.sessions.bindings().revoke(tess.id.as_str(), None);

    let hr = w
        .office
        .list_employees()
        .unwrap()
        .into_iter()
        .find(|e| e.builtin_key == Some(org_office::organization_family::records::BuiltinKey::Hr))
        .expect("built-in HR agent");
    let err = bind_workspace(
        &mut owner_transport,
        &WorkspaceBindRequest {
            member: hr.id.as_str(),
            workspace_dir: &ws,
            label: None,
            ttl_secs: None,
            takeover: true,
            command_id: None,
            state_root: Some(&state_root),
        },
    )
    .unwrap_err();
    assert_eq!(
        err.code,
        ErrorCode::NotAuthorized,
        "built-in HR agent must fail NotAuthorized: {err:?}"
    );
    assert!(!state.dir().exists());

    // 5. Targeting a Person returns `InvalidRequest`; targeting a paused or offboarded agent
    // returns `Conflict`.
    let err = bind_workspace(
        &mut owner_transport,
        &WorkspaceBindRequest {
            member: &alice_id,
            workspace_dir: &ws,
            label: None,
            ttl_secs: None,
            takeover: false,
            command_id: None,
            state_root: Some(&state_root),
        },
    )
    .unwrap_err();
    assert_eq!(
        err.code,
        ErrorCode::InvalidRequest,
        "Person target: {err:?}"
    );
    assert!(!state.dir().exists());

    let ben_version = w
        .office
        .org_snapshot()
        .unwrap()
        .members
        .into_iter()
        .find(|m| m.id == w.ben)
        .unwrap()
        .version;
    ok_value(grant_call::<org_proto::hiring::AgentSetStatus>(
        &w,
        &org_proto::hiring::AgentSetStatusParams {
            command_id: w.office.next_uuid_v7(),
            member_id: w.ben.clone(),
            expected_version: ben_version,
            paused: true,
        },
    ));
    let err = bind_workspace(
        &mut owner_transport,
        &WorkspaceBindRequest {
            member: "@ben",
            workspace_dir: &ws,
            label: None,
            ttl_secs: None,
            takeover: false,
            command_id: None,
            state_root: Some(&state_root),
        },
    )
    .unwrap_err();
    assert_eq!(
        err.code,
        ErrorCode::Conflict,
        "paused agent target: {err:?}"
    );
    assert!(!state.dir().exists());

    let ben_version2 = w
        .office
        .org_snapshot()
        .unwrap()
        .members
        .into_iter()
        .find(|m| m.id == w.ben)
        .unwrap()
        .version;
    ok_value(grant_call::<AgentOffboard>(
        &w,
        &AgentOffboardParams {
            command_id: w.office.next_uuid_v7(),
            member_id: w.ben.clone(),
            expected_version: ben_version2,
        },
    ));
    let err = bind_workspace(
        &mut owner_transport,
        &WorkspaceBindRequest {
            member: &w.ben,
            workspace_dir: &ws,
            label: None,
            ttl_secs: None,
            takeover: false,
            command_id: None,
            state_root: Some(&state_root),
        },
    )
    .unwrap_err();
    assert_eq!(
        err.code,
        ErrorCode::Conflict,
        "offboarded agent target: {err:?}"
    );
    assert!(!state.dir().exists());

    // 6. None of the refused calls polluted pending logins: owner still binds `@ada` cleanly.
    let ok = bind_workspace(
        &mut owner_transport,
        &WorkspaceBindRequest {
            member: "@ada",
            workspace_dir: &ws,
            label: None,
            ttl_secs: None,
            takeover: false,
            command_id: None,
            state_root: Some(&state_root),
        },
    )
    .unwrap();
    assert_eq!(ok.session.employee_id, w.ada);
}

#[test]
fn one_step_bind_succeeds_for_non_owner_operators_warm_idle_sessions_and_full_pending_logins() {
    use org_agent_cli::external_auth::bind::{bind_workspace, WorkspaceBindRequest};
    use org_proto::org::{MemberAddPersonParams, TeamSetLeadParams};
    use org_proto::sessions::{MemberSessionStartParams, SessionState};

    let w = world();
    let snap = w.office.org_snapshot().unwrap();
    let team = snap.teams[0].clone();
    let tmp = tempfile::tempdir().unwrap();
    let ws_hirer = tmp.path().join("ws-hirer");
    let ws_lead = tmp.path().join("ws-lead");
    let ws_warm = tmp.path().join("ws-warm");
    let ws_dos = tmp.path().join("ws-dos");
    for d in [&ws_hirer, &ws_lead, &ws_warm, &ws_dos] {
        std::fs::create_dir(d).unwrap();
    }
    let state_root = tmp.path().join("state");

    // 1. Authorized non-owner operator with `may_hire = true` succeeds on `@ada`.
    let hirer_id = w.office.next_uuid_v7();
    w.office
        .submit_org(
            &w.owner,
            org_office::rules::OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: w.office.next_uuid_v7(),
                member_id: hirer_id.clone(),
                name: "Hirer".into(),
                handle: Some("hirer".into()),
                home_team_id: Some(team.id.clone()),
                contact: None,
            }),
        )
        .unwrap()
        .unwrap();
    let mut hirer_transport = GrantTransport::new(&w, &hirer_id);
    let bound_hirer = bind_workspace(
        &mut hirer_transport,
        &WorkspaceBindRequest {
            member: "@ada",
            workspace_dir: &ws_hirer,
            label: Some("hirer-ws"),
            ttl_secs: None,
            takeover: false,
            command_id: None,
            state_root: Some(&state_root),
        },
    )
    .unwrap();
    assert_eq!(bound_hirer.session.approved_by, hirer_id);
    let hirer_state = WorkspaceState::for_root(&state_root, &ws_hirer.canonicalize().unwrap());
    let (code, who) = agent_os(&w, binding_from_workspace_state(&hirer_state), &["who"]);
    assert_eq!(code, 0, "hirer-bound workspace must authenticate: {who}");

    // 2. Authorized non-owner team lead (with `may_hire = false`!) succeeds on their team's agent.
    let lead_id = w.office.next_uuid_v7();
    w.office
        .submit_org(
            &w.owner,
            org_office::rules::OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: w.office.next_uuid_v7(),
                member_id: lead_id.clone(),
                name: "Lead".into(),
                handle: Some("lead".into()),
                home_team_id: Some(team.id.clone()),
                contact: None,
            }),
        )
        .unwrap()
        .unwrap();
    let lead_version = w
        .office
        .org_snapshot()
        .unwrap()
        .members
        .into_iter()
        .find(|m| m.id == lead_id)
        .unwrap()
        .version;
    ok_value(grant_call::<org_proto::hiring::MemberSetMayHire>(
        &w,
        &org_proto::hiring::MemberSetMayHireParams {
            command_id: w.office.next_uuid_v7(),
            member_id: lead_id.clone(),
            expected_version: lead_version,
            may_hire: false,
        },
    ));
    w.office
        .submit_org(
            &w.owner,
            org_office::rules::OrgCommand::TeamSetLead(TeamSetLeadParams {
                command_id: w.office.next_uuid_v7(),
                team_id: team.id.clone(),
                expected_version: team.version,
                lead_member_id: Some(lead_id.clone()),
            }),
        )
        .unwrap()
        .unwrap();
    let mut lead_transport = GrantTransport::new(&w, &lead_id);
    let bound_lead = bind_workspace(
        &mut lead_transport,
        &WorkspaceBindRequest {
            member: "@ben",
            workspace_dir: &ws_lead,
            label: Some("lead-ws"),
            ttl_secs: None,
            takeover: false,
            command_id: None,
            state_root: Some(&state_root),
        },
    )
    .unwrap();
    assert_eq!(bound_lead.session.approved_by, lead_id);
    let lead_state = WorkspaceState::for_root(&state_root, &ws_lead.canonicalize().unwrap());
    let (code, who_ben) = agent_os(&w, binding_from_workspace_state(&lead_state), &["who"]);
    assert_eq!(
        code, 0,
        "team-lead-bound workspace must authenticate: {who_ben}"
    );

    // 3. Filling `PendingLogins` (3 pending logins for `@ada`) must NEVER DoS `member.external_bind`.
    for _ in 0..3 {
        let _ = login(&w, "@ada");
    }
    let (_, extra_pubkey) = client::generate_key().unwrap();
    let full = client::call::<AuthLoginRequest>(
        &target(&w).connect().unwrap(),
        &AuthLoginRequestParams {
            member: "@ada".into(),
            pubkey_ed25519_b64: extra_pubkey,
            workspace_label: "fourth-pending".into(),
            hostname: None,
            workspace_path: None,
        },
    )
    .unwrap_err();
    assert_eq!(
        full.code,
        ErrorCode::Unavailable,
        "4th pending login must hit MAX_PENDING_PER_EMPLOYEE"
    );
    let mut owner_transport = GrantTransport::new(&w, &w.owner);
    let bound_dos = bind_workspace(
        &mut owner_transport,
        &WorkspaceBindRequest {
            member: "@ada",
            workspace_dir: &ws_dos,
            label: Some("post-dos"),
            ttl_secs: None,
            takeover: false,
            command_id: None,
            state_root: Some(&state_root),
        },
    )
    .unwrap();
    assert_eq!(bound_dos.session.employee_id, w.ada);
    let dos_state = WorkspaceState::for_root(&state_root, &ws_dos.canonicalize().unwrap());
    let (code, who_dos) = agent_os(&w, binding_from_workspace_state(&dos_state), &["who"]);
    assert_eq!(
        code, 0,
        "bind must succeed even when PendingLogins is full: {who_dos}"
    );

    // 4. Warm `SessionState::Idle` managed pump binds cleanly WITHOUT `--takeover` (parity with
    // `external_approve`), parks the warm pump, and authenticates immediately.
    ok_value(grant_call::<MemberExternalRevoke>(
        &w,
        &MemberExternalRevokeParams {
            command_id: None,
            employee_id: "@ada".into(),
            session_id: None,
        },
    ));
    w.node
        .sessions
        .start_session(
            &w.owner,
            &MemberSessionStartParams {
                command_id: w.office.next_uuid_v7(),
                employee_id: w.ada.clone(),
                instruction: Some("warm up".into()),
                workspace: None,
            },
        )
        .unwrap();
    wait_until(
        Duration::from_secs(5),
        "managed session finishes turn and stays warm idle",
        || {
            w.node.recorded.count_for("ada") >= 1
                && w.node
                    .sessions
                    .inspect_session(&w.owner, Some(&w.ada))
                    .ok()
                    .is_some_and(|insp| insp.session.state == SessionState::Idle)
        },
    );
    let bound_warm = bind_workspace(
        &mut owner_transport,
        &WorkspaceBindRequest {
            member: "@ada",
            workspace_dir: &ws_warm,
            label: Some("warm-idle-bind"),
            ttl_secs: None,
            takeover: false,
            command_id: None,
            state_root: Some(&state_root),
        },
    )
    .unwrap();
    assert!(
        !bound_warm.session.took_over_managed,
        "warm idle session is not a live turn takeover"
    );
    assert!(!w.node.sessions.bindings().has_active(&w.ada));
    let warm_state = WorkspaceState::for_root(&state_root, &ws_warm.canonicalize().unwrap());
    let (code, who_warm) = agent_os(&w, binding_from_workspace_state(&warm_state), &["who"]);
    assert_eq!(
        code, 0,
        "workspace bound over warm idle session must authenticate immediately: {who_warm}"
    );
}

#[test]
fn external_renew_and_pending_enforce_human_only_auth_masking_and_familiarity() {
    let w = world();
    let tess = w
        .office
        .chief_of_staff()
        .unwrap()
        .unwrap()
        .id
        .as_str()
        .to_owned();

    // 1. First-time login request from Ada's workstation.
    let (pkcs8_1, pubkey_1) = client::generate_key().unwrap();
    let req1 = client::call::<AuthLoginRequest>(
        &target(&w).connect().unwrap(),
        &AuthLoginRequestParams {
            member: "@ada".into(),
            pubkey_ed25519_b64: pubkey_1.clone(),
            workspace_label: "~/repo".into(),
            hostname: Some("dev-mac".into()),
            workspace_path: Some("/Users/ada/repo".into()),
        },
    )
    .unwrap();

    // 2. Owner sees masked code_prefix (`XXXX-••••`, never secret half) and `first_time`.
    let pending: MemberExternalPendingResult =
        serde_json::from_value(ok_value(grant_call::<MemberExternalPending>(
            &w,
            &MemberExternalPendingParams { employee_id: None },
        )))
        .unwrap();
    assert_eq!(pending.pending.len(), 1);
    let p1 = &pending.pending[0];
    assert_eq!(p1.request_id, req1.request_id);
    assert_eq!(p1.handle.as_deref(), Some("@ada"));
    assert_eq!(p1.employee_id, w.ada);
    assert_eq!(p1.code_prefix, format!("{}-••••", &req1.approval_code[..4]));
    assert!(!p1.code_prefix.contains(&req1.approval_code[5..]));
    assert_eq!(p1.hostname.as_deref(), Some("dev-mac"));
    assert_eq!(p1.workspace_path.as_deref(), Some("/Users/ada/repo"));
    assert_eq!(p1.workspace_familiarity, WorkspaceFamiliarity::FirstTime);
    assert!(p1.last_approved_by.is_none() && p1.last_approved_at.is_none());

    // 3. Approve with 1h TTL, exchange fast token, then verify STRICT human-only gate (ADR-0036):
    // managed agent, external BYOA agent, and Chief of Staff (Tess) MUST ALL fail closed!
    let view1: ExternalSessionView =
        serde_json::from_value(ok_value(grant_call::<MemberExternalApprove>(
            &w,
            &MemberExternalApproveParams {
                command_id: None,
                code: req1.approval_code.clone(),
                ttl_secs: Some(3_600),
                takeover: false,
            },
        )))
        .unwrap();
    let ext_token = client::exchange(
        &target(&w).connect().unwrap(),
        &signer(&w, &pkcs8_1, &w.ada, &view1.session_id),
    )
    .unwrap();

    let renew_req = Request::new::<MemberExternalRenew>(
        10,
        &MemberExternalRenewParams {
            employee_id: "@ada".into(),
            ttl_secs: Some(MAX_LEASE_TTL_SECS),
            session_id: None,
            command_id: None,
        },
    )
    .unwrap();
    let pending_req = Request::new::<MemberExternalPending>(
        11,
        &MemberExternalPendingParams { employee_id: None },
    )
    .unwrap();

    // Non-grant agent callers (managed agent, Tess) refused fail-closed.
    for agent_id in [&w.ada, &w.ben, &tess] {
        assert_eq!(
            error_code(&w.node.shared.answer_for_test(&renew_req, agent_id, false)),
            ErrorCode::NotAuthorized,
            "agent {agent_id} must be refused member.external_renew"
        );
        assert_eq!(
            error_code(&w.node.shared.answer_for_test(&pending_req, agent_id, false)),
            ErrorCode::NotAuthorized,
            "agent {agent_id} must be refused member.external_pending"
        );
    }

    // External BYOA agent over real socket refused fail-closed.
    let ext_conn = target(&w).connect().unwrap();
    client::bind(&ext_conn, &w.ada, &ext_token.token).unwrap();
    assert_eq!(
        client::call::<MemberExternalRenew>(
            &ext_conn,
            &MemberExternalRenewParams {
                employee_id: "@ada".into(),
                ttl_secs: Some(MAX_LEASE_TTL_SECS),
                session_id: None,
                command_id: None,
            },
        )
        .unwrap_err()
        .code,
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        client::call::<MemberExternalPending>(
            &ext_conn,
            &MemberExternalPendingParams { employee_id: None },
        )
        .unwrap_err()
        .code,
        ErrorCode::NotAuthorized
    );

    // 4. Owner renews active lease via grant -> expires_at extended + `renewed` audit entry.
    let renewed: ExternalSessionView =
        serde_json::from_value(ok_value(grant_call::<MemberExternalRenew>(
            &w,
            &MemberExternalRenewParams {
                employee_id: "@ada".into(),
                ttl_secs: Some(MAX_LEASE_TTL_SECS),
                session_id: Some(view1.session_id.clone()),
                command_id: None,
            },
        )))
        .unwrap();
    assert_eq!(renewed.session_id, view1.session_id);
    assert!(renewed.expires_at > view1.expires_at);
    let sessions = ok_value(grant_call::<MemberExternalSessions>(
        &w,
        &MemberExternalSessionsParams {
            employee_id: "@ada".into(),
            limit: Some(20),
        },
    ));
    let events: Vec<&str> = sessions["history"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["event"].as_str())
        .collect();
    assert!(events.contains(&"renewed"), "{events:?}");

    // Revoking causes subsequent renew to fail closed.
    ok_value(grant_call::<MemberExternalRevoke>(
        &w,
        &MemberExternalRevokeParams {
            command_id: None,
            employee_id: "@ada".into(),
            session_id: None,
        },
    ));
    assert_eq!(
        error_code(&grant_call::<MemberExternalRenew>(
            &w,
            &MemberExternalRenewParams {
                employee_id: "@ada".into(),
                ttl_secs: Some(3_600),
                session_id: None,
                command_id: None,
            },
        )),
        ErrorCode::Conflict
    );

    // 5. Re-login with SAME key (`returning_known_key`) and NEW key (`returning_new_key`).
    let req_known = client::call::<AuthLoginRequest>(
        &target(&w).connect().unwrap(),
        &AuthLoginRequestParams {
            member: "@ada".into(),
            pubkey_ed25519_b64: pubkey_1,
            workspace_label: "~/repo".into(),
            hostname: Some("dev-mac".into()),
            workspace_path: Some("/Users/ada/repo".into()),
        },
    )
    .unwrap();
    let (_, pubkey_2) = client::generate_key().unwrap();
    let req_new = client::call::<AuthLoginRequest>(
        &target(&w).connect().unwrap(),
        &AuthLoginRequestParams {
            member: "@ada".into(),
            pubkey_ed25519_b64: pubkey_2,
            workspace_label: "~/repo".into(),
            hostname: Some("dev-mac".into()),
            workspace_path: Some("/Users/ada/repo".into()),
        },
    )
    .unwrap();
    let filtered: MemberExternalPendingResult =
        serde_json::from_value(ok_value(grant_call::<MemberExternalPending>(
            &w,
            &MemberExternalPendingParams {
                employee_id: Some("@ada".into()),
            },
        )))
        .unwrap();
    assert_eq!(filtered.pending.len(), 2);
    let known = filtered
        .pending
        .iter()
        .find(|p| p.request_id == req_known.request_id)
        .unwrap();
    assert_eq!(
        known.workspace_familiarity,
        WorkspaceFamiliarity::ReturningKnownKey
    );
    assert_eq!(known.last_approved_by.as_deref(), Some(w.owner.as_str()));
    assert!(known.last_approved_at.is_some());

    let rotated = filtered
        .pending
        .iter()
        .find(|p| p.request_id == req_new.request_id)
        .unwrap();
    assert_eq!(
        rotated.workspace_familiarity,
        WorkspaceFamiliarity::ReturningNewKey
    );
    assert_eq!(rotated.last_approved_by.as_deref(), Some(w.owner.as_str()));
    assert!(rotated.last_approved_at.is_some());
}
