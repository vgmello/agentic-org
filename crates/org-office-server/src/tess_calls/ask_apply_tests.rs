use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use org_office::rules::OrgCommand;
use org_office::Office;
use org_office_sessions::{
    SessionActivity, SessionStreamEvent, SessionStreamFrame, SessionStreamHub, SessionStreamKey,
    TurnEnd,
};
use org_proto::org::{MemberAddPersonParams, MemberRemoveParams};
use org_proto::sessions::ActivityPhase;
use org_proto::tasks::TasksUpdateParams;
use org_proto::tess::{
    SuggestionApply, SuggestionApplyParams, SuggestionCreate, SuggestionCreateParams,
    SuggestionDismiss, SuggestionDismissParams, SuggestionInspect, SuggestionInspectParams,
    SuggestionStatus, TessAsk, TessAskParams, TessAskStatus, TessSessionReset,
    TessSessionResetParams, TessTurnCancel, TessTurnCancelParams, TessTurnEventKind,
    TessTurnEvents, TessTurnEventsParams, TessTurnOutcome, TessViewportContext, UiEntityKind,
    UiFollowUp, UiPresent, UiPresentParams,
};
use org_proto::{ApiError, Call, ErrorCode, Outcome, Request, Response};
use org_tess::{ApplyExecFn, TessRuntimePort, TessStartedSession};
use serde_json::json;

use crate::live::LiveEvents;
use crate::task_calls::TaskService;
use crate::tess_calls::{follow_tess_stream, TessService};

use super::{harness, map, next_id, Harness};

#[derive(Default)]
struct FakeRuntime {
    readiness_err: Mutex<Option<String>>,
    started: Mutex<Vec<(String, String, String)>>,
    cancelled: Mutex<Vec<(String, String)>>,
    resets: Mutex<Vec<(String, String)>>,
    next_session: AtomicUsize,
    partition_live: Mutex<Option<bool>>,
}

impl FakeRuntime {
    fn ready() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn unavailable(reason: &str) -> Arc<Self> {
        Arc::new(Self {
            readiness_err: Mutex::new(Some(reason.to_owned())),
            ..Self::default()
        })
    }

    fn set_live(&self, live: bool) {
        *self.partition_live.lock().unwrap() = Some(live);
    }
}

impl TessRuntimePort for FakeRuntime {
    fn check_readiness(&self, _caller_id: &str, _tess_id: &str) -> Result<(), String> {
        if let Some(reason) = self.readiness_err.lock().unwrap().clone() {
            return Err(reason);
        }
        Ok(())
    }

    fn start_ask_turn(
        &self,
        caller_id: &str,
        tess_id: &str,
        _command_id: &str,
        instruction: &str,
    ) -> Result<TessStartedSession, ApiError> {
        self.started.lock().unwrap().push((
            caller_id.to_owned(),
            tess_id.to_owned(),
            instruction.to_owned(),
        ));
        let n = self.next_session.fetch_add(1, Ordering::Relaxed) + 1;
        Ok(TessStartedSession {
            session_id: format!("sess-{n}"),
        })
    }

    fn cancel_ask_turn(&self, caller_id: &str, tess_id: &str) -> Result<bool, ApiError> {
        self.cancelled
            .lock()
            .unwrap()
            .push((caller_id.to_owned(), tess_id.to_owned()));
        Ok(true)
    }

    fn reset_caller_session(&self, caller_id: &str, tess_id: &str) -> Result<bool, ApiError> {
        self.resets
            .lock()
            .unwrap()
            .push((caller_id.to_owned(), tess_id.to_owned()));
        Ok(true)
    }

    fn is_partition_active(&self, _caller_id: &str, _tess_id: &str) -> bool {
        self.partition_live.lock().unwrap().unwrap_or(true)
    }
}

fn call_ctx<C: Call>(
    svc: &TessService,
    member: &str,
    via_grant: bool,
    apply_exec: Option<&ApplyExecFn<'_>>,
    params: &C::Params,
) -> Result<C::Result, ApiError> {
    let req = Request::new::<C>(1, params).unwrap();
    let stop = AtomicBool::new(false);
    let resp = svc
        .dispatch_with_context(Some(member), via_grant, &stop, apply_exec, &req)
        .expect("tess service handles method");
    match resp.outcome {
        Outcome::Result(val) => Ok(serde_json::from_value(val).unwrap()),
        Outcome::Error(err) => Err(err),
    }
}

fn add_person(office: &Office, owner: &str, team_id: &str, name: &str, handle: &str) -> String {
    let id = next_id();
    office
        .submit_org(
            owner,
            OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: next_id(),
                member_id: id.clone(),
                name: name.into(),
                handle: Some(handle.into()),
                home_team_id: Some(team_id.into()),
                contact: None,
            }),
        )
        .unwrap()
        .unwrap();
    id
}

fn build_real_apply_executor(
    office: Arc<Office>,
    exec_calls: Arc<AtomicUsize>,
) -> impl Fn(&str, &Request) -> Response + Send + Sync {
    let tasks = Arc::new(TaskService::new(
        Arc::clone(&office),
        Arc::new(LiveEvents::new(Box::new(|| {}))),
        None,
    ));
    move |caller_id: &str, req: &Request| {
        exec_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(resp) = tasks.dispatch(Some(caller_id), req) {
            return resp;
        }
        if req.method == "org.snapshot" {
            let snap = office.org_snapshot().unwrap();
            return Response {
                id: req.id,
                outcome: Outcome::Result(serde_json::to_value(snap).unwrap()),
                waiting_messages: None,
            };
        }
        if req.method == "who" {
            let who = office.who(&Default::default()).unwrap().unwrap();
            return Response {
                id: req.id,
                outcome: Outcome::Result(serde_json::to_value(who).unwrap()),
                waiting_messages: None,
            };
        }
        if let Ok(Some(cmd)) = OrgCommand::parse(req) {
            return match office.submit_org(caller_id, cmd).unwrap() {
                Ok(accepted) => Response {
                    id: req.id,
                    outcome: Outcome::Result(serde_json::to_value(accepted).unwrap()),
                    waiting_messages: None,
                },
                Err(err) => Response::error(req.id, err),
            };
        }
        Response::error(
            req.id,
            ApiError::new(
                ErrorCode::UnknownMethod,
                format!("unexpected test executor method `{}`", req.method),
            ),
        )
    }
}

fn ask_params(prompt: &str) -> TessAskParams {
    TessAskParams {
        prompt: prompt.into(),
        viewport_context: None,
    }
}

fn events_params(turn_id: &str, after_seq: Option<u64>) -> TessTurnEventsParams {
    TessTurnEventsParams {
        turn_id: turn_id.into(),
        after_seq,
        wait_ms: Some(0),
    }
}

fn apply_params(suggestion_id: &str, expected_revision: Option<u64>) -> SuggestionApplyParams {
    SuggestionApplyParams {
        suggestion_id: suggestion_id.into(),
        expected_revision,
        if_match: None,
    }
}

fn build_streaming_harness() -> (
    Harness,
    Arc<FakeRuntime>,
    Arc<TessService>,
    SessionStreamHub,
) {
    let h = harness();
    let tess_id = h.tess.clone();
    let owner_id = h.owner.clone();
    let rt = FakeRuntime::ready();
    let svc = Arc::new(
        TessService::new(
            Arc::clone(&h.office),
            Arc::new(move |member| (member == tess_id).then(|| owner_id.clone())),
        )
        .with_runtime(Arc::clone(&rt) as Arc<dyn TessRuntimePort>),
    );
    let hub = SessionStreamHub::new();
    assert!(follow_tess_stream(&svc, &h.office, &hub));
    (h, rt, svc, hub)
}

#[test]
fn tess_ask_returns_honest_fallback_when_unconfigured() {
    let h = harness();
    let fb =
        call_ctx::<TessAsk>(&h.svc, &h.owner, true, None, &ask_params("Summarize tasks")).unwrap();
    assert_eq!(fb.status, TessAskStatus::Fallback);
    assert!(fb.turn_id.is_none());
    assert_eq!(fb.partition, format!("tess:ask:{}", h.owner));
    assert!(fb
        .unavailable_reason
        .as_deref()
        .is_some_and(|r| !r.is_empty()));

    let unready_svc = TessService::new(Arc::clone(&h.office), Arc::new(|_| None))
        .with_runtime(FakeRuntime::unavailable("No provider connection bound."));
    let fb2 =
        call_ctx::<TessAsk>(&unready_svc, &h.owner, true, None, &ask_params("Summarize")).unwrap();
    assert_eq!(fb2.status, TessAskStatus::Fallback);
    assert_eq!(
        fb2.unavailable_reason.as_deref(),
        Some("No provider connection bound.")
    );
}

#[test]
fn tess_ask_validates_viewport_and_caller_auth() {
    let h = harness();
    let rt = FakeRuntime::ready();
    let svc = TessService::new(Arc::clone(&h.office), Arc::new(|_| None))
        .with_runtime(Arc::clone(&rt) as Arc<dyn TessRuntimePort>);

    let started = call_ctx::<TessAsk>(
        &svc,
        &h.owner,
        true,
        None,
        &TessAskParams {
            prompt: "What blocks this task?".into(),
            viewport_context: Some(TessViewportContext {
                route: Some(format!("/tasks/{}", h.task_key)),
                route_label: Some("Task detail <<<inject>>>".into()),
                entity_kind: Some(UiEntityKind::Task),
                entity_id: Some(h.task_key.clone()),
                entity_title: Some("Ship Tess foundations".into()),
                decision_id: None,
                filters: BTreeMap::from([("status".into(), "blocked".into())]),
            }),
        },
    )
    .unwrap();
    assert_eq!(started.status, TessAskStatus::Streaming);
    assert_eq!(started.session_id.as_deref(), Some("sess-1"));
    let instr = rt.started.lock().unwrap()[0].2.clone();
    assert!(
        instr.find("What blocks this task?").unwrap()
            < instr.find("<<<UNTRUSTED_VIEWPORT_CONTEXT").unwrap()
    );
    assert!(instr.ends_with("<<<END_UNTRUSTED_VIEWPORT_CONTEXT>>>"));
    assert!(!instr.contains("<<<inject>>>"));

    let big_vp = TessAskParams {
        prompt: "Check".into(),
        viewport_context: Some(TessViewportContext {
            route: Some("/tasks".into()),
            entity_title: Some("x".repeat(900)),
            ..TessViewportContext::default()
        }),
    };
    assert_eq!(
        call_ctx::<TessAsk>(&svc, &h.owner, true, None, &big_vp)
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        call_ctx::<TessAsk>(&svc, &h.owner, false, None, &ask_params("Hi"))
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        call_ctx::<TessAsk>(&svc, &h.tess, true, None, &ask_params("Hi"))
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );

    let ex = add_person(&h.office, &h.owner, &h.team_id, "Ex Member", "exmember");
    h.office
        .submit_org(
            &h.owner,
            OrgCommand::MemberRemove(MemberRemoveParams {
                command_id: next_id(),
                member_id: ex.clone(),
                expected_version: 1,
            }),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        call_ctx::<TessAsk>(&svc, &ex, true, None, &ask_params("Still here?"))
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
}

#[test]
fn tess_ask_rejects_concurrent_turn_and_surfaces_crashed_session_error() {
    let (h, rt, svc, _hub) = build_streaming_harness();
    let turn_id = call_ctx::<TessAsk>(&svc, &h.owner, true, None, &ask_params("Turn 1"))
        .unwrap()
        .turn_id
        .unwrap();
    // Second concurrent Ask Tess from the same caller fails closed with Conflict (no cross-wiring).
    assert_eq!(
        call_ctx::<TessAsk>(&svc, &h.owner, true, None, &ask_params("Turn 2"))
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    // When the underlying runtime session dies without emitting TurnEnded, polling transitions
    // the active turn to TurnError + done = true instead of stranding SSE forever.
    rt.set_live(false);
    let page = call_ctx::<TessTurnEvents>(
        &svc,
        &h.owner,
        true,
        None,
        &events_params(&turn_id, Some(1)),
    )
    .unwrap();
    assert!(page.done);
    assert!(page
        .events
        .iter()
        .any(|e| matches!(e.event, TessTurnEventKind::TurnError { .. })));
    // Once terminated, the caller can start a new turn cleanly.
    rt.set_live(true);
    let next = call_ctx::<TessAsk>(&svc, &h.owner, true, None, &ask_params("Turn 3")).unwrap();
    assert_eq!(next.status, TessAskStatus::Streaming);
}

#[test]
fn tess_stream_delivers_resumes_and_isolates_three_roles() {
    let (h, _rt, svc, hub) = build_streaming_harness();
    let pm = add_person(&h.office, &h.owner, &h.team_id, "Pat PM", "patpm");
    let owner_turn = call_ctx::<TessAsk>(&svc, &h.owner, true, None, &ask_params("Owner q"))
        .unwrap()
        .turn_id
        .unwrap();
    let bob_turn = call_ctx::<TessAsk>(&svc, &h.bob, true, None, &ask_params("Bob q"))
        .unwrap()
        .turn_id
        .unwrap();

    for other in [pm.as_str(), h.bob.as_str()] {
        assert_eq!(
            call_ctx::<TessTurnEvents>(&svc, other, true, None, &events_params(&owner_turn, None))
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            call_ctx::<TessTurnCancel>(
                &svc,
                other,
                true,
                None,
                &TessTurnCancelParams {
                    turn_id: owner_turn.clone()
                },
            )
            .unwrap_err()
            .code,
            ErrorCode::NotFound
        );
    }

    let org_id = h.office.info().org_id.clone();
    let publish = |emp: &str, part: &str, event: SessionStreamEvent| {
        hub.publish(&SessionStreamFrame {
            key: SessionStreamKey {
                org_id: org_id.clone(),
                employee_id: emp.to_owned(),
                partition: Some(part.to_owned()),
            },
            at: "2026-11-27T07:00:00.000Z".into(),
            event,
        });
    };
    let owner_part = format!("tess:ask:{}", h.owner);
    publish(
        &h.tess,
        "tess:wake",
        SessionStreamEvent::Delta {
            text: "WAKE_SECRET".into(),
        },
    );
    publish(
        &h.bob,
        &owner_part,
        SessionStreamEvent::Delta {
            text: "SPOOF_SECRET".into(),
        },
    );
    publish(
        &h.tess,
        &owner_part,
        SessionStreamEvent::Activity(SessionActivity {
            phase: Some(ActivityPhase::RunningTool),
            active_tool_name: Some("tasks.list".into()),
            active_tool_started_at: None,
            supports_steer: false,
        }),
    );
    publish(
        &h.tess,
        &owner_part,
        SessionStreamEvent::Delta {
            text: "Checking tasks...".into(),
        },
    );
    call_ctx::<UiPresent>(
        &svc,
        &h.tess,
        false,
        None,
        &UiPresentParams {
            title: Some("Summary".into()),
            follow_ups: vec![UiFollowUp {
                label: "Details".into(),
                prompt: "Show details".into(),
            }],
            ..UiPresentParams::default()
        },
    )
    .unwrap();
    call_ctx::<SuggestionCreate>(
        &svc,
        &h.tess,
        false,
        None,
        &SuggestionCreateParams {
            command: "task update".into(),
            input: map(json!({"task": h.task_key, "title": "Next title"})),
            summary: "Update task title".into(),
            rationale: None,
            expected_revision: Some(h.task_revision),
            ttl_seconds: Some(300),
            dry_run: false,
        },
    )
    .unwrap();
    publish(
        &h.tess,
        &owner_part,
        SessionStreamEvent::TurnEnded {
            outcome: TurnEnd::Completed,
        },
    );

    let all = call_ctx::<TessTurnEvents>(
        &svc,
        &h.owner,
        true,
        None,
        &events_params(&owner_turn, Some(0)),
    )
    .unwrap();
    assert!(all.done && !all.lagged);
    let encoded = serde_json::to_string(&all.events).unwrap();
    assert!(!encoded.contains("WAKE_SECRET") && !encoded.contains("SPOOF_SECRET"));
    assert!(all.events.iter().any(|e| matches!(&e.event, TessTurnEventKind::TextDelta { delta } if delta == "Checking tasks...")));
    assert!(all
        .events
        .iter()
        .any(|e| matches!(e.event, TessTurnEventKind::UiBlock { .. })));
    assert!(all
        .events
        .iter()
        .any(|e| matches!(e.event, TessTurnEventKind::SuggestionCreated { .. })));
    let mid_seq = all.events[2].seq;
    let resumed = call_ctx::<TessTurnEvents>(
        &svc,
        &h.owner,
        true,
        None,
        &events_params(&owner_turn, Some(mid_seq)),
    )
    .unwrap();
    assert_eq!(resumed.events.len(), all.events.len() - 3);
    assert!(resumed.events.iter().all(|e| e.seq > mid_seq));

    let bob_ev =
        call_ctx::<TessTurnEvents>(&svc, &h.bob, true, None, &events_params(&bob_turn, Some(0)))
            .unwrap();
    assert!(!bob_ev.done && bob_ev.events.len() == 1);
}

#[test]
fn tess_cancel_and_session_reset_are_strictly_caller_scoped() {
    let (h, rt, svc, _hub) = build_streaming_harness();
    let owner_turn = call_ctx::<TessAsk>(&svc, &h.owner, true, None, &ask_params("Owner turn"))
        .unwrap()
        .turn_id
        .unwrap();
    let bob_turn = call_ctx::<TessAsk>(&svc, &h.bob, true, None, &ask_params("Bob turn"))
        .unwrap()
        .turn_id
        .unwrap();

    let cancelled = call_ctx::<TessTurnCancel>(
        &svc,
        &h.bob,
        true,
        None,
        &TessTurnCancelParams {
            turn_id: bob_turn.clone(),
        },
    )
    .unwrap();
    assert!(cancelled.cancelled);
    assert_eq!(
        *rt.cancelled.lock().unwrap(),
        vec![(h.bob.clone(), h.tess.clone())]
    );
    let bob_after =
        call_ctx::<TessTurnEvents>(&svc, &h.bob, true, None, &events_params(&bob_turn, Some(1)))
            .unwrap();
    assert!(bob_after.done);
    assert!(matches!(
        bob_after.events.last().unwrap().event,
        TessTurnEventKind::TurnDone {
            outcome: TessTurnOutcome::Cancelled,
            ..
        }
    ));

    let reset =
        call_ctx::<TessSessionReset>(&svc, &h.bob, true, None, &TessSessionResetParams {}).unwrap();
    assert!(reset.reset && reset.stopped_live_session);
    assert_eq!(
        *rt.resets.lock().unwrap(),
        vec![(h.bob.clone(), h.tess.clone())]
    );

    let owner_ev = call_ctx::<TessTurnEvents>(
        &svc,
        &h.owner,
        true,
        None,
        &events_params(&owner_turn, Some(0)),
    )
    .unwrap();
    assert!(!owner_ev.done);

    call_ctx::<TessAsk>(&svc, &h.bob, true, None, &ask_params("Fresh question")).unwrap();
    assert!(rt
        .started
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .2
        .starts_with("[Session reset by caller:"));
}

#[test]
fn suggestion_apply_enforces_human_grant_isolation_and_cas() {
    let h = harness();
    let pm = add_person(&h.office, &h.owner, &h.team_id, "Pat PM", "patpm");
    let exec_calls = Arc::new(AtomicUsize::new(0));
    let exec = build_real_apply_executor(Arc::clone(&h.office), Arc::clone(&exec_calls));
    let exec_ref: &ApplyExecFn<'_> = &exec;

    let prop = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "task update".into(),
                input: map(json!({"task": h.task_key, "title": "Applied by Owner"})),
                summary: "Rename task".into(),
                rationale: Some("Clearer milestone name".into()),
                expected_revision: Some(h.task_revision),
                ttl_seconds: Some(300),
                dry_run: false,
            },
        )
        .unwrap()
        .suggestion;

    assert_eq!(
        call_ctx::<SuggestionApply>(
            &h.svc,
            &h.tess,
            true,
            Some(exec_ref),
            &apply_params(&prop.id, Some(h.task_revision))
        )
        .unwrap_err()
        .code,
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        call_ctx::<SuggestionApply>(
            &h.svc,
            &h.owner,
            false,
            Some(exec_ref),
            &apply_params(&prop.id, Some(h.task_revision))
        )
        .unwrap_err()
        .code,
        ErrorCode::NotAuthorized
    );
    for other in [pm.as_str(), h.bob.as_str()] {
        assert_eq!(
            call_ctx::<SuggestionApply>(
                &h.svc,
                other,
                true,
                Some(exec_ref),
                &apply_params(&prop.id, Some(h.task_revision))
            )
            .unwrap_err()
            .code,
            ErrorCode::NotFound
        );
    }

    let concurrent: TasksUpdateParams = serde_json::from_value(json!({
        "command_id": next_id(),
        "task": h.task_key,
        "expected_revision": h.task_revision,
        "title": "Concurrent edit",
    }))
    .unwrap();
    h.office.task_update(&h.owner, concurrent).unwrap().unwrap();
    assert_eq!(
        call_ctx::<SuggestionApply>(
            &h.svc,
            &h.owner,
            true,
            Some(exec_ref),
            &apply_params(&prop.id, Some(h.task_revision))
        )
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
    assert_eq!(exec_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn suggestion_apply_executes_once_with_dual_attribution_and_rejects_replay() {
    let h = harness();
    let exec_calls = Arc::new(AtomicUsize::new(0));
    let exec = build_real_apply_executor(Arc::clone(&h.office), Arc::clone(&exec_calls));
    let exec_ref: &ApplyExecFn<'_> = &exec;

    let prop = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "task update".into(),
                input: map(json!({"task": h.task_key, "title": "Applied cleanly"})),
                summary: "Rename task cleanly".into(),
                rationale: None,
                expected_revision: Some(h.task_revision),
                ttl_seconds: Some(300),
                dry_run: false,
            },
        )
        .unwrap()
        .suggestion;

    let applied = call_ctx::<SuggestionApply>(
        &h.svc,
        &h.owner,
        true,
        Some(exec_ref),
        &SuggestionApplyParams {
            suggestion_id: prop.id.clone(),
            expected_revision: None,
            if_match: Some(format!("rev-{}", h.task_revision)),
        },
    )
    .unwrap();
    assert_eq!(applied.suggestion.status, SuggestionStatus::Applied);
    assert_eq!(applied.attribution.actor_member_id, h.owner);
    assert_eq!(applied.attribution.proposed_by_member_id, h.tess);
    assert_eq!(
        applied.receipt.entity_href.as_deref(),
        Some(format!("/work/{}", h.task_key).as_str())
    );
    assert_eq!(
        h.office
            .task_inspect(&h.task_key)
            .unwrap()
            .unwrap()
            .task
            .title,
        "Applied cleanly"
    );

    let stored = h
        .call::<SuggestionInspect>(
            &h.owner,
            &SuggestionInspectParams {
                suggestion_id: prop.id.clone(),
            },
        )
        .unwrap()
        .suggestion;
    assert_eq!(stored.status, SuggestionStatus::Applied);

    let before_replay = exec_calls.load(Ordering::SeqCst);
    assert_eq!(
        call_ctx::<SuggestionApply>(
            &h.svc,
            &h.owner,
            true,
            Some(exec_ref),
            &apply_params(&prop.id, None)
        )
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
    assert_eq!(exec_calls.load(Ordering::SeqCst), before_replay);
    assert_eq!(
        h.call::<SuggestionDismiss>(
            &h.owner,
            &SuggestionDismissParams {
                suggestion_id: prop.id,
                reason: None
            }
        )
        .unwrap_err(),
        ErrorCode::Conflict
    );

    let team_prop = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "team create".into(),
                input: map(json!({"name": "Applied Ops", "parent_team_id": h.team_id})),
                summary: "Create Applied Ops team".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: Some(300),
                dry_run: false,
            },
        )
        .unwrap()
        .suggestion;
    let applied_team = call_ctx::<SuggestionApply>(
        &h.svc,
        &h.owner,
        true,
        Some(exec_ref),
        &apply_params(&team_prop.id, None),
    )
    .unwrap();
    assert_eq!(applied_team.suggestion.status, SuggestionStatus::Applied);
    assert!(h
        .office
        .org_snapshot()
        .unwrap()
        .teams
        .iter()
        .any(|t| t.name == "Applied Ops"));
}

#[test]
fn suggestion_apply_concurrent_race_and_panic_guard_reset() {
    let h = harness();
    let prop = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "task update".into(),
                input: map(json!({"task": h.task_key, "title": "After panic recovery"})),
                summary: "Rename task".into(),
                rationale: None,
                expected_revision: Some(h.task_revision),
                ttl_seconds: Some(300),
                dry_run: false,
            },
        )
        .unwrap()
        .suggestion;

    // 1) A concurrent second apply while first is in-flight fails closed with Conflict.
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    let svc = Arc::new(h.svc);
    let owner = h.owner.clone();
    let prop_id = prop.id.clone();
    let rev = h.task_revision;
    let svc_t1 = Arc::clone(&svc);
    let owner_t1 = owner.clone();
    let id_t1 = prop_id.clone();

    let blocking_exec = move |_caller: &str, req: &Request| -> Response {
        let _ = entered_tx.send(());
        let _ = release_rx.lock().unwrap().recv();
        Response::error(
            req.id,
            ApiError::new(ErrorCode::Unavailable, "simulated abort"),
        )
    };
    let t1 = std::thread::spawn(move || {
        let exec_ref: &ApplyExecFn<'_> = &blocking_exec;
        call_ctx::<SuggestionApply>(
            &svc_t1,
            &owner_t1,
            true,
            Some(exec_ref),
            &apply_params(&id_t1, Some(rev)),
        )
    });
    entered_rx.recv().unwrap();
    let noop_exec = |_c: &str, r: &Request| {
        Response::error(r.id, ApiError::new(ErrorCode::Unavailable, "nope"))
    };
    assert_eq!(
        call_ctx::<SuggestionApply>(
            &svc,
            &owner,
            true,
            Some(&noop_exec),
            &apply_params(&prop_id, Some(rev))
        )
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
    release_tx.send(()).unwrap();
    assert_eq!(t1.join().unwrap().unwrap_err().code, ErrorCode::Unavailable);

    // 2) A panicking executor unwinds ApplyInFlightGuard so the proposal returns to Pending cleanly.
    let panicking = |_c: &str, _r: &Request| -> Response { panic!("simulated executor panic") };
    let panic_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        call_ctx::<SuggestionApply>(
            &svc,
            &owner,
            true,
            Some(&panicking),
            &apply_params(&prop_id, Some(rev)),
        )
    }));
    assert!(panic_res.is_err());

    let exec_calls = Arc::new(AtomicUsize::new(0));
    let real_exec = build_real_apply_executor(Arc::clone(&h.office), exec_calls);
    let recovered = call_ctx::<SuggestionApply>(
        &svc,
        &owner,
        true,
        Some(&real_exec),
        &apply_params(&prop_id, Some(rev)),
    )
    .unwrap();
    assert_eq!(recovered.suggestion.status, SuggestionStatus::Applied);
}

#[test]
fn suggestion_create_blocks_local_attach_and_validates_task_project_and_team_ids() {
    let h = harness();
    // Local `attach` paths are unconditionally rejected in suggestions.
    let err = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "task comment".into(),
                input: map(
                    json!({"task": h.task_key, "body": "see file", "attach": ["/etc/passwd"]}),
                ),
                summary: "Comment with local file".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: Some(300),
                dry_run: false,
            },
        )
        .unwrap_err();
    assert_eq!(err, ErrorCode::InvalidRequest);

    // Unknown `attachment_id` fails preflight.
    let err = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "task comment".into(),
                input: map(json!({"task": h.task_key, "body": "see file", "attachment_id": ["01999999-9999-7999-8999-999999999999"]})),
                summary: "Comment with missing attachment".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: Some(300),
                dry_run: false,
            },
        )
        .unwrap_err();
    assert!(matches!(
        err,
        ErrorCode::NotFound | ErrorCode::NotAuthorized
    ));

    // `tasks.create` validates real CLI schema keys `project_id` and `team_id`.
    let bad_proj = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "task create".into(),
                input: map(json!({"title": "Bad proj", "project_id": "nonexistent-project"})),
                summary: "Create task in nonexistent project".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: Some(300),
                dry_run: false,
            },
        )
        .unwrap_err();
    assert_eq!(bad_proj, ErrorCode::NotFound);

    let bad_team = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "task create".into(),
                input: map(json!({"title": "Bad team", "team_id": "nonexistent-team"})),
                summary: "Create task in nonexistent team".into(),
                rationale: None,
                expected_revision: None,
                ttl_seconds: Some(300),
                dry_run: false,
            },
        )
        .unwrap_err();
    assert_eq!(bad_team, ErrorCode::NotFound);
}
