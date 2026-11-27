use super::*;
use crate::serve::LaunchAuth;
use crate::test_support::sign_grant as sign;
use org_mailbox::testkit::fixture::{OWNER_ID, POSITION};
use org_office::rules::OrgCommand;
use org_proto::directory::WhoResult;
use org_proto::{Grant, Member, MemberKind};
use serde_json::{json, Value};

fn shared() -> Shared {
    let mut ports = super::super::super::tests::ports(Box::new(|| {
        Ok(
            serde_json::from_value(json!({"position":{"epoch":1,"seq":1},"teams":[],
            "members":[],"projects":[],"project_teams":[]}))
            .unwrap(),
        )
    }));
    ports.submit = Box::new(|member, _, _| {
        // The command runs as the member the caller's grant is bound to.
        assert_eq!(member, OWNER_ID);
        Ok(Ok(serde_json::from_value(
            json!({"status":"committed","position":{"epoch":1,"seq":2}}),
        )
        .unwrap()))
    });
    ports.who = Box::new(|_| Ok(Ok(WhoResult { members: vec![] })));
    ports.projects_list = Box::new(|_| {
        Ok(Ok(org_proto::projects::ProjectListResult {
            projects: vec![],
        }))
    });
    ports.project_inspect = Box::new(|_| {
        Ok(Ok(org_proto::projects::ProjectRecord {
            id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071f1".into(),
            number: 1,
            key: "P-1".into(),
            slug: "fixture".into(),
            name: "Fixture".into(),
            owner_team_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071f2".into(),
            contributing_team_ids: vec![],
            archived: false,
            revision: 1,
            created_at: None,
            updated_at: None,
            default_dod: None,
        }))
    });
    ports.operation_get = Box::new(|_, _| {
        Ok(Ok(org_proto::OperationOutcome::Committed {
            result: json!({"status":"committed"}),
        }))
    });
    let mut shared = Shared::new(
        &super::super::super::tests::test_info(),
        Path::new("unused-host.sock"),
        ports,
        LaunchAuth::from(crate::test_support::grant_key()),
    );
    shared.register_inbox(
        crate::inbox::Hq {
            sources: vec![],
            clock: crate::inbox::Clock::new("host-test".into()),
            list_office: None,
        },
        Ok(true),
    );
    shared
}
fn signed(shared: &Shared, method: &str, params: Value) -> Request {
    let office = &shared.hello.office.as_ref().unwrap().office;
    let mut grant = json!({"account":"account","member":OWNER_ID,
        "org_id":office.organization_id,"office_id":office.id,"channel_id":"opaque-channel",
        "session_id":"session","exp":u64::MAX,"mac":"00".repeat(32)});
    sign(&mut grant);
    Request {
        id: 71,
        method: method.into(),
        params,
        principal: Some(json!({"grant":grant})),
    }
}
fn answer(shared: &Shared, ctx: &HostChannel, request: &Request) -> Value {
    serde_json::to_value(dispatch(
        request,
        shared,
        ctx,
        &authority::Live {
            shared,
            revocations: &ctx._revocations,
        },
    ))
    .unwrap()
}
fn refused(response: Value, code: &str) {
    assert_eq!(response["id"], 71);
    assert_eq!(response["error"]["code"], code);
    assert_eq!(
        response["error"]["message"],
        "request refused on host channel"
    );
}

#[test]
fn verified_owner_dispatches_snapshot_who_operation_inbox_and_all_current_writes() {
    let shared = shared();
    let ctx = context();
    for (method, params) in [
        ("org.snapshot", json!({})),
        ("project.list", json!({})),
        ("project.inspect", json!({"project": "P-1"})),
        ("who", json!({})),
        (
            "operation.get",
            json!({"kind":"org","id":"0190a5d2-7c3e-7a01-8b2c-3d4e5f6071e0"}),
        ),
        ("inbox.list", json!({"state":"open"})),
    ] {
        let response = answer(&shared, &ctx, &signed(&shared, method, params));
        assert!(response.get("result").is_some(), "{method}: {response}");
    }
    let rows: Value = serde_json::from_str(include_str!("operations.json")).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 19);
    for row in rows.as_array().unwrap() {
        let method = row["method"].as_str().unwrap();
        let request = signed(&shared, method, row["params"].clone());
        assert!(OrgCommand::parse(&request).unwrap().is_some(), "{method}");
        let response = answer(&shared, &ctx, &request);
        if method == "member.set_run_mode" {
            refused(response, "not_authorized");
        } else {
            assert!(response.get("result").is_some(), "{method}: {response}");
        }
    }
}

#[test]
fn host_policy_is_closed_and_preserves_unknown_method_for_unserved_allowed_families() {
    let shared = shared();
    let ctx = context();
    for method in [
        "subscribe",
        "session.open",
        "session.list",
        "session.close",
        "channel.open",
        "channel.send",
        "bridge.open",
        "agent.resume",
        "agent.start",
        "run.start",
        "approval.decide",
        "runtime.status",
        "permission_policy.get",
        "permission_policy.set",
        "inbox.changed",
        "org.future",
        "future.method",
        "work.future",
        "task.future",
        "bug.future",
        "checklist.future",
        "project.future",
        "area.future",
        "chat.future",
        "chat.open",
        "msg.future",
        "member.future",
    ] {
        refused(
            answer(&shared, &ctx, &signed(&shared, method, json!({}))),
            "not_authorized",
        );
        let mut wrong = signed(&shared, method, json!({}));
        wrong.principal.as_mut().unwrap()["grant"]["org_id"] = "different".into();
        sign(&mut wrong.principal.as_mut().unwrap()["grant"]);
        refused(answer(&shared, &ctx, &wrong), "not_found");
    }
    for method in [
        "msg.ask",
        "msg.notify",
        "msg.threads",
        "msg.human_needs_you",
    ] {
        let response = answer(&shared, &ctx, &signed(&shared, method, json!({})));
        assert_eq!(
            response["error"]["code"], "unknown_method",
            "{method}: {response}"
        );
    }
    assert!(
        crate::serve::external::external_may_call("agent.resume"),
        "external (BYOA) agents must be allowed to call agent.resume"
    );
    let mut resume_no_principal = signed(&shared, "agent.resume", json!({}));
    resume_no_principal.principal = None;
    refused(
        answer(&shared, &ctx, &resume_no_principal),
        "not_authorized",
    );
}

/// The retired phase-1 work calls have no policy row, so a host channel refuses each one like any
/// method it does not list, before the office sees it: with the owner's verified grant, with a
/// grant for another office, and with no principal at all.
#[test]
fn retired_work_methods_have_no_policy_row_and_are_refused_on_host_channels() {
    let shared = shared();
    let ctx = context();
    for method in crate::serve::tests::retired_work::RETIRED {
        assert!(
            operations().iter().all(|row| row.method != *method),
            "{method}"
        );
        let params = json!({"change_id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","item_id":"item",
            "project_id":"project","title":"Task","expected_version":1});
        let mut request = signed(&shared, method, params);
        refused(answer(&shared, &ctx, &request), "not_authorized");
        request.principal.as_mut().unwrap()["grant"]["office_id"] = "wrong".into();
        sign(&mut request.principal.as_mut().unwrap()["grant"]);
        refused(answer(&shared, &ctx, &request), "not_found");
        request.principal = None;
        refused(answer(&shared, &ctx, &request), "not_authorized");
    }
}

struct NoChecks;
impl authority::Checks for NoChecks {
    fn verify(&self, _: &Grant) -> bool {
        panic!("must refuse before MAC verification")
    }
    fn now_ms(&self) -> Option<u64> {
        panic!("must refuse before clock")
    }
    fn revoked(&self, _: &str) -> bool {
        panic!("must refuse before revocations")
    }
    fn owner(&self, _: &str) -> Option<String> {
        panic!("must refuse before owner")
    }
}
#[test]
fn binding_trio_precedes_principal_and_authority_refusals_are_fixed_and_echo_ids() {
    let shared = shared();
    let ctx = context();
    for method in ["msg.bind", "msg.wait", "status.set"] {
        for principal in [
            None,
            Some(Value::Null),
            Some(json!({"grant":{}})),
            signed(&shared, method, json!({})).principal,
        ] {
            let request = Request {
                id: 71,
                method: method.into(),
                params: json!({}),
                principal,
            };
            refused(
                serde_json::to_value(dispatch(&request, &shared, &ctx, &NoChecks)).unwrap(),
                "invalid_request",
            );
        }
    }
    let original = signed(&shared, "org.snapshot", json!({}));
    for principal in [None, Some(Value::Null), Some(json!({"grant":{}}))] {
        let mut request = original.clone();
        request.principal = principal;
        refused(answer(&shared, &ctx, &request), "not_authorized");
    }
    for (field, value, code, resign) in [
        ("channel_id", json!("other"), "not_authorized", true),
        ("mac", json!("01".repeat(32)), "not_authorized", false),
        ("exp", json!(0), "not_authorized", true),
        ("office_id", json!("other"), "not_found", true),
        ("org_id", json!("other"), "not_found", true),
        ("member", json!("owner"), "not_authorized", true),
    ] {
        let mut request = original.clone();
        let grant = &mut request.principal.as_mut().unwrap()["grant"];
        grant[field] = value;
        if resign {
            sign(grant);
        }
        refused(answer(&shared, &ctx, &request), code);
    }
}

struct Directory {
    member: Mutex<Option<Member>>,
    failed: std::sync::atomic::AtomicBool,
    calls: std::sync::atomic::AtomicUsize,
}
impl org_mailbox::MemberDirectory for Directory {
    fn resolve_member(
        &self,
        _: &str,
    ) -> Result<Option<(Member, org_proto::Position)>, org_mailbox::StoreError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.failed.load(Ordering::SeqCst) {
            return Err(org_mailbox::StoreError::new("SECRET lookup"));
        }
        Ok(self.member.lock().unwrap().clone().map(|m| (m, POSITION)))
    }
}
#[test]
fn live_owner_and_revocations_are_rechecked_per_request_and_poison_or_legacy_refuse() {
    let mut shared = shared();
    let ctx = context();
    let canonical = org_mailbox::testkit::fixture::members()
        .into_iter()
        .find(|m| m.owner)
        .unwrap();
    let directory = Arc::new(Directory {
        member: Mutex::new(Some(canonical.clone())),
        failed: Default::default(),
        calls: Default::default(),
    });
    shared.ports.directory = directory.clone();
    let request = signed(&shared, "org.snapshot", json!({}));
    assert!(answer(&shared, &ctx, &request).get("result").is_some());
    // A non-owner live person may read org.snapshot, but is refused on owner-only structure
    // mutations, agent lifecycle mutations, and inbox.list.
    {
        let mut non_owner = canonical.clone();
        non_owner.owner = false;
        *directory.member.lock().unwrap() = Some(non_owner);
        assert!(answer(&shared, &ctx, &request).get("result").is_some());
        for method in [
            "team.create",
            "inbox.list",
            "member.add_agent",
            "member.update_agent",
            "member.set_agent_status",
            "member.offboard",
        ] {
            let req = signed(&shared, method, json!({}));
            refused(answer(&shared, &ctx, &req), "not_authorized");
        }
    }
    for (kind, owner, id) in [
        (MemberKind::Agent, true, OWNER_ID),
        (MemberKind::Unknown, true, OWNER_ID),
        (MemberKind::Person, true, "other"),
    ] {
        let mut member = canonical.clone();
        member.kind = kind;
        member.owner = owner;
        member.id = id.into();
        *directory.member.lock().unwrap() = Some(member);
        refused(answer(&shared, &ctx, &request), "not_authorized");
    }
    *directory.member.lock().unwrap() = None;
    refused(answer(&shared, &ctx, &request), "not_authorized");
    *directory.member.lock().unwrap() = Some(canonical);
    directory.failed.store(true, Ordering::SeqCst);
    refused(answer(&shared, &ctx, &request), "not_authorized");
    directory.failed.store(false, Ordering::SeqCst);
    assert!(answer(&shared, &ctx, &request).get("result").is_some());
    // Already-checked requests may finish; each next request reads tombstones before directory.
    ctx._revocations
        .lock()
        .unwrap()
        .apply_batch(vec!["other".into()])
        .unwrap();
    assert!(answer(&shared, &ctx, &request).get("result").is_some());
    ctx._revocations
        .lock()
        .unwrap()
        .apply_batch(vec!["session".into()])
        .unwrap();
    let before = directory.calls.load(Ordering::SeqCst);
    for method in ["org.snapshot", "inbox.list"] {
        refused(
            answer(
                &shared,
                &ctx,
                &signed(&shared, method, json!({"state":"open"})),
            ),
            "not_authorized",
        );
    }
    assert_eq!(directory.calls.load(Ordering::SeqCst), before);
    let ctx = context();
    let _ = std::panic::catch_unwind(|| {
        let _guard = ctx._revocations.lock().unwrap();
        panic!("poison fixture");
    });
    refused(answer(&shared, &ctx, &request), "not_authorized");
    assert_eq!(directory.calls.load(Ordering::SeqCst), before);
    shared.auth = LaunchAuth::from(org_proto::GrantKey::from_bytes([9; 32]));
    refused(answer(&shared, &context(), &request), "not_authorized");
    assert_eq!(directory.calls.load(Ordering::SeqCst), before);
}

#[test]
fn served_message_calls_act_as_the_canonical_owner() {
    use org_mailbox::testkit::fixture::PAT_ID;
    let shared = shared();
    let ctx = context();
    let sent = answer(
        &shared,
        &ctx,
        &signed(
            &shared,
            "msg.send",
            json!({
        "id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","to":PAT_ID,"kind":"msg","body":"from owner"}),
        ),
    );
    assert!(sent.get("result").is_some(), "{sent}");
    let inbox = shared
        .ports
        .mailbox
        .as_ref()
        .unwrap()
        .inbox(PAT_ID, Default::default())
        .unwrap();
    assert_eq!(inbox.headers.len(), 1);
    assert_eq!(inbox.headers[0].from_member_id, OWNER_ID);
    shared
        .ports
        .mailbox
        .as_ref()
        .unwrap()
        .send(
            PAT_ID,
            serde_json::from_value(json!({
        "id":"01ARZ3NDEKTSV4RRFFQ69G5FAW","to":OWNER_ID,"kind":"msg","body":"to owner"}))
            .unwrap(),
        )
        .unwrap();
    let inbox = answer(&shared, &ctx, &signed(&shared, "msg.inbox", json!({})));
    assert_eq!(inbox["result"]["headers"][0]["to_member_id"], OWNER_ID);
    let read = answer(
        &shared,
        &ctx,
        &signed(
            &shared,
            "msg.read",
            json!({"id":"01ARZ3NDEKTSV4RRFFQ69G5FAW"}),
        ),
    );
    assert_eq!(
        read["result"]["message"]["header"]["to_member_id"],
        OWNER_ID
    );
    assert_eq!(read["result"]["message"]["body"], "to owner");
}

#[test]
fn inbox_rechecking_owner_cannot_reveal_that_grant_authority_passed() {
    #[derive(Clone, Copy)]
    enum Next {
        Owner,
        Missing,
        NonOwner,
        Agent,
        Error,
    }
    struct ChangingDirectory {
        next: Next,
        calls: std::sync::atomic::AtomicUsize,
    }
    impl org_mailbox::MemberDirectory for ChangingDirectory {
        fn resolve_member(
            &self,
            id: &str,
        ) -> Result<Option<(Member, org_proto::Position)>, org_mailbox::StoreError> {
            assert_eq!(id, OWNER_ID);
            let mut member = org_mailbox::testkit::fixture::members()
                .into_iter()
                .find(|m| m.owner)
                .unwrap();
            if self.calls.fetch_add(1, Ordering::SeqCst) > 0 {
                match self.next {
                    Next::Owner => {}
                    Next::Missing => return Ok(None),
                    Next::NonOwner => member.owner = false,
                    Next::Agent => member.kind = MemberKind::Agent,
                    Next::Error => {
                        return Err(org_mailbox::StoreError::new("SECRET second lookup"))
                    }
                }
            }
            Ok(Some((member, POSITION)))
        }
    }
    for next in [
        Next::Owner,
        Next::Missing,
        Next::NonOwner,
        Next::Agent,
        Next::Error,
    ] {
        let mut shared = shared();
        let directory = Arc::new(ChangingDirectory {
            next,
            calls: Default::default(),
        });
        shared.ports.directory = directory.clone();
        let request = signed(&shared, "inbox.list", json!({"state":"open"}));
        let response = answer(&shared, &context(), &request);
        if matches!(next, Next::Owner) {
            assert!(
                response.get("result").is_some(),
                "stable owner must remain allowed: {response}"
            );
        } else {
            refused(response, "not_authorized");
        }
        assert_eq!(directory.calls.load(Ordering::SeqCst), 2);
    }
    let mut shared = shared();
    let invalid = answer(
        &shared,
        &context(),
        &signed(&shared, "inbox.list", json!({"state":"bogus"})),
    );
    assert_eq!(invalid["error"]["code"], "invalid_request");
    assert_ne!(
        invalid["error"]["message"],
        "request refused on host channel"
    );
    shared.inbox = None;
    let unserved = answer(
        &shared,
        &context(),
        &signed(&shared, "inbox.list", json!({"state":"open"})),
    );
    assert_eq!(unserved["error"]["code"], "unknown_method");
    assert_ne!(
        unserved["error"]["message"],
        "request refused on host channel"
    );
}

#[test]
fn host_keeps_operation_result_refusals_distinct_from_channel_authority_refusals() {
    let mut shared = shared();
    shared.ports.submit = Box::new(|_, _, _| {
        Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "org operation refusal",
        )))
    });
    shared.ports.operation_get = Box::new(|_, _| {
        Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "operation lookup refusal",
        )))
    });
    for (method, params, message) in [
        (
            "team.create",
            json!({"command_id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","team_id":"team","name":"Team"}),
            "org operation refusal",
        ),
        (
            "operation.get",
            json!({"kind":"org","id":"0190a5d2-7c3e-7a01-8b2c-3d4e5f6071e0"}),
            "operation lookup refusal",
        ),
    ] {
        let response = answer(&shared, &context(), &signed(&shared, method, params));
        assert_eq!(response["id"], 71);
        assert_eq!(response["error"]["code"], "not_authorized");
        assert_eq!(response["error"]["message"], message);
    }
}

/// The organization commands the app used to send over an app-token bind on the office socket
/// now come from the owner's grant on a host channel: what the office answers is what the caller
/// gets, a storage failure is a fixed text, and a garbled command and an unknown method stay apart.
#[test]
fn what_the_office_answers_is_what_the_grant_bound_owner_gets() {
    let mut shared = shared();
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let answers = std::sync::Arc::clone(&calls);
    shared.ports.submit =
        Box::new(
            move |_, _, _| match answers.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
                0 => Ok(Err(ApiError::new(ErrorCode::AlreadyExists, "taken"))),
                _ => Err("disk on fire at /secret/path".to_owned()),
            },
        );
    let ctx = context();
    let create =
        json!({"command_id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","team_id":"T1","name":"Platform"});
    let refusal = answer(
        &shared,
        &ctx,
        &signed(&shared, "team.create", create.clone()),
    );
    assert_eq!(refusal["error"]["code"], "already_exists", "{refusal}");
    let failed = answer(&shared, &ctx, &signed(&shared, "team.create", create));
    assert_eq!(failed["error"]["code"], "storage_error", "{failed}");
    assert!(!failed.to_string().contains("secret"), "{failed}");

    let garbled = answer(
        &shared,
        &ctx,
        &signed(&shared, "team.create", json!({"nope": 1})),
    );
    assert_eq!(garbled["error"]["code"], "invalid_request", "{garbled}");
    let unknown = answer(&shared, &ctx, &signed(&shared, "team.fly", json!({})));
    // The host channel's operation policy refuses a method it does not list before dispatch.
    assert_eq!(unknown["error"]["code"], "not_authorized", "{unknown}");
    // Neither of the last two reached the office.
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[test]
fn msg_send_enforces_hr_message_guard_before_delivery() {
    let mut shared = shared();
    shared.ports.hr_message_guard = Some(Box::new(|_sender, target| {
        let trimmed = target.trim().trim_start_matches('@');
        if trimmed.eq_ignore_ascii_case("hr") {
            Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "You don't have access to hire. Ask your team lead.",
            )))
        } else {
            Ok(Ok(()))
        }
    }));
    let ctx = context();
    for target in ["hr", " hr ", "@hr"] {
        let req = signed(
            &shared,
            "msg.send",
            json!({
                "id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "to": target,
                "kind": "msg",
                "body": "please hire",
            }),
        );
        let resp = answer(&shared, &ctx, &req);
        assert_eq!(resp["error"]["code"], "not_authorized", "{target}: {resp}");
        assert_eq!(
            resp["error"]["message"],
            "You don't have access to hire. Ask your team lead."
        );
    }
}

#[test]
fn task140_host_channel_blocking_events_poll_avoids_hol_blocking_and_enforces_revocation() {
    let mut shared = shared();
    let entered = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let release = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let entered_cb = Arc::clone(&entered);
    let release_cb = Arc::clone(&release);
    shared.ports.chat = Some(Box::new(move |_member, _via_grant, _stop, req| {
        if !req.is::<org_proto::chat::ChatEventsPoll>() {
            return None;
        }
        {
            let mut g = entered_cb.0.lock().unwrap();
            *g = true;
            entered_cb.1.notify_all();
        }
        let mut r = release_cb.0.lock().unwrap();
        while !*r {
            r = release_cb.1.wait(r).unwrap();
        }
        Some(
            Response::ok::<org_proto::chat::ChatEventsPoll>(
                req.id,
                &org_proto::chat::ChatEventsPollResult {
                    org_head_seq: 1,
                    live_head_seq: 0,
                    presence_head_seq: 0,
                    snapshot_required: false,
                    live_lost: false,
                    org_changes: Vec::new(),
                    messages_received: Vec::new(),
                    message_states: Vec::new(),
                    presence_changes: Vec::new(),
                    activity_changes: Vec::new(),
                    inbox_changes: Vec::new(),
                },
            )
            .unwrap(),
        )
    }));
    let ctx = context();
    let revocations = Arc::clone(&ctx._revocations);
    let poll_req = {
        let mut r = signed(
            &shared,
            "chat.events_poll",
            json!({"after_org_seq": 1, "wait_ms": 2000}),
        );
        r.id = 101;
        r
    };
    let who_req = {
        let mut r = signed(&shared, "who", json!({}));
        r.id = 102;
        r
    };

    let (socket, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let shared = Arc::new(shared);
    let srv_shared = Arc::clone(&shared);
    let srv = std::thread::spawn(move || {
        serve(socket, &srv_shared, ctx, &Default::default());
    });
    let mut reader = std::io::BufReader::new(peer);

    // Negotiate host channel v2 hello.
    let hello = exchange(&mut reader, "hello", json!({"api": {"min": 2, "max": 2}}));
    assert_eq!(hello["result"]["api_version"], 2);

    // Send held chat.events_poll (id 101) and wait until its worker thread is blocking.
    frame::write_message(reader.get_mut(), &poll_req).unwrap();
    {
        let mut g = entered.0.lock().unwrap();
        while !*g {
            g = entered.1.wait(g).unwrap();
        }
    }

    // Send normal `who` (id 102) on the same multiplexed channel while 101 is held.
    let who_started = std::time::Instant::now();
    frame::write_message(reader.get_mut(), &who_req).unwrap();
    let first_reply: Value = frame::read_message(&mut reader).unwrap().unwrap();
    assert_eq!(
        first_reply["id"], 102,
        "non-blocking RPC must not wait behind held chat.events_poll: {first_reply}"
    );
    assert!(
        first_reply["result"]["members"].is_array(),
        "unexpected who reply: {first_reply}"
    );
    assert!(
        who_started.elapsed() < Duration::from_millis(500),
        "who took {:?}",
        who_started.elapsed()
    );

    // Revoke the browser session while 101 is still waiting, then release 101.
    revocations
        .lock()
        .unwrap()
        .apply_batch(vec!["session".into()])
        .unwrap();
    {
        let mut r = release.0.lock().unwrap();
        *r = true;
        release.1.notify_all();
    }
    let second_reply: Value = frame::read_message(&mut reader).unwrap().unwrap();
    assert_eq!(second_reply["id"], 101);
    assert_eq!(
        second_reply["error"]["code"], "not_authorized",
        "revoked session must never receive held poll result: {second_reply}"
    );

    drop(reader);
    srv.join().unwrap();
}

#[test]
fn task140_host_channel_held_events_poll_refuses_offboarded_person() {
    struct TogglePersonDirectory {
        active: Arc<std::sync::atomic::AtomicBool>,
    }
    impl org_mailbox::MemberDirectory for TogglePersonDirectory {
        fn resolve_member(
            &self,
            id: &str,
        ) -> Result<Option<(Member, org_proto::Position)>, org_mailbox::StoreError> {
            if !self.active.load(Ordering::SeqCst) {
                return Ok(None);
            }
            let member = org_mailbox::testkit::fixture::members()
                .into_iter()
                .find(|m| m.id == id)
                .expect("fixture member");
            Ok(Some((member, POSITION)))
        }
    }

    let mut shared = shared();
    let person_active = Arc::new(std::sync::atomic::AtomicBool::new(true));
    shared.ports.directory = Arc::new(TogglePersonDirectory {
        active: Arc::clone(&person_active),
    });
    let entered = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let release = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let entered_cb = Arc::clone(&entered);
    let release_cb = Arc::clone(&release);
    shared.ports.chat = Some(Box::new(move |_member, _via_grant, _stop, req| {
        if !req.is::<org_proto::chat::ChatEventsPoll>() {
            return None;
        }
        {
            let mut g = entered_cb.0.lock().unwrap();
            *g = true;
            entered_cb.1.notify_all();
        }
        let mut r = release_cb.0.lock().unwrap();
        while !*r {
            r = release_cb.1.wait(r).unwrap();
        }
        Some(
            Response::ok::<org_proto::chat::ChatEventsPoll>(
                req.id,
                &org_proto::chat::ChatEventsPollResult {
                    org_head_seq: 2,
                    live_head_seq: 0,
                    presence_head_seq: 0,
                    snapshot_required: false,
                    live_lost: false,
                    org_changes: Vec::new(),
                    messages_received: Vec::new(),
                    message_states: Vec::new(),
                    presence_changes: Vec::new(),
                    activity_changes: Vec::new(),
                    inbox_changes: Vec::new(),
                },
            )
            .unwrap(),
        )
    }));
    let ctx = context();
    let mut poll_req = signed(
        &shared,
        "chat.events_poll",
        json!({"after_org_seq": 1, "wait_ms": 2000}),
    );
    poll_req.id = 201;

    let (socket, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let shared = Arc::new(shared);
    let srv_shared = Arc::clone(&shared);
    let srv = std::thread::spawn(move || {
        serve(socket, &srv_shared, ctx, &Default::default());
    });
    let mut reader = std::io::BufReader::new(peer);
    let hello = exchange(&mut reader, "hello", json!({"api": {"min": 2, "max": 2}}));
    assert_eq!(hello["result"]["api_version"], 2);

    frame::write_message(reader.get_mut(), &poll_req).unwrap();
    {
        let mut g = entered.0.lock().unwrap();
        while !*g {
            g = entered.1.wait(g).unwrap();
        }
    }

    // Offboard/remove the person WITHOUT revoking the browser session token, then release poll.
    person_active.store(false, Ordering::SeqCst);
    {
        let mut r = release.0.lock().unwrap();
        *r = true;
        release.1.notify_all();
    }
    let reply: Value = frame::read_message(&mut reader).unwrap().unwrap();
    assert_eq!(reply["id"], 201);
    assert_eq!(
        reply["error"]["code"], "not_authorized",
        "offboarded/removed person must never receive held poll events: {reply}"
    );

    drop(reader);
    srv.join().unwrap();
}

#[test]
fn task80_host_channel_blocking_tess_turn_events_avoids_hol_blocking_and_enforces_revocation() {
    let mut shared = shared();
    let entered = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let release = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let entered_cb = Arc::clone(&entered);
    let release_cb = Arc::clone(&release);
    shared.ports.tess = Some(Box::new(
        move |_member, via_grant, _stop, _apply_exec, req| {
            if !req.is::<org_proto::tess::TessTurnEvents>() {
                return None;
            }
            assert!(
                via_grant,
                "HostChannel must invoke tess_rpc with via_grant=true"
            );
            {
                let mut g = entered_cb.0.lock().unwrap();
                *g = true;
                entered_cb.1.notify_all();
            }
            let mut r = release_cb.0.lock().unwrap();
            while !*r {
                r = release_cb.1.wait(r).unwrap();
            }
            Some(
                Response::ok::<org_proto::tess::TessTurnEvents>(
                    req.id,
                    &org_proto::tess::TessTurnEventsResult {
                        turn_id: "turn-1".into(),
                        partition: format!("tess:ask:{OWNER_ID}"),
                        events: Vec::new(),
                        head_seq: 1,
                        done: false,
                        lagged: false,
                    },
                )
                .unwrap(),
            )
        },
    ));
    let ctx = context();
    let revocations = Arc::clone(&ctx._revocations);
    let mut poll_req = signed(
        &shared,
        "tess.turn_events",
        json!({"turn_id": "turn-1", "after_seq": 1, "wait_ms": 2000}),
    );
    poll_req.id = 301;
    let mut who_req = signed(&shared, "who", json!({}));
    who_req.id = 302;

    let (socket, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let shared = Arc::new(shared);
    let srv_shared = Arc::clone(&shared);
    let srv = std::thread::spawn(move || {
        serve(socket, &srv_shared, ctx, &Default::default());
    });
    let mut reader = std::io::BufReader::new(peer);

    let hello = exchange(&mut reader, "hello", json!({"api": {"min": 2, "max": 2}}));
    assert_eq!(hello["result"]["api_version"], 2);

    // Send held tess.turn_events (id 301) and wait until its worker thread blocks.
    frame::write_message(reader.get_mut(), &poll_req).unwrap();
    {
        let mut g = entered.0.lock().unwrap();
        while !*g {
            g = entered.1.wait(g).unwrap();
        }
    }

    // Normal `who` (id 302) completes immediately on the same multiplexed HostChannel.
    let started = std::time::Instant::now();
    frame::write_message(reader.get_mut(), &who_req).unwrap();
    let first: Value = frame::read_message(&mut reader).unwrap().unwrap();
    assert_eq!(
        first["id"], 302,
        "who blocked behind tess.turn_events: {first}"
    );
    assert!(started.elapsed() < Duration::from_millis(500));

    // Revoke session while held, then release -> refused with not_authorized.
    revocations
        .lock()
        .unwrap()
        .apply_batch(vec!["session".into()])
        .unwrap();
    {
        let mut r = release.0.lock().unwrap();
        *r = true;
        release.1.notify_all();
    }
    let second: Value = frame::read_message(&mut reader).unwrap().unwrap();
    assert_eq!(second["id"], 301);
    assert_eq!(second["error"]["code"], "not_authorized", "{second}");

    drop(reader);
    srv.join().unwrap();
}

#[test]
fn task210_host_channel_agent_sessions_authorizes_persons_and_gives_accurate_refusals() {
    let mut shared = shared();
    let office_id = shared.hello.office.as_ref().unwrap().office.id.clone();
    shared.ports.sessions = Some(Box::new(|actor, req| {
        if !req.is::<org_proto::sessions::MemberSessionList>() {
            return None;
        }
        let params = req
            .params::<org_proto::sessions::MemberSessionList>()
            .unwrap();
        Some(
            Response::ok::<org_proto::sessions::MemberSessionList>(
                req.id,
                &org_proto::sessions::SessionListResult {
                    employee_id: params
                        .employee_id
                        .unwrap_or_else(|| actor.unwrap().to_owned()),
                    sessions: Vec::new(),
                    live_session_id: None,
                },
            )
            .unwrap(),
        )
    }));

    // 1. Authorized owner succeeds on `member.session_list` over the host channel.
    let ctx = context();
    let ok_resp = answer(
        &shared,
        &ctx,
        &signed(
            &shared,
            "member.session_list",
            json!({"employee_id": org_mailbox::testkit::fixture::PAT_ID}),
        ),
    );
    assert!(
        ok_resp.get("result").is_some(),
        "owner must succeed on member.session_list over host channel: {ok_resp}"
    );

    // 2. Valid grant whose member is NOT an active person (e.g., an agent or offboarded member)
    //    gets an accurate authorization error + hint on agent session read methods rather than
    //    opaque `request refused on host channel`.
    let make_grant_req = |shared: &Shared, member: &str, method: &str, params: Value| {
        let office = &shared.hello.office.as_ref().unwrap().office;
        let mut grant = json!({
            "account": "account",
            "member": member,
            "org_id": office.organization_id,
            "office_id": office.id,
            "channel_id": "opaque-channel",
            "session_id": "session",
            "exp": u64::MAX,
            "mac": "00".repeat(32)
        });
        sign(&mut grant);
        Request {
            id: 71,
            method: method.into(),
            params,
            principal: Some(json!({"grant": grant})),
        }
    };

    let agent_id = org_mailbox::testkit::fixture::members()
        .into_iter()
        .find(|m| m.kind == MemberKind::Agent)
        .expect("fixture agent")
        .id;
    for (method, params) in [
        ("member.external_sessions", json!({"employee_id": "@ada"})),
        ("member.external_pending", json!({})),
        ("member.session_list", json!({"employee_id": "@ada"})),
    ] {
        let resp = answer(
            &shared,
            &ctx,
            &make_grant_req(&shared, &agent_id, method, params.clone()),
        );
        assert_eq!(resp["error"]["code"], "not_authorized", "{method}: {resp}");
        assert_ne!(
            resp["error"]["message"], "request refused on host channel",
            "{method} must return accurate authorization error, not opaque refusal: {resp}"
        );
        assert!(
            resp["error"]["message"]
                .as_str()
                .is_some_and(|m| m.contains("active organization person")),
            "{method}: {resp}"
        );
        assert!(
            resp["error"]["hint"]
                .as_str()
                .is_some_and(|h| !h.is_empty()),
            "{method} must carry an actionable hint: {resp}"
        );

        // Unauthenticated / tampered MAC / expired grants STILL return opaque refusal!
        let mut bad_mac = make_grant_req(&shared, OWNER_ID, method, params.clone());
        bad_mac.principal.as_mut().unwrap()["grant"]["mac"] = json!("ff".repeat(32));
        refused(answer(&shared, &ctx, &bad_mac), "not_authorized");
    }

    // 3. Unlinked satellite operator grant (`office.role = Satellite`, `claims.member == office.id`)
    //    calling agent session reads receives an accurate unlinked-account authorization error.
    shared.hello.office.as_mut().unwrap().office.role = org_proto::OfficeRole::Satellite;
    for (method, params) in [
        ("member.external_sessions", json!({"employee_id": "@ada"})),
        ("member.external_pending", json!({})),
        ("member.session_list", json!({"employee_id": "@ada"})),
    ] {
        let resp = answer(
            &shared,
            &ctx,
            &make_grant_req(&shared, &office_id, method, params),
        );
        assert_eq!(resp["error"]["code"], "not_authorized", "{method}: {resp}");
        assert_ne!(
            resp["error"]["message"], "request refused on host channel",
            "{method} on satellite must return accurate refusal: {resp}"
        );
        assert!(
            resp["error"]["message"]
                .as_str()
                .is_some_and(|m| m.contains("not linked to an active organization person")),
            "{method}: {resp}"
        );
    }
}

#[test]
fn copilot_features_offloads_off_host_channel_loop_without_hol_blocking() {
    let mut shared = shared();
    let entered = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let release = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let entered_cb = Arc::clone(&entered);
    let release_cb = Arc::clone(&release);
    shared.ports.copilot = Some(Box::new(move |_member, req| {
        if !req.is::<org_proto::copilot::CopilotFeatures>() {
            return None;
        }
        {
            let mut g = entered_cb.0.lock().unwrap();
            *g = true;
            entered_cb.1.notify_all();
        }
        let mut r = release_cb.0.lock().unwrap();
        while !*r {
            r = release_cb.1.wait(r).unwrap();
        }
        Some(
            Response::ok::<org_proto::copilot::CopilotFeatures>(
                req.id,
                &org_proto::copilot::CopilotFeaturesResult {
                    features: Vec::new(),
                },
            )
            .unwrap(),
        )
    }));
    let ctx = context();
    let mut feat_req = signed(&shared, "copilot.features", json!({}));
    feat_req.id = 201;
    let mut who_req = signed(&shared, "who", json!({}));
    who_req.id = 202;

    let (socket, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let shared = Arc::new(shared);
    let srv_shared = Arc::clone(&shared);
    let srv = std::thread::spawn(move || {
        serve(socket, &srv_shared, ctx, &Default::default());
    });
    let mut reader = std::io::BufReader::new(peer);
    let hello = exchange(&mut reader, "hello", json!({"api": {"min": 2, "max": 2}}));
    assert_eq!(hello["result"]["api_version"], 2);

    frame::write_message(reader.get_mut(), &feat_req).unwrap();
    {
        let mut g = entered.0.lock().unwrap();
        while !*g {
            g = entered.1.wait(g).unwrap();
        }
    }

    // `who` must answer immediately while `copilot.features` is still probing on its worker thread.
    let who_started = std::time::Instant::now();
    frame::write_message(reader.get_mut(), &who_req).unwrap();
    let first: Value = frame::read_message(&mut reader).unwrap().unwrap();
    assert_eq!(
        first["id"], 202,
        "who must not head-of-line block behind copilot.features: {first}"
    );
    assert!(who_started.elapsed() < Duration::from_millis(500));

    {
        let mut r = release.0.lock().unwrap();
        *r = true;
        release.1.notify_all();
    }
    let second: Value = frame::read_message(&mut reader).unwrap().unwrap();
    assert_eq!(second["id"], 201);
    assert!(second["result"]["features"].is_array(), "{second}");

    drop(reader);
    srv.join().unwrap();
}

#[path = "inventory.rs"]
mod inventory;
