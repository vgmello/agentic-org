use std::collections::BTreeMap;
use std::sync::Arc;

use org_office::git::{OfficeGitState, TaskWorkspaceRecord};
use org_office::{EngineChoice, Office, TestOffice};
use org_proto::ask::{
    AskQuestion, AskQuestionType, ChatAnswerParams, ChatAskParams, QuestionAnswer,
};
use org_proto::budgets::{
    BudgetPeriodKind, BudgetScopeKind, BudgetSetParams, BudgetStateSummary, BudgetUnit,
};
use org_proto::chat::ChatSendParams;
use org_proto::external_auth::SessionMode;
use org_proto::hiring::{HireProposalWire, HrHire, HrHireParams};
use org_proto::msg::MessagePriority;
use org_proto::org::MemberKind;
use org_proto::private_notes::{
    PrivateNoteAddParams, PrivateNoteKind, PrivateNoteListParams, PRIVATE_NOTE_CHECKPOINT_TAG,
};
use org_proto::resume::{
    AgentResume, AgentResumeParams, AgentResumeResult, ResumeAskDirection,
    RESUME_NOTE_EXCERPT_MAX_BYTES,
};
use org_proto::tasks::{TaskStatus, TasksCreateParams, TasksTransitionParams};
use org_proto::{ErrorCode, Outcome, Request};

use crate::git_calls::GitService;
use crate::live::LiveEvents;
use crate::resume_calls::ResumeService;
use crate::serve::delegation::{self, Access};

fn uuid(n: u64) -> String {
    let ms = org_office::organization_family::testkit::CREATION_AT.0;
    let mut bytes = [0u8; 16];
    bytes[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
    bytes[6] = 0x70 | ((n >> 8) as u8 & 0x0f);
    bytes[7] = (n & 0xff) as u8;
    bytes[8] = 0x80;
    bytes[15] = (n & 0xff) as u8;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

fn hire_agent(office: &Arc<Office>, owner: &str, name: &str, handle: &str, seed: u64) -> String {
    let root = office.org_snapshot().unwrap().teams[0].id.clone();
    let employee_id = uuid(seed);
    let request = Request::new::<HrHire>(
        1,
        &HrHireParams {
            command_id: uuid(seed + 1),
            request_id: uuid(seed + 2),
            employee_id: employee_id.clone(),
            home_team_id: root,
            role_id: None,
            proposal: HireProposalWire {
                name: name.into(),
                handle: handle.into(),
                title_in_team: Some("Engineer".into()),
                ..Default::default()
            },
            reason: "resume tests".into(),
        },
    )
    .unwrap();
    let resp = office.dispatch_hiring_rpc(Some(owner), &request).unwrap();
    assert!(matches!(resp.outcome, Outcome::Result(_)), "{resp:?}");
    employee_id
}

fn create_task(
    office: &Office,
    actor: &str,
    team_id: &str,
    assignee: &str,
    title: &str,
    seed: u64,
) -> org_proto::tasks::TaskView {
    office
        .task_create(
            actor,
            TasksCreateParams {
                command_id: uuid(seed),
                task_id: uuid(seed + 1),
                team_id: Some(team_id.to_owned()),
                project_id: None,
                assigned_team_id: None,
                assignee_id: Some(assignee.to_owned()),
                task_type: Some("task".into()),
                work_type: None,
                title: title.into(),
                body_markdown: String::new(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: None,
                labels: vec![],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                attachment_ids: vec![],
                details: None,
                dod: None,
            },
        )
        .unwrap()
        .unwrap()
        .task
}

fn send_dm(
    office: &Office,
    sender: &str,
    to: &str,
    subject: &str,
    priority: MessagePriority,
    seed: u64,
) {
    office
        .chat_send(
            sender,
            ChatSendParams {
                id: uuid(seed),
                to: to.to_owned(),
                kind: "msg".into(),
                priority: Some(priority),
                subject: Some(subject.to_owned()),
                body: format!("body: {subject}"),
                meta: BTreeMap::new(),
                reply_to: None,
                also_in_channel: false,
                reason: None,
                attachment_ids: vec![],
            },
        )
        .unwrap()
        .unwrap();
}

fn send_ask(office: &Office, sender: &str, to: &str, prompt: &str, seed: u64) -> String {
    office
        .msg_ask(
            sender,
            ChatAskParams {
                id: uuid(seed),
                to: to.to_owned(),
                body: None,
                subject: None,
                questions: vec![AskQuestion {
                    id: "q1".into(),
                    question_type: AskQuestionType::FreeText,
                    prompt: prompt.into(),
                    options: vec![],
                    allow_multiple: false,
                    default_value: None,
                    optional: false,
                }],
                required: true,
                wait_ms: Some(600_000),
                hold_ms: None,
                blocks: Some("T-A1".into()),
                approval: false,
                addressed_to: None,
                meta: BTreeMap::new(),
                attachment_ids: vec![],
            },
        )
        .unwrap()
        .unwrap()
        .result
        .ask_id
}

fn dispatch_resume(
    svc: &ResumeService,
    member: Option<&str>,
    mode: Option<SessionMode>,
    params: AgentResumeParams,
) -> Result<AgentResumeResult, org_proto::ApiError> {
    let req = Request::new::<AgentResume>(1, &params).unwrap();
    match svc.dispatch(member, mode, &req).unwrap().outcome {
        Outcome::Result(v) => Ok(serde_json::from_value(v).unwrap()),
        Outcome::Error(e) => Err(e),
    }
}

fn run_full_resume_suite(engine: EngineChoice) {
    let office = Arc::new(Office::for_test(TestOffice { engine }));
    let owner = office.owner_member_id().to_owned();
    let team_id = office.org_snapshot().unwrap().teams[0].id.clone();
    let ada = hire_agent(&office, &owner, "Ada", "ada", 100);
    let bob = hire_agent(&office, &owner, "Bob", "bob", 110);

    // Seed tasks for Ada (todo, in_progress, blocked, done) + one for Bob.
    let t_todo = create_task(&office, &owner, &team_id, &ada, "Todo task", 200);
    let t_prog = create_task(&office, &owner, &team_id, &ada, "In-progress task", 210);
    let t_blk = create_task(&office, &owner, &team_id, &ada, "Blocked task", 220);
    let t_done = create_task(&office, &owner, &team_id, &ada, "Done task", 230);
    let _t_bob = create_task(&office, &owner, &team_id, &bob, "Bob task", 240);

    office
        .task_transition(
            &ada,
            TasksTransitionParams {
                command_id: uuid(250),
                task: t_prog.id.clone(),
                expected_revision: t_prog.revision,
                status: TaskStatus::InProgress,
                blocked_reason: None,
                resolution: None,
                note: None,
                final_summary: None,
                done_anyway_reason: None,
            },
        )
        .unwrap()
        .unwrap();
    office
        .task_transition(
            &ada,
            TasksTransitionParams {
                command_id: uuid(251),
                task: t_blk.id.clone(),
                expected_revision: t_blk.revision,
                status: TaskStatus::Blocked,
                blocked_reason: Some("awaiting spec".into()),
                resolution: None,
                note: None,
                final_summary: None,
                done_anyway_reason: None,
            },
        )
        .unwrap()
        .unwrap();
    office
        .task_transition(
            &ada,
            TasksTransitionParams {
                command_id: uuid(252),
                task: t_done.id.clone(),
                expected_revision: t_done.revision,
                status: TaskStatus::Done,
                blocked_reason: None,
                resolution: Some("fixed".into()),
                note: None,
                final_summary: None,
                done_anyway_reason: Some("test fixture".into()),
            },
        )
        .unwrap()
        .unwrap();

    // ADR-0031 negative test: Bob adds a private note on Ada's in_progress task.
    office
        .task_note_add(
            &bob,
            PrivateNoteAddParams {
                command_id: uuid(300),
                task: Some(t_prog.id.clone()),
                project_id: None,
                note_id: uuid(301),
                kind: PrivateNoteKind::Note,
                title: Some("BOB-SECRET-TITLE".into()),
                why: None,
                alternatives: None,
                tags: vec!["checkpoint".into()],
                body_markdown: "BOB-SECRET-BODY never leak to Ada".into(),
                checkpoint: true,
            },
        )
        .unwrap()
        .unwrap();

    // Seed DMs: 2 to Ada (1 urgent, 1 normal) and 1 private DM to Bob.
    send_dm(
        &office,
        &owner,
        &ada,
        "Normal hello",
        MessagePriority::Normal,
        310,
    );
    send_dm(
        &office,
        &owner,
        &ada,
        "Urgent alert",
        MessagePriority::Urgent,
        311,
    );
    send_dm(
        &office,
        &owner,
        &bob,
        "BOB-PRIVATE-DM",
        MessagePriority::Urgent,
        312,
    );

    // Seed asks: 1 awaiting Ada, 1 sent by Ada, 1 answered ask (excluded), 1 private ask to Bob.
    let _ask_to_ada = send_ask(&office, &owner, &ada, "Can you verify SQLite path?", 320);
    let _ask_from_ada = send_ask(&office, &ada, &owner, "Approve schema rollout?", 321);
    let answered_ask = send_ask(&office, &owner, &ada, "Already answered question", 322);
    office
        .chat_answer(
            &ada,
            ChatAnswerParams {
                ask_id: answered_ask,
                answers: vec![QuestionAnswer {
                    question_id: "q1".into(),
                    option_ids: vec![],
                    text: Some("Done".into()),
                }],
                option_id: None,
                text: None,
                attachment_ids: vec![],
            },
        )
        .unwrap()
        .unwrap();
    let _ask_to_bob = send_ask(&office, &owner, &bob, "BOB-PRIVATE-ASK", 323);

    // Seed runtime git-state.json with active worktree for Ada, cleaned for Ada, active for Bob.
    let mut git_state = OfficeGitState::default();
    let repo_id = uuid(400);
    git_state.workspaces.insert(
        OfficeGitState::workspace_key(&t_prog.id, &repo_id, &ada),
        TaskWorkspaceRecord {
            office_id: office.info().office_id.clone(),
            employee_id: ada.clone(),
            task_id: t_prog.id.clone(),
            task_key: t_prog.key.clone(),
            repository_id: repo_id.clone(),
            repository_key: Some("core".into()),
            branch: "task/t-a2-resume".into(),
            worktree_path: "/tmp/wt-ada".into(),
            admin_dir_name: "wt-ada".into(),
            base_commit: "0123456789abcdef".into(),
            pushed_head: None,
            state: "active".into(),
        },
    );
    git_state.workspaces.insert(
        OfficeGitState::workspace_key(&t_done.id, &repo_id, &ada),
        TaskWorkspaceRecord {
            office_id: office.info().office_id.clone(),
            employee_id: ada.clone(),
            task_id: t_done.id.clone(),
            task_key: t_done.key.clone(),
            repository_id: repo_id.clone(),
            repository_key: Some("core".into()),
            branch: "task/t-a4-done".into(),
            worktree_path: "/tmp/wt-ada-cleaned".into(),
            admin_dir_name: "wt-ada-cleaned".into(),
            base_commit: "0123456789abcdef".into(),
            pushed_head: None,
            state: "cleaned".into(),
        },
    );
    git_state.workspaces.insert(
        OfficeGitState::workspace_key(&t_todo.id, &repo_id, &bob),
        TaskWorkspaceRecord {
            office_id: office.info().office_id.clone(),
            employee_id: bob.clone(),
            task_id: t_todo.id.clone(),
            task_key: t_todo.key.clone(),
            repository_id: repo_id,
            repository_key: Some("core".into()),
            branch: "task/bob-secret".into(),
            worktree_path: "/tmp/wt-bob".into(),
            admin_dir_name: "wt-bob".into(),
            base_commit: "0123456789abcdef".into(),
            pushed_head: None,
            state: "active".into(),
        },
    );
    git_state
        .save(office.runtime_root(), &office.info().office_id)
        .unwrap();

    let live = Arc::new(LiveEvents::new(Box::new(|| {})));
    let git_svc = Arc::new(GitService::new(Arc::clone(&office), live));
    let svc = ResumeService::new(Arc::clone(&office), git_svc);

    // 1. Before Ada adds any private note: Bob's note MUST NOT appear; budget is None (omitted).
    let snap = dispatch_resume(
        &svc,
        Some(&ada),
        Some(SessionMode::Managed),
        AgentResumeParams {
            inbox_limit: Some(1),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(snap.identity.member_id, ada);
    assert_eq!(snap.identity.handle.as_deref(), Some("ada"));
    assert_eq!(snap.identity.kind, MemberKind::Agent);
    assert_eq!(snap.identity.session_mode, Some(SessionMode::Managed));

    // Inbox bounded + zero Bob DM leakage (2 DMs + 1 unread open ask to Ada = 3 unread, 2 urgent).
    assert_eq!(snap.unread_inbox.unread_count, 3);
    assert_eq!(snap.unread_inbox.urgent_count, 2);
    assert_eq!(snap.unread_inbox.items.len(), 1);
    assert!(snap.unread_inbox.truncated);
    let snap_json = serde_json::to_string(&snap).unwrap();
    assert!(!snap_json.contains("BOB-SECRET"));
    assert!(!snap_json.contains("BOB-PRIVATE-DM"));
    assert!(!snap_json.contains("BOB-PRIVATE-ASK"));
    assert!(!snap_json.contains("/tmp/wt-bob"));
    assert!(!snap_json.contains("\"budget\""));
    assert!(!snap_json.contains("\"latest_note\""));

    // Active tasks ordered InProgress -> Blocked -> Todo, Done excluded.
    assert_eq!(snap.active_tasks_total, 3);
    assert_eq!(
        snap.active_tasks
            .iter()
            .map(|t| t.status)
            .collect::<Vec<_>>(),
        vec![
            TaskStatus::InProgress,
            TaskStatus::Blocked,
            TaskStatus::Todo
        ]
    );
    assert_eq!(
        snap.active_tasks[1].blocked_reason.as_deref(),
        Some("awaiting spec")
    );
    assert!(snap.active_tasks.iter().all(|t| t.latest_note.is_none()));

    // Open asks: 1 awaiting_caller + 1 sent_by_caller; answered ask & Bob's ask excluded.
    assert_eq!(snap.open_asks.len(), 2);
    assert!(snap
        .open_asks
        .iter()
        .any(|a| a.direction == ResumeAskDirection::AwaitingCaller
            && a.prompt_excerpt.contains("verify SQLite")));
    assert!(snap
        .open_asks
        .iter()
        .any(|a| a.direction == ResumeAskDirection::SentByCaller
            && a.prompt_excerpt.contains("Approve schema")));

    // Active worktrees: only Ada's active worktree (cleaned & Bob's excluded).
    assert_eq!(snap.active_worktrees.len(), 1);
    assert_eq!(snap.active_worktrees[0].worktree_path, "/tmp/wt-ada");
    assert!(snap.budget.is_none());

    // 2. Sparse --checkpoint upsert: create rev 1, update in-place to rev 2 with multibyte UTF-8.
    let cp1 = office
        .task_note_add(
            &ada,
            PrivateNoteAddParams {
                command_id: uuid(500),
                task: Some(t_prog.id.clone()),
                project_id: None,
                note_id: uuid(501),
                kind: PrivateNoteKind::Note,
                title: None,
                why: None,
                alternatives: None,
                tags: vec![],
                body_markdown: "Step 1 complete".into(),
                checkpoint: true,
            },
        )
        .unwrap()
        .unwrap()
        .note;
    assert_eq!(cp1.revision, 1);
    assert!(cp1.tags.iter().any(|t| t == PRIVATE_NOTE_CHECKPOINT_TAG));

    let multibyte = format!("Checkpoint 🚀 {}", "日本語".repeat(45));
    let cp2_cmd = uuid(502);
    let cp2 = office
        .task_note_add(
            &ada,
            PrivateNoteAddParams {
                command_id: cp2_cmd.clone(),
                task: Some(t_prog.id.clone()),
                project_id: None,
                note_id: uuid(503),
                kind: PrivateNoteKind::Note,
                title: None,
                why: None,
                alternatives: None,
                tags: vec![],
                body_markdown: multibyte.clone(),
                checkpoint: true,
            },
        )
        .unwrap()
        .unwrap()
        .note;
    assert_eq!(cp2.id, cp1.id, "checkpoint upsert must update in place");
    assert_eq!(cp2.revision, 2);

    // Exact replay with same command_id succeeds; reuse with different text fails OperationReused.
    let cp2_replay = office
        .task_note_add(
            &ada,
            PrivateNoteAddParams {
                command_id: cp2_cmd.clone(),
                task: Some(t_prog.id.clone()),
                project_id: None,
                note_id: uuid(503),
                kind: PrivateNoteKind::Note,
                title: None,
                why: None,
                alternatives: None,
                tags: vec![],
                body_markdown: multibyte,
                checkpoint: true,
            },
        )
        .unwrap()
        .unwrap()
        .note;
    assert_eq!(cp2_replay.id, cp1.id);
    assert_eq!(cp2_replay.revision, 2);

    let reuse_err = office
        .task_note_add(
            &ada,
            PrivateNoteAddParams {
                command_id: cp2_cmd,
                task: Some(t_prog.id.clone()),
                project_id: None,
                note_id: uuid(503),
                kind: PrivateNoteKind::Note,
                title: None,
                why: None,
                alternatives: None,
                tags: vec![],
                body_markdown: "different payload".into(),
                checkpoint: true,
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(reuse_err.code, ErrorCode::OperationReused);

    // Only one active checkpoint note exists for Ada on t_prog.
    let listed = office
        .task_note_list(
            &ada,
            PrivateNoteListParams {
                task: Some(t_prog.id.clone()),
                tag: Some(PRIVATE_NOTE_CHECKPOINT_TAG.into()),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(listed.notes.len(), 1);

    // 3. Configure daily budget for Ada and call resume again in External mode.
    office
        .budget_set(
            &owner,
            BudgetSetParams {
                operation_id: uuid(600),
                scope_kind: BudgetScopeKind::Agent,
                task_id: None,
                agent_id: Some(ada.clone()),
                release_id: None,
                period_kind: BudgetPeriodKind::Day,
                unit: BudgetUnit::MicroUsd,
                limit_amount: 5_000_000,
                time_zone: Some("UTC".into()),
                time_zone_source: None,
                reason: Some("resume test budget".into()),
            },
        )
        .unwrap()
        .unwrap();

    let snap2 = dispatch_resume(
        &svc,
        Some(&ada),
        Some(SessionMode::External),
        AgentResumeParams::default(),
    )
    .unwrap();
    assert_eq!(snap2.identity.session_mode, Some(SessionMode::External));
    let note_ptr = snap2.active_tasks[0]
        .latest_note
        .as_ref()
        .expect("Ada sees her own checkpoint note");
    assert_eq!(note_ptr.note_id, cp1.id);
    assert_eq!(note_ptr.revision, 2);
    assert!(note_ptr.is_checkpoint);
    assert!(note_ptr.excerpt.len() <= RESUME_NOTE_EXCERPT_MAX_BYTES);
    assert!(note_ptr.excerpt.ends_with('…'));
    // Tasks without notes keep latest_note = None.
    assert!(snap2.active_tasks[1].latest_note.is_none());
    assert!(snap2.active_tasks[2].latest_note.is_none());

    let budget = snap2
        .budget
        .as_ref()
        .expect("configured daily budget included");
    assert_eq!(budget.status, BudgetStateSummary::Ok);
    assert_eq!(
        budget.daily.as_ref().and_then(|d| d.effective_limit),
        Some(5_000_000)
    );
    assert!(budget.monthly.is_none());
}

#[test]
fn agent_resume_works_end_to_end_on_memory_and_sqlite_with_adr0031_isolation() {
    org_office_sqlite::install();
    run_full_resume_suite(EngineChoice::Memory);
    run_full_resume_suite(EngineChoice::Sqlite);
}

#[test]
fn agent_resume_enforces_binding_bounds_and_policy_gates() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner = office.owner_member_id().to_owned();
    let live = Arc::new(LiveEvents::new(Box::new(|| {})));
    let git_svc = Arc::new(GitService::new(Arc::clone(&office), live));
    let svc = ResumeService::new(Arc::clone(&office), git_svc);

    let err_unbound = dispatch_resume(&svc, None, None, AgentResumeParams::default()).unwrap_err();
    assert_eq!(err_unbound.code, ErrorCode::NotBound);

    let err_zero = dispatch_resume(
        &svc,
        Some(&owner),
        None,
        AgentResumeParams {
            tasks_limit: Some(0),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(err_zero.code, ErrorCode::InvalidRequest);

    let err_over = dispatch_resume(
        &svc,
        Some(&owner),
        None,
        AgentResumeParams {
            inbox_limit: Some(99),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(err_over.code, ErrorCode::InvalidRequest);

    // Tess delegation fails closed on agent.resume (Access::Never).
    assert_eq!(delegation::access("agent.resume"), Access::Never);

    // UTF-8 excerpt truncation handles emoji, CJK, whitespace, and tiny budgets safely.
    assert_eq!(
        crate::resume_calls::assemble::truncate_utf8_excerpt("  hello\n\tworld  ", 240),
        "hello world"
    );
    let emoji = "🚀".repeat(80);
    let out = crate::resume_calls::assemble::truncate_utf8_excerpt(&emoji, 240);
    assert!(out.len() <= 240 && out.ends_with('…'));
    assert!(crate::resume_calls::assemble::truncate_utf8_excerpt("🚀abc", 2).len() <= 2);
}
