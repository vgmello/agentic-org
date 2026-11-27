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
    TessSessionResetParams, TessToolState, TessTurnCancel, TessTurnCancelParams, TessTurnEventKind,
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

#[test]
fn tess_ask_returns_honest_fallback_when_unconfigured_and_validates_viewport_and_caller() {
    let h = harness();

    // No runtime attached -> structured honest fallback, never a raw error.
    let fb = call_ctx::<TessAsk>(
        &h.svc,
        &h.owner,
        true,
        None,
        &TessAskParams {
            prompt: "Summarize open tasks".into(),
            viewport_context: None,
        },
    )
    .unwrap();
    assert_eq!(fb.status, TessAskStatus::Fallback);
    assert!(fb.turn_id.is_none());
    assert_eq!(fb.partition, format!("tess:ask:{}", h.owner));
    assert!(fb
        .unavailable_reason
        .as_deref()
        .is_some_and(|r| !r.is_empty()));

    // Runtime attached but provider/readiness not ready -> honest fallback with reason.
    let unready_svc = TessService::new(Arc::clone(&h.office), Arc::new(|_| None))
        .with_runtime(FakeRuntime::unavailable("No provider connection bound."));
    let fb2 = call_ctx::<TessAsk>(
        &unready_svc,
        &h.owner,
        true,
        None,
        &TessAskParams {
            prompt: "Summarize open tasks".into(),
            viewport_context: None,
        },
    )
    .unwrap();
    assert_eq!(fb2.status, TessAskStatus::Fallback);
    assert_eq!(
        fb2.unavailable_reason.as_deref(),
        Some("No provider connection bound.")
    );

    // Ready runtime accepts global and item-scoped viewport context (<= 800 B, appended LAST).
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
    assert!(started.turn_id.is_some());
    assert_eq!(started.session_id.as_deref(), Some("sess-1"));
    assert_eq!(started.partition, format!("tess:ask:{}", h.owner));

    let calls = rt.started.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, h.owner);
    assert_eq!(calls[0].1, h.tess);
    let instr = &calls[0].2;
    let prompt_pos = instr.find("What blocks this task?").unwrap();
    let fence_pos = instr.find("<<<UNTRUSTED_VIEWPORT_CONTEXT").unwrap();
    assert!(prompt_pos < fence_pos);
    assert!(instr.ends_with("<<<END_UNTRUSTED_VIEWPORT_CONTEXT>>>"));
    assert!(!instr.contains("<<<inject>>>"));
    drop(calls);

    // Oversized (> 800 B) viewport context rejected cleanly without touching runtime.
    let err = call_ctx::<TessAsk>(
        &svc,
        &h.owner,
        true,
        None,
        &TessAskParams {
            prompt: "Check".into(),
            viewport_context: Some(TessViewportContext {
                route: Some("/tasks".into()),
                entity_title: Some("x".repeat(900)),
                ..TessViewportContext::default()
            }),
        },
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);

    // Non-grant human caller rejected.
    let err = call_ctx::<TessAsk>(
        &svc,
        &h.owner,
        false,
        None,
        &TessAskParams {
            prompt: "Hello".into(),
            viewport_context: None,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotAuthorized);

    // Agent / Tess caller rejected even with via_grant=true.
    let err = call_ctx::<TessAsk>(
        &svc,
        &h.tess,
        true,
        None,
        &TessAskParams {
            prompt: "Recursive ask".into(),
            viewport_context: None,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotAuthorized);

    // Offboarded / inactive human caller rejected.
    let ex_member = add_person(&h.office, &h.owner, &h.team_id, "Ex Member", "exmember");
    h.office
        .submit_org(
            &h.owner,
            OrgCommand::MemberRemove(MemberRemoveParams {
                command_id: next_id(),
                member_id: ex_member.clone(),
                expected_version: 1,
            }),
        )
        .unwrap()
        .unwrap();
    let err = call_ctx::<TessAsk>(
        &svc,
        &ex_member,
        true,
        None,
        &TessAskParams {
            prompt: "Still here?".into(),
            viewport_context: None,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotAuthorized);
}

#[test]
fn tess_stream_resume_cancel_reset_and_three_role_cross_caller_isolation() {
    let h: Harness = harness();
    let pm = add_person(&h.office, &h.owner, &h.team_id, "Pat PM", "patpm");
    let acting_for = Arc::new(Mutex::new(Some(h.owner.clone())));
    let slot = Arc::clone(&acting_for);
    let tess_id = h.tess.clone();
    let rt = FakeRuntime::ready();
    let svc = Arc::new(
        TessService::new(
            Arc::clone(&h.office),
            Arc::new(move |member| {
                (member == tess_id)
                    .then(|| slot.lock().unwrap().clone())
                    .flatten()
            }),
        )
        .with_runtime(Arc::clone(&rt) as Arc<dyn TessRuntimePort>),
    );

    let hub = SessionStreamHub::new();
    assert!(follow_tess_stream(&svc, &h.office, &hub));

    // Owner and Bob each start an Ask Tess turn.
    let owner_turn = call_ctx::<TessAsk>(
        &svc,
        &h.owner,
        true,
        None,
        &TessAskParams {
            prompt: "Owner question".into(),
            viewport_context: None,
        },
    )
    .unwrap()
    .turn_id
    .unwrap();
    let bob_turn = call_ctx::<TessAsk>(
        &svc,
        &h.bob,
        true,
        None,
        &TessAskParams {
            prompt: "Bob question".into(),
            viewport_context: None,
        },
    )
    .unwrap()
    .turn_id
    .unwrap();

    // Cross-caller isolation: PM and Bob CANNOT read or cancel Owner's turn.
    for other in [pm.as_str(), h.bob.as_str()] {
        assert_eq!(
            call_ctx::<TessTurnEvents>(
                &svc,
                other,
                true,
                None,
                &TessTurnEventsParams {
                    turn_id: owner_turn.clone(),
                    after_seq: None,
                    wait_ms: Some(0),
                },
            )
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
                    turn_id: owner_turn.clone(),
                },
            )
            .unwrap_err()
            .code,
            ErrorCode::NotFound
        );
    }

    // Stream frames on Owner's partition + noise on tess:wake, Bob's partition, and non-Tess agent.
    let org_id = h.office.info().org_id.clone();
    let owner_key = SessionStreamKey {
        org_id: org_id.clone(),
        employee_id: h.tess.clone(),
        partition: Some(format!("tess:ask:{}", h.owner)),
    };
    let wake_key = SessionStreamKey {
        org_id: org_id.clone(),
        employee_id: h.tess.clone(),
        partition: Some("tess:wake".into()),
    };
    let spoof_key = SessionStreamKey {
        org_id,
        employee_id: h.bob.clone(),
        partition: Some(format!("tess:ask:{}", h.owner)),
    };

    let publish = |key: &SessionStreamKey, event: SessionStreamEvent| {
        hub.publish(&SessionStreamFrame {
            key: key.clone(),
            at: "2026-11-27T07:00:00.000Z".into(),
            event,
        });
    };

    publish(
        &wake_key,
        SessionStreamEvent::Delta {
            text: "WAKE_SECRET".into(),
        },
    );
    publish(
        &spoof_key,
        SessionStreamEvent::Delta {
            text: "SPOOF_SECRET".into(),
        },
    );
    publish(
        &owner_key,
        SessionStreamEvent::Activity(SessionActivity {
            phase: Some(ActivityPhase::RunningTool),
            active_tool_name: Some("tasks.list".into()),
            active_tool_started_at: None,
            supports_steer: false,
        }),
    );
    publish(
        &owner_key,
        SessionStreamEvent::Delta {
            text: "Checking tasks...".into(),
        },
    );

    // In-turn `ui.present` and `suggestion.create` publish into Owner's active turn.
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
        &owner_key,
        SessionStreamEvent::TurnEnded {
            outcome: TurnEnd::Completed,
        },
    );

    // Owner polls from seq 0, then resumes with `after_seq` (`Last-Event-ID`).
    let all = call_ctx::<TessTurnEvents>(
        &svc,
        &h.owner,
        true,
        None,
        &TessTurnEventsParams {
            turn_id: owner_turn.clone(),
            after_seq: Some(0),
            wait_ms: Some(0),
        },
    )
    .unwrap();
    assert!(all.done);
    assert!(!all.lagged);
    let encoded_all = serde_json::to_string(&all.events).unwrap();
    assert!(!encoded_all.contains("WAKE_SECRET"));
    assert!(!encoded_all.contains("SPOOF_SECRET"));
    assert!(matches!(
        all.events[0].event,
        TessTurnEventKind::TurnStarted { .. }
    ));
    assert!(all.events.iter().any(|e| matches!(
        &e.event,
        TessTurnEventKind::ToolProgress {
            label,
            state: TessToolState::Running,
            ..
        } if label == "tasks.list"
    )));
    assert!(all.events.iter().any(|e| matches!(
        &e.event,
        TessTurnEventKind::TextDelta { delta } if delta == "Checking tasks..."
    )));
    assert!(all
        .events
        .iter()
        .any(|e| matches!(e.event, TessTurnEventKind::UiBlock { .. })));
    assert!(all
        .events
        .iter()
        .any(|e| matches!(e.event, TessTurnEventKind::SuggestionCreated { .. })));
    assert!(matches!(
        &all.events.last().unwrap().event,
        TessTurnEventKind::TurnDone {
            outcome: TessTurnOutcome::Completed,
            follow_ups,
            ..
        } if follow_ups == &vec!["Show details".to_owned()]
    ));

    // Monotonic seq + `Last-Event-ID` resume returns strictly higher sequences.
    let mid_seq = all.events[2].seq;
    let resumed = call_ctx::<TessTurnEvents>(
        &svc,
        &h.owner,
        true,
        None,
        &TessTurnEventsParams {
            turn_id: owner_turn,
            after_seq: Some(mid_seq),
            wait_ms: Some(0),
        },
    )
    .unwrap();
    assert_eq!(resumed.events.len(), all.events.len() - 3);
    assert!(resumed.events.iter().all(|e| e.seq > mid_seq));

    // Bob's active turn never saw Owner's frames and is still active.
    let bob_ev = call_ctx::<TessTurnEvents>(
        &svc,
        &h.bob,
        true,
        None,
        &TessTurnEventsParams {
            turn_id: bob_turn.clone(),
            after_seq: Some(0),
            wait_ms: Some(0),
        },
    )
    .unwrap();
    assert!(!bob_ev.done);
    assert_eq!(bob_ev.events.len(), 1);

    // Bob cancels his own active turn; runtime receives cancel only for Bob.
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
    assert_eq!(cancelled.partition, format!("tess:ask:{}", h.bob));
    assert_eq!(
        *rt.cancelled.lock().unwrap(),
        vec![(h.bob.clone(), h.tess.clone())]
    );
    let bob_after = call_ctx::<TessTurnEvents>(
        &svc,
        &h.bob,
        true,
        None,
        &TessTurnEventsParams {
            turn_id: bob_turn,
            after_seq: Some(1),
            wait_ms: Some(0),
        },
    )
    .unwrap();
    assert!(bob_after.done);
    assert!(matches!(
        bob_after.events.last().unwrap().event,
        TessTurnEventKind::TurnDone {
            outcome: TessTurnOutcome::Cancelled,
            ..
        }
    ));

    // Owner starts a new turn; Bob resetting his own session NEVER stops Owner's active turn!
    let owner_turn2 = call_ctx::<TessAsk>(
        &svc,
        &h.owner,
        true,
        None,
        &TessAskParams {
            prompt: "Owner turn 2".into(),
            viewport_context: None,
        },
    )
    .unwrap()
    .turn_id
    .unwrap();
    let reset =
        call_ctx::<TessSessionReset>(&svc, &h.bob, true, None, &TessSessionResetParams {}).unwrap();
    assert!(reset.reset);
    assert!(reset.stopped_live_session);
    assert_eq!(reset.partition, format!("tess:ask:{}", h.bob));
    assert_eq!(
        *rt.resets.lock().unwrap(),
        vec![(h.bob.clone(), h.tess.clone())]
    );

    // Owner's second turn is still active; Bob's next turn carries the fresh-session reset note.
    let owner_turn2_ev = call_ctx::<TessTurnEvents>(
        &svc,
        &h.owner,
        true,
        None,
        &TessTurnEventsParams {
            turn_id: owner_turn2,
            after_seq: Some(0),
            wait_ms: Some(0),
        },
    )
    .unwrap();
    assert!(!owner_turn2_ev.done);

    call_ctx::<TessAsk>(
        &svc,
        &h.bob,
        true,
        None,
        &TessAskParams {
            prompt: "Fresh question".into(),
            viewport_context: None,
        },
    )
    .unwrap();
    let started_calls = rt.started.lock().unwrap();
    let bob_fresh = &started_calls.last().unwrap().2;
    assert!(bob_fresh.starts_with("[Session reset by caller:"));
}

#[test]
fn suggestion_apply_enforces_human_grant_isolation_cas_single_use_and_real_execution() {
    let h = harness();
    let pm = add_person(&h.office, &h.owner, &h.team_id, "Pat PM", "patpm");
    let exec_calls = Arc::new(AtomicUsize::new(0));
    let exec = build_real_apply_executor(Arc::clone(&h.office), Arc::clone(&exec_calls));
    let exec_ref: &ApplyExecFn<'_> = &exec;

    // 1) Tess creates a pinned task update proposal for Owner.
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

    // 2) Security boundaries: Tess/agent NEVER applies; non-grant human NEVER applies;
    //    other humans (PM & Bob) get NotFound (zero existence oracle).
    assert_eq!(
        call_ctx::<SuggestionApply>(
            &h.svc,
            &h.tess,
            true,
            Some(exec_ref),
            &SuggestionApplyParams {
                suggestion_id: prop.id.clone(),
                expected_revision: Some(h.task_revision),
                if_match: None,
            },
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
            &SuggestionApplyParams {
                suggestion_id: prop.id.clone(),
                expected_revision: Some(h.task_revision),
                if_match: None,
            },
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
                &SuggestionApplyParams {
                    suggestion_id: prop.id.clone(),
                    expected_revision: Some(h.task_revision),
                    if_match: None,
                },
            )
            .unwrap_err()
            .code,
            ErrorCode::NotFound
        );
    }
    assert_eq!(exec_calls.load(Ordering::SeqCst), 0);

    // 3) Concurrent mutation makes pinned expected_revision stale -> live preflight returns Conflict.
    let concurrent_update: TasksUpdateParams = serde_json::from_value(json!({
        "command_id": next_id(),
        "task": h.task_key,
        "expected_revision": h.task_revision,
        "title": "Concurrent edit",
    }))
    .unwrap();
    h.office
        .task_update(&h.owner, concurrent_update)
        .unwrap()
        .unwrap();
    let new_rev = h
        .office
        .task_inspect(&h.task_key)
        .unwrap()
        .unwrap()
        .task
        .revision;
    assert!(new_rev > h.task_revision);

    assert_eq!(
        call_ctx::<SuggestionApply>(
            &h.svc,
            &h.owner,
            true,
            Some(exec_ref),
            &SuggestionApplyParams {
                suggestion_id: prop.id.clone(),
                expected_revision: Some(h.task_revision),
                if_match: None,
            },
        )
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
    assert_eq!(exec_calls.load(Ordering::SeqCst), 0);

    // 4) Create fresh proposal at `new_rev` and apply ONCE under Owner's verified grant.
    let prop2 = h
        .call::<SuggestionCreate>(
            &h.tess,
            &SuggestionCreateParams {
                command: "task update".into(),
                input: map(json!({"task": h.task_key, "title": "Applied cleanly"})),
                summary: "Rename task cleanly".into(),
                rationale: None,
                expected_revision: Some(new_rev),
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
            suggestion_id: prop2.id.clone(),
            expected_revision: None,
            if_match: Some(format!("rev-{new_rev}")),
        },
    )
    .unwrap();
    assert_eq!(applied.suggestion.status, SuggestionStatus::Applied);
    assert_eq!(applied.attribution.actor_member_id, h.owner);
    assert_eq!(applied.attribution.proposed_by_member_id, h.tess);
    assert_eq!(
        applied.receipt.entity_href.as_deref(),
        Some(format!("/tasks/{}", h.task_key).as_str())
    );
    assert!(exec_calls.load(Ordering::SeqCst) >= 1);

    // Real DB state mutated cleanly.
    let inspected_task = h.office.task_inspect(&h.task_key).unwrap().unwrap().task;
    assert_eq!(inspected_task.title, "Applied cleanly");

    // Stored suggestion now reports Applied.
    let stored = h
        .call::<SuggestionInspect>(
            &h.owner,
            &SuggestionInspectParams {
                suggestion_id: prop2.id.clone(),
            },
        )
        .unwrap()
        .suggestion;
    assert_eq!(stored.status, SuggestionStatus::Applied);

    // 5) Single-use: second apply and post-apply dismiss both fail with Conflict (no second exec).
    let calls_before_replay = exec_calls.load(Ordering::SeqCst);
    assert_eq!(
        call_ctx::<SuggestionApply>(
            &h.svc,
            &h.owner,
            true,
            Some(exec_ref),
            &SuggestionApplyParams {
                suggestion_id: prop2.id.clone(),
                expected_revision: None,
                if_match: None,
            },
        )
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
    assert_eq!(exec_calls.load(Ordering::SeqCst), calls_before_replay);
    assert_eq!(
        h.call::<SuggestionDismiss>(
            &h.owner,
            &SuggestionDismissParams {
                suggestion_id: prop2.id,
                reason: None,
            },
        )
        .unwrap_err(),
        ErrorCode::Conflict
    );

    // 6) Owner-only org command (`team create`) executes cleanly through HostChannel `submit_org`.
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
        &SuggestionApplyParams {
            suggestion_id: team_prop.id,
            expected_revision: None,
            if_match: None,
        },
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
