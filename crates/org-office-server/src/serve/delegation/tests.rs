use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use org_office::CallerAuthority;
use org_proto::chat::ConversationSummary;
use org_proto::hiring::{HrHire, HrHireDraft};
use org_proto::Outcome;
use serde_json::{json, Value};

use super::*;

/// A [`ChiefOfStaffPort`] the tests script: `tess` is the Chief of Staff, acting as `current`.
pub(in crate::serve) struct FakePort {
    pub(in crate::serve) tess: String,
    pub(in crate::serve) current: Mutex<Delegation>,
    /// Every read fails (the office cannot answer).
    pub(in crate::serve) broken: AtomicBool,
    /// `(caller, team)` pairs a caller may hire into.
    hire_into: Vec<(&'static str, &'static str)>,
    /// `(request, team)` of every hire request on record.
    hires: Vec<(&'static str, &'static str)>,
    /// `(member, conversations)` each member may see.
    visible: Vec<(String, Vec<ConversationSummary>)>,
    /// `(message, conversation)` of every message on record.
    messages: Vec<(String, String)>,
    drafts: Mutex<Vec<(String, String)>>,
}

impl FakePort {
    pub(in crate::serve) fn new(tess: &str, current: Delegation) -> Self {
        Self {
            tess: tess.into(),
            current: Mutex::new(current),
            broken: AtomicBool::new(false),
            hire_into: Vec::new(),
            hires: Vec::new(),
            visible: Vec::new(),
            messages: Vec::new(),
            drafts: Mutex::new(Vec::new()),
        }
    }

    fn fails(&self) -> Result<(), ApiError> {
        if self.broken.load(Ordering::SeqCst) {
            return Err(ApiError::new(ErrorCode::Unavailable, "storage is down"));
        }
        Ok(())
    }
}

impl ChiefOfStaffPort for FakePort {
    fn delegation(&self, member: &str) -> Result<Option<Delegation>, ApiError> {
        self.fails()?;
        Ok((member == self.tess).then(|| self.current.lock().unwrap().clone()))
    }

    fn caller_may_hire_into(&self, caller: &str, team_id: &str) -> Result<bool, ApiError> {
        self.fails()?;
        Ok(self.hire_into.contains(&(caller, team_id)))
    }

    fn hire_team(&self, request_id: &str) -> Result<Option<String>, ApiError> {
        self.fails()?;
        Ok(self
            .hires
            .iter()
            .find(|(id, _)| *id == request_id)
            .map(|(_, team)| (*team).to_owned()))
    }

    fn visible_conversations(&self, member: &str) -> Result<Vec<ConversationSummary>, ApiError> {
        self.fails()?;
        Ok(self
            .visible
            .iter()
            .find(|(m, _)| m == member)
            .map(|(_, all)| all.clone())
            .unwrap_or_default())
    }

    fn message_conversation(&self, message_id: &str) -> Result<Option<String>, ApiError> {
        self.fails()?;
        Ok(self
            .messages
            .iter()
            .find(|(id, _)| id == message_id)
            .map(|(_, conversation)| conversation.clone()))
    }

    fn attachment_conversation(&self, _attachment_id: &str) -> Result<Option<String>, ApiError> {
        self.fails()?;
        Ok(None)
    }

    fn resolve_member(&self, reference: &str) -> Result<Option<String>, ApiError> {
        self.fails()?;
        Ok(Some(reference.to_owned()))
    }

    fn is_chief_of_staff(&self, member: &str) -> bool {
        member == self.tess
    }

    fn record_draft(&self, request_id: &str, caller: &str) {
        self.drafts
            .lock()
            .unwrap()
            .push((request_id.into(), caller.into()));
    }

    fn draft_caller(&self, request_id: &str) -> Option<String> {
        self.drafts
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|(id, _)| id == request_id)
            .map(|(_, caller)| caller.clone())
    }
}

fn acting_for(caller: CallerAuthority) -> Delegation {
    Delegation {
        caller_id: Some("caller".into()),
        caller,
    }
}

fn owner() -> CallerAuthority {
    CallerAuthority {
        active: true,
        is_person: true,
        is_owner: true,
        may_hire: true,
    }
}

fn member(may_hire: bool) -> CallerAuthority {
    CallerAuthority {
        active: true,
        is_person: true,
        is_owner: false,
        may_hire,
    }
}

#[test]
fn secrets_and_owner_reads_are_never_hers_even_for_the_owner() {
    for method in [
        "chat.read_as_owner",
        "provider.credential_status",
        "provider.enrollment_start",
        "provider.connection.create",
        "cost.subscription_usage.credential_set",
        "auth.login_request",
        "office.invitation.create",
        "office.remove",
        "member.session_approval",
        "member.agent_execution_set",
        "member.set_may_hire",
        "member.external_approve",
        "member.external_bind",
        "git.push",
        "search.query",
    ] {
        assert_eq!(access(method), Access::Never, "{method}");
        assert!(!allowed(access(method), &acting_for(owner())), "{method}");
    }
}

#[test]
fn owner_only_work_follows_the_caller() {
    for method in [
        "cost.query",
        "cost.export_csv",
        "budget.list",
        "budget.set",
        "inbox.list",
        "member.agent_offboard",
        "member.session_start",
        "team.set_max_members",
        "role.create",
        "process.write",
        "an.unknown.method",
    ] {
        assert_eq!(access(method), Access::CallerOwner, "{method}");
        assert!(allowed(access(method), &acting_for(owner())), "{method}");
        assert!(
            !allowed(access(method), &acting_for(member(true))),
            "{method}"
        );
        let nobody = Delegation::default();
        assert!(
            !allowed(access(method), &nobody),
            "{method} acting for nobody"
        );
    }
}

#[test]
fn hiring_needs_a_caller_who_may_hire() {
    for method in [
        "hr.hire",
        "hr.hire.draft",
        "hr.hire.confirm",
        "hr.hire.withdraw",
    ] {
        assert_eq!(access(method), Access::CallerHires, "{method}");
        assert!(allowed(access(method), &acting_for(owner())));
        assert!(allowed(access(method), &acting_for(member(true))));
        assert!(!allowed(access(method), &acting_for(member(false))));
        assert!(!allowed(access(method), &Delegation::default()));
    }
    // Reading hires needs a caller who may hire too; what she sees is scoped per team.
    for method in ["hr.hire.list", "hr.hire.inspect"] {
        assert_eq!(access(method), Access::CallerHires, "{method}");
        assert!(!allowed(access(method), &acting_for(member(false))));
    }
}

#[test]
fn her_private_notes_and_whole_mailbox_reads_are_never_hers() {
    for method in [
        "tasks.notes.add",
        "tasks.notes.list",
        "tasks.notes.get",
        "tasks.notes.update",
        "tasks.notes.remove",
        "tasks.notes.publish",
        "msg.read",
        "msg.wait",
        "msg.notify",
        "chat.events_poll",
    ] {
        assert_eq!(access(method), Access::Never, "{method}");
        assert!(!allowed(access(method), &acting_for(owner())), "{method}");
    }
    // Her own identity is an open read; signing in is never hers.
    assert_eq!(access("auth.whoami"), Access::Open);
    assert_eq!(access("auth.login_request"), Access::Never);
    assert!(!allowed(
        access("auth.whoami"),
        &acting_for(CallerAuthority::default())
    ));
    // Reading her sessions (another caller's last reply) needs the owner.
    for method in ["member.session_inspect", "member.session_list"] {
        assert_eq!(access(method), Access::CallerOwner, "{method}");
    }
}

#[test]
fn reads_and_messages_are_open_while_the_caller_is_an_active_person() {
    for method in [
        "who",
        "org.snapshot",
        "tasks.list",
        "msg.send",
        "chat.messages",
        "skill.read",
        "hello",
    ] {
        assert_eq!(access(method), Access::Open, "{method}");
        assert!(
            allowed(access(method), &acting_for(member(false))),
            "{method}"
        );
        // A wake (acting for nobody) keeps her own reads.
        assert!(allowed(access(method), &Delegation::default()), "{method}");
    }
    // A caller offboarded, deactivated or unknown since the run began narrows her to nothing.
    let gone = acting_for(CallerAuthority::default());
    let inactive = acting_for(CallerAuthority {
        active: false,
        ..owner()
    });
    let agent = acting_for(CallerAuthority {
        is_person: false,
        ..owner()
    });
    for delegation in [gone, inactive, agent] {
        for method in ["who", "msg.send", "hr.hire", "cost.query"] {
            assert!(
                !allowed(access(method), &delegation),
                "{method} {delegation:?}"
            );
        }
    }
}

/// Through the real dispatch: the gate answers before any method runs, for the Chief of Staff
/// alone, and only as her delegation allows.
#[test]
fn the_gate_refuses_her_before_any_method_runs_and_leaves_everyone_else_alone() {
    use crate::serve::tests::{launch_key, ports, test_info};
    use crate::serve::SnapshotFailure;

    let _spawn = crate::transfer::tests::without_process_spawn();
    let dir = tempfile::TempDir::new().unwrap();
    let port = Arc::new(FakePort::new("tess", acting_for(member(false))));
    let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
    ports.delegation = Some(Arc::clone(&port) as Arc<dyn ChiefOfStaffPort>);
    let shared = Shared::new(
        &test_info(),
        &dir.path().join("d.sock"),
        ports,
        launch_key(),
    );
    let cost = Request {
        id: 7,
        method: "cost.query".into(),
        principal: None,
        params: serde_json::json!({}),
    };
    let refused = |member: &str, via_grant: bool| {
        let response = shared.answer_for_test(&cost, member, via_grant);
        matches!(
            response.outcome,
            org_proto::Outcome::Error(ref e)
                if e.code == ErrorCode::NotAuthorized && e.message == DELEGATION_DENIED
        )
    };
    // Acting for a member who is not the owner: org-wide spend is refused by the gate.
    assert!(refused("tess", false));
    // Another agent, and a person on a grant, never meet the gate.
    assert!(!refused("ada", false));
    assert!(!refused("tess", true));
    // Acting for the owner, the gate lets the call through to the method (which then
    // answers by its own rules: here no cost port is wired at all).
    *port.current.lock().unwrap() = acting_for(owner());
    assert!(!refused("tess", false));
    // A secret method stays refused even for the owner.
    let secret = Request {
        id: 8,
        method: "provider.credential_status".into(),
        principal: None,
        params: serde_json::json!({}),
    };
    let response = shared.answer_for_test(&secret, "tess", false);
    assert!(matches!(
        response.outcome,
        org_proto::Outcome::Error(ref e) if e.message == DELEGATION_DENIED
    ));
    // The office cannot read who she is: every bound member is refused, never let through.
    port.broken.store(true, Ordering::SeqCst);
    for member in ["tess", "ada"] {
        let response = shared.answer_for_test(&cost, member, false);
        assert!(
            matches!(
                response.outcome,
                org_proto::Outcome::Error(ref e) if e.code == ErrorCode::Unavailable
            ),
            "{member}"
        );
    }
    // A person on a grant never meets the gate.
    let response = shared.answer_for_test(&cost, "ada", true);
    assert!(!matches!(
        response.outcome,
        org_proto::Outcome::Error(ref e) if e.message == "storage is down"
    ));
}

// ---- `scope`: per-team hiring, draft ownership, and narrowed reads (pass 2) ----

const LEAD_A: &str = "lead-a";
const MEMBER_B: &str = "member-b";
const TESS: &str = "tess";

fn lead_of_team_a() -> Delegation {
    Delegation {
        caller_id: Some(LEAD_A.into()),
        caller: member(true),
    }
}

fn the_owner() -> Delegation {
    Delegation {
        caller_id: Some("owner".into()),
        caller: owner(),
    }
}

fn hiring_port() -> FakePort {
    let mut port = FakePort::new(TESS, lead_of_team_a());
    port.hire_into = vec![(LEAD_A, "team-a")];
    port.hires = vec![("req-owner-b", "team-b"), ("req-lead-a", "team-a")];
    port
}

fn call(method: &str, params: Value) -> Request {
    Request {
        id: 9,
        method: method.into(),
        principal: None,
        params,
    }
}

fn hire(method: &str, team: &str) -> Request {
    let mut params = json!({
        "command_id": "c", "request_id": "req-new",
        "home_team_id": team, "proposal": {"name": "Bot", "handle": "bot"}, "reason": "r"
    });
    if method == "hr.hire" {
        params["employee_id"] = json!("e");
    }
    let request = call(method, params);
    assert!(
        request.params::<HrHireDraft>().is_ok() || request.params::<HrHire>().is_ok(),
        "well-formed {method}"
    );
    request
}

fn decide(request_id: &str, method: &str) -> Request {
    let params = if method == "hr.hire.confirm" {
        json!({"command_id": "c", "request_id": request_id, "employee_id": "e", "expected_version": 1})
    } else {
        json!({"command_id": "c", "request_id": request_id, "expected_version": 1, "closed_reason": "no"})
    };
    call(method, params)
}

fn is_denied(result: &Result<(), Response>) -> bool {
    matches!(result, Err(r) if matches!(&r.outcome, Outcome::Error(e) if e.message == DELEGATION_DENIED))
}

#[test]
fn a_team_lead_hires_through_her_only_into_teams_they_may_hire_into() {
    let port = hiring_port();
    for method in ["hr.hire", "hr.hire.draft"] {
        let lead = lead_of_team_a();
        assert!(
            scope::admit(&hire(method, "team-a"), &port, TESS, &lead).is_ok(),
            "{method}"
        );
        // Her organization-wide reach never widens the lead's: not team B, not the root team.
        for team in ["team-b", "root"] {
            assert!(
                is_denied(&scope::admit(&hire(method, team), &port, TESS, &lead)),
                "{method} {team}"
            );
        }
        // The owner hires anywhere through her.
        assert!(scope::admit(&hire(method, "team-b"), &port, TESS, &the_owner()).is_ok());
        // Acting for nobody, nowhere.
        assert!(is_denied(&scope::admit(
            &hire(method, "team-a"),
            &port,
            TESS,
            &Delegation::default()
        )));
    }
    // Inspecting a hire follows the same teams.
    let inspect = |id: &str| call("hr.hire.inspect", json!({"request_id": id}));
    assert!(is_denied(&scope::admit(
        &inspect("req-owner-b"),
        &port,
        TESS,
        &lead_of_team_a()
    )));
    assert!(scope::admit(&inspect("req-lead-a"), &port, TESS, &lead_of_team_a()).is_ok());
    // An office that cannot answer refuses.
    port.broken.store(true, Ordering::SeqCst);
    assert!(scope::admit(&hire("hr.hire", "team-a"), &port, TESS, &lead_of_team_a()).is_err());
}

#[test]
fn a_draft_is_confirmed_or_withdrawn_only_by_the_owner_or_the_member_it_was_drafted_for() {
    let port = hiring_port();
    // She drafted `req-lead-a` for the lead (recorded when the draft succeeded).
    let draft = call(
        "hr.hire.draft",
        json!({
            "command_id": "c", "request_id": "req-lead-a", "home_team_id": "team-a",
            "proposal": {"name": "Bot", "handle": "bot"}, "reason": "r"
        }),
    );
    let ok = Response {
        id: 9,
        outcome: Outcome::Result(json!({})),
        waiting_messages: None,
    };
    scope::narrow(&draft, &port, TESS, &lead_of_team_a(), ok);
    assert_eq!(port.draft_caller("req-lead-a").as_deref(), Some(LEAD_A));
    for method in ["hr.hire.confirm", "hr.hire.withdraw"] {
        // The owner's draft in team B is not the lead's to confirm or withdraw.
        assert!(
            is_denied(&scope::admit(
                &decide("req-owner-b", method),
                &port,
                TESS,
                &lead_of_team_a()
            )),
            "{method}"
        );
        // The owner decides any draft.
        assert!(scope::admit(&decide("req-owner-b", method), &port, TESS, &the_owner()).is_ok());
        // The lead decides the draft made for them.
        assert!(scope::admit(
            &decide("req-lead-a", method),
            &port,
            TESS,
            &lead_of_team_a()
        )
        .is_ok());
        // Another member who may hire into team A did not ask for it.
        let other = Delegation {
            caller_id: Some("lead-a2".into()),
            caller: member(true),
        };
        assert!(is_denied(&scope::admit(
            &decide("req-lead-a", method),
            &port,
            TESS,
            &other
        )));
        // Acting for nobody, never.
        assert!(is_denied(&scope::admit(
            &decide("req-lead-a", method),
            &port,
            TESS,
            &Delegation::default()
        )));
        // An unknown request is the method's to refuse.
        assert!(scope::admit(
            &decide("req-unknown", method),
            &port,
            TESS,
            &lead_of_team_a()
        )
        .is_ok());
    }
    // The lead lost hiring power over team A since: no longer theirs to decide.
    let mut demoted = hiring_port();
    demoted.hire_into.clear();
    demoted.record_draft("req-lead-a", LEAD_A);
    assert!(is_denied(&scope::admit(
        &decide("req-lead-a", "hr.hire.confirm"),
        &demoted,
        TESS,
        &lead_of_team_a()
    )));
}

#[test]
fn hire_lists_show_only_hires_into_teams_the_caller_may_hire_into() {
    let port = hiring_port();
    let record = |id: &str, team: &str| {
        json!({
            "id": id, "revision": 1, "state": "draft", "requested_by_employee_id": TESS,
            "home_team_id": team, "proposal": {"name": "Bot", "handle": "bot"}, "reason": "r",
            "created_at": "2026-01-01T00:00:00.000Z", "updated_at": "2026-01-01T00:00:00.000Z"
        })
    };
    let listed = Response {
        id: 9,
        outcome: Outcome::Result(
            json!({"hires": [record("req-owner-b", "team-b"), record("req-lead-a", "team-a")]}),
        ),
        waiting_messages: None,
    };
    let request = call("hr.hire.list", json!({}));
    let narrowed = scope::narrow(&request, &port, TESS, &lead_of_team_a(), listed.clone());
    let ids = result_ids(&narrowed, "hires", "id");
    assert_eq!(ids, ["req-lead-a"]);
    let all = scope::narrow(&request, &port, TESS, &the_owner(), listed);
    assert_eq!(result_ids(&all, "hires", "id").len(), 2);
}

fn conversation(id: &str, kind: &str, members: &[&str]) -> ConversationSummary {
    let members: Vec<Value> = members
        .iter()
        .map(|m| json!({"employee_id": m, "joined_at": "2026-01-01T00:00:00.000Z"}))
        .collect();
    serde_json::from_value(json!({
        "id": id, "kind": kind, "archived": false, "revision": 1,
        "created_at": "2026-01-01T00:00:00.000Z", "members": members
    }))
    .unwrap()
}

/// Tess sees a channel, her DM with the owner and her DM with member B; B sees the channel
/// and their own DM with her.
fn conversations_port(current: Delegation) -> FakePort {
    let mut port = FakePort::new(TESS, current);
    let channel = conversation("chan-1", "channel", &[TESS, "owner", MEMBER_B]);
    let owner_dm = conversation("dm-owner-tess", "dm", ["owner", TESS].as_slice());
    let member_dm = conversation("dm-b-tess", "dm", [MEMBER_B, TESS].as_slice());
    port.visible = vec![
        (
            TESS.into(),
            vec![channel.clone(), owner_dm, member_dm.clone()],
        ),
        (MEMBER_B.into(), vec![channel, member_dm]),
    ];
    port
}

fn acting_for_b() -> Delegation {
    Delegation {
        caller_id: Some(MEMBER_B.into()),
        caller: member(false),
    }
}

fn result_ids(response: &Response, list: &str, field: &str) -> Vec<String> {
    let Outcome::Result(value) = &response.outcome else {
        panic!("expected a result: {response:?}");
    };
    value[list]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item[field].as_str().unwrap().to_owned())
        .collect()
}

fn answer(value: Value) -> Response {
    Response {
        id: 9,
        outcome: Outcome::Result(value),
        waiting_messages: None,
    }
}

fn not_found(response: &Response) -> bool {
    matches!(&response.outcome, Outcome::Error(e) if e.code == ErrorCode::NotFound)
}

#[test]
fn another_member_never_sees_the_owners_dm_with_her() {
    let tess_view = json!({"conversations": [
        serde_json::to_value(conversation("chan-1", "channel", &[TESS])).unwrap(),
        serde_json::to_value(conversation("dm-owner-tess", "dm", &["owner", TESS])).unwrap(),
        serde_json::to_value(conversation("dm-b-tess", "dm", &[MEMBER_B, TESS])).unwrap(),
    ]});
    let request = call("chat.conversations", json!({}));
    let port = conversations_port(acting_for_b());
    let narrowed = scope::narrow(
        &request,
        &port,
        TESS,
        &acting_for_b(),
        answer(tess_view.clone()),
    );
    assert_eq!(
        result_ids(&narrowed, "conversations", "id"),
        ["chan-1", "dm-b-tess"]
    );
    // A wake (acting for nobody) sees channels only.
    let wake = scope::narrow(
        &request,
        &port,
        TESS,
        &Delegation::default(),
        answer(tess_view),
    );
    assert_eq!(result_ids(&wake, "conversations", "id"), ["chan-1"]);

    // The owner's DM history is not found for B, nor for a wake; B's own is.
    let messages = call("chat.messages", json!({"conversation": "dm-owner-tess"}));
    let history = |id: &str| answer(json!({"conversation_id": id, "messages": []}));
    assert!(not_found(&scope::narrow(
        &messages,
        &port,
        TESS,
        &acting_for_b(),
        history("dm-owner-tess")
    )));
    assert!(not_found(&scope::narrow(
        &messages,
        &port,
        TESS,
        &Delegation::default(),
        history("dm-owner-tess")
    )));
    let own = scope::narrow(
        &messages,
        &port,
        TESS,
        &acting_for_b(),
        history("dm-b-tess"),
    );
    assert!(matches!(own.outcome, Outcome::Result(_)));

    // Her chat inbox keeps only items of conversations B sees.
    let item = |id: &str, conv: &str| {
        json!({
            "id": id, "conversation_id": conv, "thread_id": id, "from_member_id": "owner",
            "from_display_name": "Owner", "kind": "msg", "priority": "normal", "urgent": false,
            "created_at": "2026-01-01T00:00:00.000Z", "state": "unread", "triage_gen": 0,
            "delivered_because": "addressed", "local_seq": 1
        })
    };
    let inbox = answer(
        json!({"items": [item("m-1", "dm-owner-tess"), item("m-2", "dm-b-tess"), item("m-3", "chan-1")]}),
    );
    let narrowed = scope::narrow(
        &call("chat.inbox", json!({})),
        &port,
        TESS,
        &acting_for_b(),
        inbox,
    );
    assert_eq!(result_ids(&narrowed, "items", "id"), ["m-2", "m-3"]);

    // The office cannot say what B sees: refused, never passed through.
    port.broken.store(true, Ordering::SeqCst);
    let refused = scope::narrow(
        &messages,
        &port,
        TESS,
        &acting_for_b(),
        history("dm-b-tess"),
    );
    assert!(matches!(refused.outcome, Outcome::Error(ref e) if e.code == ErrorCode::Unavailable));
}

fn header(id: &str, from: &str, to: &str) -> Value {
    json!({
        "id": id, "thread_id": id, "from_member_id": from, "from_display_name": from,
        "to_member_id": to, "to_display_name": to, "kind": "msg", "priority": "normal",
        "urgent": false, "created_at": "2026-01-01T00:00:00.000Z", "body_bytes": 2,
        "local_seq": 1, "state": "unread"
    })
}

#[test]
fn her_mailbox_shows_a_member_only_their_own_messages_with_her() {
    let port = conversations_port(acting_for_b());
    let page = json!({"headers": [
        header("m-owner", "owner", TESS),
        header("m-b", MEMBER_B, TESS),
        header("m-to-b", TESS, MEMBER_B),
        header("m-hr", "hr", TESS),
    ]});
    for method in ["msg.inbox", "msg.list"] {
        let request = call(method, json!({}));
        let narrowed = scope::narrow(&request, &port, TESS, &acting_for_b(), answer(page.clone()));
        assert_eq!(
            result_ids(&narrowed, "headers", "id"),
            ["m-b", "m-to-b"],
            "{method}"
        );
        let wake = scope::narrow(
            &request,
            &port,
            TESS,
            &Delegation::default(),
            answer(page.clone()),
        );
        assert!(result_ids(&wake, "headers", "id").is_empty(), "{method}");
    }
    let get = call("msg.get", json!({"id": "m-owner"}));
    let message = |h: Value| answer(json!({"message": {"header": h, "body": "hi"}}));
    assert!(not_found(&scope::narrow(
        &get,
        &port,
        TESS,
        &acting_for_b(),
        message(header("m-owner", "owner", TESS))
    )));
    let own = scope::narrow(
        &get,
        &port,
        TESS,
        &acting_for_b(),
        message(header("m-b", MEMBER_B, TESS)),
    );
    assert!(matches!(own.outcome, Outcome::Result(_)));
    // Threads B is not in are hidden.
    let thread = |id: &str, who: &[&str]| {
        json!({"thread_id": id, "created_at": "t", "last_msg_id": id, "last_msg_at": "t",
               "message_count": 1, "participants": who})
    };
    let threads = answer(
        json!({"threads": [thread("t-owner", &["owner", TESS]), thread("t-b", &[MEMBER_B, TESS])]}),
    );
    let narrowed = scope::narrow(
        &call("msg.threads", json!({})),
        &port,
        TESS,
        &acting_for_b(),
        threads,
    );
    assert_eq!(result_ids(&narrowed, "threads", "thread_id"), ["t-b"]);
    // Needs-you rows too, with the urgent count recomputed.
    let mut urgent = header("m-owner", "owner", TESS);
    urgent["urgent"] = json!(true);
    let needs =
        answer(json!({"items": [urgent, header("m-b", MEMBER_B, TESS)], "total_urgent_unread": 1}));
    let narrowed = scope::narrow(
        &call("msg.human_needs_you", json!({})),
        &port,
        TESS,
        &acting_for_b(),
        needs,
    );
    assert_eq!(result_ids(&narrowed, "items", "id"), ["m-b"]);
    let Outcome::Result(value) = &narrowed.outcome else {
        panic!()
    };
    assert_eq!(value["total_urgent_unread"], json!(0));
}

#[test]
fn she_never_long_polls_an_ask_even_for_the_owner() {
    assert_eq!(access("chat.wait_ask"), Access::Never);
    assert!(!allowed(access("chat.wait_ask"), &acting_for(owner())));
}

/// Her aggregate unread count spans every caller's messages: it is never attached to her
/// answers, while every other member keeps theirs.
#[test]
fn her_answers_never_carry_the_waiting_messages_hint() {
    use std::sync::atomic::AtomicU32;

    use crate::serve::auth::Binding;
    use crate::serve::messaging::attach_waiting_messages_hint;
    use crate::serve::tests::{launch_key, ports, test_info};
    use crate::serve::SnapshotFailure;

    let dir = tempfile::TempDir::new().unwrap();
    let port = Arc::new(FakePort::new(TESS, acting_for(owner())));
    let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
    ports.delegation = Some(Arc::clone(&port) as Arc<dyn ChiefOfStaffPort>);
    ports.waiting_messages_counter = Some(Box::new(|_| Arc::new(AtomicU32::new(3))));
    let shared = Shared::new(
        &test_info(),
        &dir.path().join("d.sock"),
        ports,
        launch_key(),
    );
    let hint = |member: &str| {
        let binding = Binding::Member {
            id: member.to_owned(),
            via_grant: false,
        };
        let mut response = answer(json!({}));
        attach_waiting_messages_hint(&shared, &binding, &mut None, &mut response);
        response.waiting_messages
    };
    assert_eq!(hint(TESS), None);
    assert_eq!(hint("ada"), Some(3));
}
