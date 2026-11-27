//! A task's must-follow processes travel with every turn that assigns it (`doc-27` §1,
//! `doc-34` §3.1, Decision 4): on a fresh start and on the turn-end drain into a busy warm
//! session alike, for every task a coalesced batch assigns, and a batch whose must-follow text
//! exceeds the budget stays queued instead of reaching the agent without it.

use std::sync::mpsc;

use org_office::rules::OrgCommand;
use org_proto::org::{ProjectCreateParams, TeamCreateParams};
use org_proto::processes::{
    DocumentKindWire, DocumentStatusWire, ProcessScopeWire, ProcessWriteParams,
};
use org_proto::tasks::TasksCreateParams;

use super::lock_scope::stub_live_handle;
use super::*;
use crate::pump::PumpCommand;
use crate::router::WarmPermitOutcome;

/// A project owned by a team agent A is not on, with a must-follow standard on the owning team
/// and one on the project, and a task in it assigned to A.
struct ProjectTask {
    project_id: String,
    task_id: String,
    owner_sentinel: String,
    project_sentinel: String,
}

fn standard(fx: &WakeFixture, scope: ProcessScopeWire, key: &str, body: &str) {
    fx.office
        .process_write(
            &fx.owner_id,
            ProcessWriteParams {
                command_id: next_id(),
                document_id: None,
                kind: DocumentKindWire::Standard,
                scope,
                key: Some(key.to_owned()),
                title: format!("Rule {key}"),
                status: DocumentStatusWire::Current,
                owner_employee_id: Some(fx.owner_id.clone()),
                overridable: false,
                body_markdown: body.to_owned(),
                expected_revision: None,
            },
        )
        .unwrap()
        .unwrap();
}

fn project_task(fx: &WakeFixture, name: &str) -> ProjectTask {
    let root = fx.office.org_snapshot().unwrap().teams[0].id.clone();
    let team = next_id();
    fx.office
        .submit_org(
            &fx.owner_id,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: next_id(),
                team_id: team.clone(),
                name: format!("{name} Owners"),
                owner_team_id: Some(root),
            }),
        )
        .unwrap()
        .unwrap();
    let project_id = next_id();
    fx.office
        .submit_org(
            &fx.owner_id,
            OrgCommand::ProjectCreate(ProjectCreateParams {
                command_id: next_id(),
                project_id: project_id.clone(),
                name: name.to_owned(),
                key: None,
                description: None,
                teams: vec![],
                general_area_id: String::new(),
                general_area_name: None,
                repo: None,
                integration_branch: None,
                slug: Some(name.to_lowercase()),
                owner_team_id: Some(team.clone()),
                create_team_id: None,
                parent_team_id: None,
                contributing_team_ids: vec![],
            }),
        )
        .unwrap()
        .unwrap();
    let upper = name.to_uppercase();
    let owner_sentinel = format!("{upper}_OWNING_TEAM_SENTINEL");
    let project_sentinel = format!("{upper}_PROJECT_SENTINEL");
    let key = name.to_lowercase();
    standard(
        fx,
        ProcessScopeWire::Team { team_id: team },
        &format!("{key}-owners"),
        &owner_sentinel,
    );
    standard(
        fx,
        ProcessScopeWire::Project {
            project_id: project_id.clone(),
        },
        &format!("{key}-project"),
        &project_sentinel,
    );
    let task = fx
        .office
        .task_create(
            &fx.owner_id,
            TasksCreateParams {
                command_id: next_id(),
                task_id: next_id(),
                team_id: None,
                project_id: Some(project_id.clone()),
                assigned_team_id: None,
                assignee_id: Some(fx.agent_a.clone()),
                task_type: Some("task".into()),
                work_type: None,
                title: format!("Audit {name}"),
                body_markdown: "Check the invariants.".into(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: Some(1),
                labels: vec![],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                details: None,
                dod: None,
            },
        )
        .unwrap()
        .unwrap()
        .task;
    ProjectTask {
        project_id,
        task_id: task.id,
        owner_sentinel,
        project_sentinel,
    }
}

/// A must-follow standard on `task`'s project too long for any turn's budget.
fn overflow(fx: &WakeFixture, task: &ProjectTask) {
    standard(
        fx,
        ProcessScopeWire::Project {
            project_id: task.project_id.clone(),
        },
        "oversized",
        &"x".repeat(130 * 1024),
    );
}

fn granted() -> crate::lifecycle::WarmPermitAcquirer {
    Arc::new(|_office, _hq, _conn, _emp, _task, _digest| {
        WarmPermitOutcome::Granted(("permit-a".into(), 7))
    })
}

/// A's warm session, busy until the test ends its turn; its pump commands are captured.
fn busy_warm_session(
    fx: &WakeFixture,
    mgr: &SessionManager,
) -> (
    Arc<Mutex<crate::pump::SessionRuntimeState>>,
    mpsc::Receiver<PumpCommand>,
) {
    let office_id = fx.office.info().office_id.clone();
    let (handle, commands) =
        stub_live_handle(&office_id, &fx.agent_a, SessionState::Running, Some("c-a"));
    let state = Arc::clone(&handle.state);
    mgr.registry
        .lock()
        .unwrap()
        .live
        .insert(fx.agent_a.clone(), handle);
    (state, commands)
}

fn assign(fx: &WakeFixture, mgr: &SessionManager, task: &ProjectTask) -> WakeOutcome {
    mgr.on_task_assigned(
        &fx.agent_a,
        &fx.owner_id,
        "Owner",
        true,
        &task.task_id,
        "Audit",
    )
}

/// Asserts `turn` carries every sentinel of `tasks` after its fenced objective, one section per
/// task.
fn assert_carries_processes(turn: &str, tasks: &[&ProjectTask]) {
    let objective_end = turn
        .rfind("<<<END_UNTRUSTED_MESSAGE>>>")
        .expect("fenced objective");
    for task in tasks {
        assert!(
            turn.contains(&format!("## Processes for task {}\n", task.task_id)),
            "no process section for {}: {turn}",
            task.task_id
        );
        for sentinel in [&task.owner_sentinel, &task.project_sentinel] {
            let at = turn
                .find(sentinel.as_str())
                .unwrap_or_else(|| panic!("`{sentinel}` missing from the turn: {turn}"));
            assert!(at > objective_end, "processes follow the objective: {turn}");
        }
    }
}

#[test]
fn a_busy_warm_session_drains_every_assigned_tasks_must_follow_processes() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = Arc::new(fx.make_manager(50, Duration::from_secs(3600), Duration::from_secs(30)));
    SessionManager::attach_self(&mgr);
    let ledger = project_task(&fx, "Ledger");
    let billing = project_task(&fx, "Billing");
    let (state, commands) = busy_warm_session(&fx, &mgr);

    // Both tasks arrive mid-turn and queue behind it.
    for task in [&ledger, &billing] {
        assert_eq!(
            assign(&fx, &mgr, task),
            WakeOutcome::Queued {
                reason: WakeHoldReason::Busy
            }
        );
    }
    assert!(commands.try_recv().is_err(), "nothing reaches a busy turn");

    // The turn ends and the drain sends one coalesced turn into the warm session.
    state.lock().unwrap().state = SessionState::Idle;
    crate::lifecycle::drain_into_live_pump_with_acquirer(&mgr, &fx.agent_a, granted());
    let PumpCommand::SendTurn(turn) = commands
        .recv_timeout(Duration::from_secs(5))
        .expect("the drained turn is sent")
    else {
        panic!("expected SendTurn");
    };
    let turn = turn.text;
    assert!(turn.starts_with("You have 2 waiting items"), "{turn}");
    assert_carries_processes(&turn, &[&ledger, &billing]);
    assert!(
        mgr.store
            .read_pending_wakes(&fx.agent_a)
            .unwrap()
            .is_none_or(|q| q.items.is_empty()),
        "the sent batch leaves the queue"
    );
}

#[test]
fn a_drained_batch_over_the_must_follow_budget_stays_queued() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = Arc::new(fx.make_manager(50, Duration::from_secs(3600), Duration::from_secs(30)));
    SessionManager::attach_self(&mgr);
    let ledger = project_task(&fx, "Ledger");
    let billing = project_task(&fx, "Billing");
    // Only the second task of the batch overflows: the first one's rules alone would fit.
    overflow(&fx, &billing);
    let (state, commands) = busy_warm_session(&fx, &mgr);
    for task in [&ledger, &billing] {
        assert_eq!(
            assign(&fx, &mgr, task),
            WakeOutcome::Queued {
                reason: WakeHoldReason::Busy
            }
        );
    }

    state.lock().unwrap().state = SessionState::Idle;
    crate::lifecycle::drain_into_live_pump_with_acquirer(&mgr, &fx.agent_a, granted());
    assert!(
        commands.recv_timeout(Duration::from_millis(300)).is_err(),
        "no turn reaches the agent without its must-follow rules"
    );
    let queue = mgr.store.read_pending_wakes(&fx.agent_a).unwrap().unwrap();
    assert_eq!(queue.items.len(), 2, "the batch is kept whole");
    assert_eq!(queue.hold_reason, Some(WakeHoldReason::AdmissionRefused));

    // A fresh start of the same batch is refused the same way, and nothing launches.
    mgr.registry.lock().unwrap().live.remove(&fx.agent_a);
    assert_eq!(
        mgr.process_due_wakes_for(&fx.agent_a),
        WakeOutcome::Queued {
            reason: WakeHoldReason::AdmissionRefused
        }
    );
    assert!(fx.recorded.turns.lock().unwrap().is_empty());
    assert_eq!(
        mgr.store
            .read_pending_wakes(&fx.agent_a)
            .unwrap()
            .unwrap()
            .items
            .len(),
        2
    );
}

#[test]
fn a_fresh_start_of_a_coalesced_batch_carries_every_tasks_processes() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = Arc::new(fx.make_manager(50, Duration::from_secs(3600), Duration::from_secs(30)));
    SessionManager::attach_self(&mgr);
    let ledger = project_task(&fx, "Ledger");
    let billing = project_task(&fx, "Billing");
    mgr.set_budget_hold(Some("hold".into()));
    for task in [&ledger, &billing] {
        assert_eq!(
            assign(&fx, &mgr, task),
            WakeOutcome::Queued {
                reason: WakeHoldReason::BudgetHold
            }
        );
    }
    // Lifting the hold starts the agent with the whole batch.
    mgr.set_budget_hold(None);
    assert!(
        wait_until(
            || !fx.recorded.turns.lock().unwrap().is_empty(),
            Duration::from_secs(10)
        ),
        "the batch starts a session"
    );
    let turn = fx.recorded.turns.lock().unwrap()[0].clone();
    assert!(turn.starts_with("You have 2 waiting items"), "{turn}");
    assert_carries_processes(&turn, &[&ledger, &billing]);
    mgr.stop_all("done");
}
