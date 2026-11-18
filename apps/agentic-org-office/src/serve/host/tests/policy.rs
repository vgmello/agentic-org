use super::*;
use crate::test_support::sign_grant as sign;
use org_mailbox::testkit::fixture::{OWNER_ID, POSITION};
use org_proto::{Grant, Member, MemberKind};
use serde_json::{json, Value};

fn shared() -> Shared {
    let mut ports = super::super::super::tests::ports(Box::new(|| {
        Ok(
            serde_json::from_value(json!({"position":{"epoch":1,"seq":1},"teams":[],
            "members":[],"memberships":[],"projects":[],"project_teams":[],"areas":[]}))
            .unwrap(),
        )
    }));
    ports.submit = Box::new(|_| {
        Ok(Ok(serde_json::from_value(
            json!({"status":"committed","position":{"epoch":1,"seq":2}}),
        )
        .unwrap()))
    });
    ports.work = Box::new(|caller, _| {
        assert_eq!(caller, org_work::Caller::app(OWNER_ID));
        Ok(Ok(
            serde_json::from_value(json!({"status":"queued"})).unwrap()
        ))
    });
    ports.who = Box::new(|_| Ok(Ok(WhoResult { members: vec![] })));
    ports.operation_get = Box::new(|_| {
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
        ("who", json!({})),
        (
            "operation.get",
            json!({"kind":"org","id":"01ARZ3NDEKTSV4RRFFQ69G5FAV"}),
        ),
        ("inbox.list", json!({"state":"open"})),
    ] {
        let response = answer(&shared, &ctx, &signed(&shared, method, params));
        assert!(response.get("result").is_some(), "{method}: {response}");
    }
    let rows: Value = serde_json::from_str(include_str!("operations.json")).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 45);
    for row in rows.as_array().unwrap() {
        let method = row["method"].as_str().unwrap();
        let request = signed(&shared, method, row["params"].clone());
        if row["group"] == "org" {
            assert!(OrgCommand::parse(&request).unwrap().is_some(), "{method}");
        } else {
            assert!(WorkCall::parse(&request).unwrap().is_some(), "{method}");
        }
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
        "msg.reply",
        "msg.defer",
        "msg.done",
        "msg.notify",
        "msg.get",
        "msg.list",
        "msg.threads",
        "msg.human_needs_you",
        "dm.open",
        "work.list",
    ] {
        let response = answer(&shared, &ctx, &signed(&shared, method, json!({})));
        assert_eq!(
            response["error"]["code"], "unknown_method",
            "{method}: {response}"
        );
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
    for (kind, owner, id) in [
        (MemberKind::Person, false, OWNER_ID),
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
    shared.auth = LaunchAuth::without_grants();
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
        .inbox(PAT_ID, Default::default())
        .unwrap();
    assert_eq!(inbox.headers.len(), 1);
    assert_eq!(inbox.headers[0].from_member_id, OWNER_ID);
    shared
        .ports
        .mailbox
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
    shared.ports.submit = Box::new(|_| {
        Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "org operation refusal",
        )))
    });
    shared.ports.work = Box::new(|_, _| {
        Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "work operation refusal",
        )))
    });
    for (method, params, message) in [
        (
            "team.create",
            json!({"command_id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","team_id":"team","name":"Team"}),
            "org operation refusal",
        ),
        (
            "task.create",
            json!({"change_id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","item_id":"item","project_id":"project","title":"Task"}),
            "work operation refusal",
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
            move |_| match answers.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
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

#[path = "inventory.rs"]
mod inventory;
