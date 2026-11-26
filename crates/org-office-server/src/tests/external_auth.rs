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
    MemberExternalApprove, MemberExternalApproveParams, MemberExternalRevoke,
    MemberExternalRevokeParams, MemberExternalSessions, MemberExternalSessionsParams, SessionMode,
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
