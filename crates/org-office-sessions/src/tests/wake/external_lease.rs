//! External (BYOA) leases on a real `SessionManager` (`TASK-137`): approval, Ed25519 exchange,
//! supersede, the single-active-lease invariant against managed turns and wakes, revocation,
//! offboarding, restart and the RAM-only pending codes.

use std::os::unix::fs::PermissionsExt;

use crate::ExternalLeaseRecord;
use base64::Engine as _;
use org_proto::external_auth::{
    challenge_message, AuthLoginRequestParams, ChallengePurpose, ExternalLoginState,
    MemberExternalApproveParams, SessionMode, SignedChallengeParams,
};
use org_proto::ErrorCode;
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair};

use super::*;

struct Workspace {
    pair: Ed25519KeyPair,
}

impl Workspace {
    fn new() -> Self {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        Self {
            pair: Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap(),
        }
    }

    fn pubkey_b64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(self.pair.public_key().as_ref())
    }

    fn sign(
        &self,
        purpose: ChallengePurpose,
        office: &str,
        member: &str,
        session: &str,
        nonce: &str,
    ) -> String {
        let msg = challenge_message(purpose, office, member, session, nonce);
        base64::engine::general_purpose::STANDARD.encode(self.pair.sign(&msg).as_ref())
    }
}

fn login(mgr: &SessionManager, agent: &str, ws: &Workspace) -> (String, String) {
    let res = mgr
        .external_login_request(
            agent,
            &AuthLoginRequestParams {
                member: agent.into(),
                pubkey_ed25519_b64: ws.pubkey_b64(),
                workspace_label: "laptop:~/src/app".into(),
            },
        )
        .unwrap();
    assert!(res.approve_command.ends_with(&res.approval_code));
    assert_eq!(res.web_path, format!("/members/{agent}"));
    (res.request_id, res.approval_code)
}

fn approve(
    mgr: &SessionManager,
    actor: &str,
    code: &str,
    takeover: bool,
) -> Result<org_proto::external_auth::ExternalSessionView, org_proto::ApiError> {
    mgr.external_approve(
        actor,
        &MemberExternalApproveParams {
            command_id: None,
            code: code.into(),
            ttl_secs: None,
            takeover,
        },
    )
}

fn signed(
    mgr: &SessionManager,
    ws: &Workspace,
    agent: &str,
    session: &str,
    purpose: ChallengePurpose,
) -> SignedChallengeParams {
    let ch = mgr.external_challenge(agent, session, purpose).unwrap();
    SignedChallengeParams {
        member: agent.into(),
        session_id: session.into(),
        signature_ed25519_b64: ws.sign(purpose, &ch.office_id, agent, session, &ch.nonce),
        nonce: ch.nonce,
    }
}

fn exchange(mgr: &SessionManager, ws: &Workspace, agent: &str, session: &str) -> String {
    let params = signed(mgr, ws, agent, session, ChallengePurpose::Exchange);
    mgr.external_exchange(agent, &params)
        .unwrap()
        .token
        .to_string()
}

fn wake_item(sender: &str, id: &str) -> PendingWakeItem {
    PendingWakeItem {
        item_id: id.into(),
        kind: WakeKind::DirectMessage,
        sender_id: sender.into(),
        sender_name: "Owner".into(),
        sender_handle: None,
        sender_relation: None,
        sender_is_person: true,
        conversation_id: Some("conv-ext".into()),
        source_id: id.into(),
        summary: "please look".into(),
        enqueued_at_ms: now_ms(),
    }
}

#[test]
fn full_external_lifecycle_login_approve_exchange_supersede_revoke_and_restart() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));
    let ws = Workspace::new();

    // Pending codes are RAM-only: no synthetic folder appears under `sessions/`.
    let (request_id, code) = login(&mgr, &fx.agent_a, &ws);
    assert!(mgr.store().list_employee_ids().unwrap().is_empty());
    assert!(!mgr.store().employee_dir(&fx.agent_a).unwrap().exists());
    assert_eq!(
        mgr.external_login_poll(&request_id).unwrap().state,
        ExternalLoginState::Pending
    );

    // An agent can never approve (its own or anyone's) sign-in, and that costs no attempt.
    let refused = approve(&mgr, &fx.agent_b, &code, false).unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotAuthorized);

    let view = approve(&mgr, &fx.owner_id, &code, false).unwrap();
    assert!(view.active && view.session_id.starts_with("ext-"));
    assert_eq!(view.pubkey_fingerprint.len(), 16);
    let polled = mgr.external_login_poll(&request_id).unwrap();
    assert_eq!(polled.state, ExternalLoginState::Approved);
    assert_eq!(polled.session_id.as_deref(), Some(view.session_id.as_str()));
    assert!(mgr.has_active_external_lease(&fx.agent_a));

    // Durable lease and history: owner-only files in the existing session folder.
    let lease_path = mgr.store().external_lease_path(&fx.agent_a).unwrap();
    let mode = std::fs::metadata(&lease_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let dir_mode = std::fs::metadata(lease_path.parent().unwrap())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(dir_mode, 0o700);
    let raw = std::fs::read_to_string(&lease_path).unwrap();
    assert!(raw.contains(&ws.pubkey_b64()) && !raw.contains("token"));

    // 1-RTT Ed25519 exchange mints a RAM-only fast token; the hot path verifies it.
    let token = exchange(&mgr, &ws, &fx.agent_a, &view.session_id);
    assert_eq!(
        mgr.verify_external_token(&fx.agent_a, Some(&token))
            .as_deref(),
        Some(view.session_id.as_str())
    );
    let who = mgr.external_whoami(&fx.agent_a, Some(&view.session_id));
    assert_eq!(who.mode, SessionMode::External);
    assert_eq!(who.handle.as_deref(), Some("ada"));
    assert!(who.expires_at.is_some() && who.lease_expires_at.is_some());
    let history =
        std::fs::read_to_string(mgr.store().external_history_path(&fx.agent_a).unwrap()).unwrap();
    assert!(history.contains("\"approved\"") && history.contains("\"token_issued\""));
    assert!(
        !history.contains(&token),
        "a token never reaches the history"
    );

    // Office restart: the lease survives on disk, the RAM token does not; a fresh exchange
    // refreshes it without another approval.
    let mgr2 = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));
    assert!(mgr2
        .verify_external_token(&fx.agent_a, Some(&token))
        .is_none());
    assert!(mgr2.has_active_external_lease(&fx.agent_a));
    let token2 = exchange(&mgr2, &ws, &fx.agent_a, &view.session_id);
    assert!(mgr2
        .verify_external_token(&fx.agent_a, Some(&token2))
        .is_some());

    // A second approval supersedes the first lease: its token and its key stop working.
    let ws_b = Workspace::new();
    let (_, code_b) = login(&mgr2, &fx.agent_a, &ws_b);
    let view_b = approve(&mgr2, &fx.owner_id, &code_b, false).unwrap();
    assert_eq!(
        view_b.superseded_session_id.as_deref(),
        Some(view.session_id.as_str())
    );
    assert!(mgr2
        .verify_external_token(&fx.agent_a, Some(&token2))
        .is_none());
    assert!(mgr2
        .external_challenge(&fx.agent_a, &view.session_id, ChallengePurpose::Exchange)
        .is_err());
    let token_b = exchange(&mgr2, &ws_b, &fx.agent_a, &view_b.session_id);

    // The operator view lists the active lease and the history, newest last.
    let sessions = mgr2
        .external_sessions(&fx.owner_id, &fx.agent_a, Some(50))
        .unwrap();
    assert_eq!(
        sessions.active.as_ref().map(|a| a.session_id.as_str()),
        Some(view_b.session_id.as_str())
    );
    assert!(sessions.history.iter().any(|h| h.event == "superseded"));
    assert!(mgr2
        .external_sessions(&fx.agent_b, &fx.agent_a, None)
        .is_err());

    // Revoking a different session id is `not_found`; revoking the current one ends it.
    let wrong = mgr2
        .external_revoke(&fx.owner_id, &fx.agent_a, Some(&view.session_id))
        .unwrap_err();
    assert_eq!(wrong.code, ErrorCode::NotFound);
    let revoked = mgr2
        .external_revoke(&fx.owner_id, &fx.agent_a, None)
        .unwrap();
    assert!(revoked.revoked);
    assert!(mgr2
        .verify_external_token(&fx.agent_a, Some(&token_b))
        .is_none());
    assert!(!mgr2.has_active_external_lease(&fx.agent_a));
    let again = mgr2
        .external_revoke(&fx.owner_id, &fx.agent_a, None)
        .unwrap();
    assert!(!again.revoked);
}

#[test]
fn challenge_answers_are_single_use_purpose_bound_and_key_bound() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));
    let ws = Workspace::new();
    let (_, code) = login(&mgr, &fx.agent_a, &ws);
    let session = approve(&mgr, &fx.owner_id, &code, false)
        .unwrap()
        .session_id;

    let params = signed(&mgr, &ws, &fx.agent_a, &session, ChallengePurpose::Exchange);
    assert!(mgr.external_exchange(&fx.agent_a, &params).is_ok());
    let replay = mgr.external_exchange(&fx.agent_a, &params).unwrap_err();
    assert_eq!(replay.code, ErrorCode::NotAuthorized, "replayed nonce");

    // A nonce issued for an exchange cannot log out (and is spent by the attempt).
    let exchange_nonce = signed(&mgr, &ws, &fx.agent_a, &session, ChallengePurpose::Exchange);
    assert!(mgr.external_logout(&fx.agent_a, &exchange_nonce).is_err());
    assert!(mgr.external_exchange(&fx.agent_a, &exchange_nonce).is_err());

    // Another key's signature, or another agent's name, is refused.
    let intruder = Workspace::new();
    let forged = signed(
        &mgr,
        &intruder,
        &fx.agent_a,
        &session,
        ChallengePurpose::Exchange,
    );
    assert!(mgr.external_exchange(&fx.agent_a, &forged).is_err());
    let theirs = signed(&mgr, &ws, &fx.agent_a, &session, ChallengePurpose::Exchange);
    assert!(mgr.external_exchange(&fx.agent_b, &theirs).is_err());
    assert!(mgr
        .external_challenge(&fx.agent_b, &session, ChallengePurpose::Exchange)
        .is_err());

    // A signed logout ends the lease and every token with it.
    let token = exchange(&mgr, &ws, &fx.agent_a, &session);
    let logout = signed(&mgr, &ws, &fx.agent_a, &session, ChallengePurpose::Logout);
    assert!(mgr.external_logout(&fx.agent_a, &logout).unwrap().revoked);
    assert!(mgr
        .verify_external_token(&fx.agent_a, Some(&token))
        .is_none());
    assert!(mgr
        .external_challenge(&fx.agent_a, &session, ChallengePurpose::Exchange)
        .is_err());
}

#[test]
fn an_active_lease_holds_wakes_and_refuses_managed_starts_until_revoked() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));
    let ws = Workspace::new();
    let (_, code) = login(&mgr, &fx.agent_a, &ws);
    approve(&mgr, &fx.owner_id, &code, false).unwrap();

    let outcome = mgr.enqueue_wake(&fx.agent_a, wake_item(&fx.owner_id, "dm:ext-1"));
    assert_eq!(
        outcome,
        WakeOutcome::Queued {
            reason: WakeHoldReason::Busy
        }
    );
    let queue = mgr
        .store()
        .read_pending_wakes(&fx.agent_a)
        .unwrap()
        .unwrap();
    assert_eq!(queue.items.len(), 1, "the wake stays durable");
    assert_eq!(
        mgr.waiting_messages_counter(&fx.agent_a)
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    let start = mgr
        .start_session(
            &fx.owner_id,
            &MemberSessionStartParams {
                command_id: next_id(),
                employee_id: fx.agent_a.clone(),
                instruction: Some("run".into()),
                workspace: None,
            },
        )
        .unwrap_err();
    assert_eq!(start.code, ErrorCode::Conflict);
    assert!(
        fx.recorded.turns.lock().unwrap().is_empty(),
        "no managed child"
    );
    let presence = mgr.member_runtime_presence(&fx.agent_a).unwrap();
    assert_eq!(presence.presence, MemberPresence::Busy);
    assert!(presence.status.unwrap().starts_with("External session"));

    // Revoking hands the agent back: the held wake runs as a managed turn.
    assert!(
        mgr.external_revoke(&fx.owner_id, &fx.agent_a, None)
            .unwrap()
            .revoked
    );
    assert!(
        wait_until(
            || !fx.recorded.turns.lock().unwrap().is_empty(),
            Duration::from_secs(10)
        ),
        "the held wake did not run after the lease ended"
    );
    mgr.stop_all("test end");
}

#[test]
fn a_live_managed_turn_refuses_approval_without_takeover_and_is_parked_with_it() {
    let fx = WakeFixture::new(GRACE_HOLD_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));
    mgr.start_session(
        &fx.owner_id,
        &MemberSessionStartParams {
            command_id: next_id(),
            employee_id: fx.agent_a.clone(),
            instruction: Some("long task".into()),
            workspace: None,
        },
    )
    .unwrap();
    assert!(
        wait_until(
            || mgr.has_active_managed_turn(&fx.agent_a),
            Duration::from_secs(5)
        ),
        "managed turn did not start"
    );
    let ws = Workspace::new();
    let (request_id, code) = login(&mgr, &fx.agent_a, &ws);
    let refused = approve(&mgr, &fx.owner_id, &code, false).unwrap_err();
    assert_eq!(refused.code, ErrorCode::Conflict);
    assert!(
        refused.message.contains("--takeover"),
        "{}",
        refused.message
    );
    assert!(!mgr.has_active_external_lease(&fx.agent_a));
    assert!(
        mgr.has_active_managed_turn(&fx.agent_a),
        "refusal left it running"
    );

    let view = approve(&mgr, &fx.owner_id, &code, true).unwrap();
    assert!(view.took_over_managed);
    assert!(!mgr.has_active_managed_turn(&fx.agent_a));
    assert!(!mgr.bindings().has_active(&fx.agent_a));
    assert!(mgr.has_active_external_lease(&fx.agent_a));
    assert_eq!(
        mgr.external_login_poll(&request_id).unwrap().state,
        ExternalLoginState::Approved
    );
    mgr.stop_all("test end");
}

#[test]
fn offboarding_and_pausing_end_the_external_lease() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));
    let ws = Workspace::new();
    let (_, code) = login(&mgr, &fx.agent_a, &ws);
    let session = approve(&mgr, &fx.owner_id, &code, false)
        .unwrap()
        .session_id;
    let token = exchange(&mgr, &ws, &fx.agent_a, &session);

    mgr.on_agent_offboarded(&fx.agent_a);
    assert!(mgr
        .verify_external_token(&fx.agent_a, Some(&token))
        .is_none());
    assert!(!mgr.has_active_external_lease(&fx.agent_a));
    let lease = mgr
        .store()
        .read_external_lease(&fx.agent_a)
        .unwrap()
        .unwrap();
    assert!(lease.revoked_at.is_some(), "revocation is durable");
    // The pause path (`on_agent_stop`) ends it the same way.
    let (_, code_b) = login(&mgr, &fx.agent_b, &ws);
    approve(&mgr, &fx.owner_id, &code_b, false).unwrap();
    mgr.revoke_external_lease(&fx.agent_b, "agent paused");
    assert!(!mgr.has_active_external_lease(&fx.agent_b));
    let events: Vec<String> = mgr
        .external_sessions(&fx.owner_id, &fx.agent_b, None)
        .unwrap()
        .history
        .into_iter()
        .map(|e| e.event)
        .collect();
    assert_eq!(
        events.last().map(String::as_str),
        Some("paused"),
        "{events:?}"
    );
}

#[test]
fn login_requests_validate_key_label_and_agent() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));
    let ws = Workspace::new();
    let base = AuthLoginRequestParams {
        member: fx.agent_a.clone(),
        pubkey_ed25519_b64: ws.pubkey_b64(),
        workspace_label: "laptop".into(),
    };
    let mut bad_key = base.clone();
    bad_key.pubkey_ed25519_b64 = "AAAA".into();
    let mut bad_label = base.clone();
    bad_label.workspace_label = "x\u{7}y".into();
    let mut long_label = base.clone();
    long_label.workspace_label = "é".repeat(100);
    for params in [bad_key, bad_label, long_label] {
        assert_eq!(
            mgr.external_login_request(&fx.agent_a, &params)
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    assert!(
        mgr.external_login_request(&fx.owner_id, &base).is_err(),
        "a person cannot sign in as an external agent"
    );
    let ttl = mgr
        .external_approve(
            &fx.owner_id,
            &MemberExternalApproveParams {
                command_id: None,
                code: login(&mgr, &fx.agent_a, &ws).1,
                ttl_secs: Some(1),
                takeover: false,
            },
        )
        .unwrap_err();
    assert_eq!(ttl.code, ErrorCode::InvalidRequest);
}

/// Adds a person with no authority over the fixture's agents: in a team of their own, not
/// its lead, unable to hire, nobody's manager.
fn bystander(fx: &WakeFixture) -> String {
    let root = fx.office.org_snapshot().unwrap().teams[0].id.clone();
    let team = next_id();
    fx.office
        .submit_org(
            &fx.owner_id,
            org_office::rules::OrgCommand::TeamCreate(org_proto::org::TeamCreateParams {
                command_id: next_id(),
                team_id: team.clone(),
                name: "Elsewhere".into(),
                owner_team_id: Some(root),
            }),
        )
        .unwrap()
        .unwrap();
    let person = next_id();
    fx.office
        .submit_org(
            &fx.owner_id,
            org_office::rules::OrgCommand::MemberAddPerson(org_proto::org::MemberAddPersonParams {
                command_id: next_id(),
                member_id: person.clone(),
                name: "Bystander".into(),
                handle: Some("bystander".into()),
                home_team_id: Some(team),
                contact: None,
            }),
        )
        .unwrap()
        .unwrap();
    // A new person may hire by default: take that away.
    let version = fx
        .office
        .org_snapshot()
        .unwrap()
        .members
        .into_iter()
        .find(|m| m.id == person)
        .unwrap()
        .version;
    let request = Request::new::<org_proto::hiring::MemberSetMayHire>(
        1,
        &org_proto::hiring::MemberSetMayHireParams {
            command_id: next_id(),
            member_id: person.clone(),
            expected_version: version,
            may_hire: false,
        },
    )
    .unwrap();
    let answer = fx
        .office
        .dispatch_hiring_rpc(Some(&fx.owner_id), &request)
        .expect("a hiring call");
    assert!(
        matches!(answer.outcome, org_proto::Outcome::Result(_)),
        "{answer:?}"
    );
    person
}

/// B5b: a signed-in person without authority over an agent can neither spend nor lock its
/// pending approval code, whatever they try: the owner still approves with it afterwards.
#[test]
fn an_unprivileged_person_cannot_spend_or_lock_another_teams_approval_code() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));
    let ws = Workspace::new();
    let person = bystander(&fx);
    let (request_id, code) = login(&mgr, &fx.agent_a, &ws);
    let (lookup, secret) = code.split_once('-').unwrap();
    let wrong = |n: usize| {
        let mut chars: Vec<char> = secret.chars().collect();
        chars[n % 4] = if chars[n % 4] == 'Z' { 'Y' } else { 'Z' };
        format!("{lookup}-{}", chars.into_iter().collect::<String>())
    };
    for n in 0..8 {
        let refused = approve(&mgr, &person, &wrong(n), false).unwrap_err();
        assert_eq!(refused.code, ErrorCode::NotAuthorized, "{refused:?}");
        assert!(
            !refused.message.contains("attempt") && !refused.message.contains("locked"),
            "no attempt was spent: {}",
            refused.message
        );
    }
    let refused = approve(&mgr, &person, &code, false).unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotAuthorized);
    assert_eq!(
        mgr.external_login_poll(&request_id).unwrap().state,
        ExternalLoginState::Pending
    );
    // The owner's first wrong attempt is the first one counted (two left), then the right code.
    let counted = approve(&mgr, &fx.owner_id, &wrong(0), false).unwrap_err();
    assert!(counted.message.contains("2 attempt"), "{}", counted.message);
    let view = approve(&mgr, &fx.owner_id, &code, false).unwrap();
    assert!(view.active);
}

/// B4a: an exchange that passed its signature check while a newer lease was being approved
/// waits for the approval and is refused: it never replaces the newer lease's token.
#[test]
fn an_exchange_racing_a_newer_approval_never_replaces_its_token() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));
    let ws = Workspace::new();
    let (_, code) = login(&mgr, &fx.agent_a, &ws);
    let old = approve(&mgr, &fx.owner_id, &code, false)
        .unwrap()
        .session_id;
    let params = signed(&mgr, &ws, &fx.agent_a, &old, ChallengePurpose::Exchange);
    let lease = mgr.external_lease(&fx.agent_a).unwrap();
    let newer = ExternalLeaseRecord {
        session_id: "ext-newer".into(),
        ..lease.clone()
    };
    let (outcome, newer_token) = std::thread::scope(|scope| {
        let serial = mgr.external.lease_mutation_for_test();
        let (tx, rx) = std::sync::mpsc::channel();
        let mgr_ref = &mgr;
        let agent = fx.agent_a.clone();
        let params_ref = &params;
        scope.spawn(move || {
            let _ = tx.send(mgr_ref.external_exchange(&agent, params_ref));
        });
        assert!(
            rx.recv_timeout(Duration::from_millis(300)).is_err(),
            "the exchange must wait for the approval in progress"
        );
        // Meanwhile the approval publishes a newer lease and its workspace gets a token.
        mgr.external.publish_for_test(newer.clone());
        mgr.bindings().revoke_external(&fx.agent_a, None);
        let newer_token = mgr
            .bindings()
            .issue_external(&fx.agent_a, "ext-newer", now_ms() + 3_600_000)
            .unwrap()
            .expose_token()
            .to_owned();
        drop(serial);
        (rx.recv().unwrap(), newer_token)
    });
    assert_eq!(outcome.unwrap_err().code, ErrorCode::NotAuthorized);
    assert_eq!(
        mgr.verify_external_token(&fx.agent_a, Some(&newer_token))
            .as_deref(),
        Some("ext-newer"),
        "the newer lease keeps its token"
    );

    // The same race against a revocation: the exchange issues nothing.
    let params = {
        mgr.external.publish_for_test(lease.clone());
        signed(&mgr, &ws, &fx.agent_a, &old, ChallengePurpose::Exchange)
    };
    let outcome = std::thread::scope(|scope| {
        let serial = mgr.external.lease_mutation_for_test();
        let (tx, rx) = std::sync::mpsc::channel();
        let mgr_ref = &mgr;
        let agent = fx.agent_a.clone();
        let params_ref = &params;
        scope.spawn(move || {
            let _ = tx.send(mgr_ref.external_exchange(&agent, params_ref));
        });
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
        mgr.external.publish_for_test(ExternalLeaseRecord {
            revoked_at: Some("2026-11-26T00:00:00.000Z".into()),
            ..lease.clone()
        });
        mgr.bindings().revoke_external(&fx.agent_a, None);
        drop(serial);
        rx.recv().unwrap()
    });
    assert_eq!(outcome.unwrap_err().code, ErrorCode::NotAuthorized);
    assert!(!mgr.bindings().has_active_external(&fx.agent_a, 0));
}

/// A revocation whose file cannot be rewritten still ends the lease in RAM (fail closed) and
/// is written by the next revocation call.
#[test]
fn a_revocation_the_disk_refuses_stays_revoked_in_ram_and_is_written_later() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));
    let ws = Workspace::new();
    let (_, code) = login(&mgr, &fx.agent_a, &ws);
    let session = approve(&mgr, &fx.owner_id, &code, false)
        .unwrap()
        .session_id;
    let token = exchange(&mgr, &ws, &fx.agent_a, &session);
    // The lease file's place is taken by a non-empty folder: no rename can replace it.
    let path = mgr.store().external_lease_path(&fx.agent_a).unwrap();
    let saved = fs::read(&path).unwrap();
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    fs::write(path.join("blocker"), b"x").unwrap();

    let refused = mgr
        .external_revoke(&fx.owner_id, &fx.agent_a, None)
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::StorageError);
    assert!(!mgr.has_active_external_lease(&fx.agent_a));
    assert!(mgr
        .verify_external_token(&fx.agent_a, Some(&token))
        .is_none());
    assert!(mgr
        .external_challenge(&fx.agent_a, &session, ChallengePurpose::Exchange)
        .is_err());
    // A pause's best-effort revocation fails closed the same way.
    mgr.revoke_external_lease(&fx.agent_a, "paused");
    assert!(!mgr.has_active_external_lease(&fx.agent_a));

    // Once the disk takes writes again, the next call writes the owed revocation.
    fs::remove_dir_all(&path).unwrap();
    fs::write(&path, &saved).unwrap();
    let done = mgr
        .external_revoke(&fx.owner_id, &fx.agent_a, None)
        .unwrap();
    assert_eq!(done.session_id.as_deref(), Some(session.as_str()));
    let on_disk = mgr
        .store()
        .read_external_lease(&fx.agent_a)
        .unwrap()
        .unwrap();
    assert!(on_disk.revoked_at.is_some());
    // Nothing is owed any more.
    let again = mgr
        .external_revoke(&fx.owner_id, &fx.agent_a, None)
        .unwrap();
    assert!(!again.revoked);
}

/// B3: commands of an externally bound agent are counted in RAM only; the history file keeps
/// lifecycle events alone.
#[test]
fn external_commands_are_recorded_in_ram_and_never_appended_to_the_history() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));
    let ws = Workspace::new();
    let (_, code) = login(&mgr, &fx.agent_a, &ws);
    let session = approve(&mgr, &fx.owner_id, &code, false)
        .unwrap()
        .session_id;
    let _token = exchange(&mgr, &ws, &fx.agent_a, &session);
    let history = mgr.store().external_history_path(&fx.agent_a).unwrap();
    let before = fs::read(&history).unwrap();
    for _ in 0..50 {
        mgr.record_accepted_command(&fx.agent_a, "msg.inbox");
    }
    assert_eq!(fs::read(&history).unwrap(), before, "no per-command line");
    assert_eq!(
        mgr.bindings().external_accepted_commands(&fx.agent_a).len(),
        50
    );
    let events: Vec<String> = mgr
        .external_sessions(&fx.owner_id, &fx.agent_a, Some(200))
        .unwrap()
        .history
        .into_iter()
        .map(|e| e.event)
        .collect();
    assert_eq!(events, ["approved", "token_issued"], "{events:?}");
}
