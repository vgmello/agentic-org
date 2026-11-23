//! Engine-independent conformance tests for `TASK-53` (`L2` immutable usage & cost ledger,
//! versioned USD pricing, attribution corrections, timezone queries, CSV export; `L3` task,
//! agent daily/monthly, and release budgets, 80/100 notices, one-time +20% extensions,
//! independent holds, calendar resets, and own-agent status) on BOTH `Memory` and `Sqlite`,
//! including SQLite `protected/accounting.db` and `protected/budgets.db` delete+rebuild.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use org_proto::budgets::{
    BudgetAgentOwnParams, BudgetClearParams, BudgetExtendOnceParams, BudgetGetParams,
    BudgetHoldState, BudgetKeepPausedParams, BudgetListParams, BudgetPeriodKind, BudgetPolicyState,
    BudgetScopeKind, BudgetSetParams, BudgetStateSummary, BudgetStopParams, BudgetTimeZoneSource,
    BudgetUnit,
};
use org_proto::costs::{
    AttributionBasis, CompletenessStatus, CostBreakdownGroup, CostPeriodPreset,
    CostPriceListParams, CostPriceSetParams, CostQueryParams, CostUsageCorrectParams,
    CostUsageRecordParams, UsageActorKind, UsageCostBasis, UsageScopeKind, WorkType,
};
use org_proto::org::{
    MemberAddPersonParams, ProjectCreateParams, TeamCreateParams, TeamSetLeadParams,
};
use org_proto::tasks::{TaskStatus, TasksCreateParams, TasksTransitionParams};
use org_proto::ErrorCode;
use org_store::testkit::{identity, FakeCustody};
use org_store::{Clock, Timestamp};

use crate::office::testkit::{places, record, request, CA_SEED};
use crate::office::{Custody, DataRoots, Office, Options};
use crate::rules::OrgCommand;
use crate::store::open::EngineChoice;

static NEXT_ID: AtomicU64 = AtomicU64::new(90_000);
const BASE_TEST_MS: u64 = 1_772_971_200_000; // 2026-03-08T12:00:00.000Z

fn next_uuid() -> String {
    let n = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let ms = BASE_TEST_MS + (n % 10_000);
    let mut b = [0u8; 16];
    b[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
    b[6] = 0x70 | ((n >> 8) as u8 & 0x0f);
    b[7] = (n & 0xff) as u8;
    b[8] = 0x80;
    b[15] = (n & 0xff) as u8;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

struct ManualClock(AtomicU64);
impl ManualClock {
    fn new(ms: u64) -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(ms)))
    }
    fn set(&self, ms: u64) {
        self.0.store(ms, Ordering::SeqCst);
    }
}
impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        Timestamp(self.0.load(Ordering::SeqCst))
    }
}

struct CostBudgetFixture {
    _tmp: tempfile::TempDir,
    folder: std::path::PathBuf,
    roots: DataRoots,
    custody: Custody,
    clock: Arc<ManualClock>,
    office: Office,
    owner: String,
    pm_person: String,
    pm_agent: String,
    other_person: String,
    worker_agent: String,
    project_a: String,
    project_b: String,
    task_a: String,
    task_b: String,
}

fn seed_agent(office: &Office, owner: &str, team_id: &str, name: &str, handle: &str) -> String {
    use org_proto::hiring::{HireProposalWire, HrHire, HrHireParams};
    use org_proto::{Outcome, Request};
    let employee_id = next_uuid();
    let req = Request::new::<HrHire>(
        1,
        &HrHireParams {
            command_id: next_uuid(),
            request_id: next_uuid(),
            employee_id: employee_id.clone(),
            home_team_id: team_id.into(),
            role_id: None,
            proposal: HireProposalWire {
                name: name.into(),
                handle: handle.into(),
                title_in_team: Some("Engineer".into()),
                ..Default::default()
            },
            reason: "seed test agent".into(),
        },
    )
    .expect("hr.hire request");
    let resp = office.dispatch_hiring_rpc(Some(owner), &req).unwrap();
    assert!(matches!(resp.outcome, Outcome::Result(_)));
    employee_id
}

fn seed_person(office: &Office, owner: &str, team_id: &str, name: &str, handle: &str) -> String {
    let member_id = next_uuid();
    office
        .submit_org(
            owner,
            OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: next_uuid(),
                member_id: member_id.clone(),
                name: name.into(),
                handle: Some(handle.into()),
                home_team_id: Some(team_id.into()),
                contact: None,
            }),
        )
        .unwrap()
        .unwrap();
    member_id
}

fn seed_team_and_project(
    office: &Office,
    owner: &str,
    root_team: &str,
    team_name: &str,
    project_name: &str,
) -> (String, String) {
    let team_id = next_uuid();
    office
        .submit_org(
            owner,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: next_uuid(),
                team_id: team_id.clone(),
                name: team_name.into(),
                owner_team_id: Some(root_team.into()),
            }),
        )
        .unwrap()
        .unwrap();
    let project_id = next_uuid();
    office
        .submit_org(
            owner,
            OrgCommand::ProjectCreate(ProjectCreateParams {
                command_id: next_uuid(),
                project_id: project_id.clone(),
                name: project_name.into(),
                key: None,
                description: None,
                teams: vec![],
                general_area_id: String::new(),
                general_area_name: None,
                repo: None,
                integration_branch: None,
                slug: None,
                owner_team_id: Some(team_id.clone()),
                create_team_id: None,
                parent_team_id: None,
                contributing_team_ids: vec![],
            }),
        )
        .unwrap()
        .unwrap();
    (team_id, project_id)
}

fn seed_task(office: &Office, actor: &str, team_id: &str, project_id: &str, title: &str) -> String {
    office
        .task_create(
            actor,
            TasksCreateParams {
                command_id: next_uuid(),
                task_id: next_uuid(),
                team_id: Some(team_id.into()),
                project_id: Some(project_id.into()),
                assigned_team_id: None,
                assignee_id: None,
                task_type: Some("task".into()),
                work_type: Some(org_proto::tasks::WorkType::Build),
                title: title.into(),
                body_markdown: "Test task body".into(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: None,
                labels: vec![],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                details: None,
            },
        )
        .unwrap()
        .unwrap()
        .task
        .id
}

fn seed_fixture(engine: EngineChoice) -> CostBudgetFixture {
    // Base time: 2026-03-08T12:00:00.000Z (America/New_York spring-forward DST day = 23 hours!).
    let base_ms = org_proto::parse_rfc3339_ms("2026-03-08T12:00:00.000Z").unwrap();
    let clock = ManualClock::new(base_ms);
    let tmp = tempfile::tempdir().expect("tempdir");
    let (folder, roots) = places(tmp.path());
    std::fs::create_dir_all(&roots.durable).unwrap();
    std::fs::create_dir_all(&roots.runtime).unwrap();
    let here = identity();
    let custody = Custody {
        identity: here,
        authority: Arc::new(FakeCustody::new()),
    };
    let (new, binding) = request();
    let options = Options {
        engine,
        clock: clock.clone(),
        testing: Default::default(),
        before_final_checks: None,
    };
    let office = Office::provision_with(
        folder.clone(),
        &new,
        None,
        &binding,
        roots.clone(),
        custody.clone(),
        |_| record(CA_SEED),
        options,
    )
    .expect("provision test office");
    let owner = office.owner_member_id().to_owned();
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();

    let (team_a, project_a) =
        seed_team_and_project(&office, &owner, &root_team, "Alpha Team", "Alpha");
    let (team_b, project_b) =
        seed_team_and_project(&office, &owner, &root_team, "Beta Team", "Beta");

    let pm_person = seed_person(&office, &owner, &team_a, "Pat PM", "patpm");
    let pm_agent = seed_agent(&office, &owner, &team_b, "Bot PM", "botpm");
    let other_person = seed_person(&office, &owner, &root_team, "Viewer", "viewer");
    let worker_agent = seed_agent(&office, &owner, &team_a, "Worker", "worker");

    for (tid, lid) in [(&team_a, &pm_person), (&team_b, &pm_agent)] {
        let ver = office
            .org_snapshot()
            .unwrap()
            .teams
            .into_iter()
            .find(|t| &t.id == tid)
            .unwrap()
            .version;
        office
            .submit_org(
                &owner,
                OrgCommand::TeamSetLead(TeamSetLeadParams {
                    command_id: next_uuid(),
                    team_id: tid.clone(),
                    expected_version: ver,
                    lead_member_id: Some(lid.clone()),
                }),
            )
            .unwrap()
            .unwrap();
    }

    let task_a = seed_task(&office, &owner, &team_a, &project_a, "Alpha task");
    let task_b = seed_task(&office, &owner, &team_b, &project_b, "Beta task");

    CostBudgetFixture {
        _tmp: tmp,
        folder,
        roots,
        custody,
        clock,
        office,
        owner,
        pm_person,
        pm_agent,
        other_person,
        worker_agent,
        project_a,
        project_b,
        task_a,
        task_b,
    }
}

fn usage_event(
    event_id: &str,
    agent_id: &str,
    occurred_at: &str,
    tokens: (u64, u64),
    task_id: Option<&str>,
    project_id: Option<&str>,
    release_id: Option<&str>,
) -> CostUsageRecordParams {
    let (input, output) = tokens;
    CostUsageRecordParams {
        id: None,
        producer_id: "prod-1".into(),
        source_event_id: event_id.into(),
        run_id: Some("run-1".into()),
        actor_kind: UsageActorKind::Agent,
        employee_id: Some(agent_id.into()),
        runtime: "claude_code".into(),
        provider: Some("anthropic".into()),
        model: Some("claude-sonnet-4".into()),
        occurred_at: occurred_at.into(),
        source_metadata_json: None,
        input_tokens: Some(input),
        output_tokens: Some(output),
        cache_read_tokens: Some(0),
        cache_write_tokens: Some(0),
        reasoning_tokens: Some(0),
        token_issue: None,
        runtime_cost_micro_usd: None,
        is_subscription: false,
        scope_kind: if task_id.is_some() {
            UsageScopeKind::Task
        } else {
            UsageScopeKind::Project
        },
        task_id: task_id.map(str::to_owned),
        project_id: project_id.map(str::to_owned),
        team_id: None,
        feature_id: None,
        milestone_id: None,
        release_id: release_id.map(str::to_owned),
        task_type_snapshot: Some("task".into()),
        work_type: Some(WorkType::Build),
        attribution_basis: AttributionBasis::CurrentTask,
    }
}

fn run_l2_accounting_suite(fx: &CostBudgetFixture) {
    // 1. Versioned USD pricing: PM cannot set prices; owner publishes v1 & v2.
    assert_eq!(
        fx.office
            .cost_price_set(
                &fx.pm_person,
                CostPriceSetParams {
                    id: None,
                    provider: "anthropic".into(),
                    model: "claude-sonnet-4".into(),
                    effective_from: "2026-03-01T00:00:00.000Z".into(),
                    input_micro_usd_per_million: Some(1_000_000),
                    output_micro_usd_per_million: Some(2_000_000),
                    cache_read_micro_usd_per_million: Some(500_000),
                    cache_write_micro_usd_per_million: Some(1_250_000),
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    let p1 = fx
        .office
        .cost_price_set(
            &fx.owner,
            CostPriceSetParams {
                id: None,
                provider: "anthropic".into(),
                model: "claude-sonnet-4".into(),
                effective_from: "2026-03-01T00:00:00.000Z".into(),
                input_micro_usd_per_million: Some(1_000_000),
                output_micro_usd_per_million: Some(2_000_000),
                cache_read_micro_usd_per_million: Some(500_000),
                cache_write_micro_usd_per_million: Some(1_250_000),
            },
        )
        .unwrap()
        .unwrap()
        .price;

    // Record usage under p1: 10_000 in + 5_000 out = 10_000 + 10_000 = 20_000 micro-USD.
    let ev1 = usage_event(
        "evt-1",
        &fx.worker_agent,
        "2026-03-08T10:00:00.000Z",
        (10_000, 5_000),
        Some(&fx.task_a),
        Some(&fx.project_a),
        Some("rel-1"),
    );
    let r1 = fx
        .office
        .cost_usage_record(&fx.owner, ev1.clone())
        .unwrap()
        .unwrap();
    assert!(!r1.replayed);
    assert_eq!(r1.record.cost_micro_usd, Some(20_000));
    assert_eq!(r1.record.cost_basis, UsageCostBasis::TokenEstimate);
    assert_eq!(r1.record.price_id.as_deref(), Some(p1.id.as_str()));

    // Exact replay is idempotent; changed payload for same source_event_id is Conflict.
    let r1_replay = fx
        .office
        .cost_usage_record(&fx.owner, ev1.clone())
        .unwrap()
        .unwrap();
    assert!(r1_replay.replayed);
    let mut bad_ev1 = ev1;
    bad_ev1.input_tokens = Some(99_999);
    assert_eq!(
        fx.office
            .cost_usage_record(&fx.owner, bad_ev1)
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );

    // Publish p2 (10x higher) effective 2026-03-08T11:00:00Z — never reprices r1!
    fx.office
        .cost_price_set(
            &fx.owner,
            CostPriceSetParams {
                id: None,
                provider: "anthropic".into(),
                model: "claude-sonnet-4".into(),
                effective_from: "2026-03-08T11:00:00.000Z".into(),
                input_micro_usd_per_million: Some(10_000_000),
                output_micro_usd_per_million: Some(20_000_000),
                cache_read_micro_usd_per_million: Some(0),
                cache_write_micro_usd_per_million: Some(0),
            },
        )
        .unwrap()
        .unwrap();
    let r2 = fx
        .office
        .cost_usage_record(
            &fx.owner,
            usage_event(
                "evt-2",
                &fx.worker_agent,
                "2026-03-08T11:30:00.000Z",
                (1_000, 0),
                Some(&fx.task_b),
                Some(&fx.project_b),
                None,
            ),
        )
        .unwrap()
        .unwrap();
    assert_eq!(r2.record.cost_micro_usd, Some(10_000));
    assert_eq!(
        fx.office
            .cost_price_list(&fx.pm_person, CostPriceListParams::default())
            .unwrap()
            .unwrap()
            .prices
            .len(),
        2
    );

    // 2. Visibility scoping (`doc-51` §1): non-PM refused, PM sees ONLY project_a, owner sees both.
    assert_eq!(
        fx.office
            .cost_query(&fx.other_person, CostQueryParams::default())
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    let pm_q = fx
        .office
        .cost_query(
            &fx.pm_person,
            CostQueryParams {
                period: Some(CostPeriodPreset::Today),
                time_zone: Some("America/New_York".into()),
                group_by: Some(CostBreakdownGroup::Project),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
    assert!(pm_q.scoped_to_projects);
    assert_eq!(pm_q.summary.known_cost_micro_usd, Some(20_000));
    // DST spring-forward day in America/New_York (2026-03-08) is exactly 23 hours: 05:00Z -> 04:00Z!
    assert_eq!(pm_q.starts_at.as_deref(), Some("2026-03-08T05:00:00.000Z"));
    assert_eq!(pm_q.ends_at.as_deref(), Some("2026-03-09T04:00:00.000Z"));

    let owner_q = fx
        .office
        .cost_query(&fx.owner, CostQueryParams::default())
        .unwrap()
        .unwrap();
    assert!(!owner_q.scoped_to_projects);
    assert_eq!(owner_q.summary.known_cost_micro_usd, Some(30_000));

    // 3. Attribution correction: PM cannot move across unauthorized project; "Not on a task"
    // keeps project overhead; CSV neutralizes formula injection (`=cmd`).
    assert_eq!(
        fx.office
            .cost_usage_correct(
                &fx.pm_person,
                CostUsageCorrectParams {
                    operation_id: next_uuid(),
                    supersedes_id: r1.record.id.clone(),
                    reason: "cross project attempt".into(),
                    scope_kind: UsageScopeKind::Task,
                    task_id: Some(fx.task_b.clone()),
                    project_id: Some(fx.project_b.clone()),
                    team_id: None,
                    feature_id: None,
                    milestone_id: None,
                    release_id: None,
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    let corr = fx
        .office
        .cost_usage_correct(
            &fx.pm_person,
            CostUsageCorrectParams {
                operation_id: next_uuid(),
                supersedes_id: r1.record.id.clone(),
                reason: "=HYPERLINK(\"bad\") project overhead".into(),
                scope_kind: UsageScopeKind::Project,
                task_id: None,
                project_id: None,
                team_id: None,
                feature_id: None,
                milestone_id: None,
                release_id: Some("rel-1".into()),
            },
        )
        .unwrap()
        .unwrap()
        .record;
    assert_eq!(corr.project_id.as_deref(), Some(fx.project_a.as_str()));
    assert_eq!(corr.task_id, None);
    // Old superseded record cannot be superseded again.
    assert_eq!(
        fx.office
            .cost_usage_correct(
                &fx.pm_person,
                CostUsageCorrectParams {
                    operation_id: next_uuid(),
                    supersedes_id: r1.record.id,
                    reason: "second supersede".into(),
                    scope_kind: UsageScopeKind::Project,
                    task_id: None,
                    project_id: Some(fx.project_a.clone()),
                    team_id: None,
                    feature_id: None,
                    milestone_id: None,
                    release_id: None,
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    let csv = fx
        .office
        .cost_export_csv(&fx.pm_person, CostQueryParams::default())
        .unwrap()
        .unwrap();
    assert_eq!(csv.row_count, 1);
    assert!(csv.csv.contains("\"'=HYPERLINK("));
}

fn run_l3_task_and_agent_budget_suite(fx: &CostBudgetFixture) {
    // Move task_b to in_progress so we verify auto-block on budget reach (`doc-92`).
    let rev = fx
        .office
        .task_inspect(&fx.task_b)
        .unwrap()
        .unwrap()
        .task
        .revision;
    fx.office
        .task_transition(
            &fx.owner,
            TasksTransitionParams {
                command_id: next_uuid(),
                task: fx.task_b.clone(),
                expected_revision: rev,
                status: TaskStatus::InProgress,
                blocked_reason: None,
                resolution: None,
                note: None,
            },
        )
        .unwrap()
        .unwrap();

    // Agent PM (`pm_agent`) may first-set and lower task_b budget in same unit, but CANNOT raise or change unit!
    let b_task = fx
        .office
        .budget_set(
            &fx.pm_agent,
            BudgetSetParams {
                operation_id: next_uuid(),
                scope_kind: BudgetScopeKind::Task,
                task_id: Some(fx.task_b.clone()),
                agent_id: None,
                release_id: None,
                period_kind: BudgetPeriodKind::Lifetime,
                unit: BudgetUnit::MicroUsd,
                limit_amount: 25_000,
                time_zone: None,
                time_zone_source: None,
                reason: Some("initial cap".into()),
            },
        )
        .unwrap()
        .unwrap()
        .budget;
    assert_eq!(b_task.limit_amount, Some(25_000));
    // Lower in same unit -> allowed.
    let lowered = fx
        .office
        .budget_set(
            &fx.pm_agent,
            BudgetSetParams {
                operation_id: next_uuid(),
                scope_kind: BudgetScopeKind::Task,
                task_id: Some(fx.task_b.clone()),
                agent_id: None,
                release_id: None,
                period_kind: BudgetPeriodKind::Lifetime,
                unit: BudgetUnit::MicroUsd,
                limit_amount: 20_000,
                time_zone: None,
                time_zone_source: None,
                reason: Some("tighten".into()),
            },
        )
        .unwrap()
        .unwrap()
        .budget;
    assert_eq!(lowered.limit_amount, Some(20_000));
    // Raise by agent PM -> refused with exact `doc-92` message!
    let err = fx
        .office
        .budget_set(
            &fx.pm_agent,
            BudgetSetParams {
                operation_id: next_uuid(),
                scope_kind: BudgetScopeKind::Task,
                task_id: Some(fx.task_b.clone()),
                agent_id: None,
                release_id: None,
                period_kind: BudgetPeriodKind::Lifetime,
                unit: BudgetUnit::MicroUsd,
                limit_amount: 30_000,
                time_zone: None,
                time_zone_source: None,
                reason: None,
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotAuthorized);
    assert_eq!(
        err.message,
        "Only a person can raise, clear or extend a budget."
    );

    // Non-PM viewer sees state-only view with amounts stripped (`amounts_visible = false`).
    let viewer_view = fx
        .office
        .budget_get(
            &fx.other_person,
            BudgetGetParams {
                budget_id: Some(b_task.id.clone()),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap()
        .budget
        .unwrap();
    assert!(!viewer_view.amounts_visible);
    assert_eq!(viewer_view.limit_amount, None);
    assert_eq!(viewer_view.used_amount, None);
    assert_eq!(viewer_view.state_summary, BudgetStateSummary::Ok);

    // Extending below reach is refused without consuming extension.
    assert_eq!(
        fx.office
            .budget_extend_once(
                &fx.owner,
                BudgetExtendOnceParams {
                    operation_id: next_uuid(),
                    budget_id: b_task.id.clone(),
                    reason: None,
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );

    // Push task_b spend to 25_000 (task_b already has 10_000 from r2 -> add 15_000 -> exceeds 20_000!).
    let r_over = fx
        .office
        .cost_usage_record(
            &fx.owner,
            usage_event(
                "evt-task-b-over",
                &fx.worker_agent,
                "2026-03-08T11:45:00.000Z",
                (1_500, 0),
                Some(&fx.task_b),
                Some(&fx.project_b),
                None,
            ),
        )
        .unwrap()
        .unwrap()
        .record;
    assert_eq!(r_over.cost_micro_usd, Some(15_000));
    // Task auto-blocked with "Budget reached" + active hold + owner Inbox notice!
    let t_inspected = fx.office.task_inspect(&fx.task_b).unwrap().unwrap().task;
    assert_eq!(t_inspected.status, TaskStatus::Blocked);
    assert_eq!(
        t_inspected.blocked_reason.as_deref(),
        Some("Budget reached")
    );
    assert_eq!(
        fx.office
            .active_budget_holds(None, Some(&fx.task_b))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(fx.office.open_budget_inbox_notices().unwrap().len(), 1);

    // Agent PM CANNOT reduce a reached task budget via attribution correction!
    let agent_corr_err = fx
        .office
        .cost_usage_correct(
            &fx.pm_agent,
            CostUsageCorrectParams {
                operation_id: next_uuid(),
                supersedes_id: r_over.id.clone(),
                reason: "try move off reached task".into(),
                scope_kind: UsageScopeKind::Project,
                task_id: None,
                project_id: Some(fx.project_b.clone()),
                team_id: None,
                feature_id: None,
                milestone_id: None,
                release_id: None,
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(
        agent_corr_err.message,
        "Only a person can raise, clear or extend a budget."
    );

    // +20% on 20_000 = 24_000, which is STILL <= 25_000 spend -> refused without consuming extension!
    assert_eq!(
        fx.office
            .budget_extend_once(
                &fx.owner,
                BudgetExtendOnceParams {
                    operation_id: next_uuid(),
                    budget_id: b_task.id.clone(),
                    reason: None,
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    // Raise to 25_000 -> still reached (25_000 == 25_000): superseded rev hold/notice resolved so
    // exactly ONE active hold and ONE open owner Inbox notice remain (`Pass 3 Med`).
    fx.office
        .budget_set(
            &fx.owner,
            BudgetSetParams {
                operation_id: next_uuid(),
                scope_kind: BudgetScopeKind::Task,
                task_id: Some(fx.task_b.clone()),
                agent_id: None,
                release_id: None,
                period_kind: BudgetPeriodKind::Lifetime,
                unit: BudgetUnit::MicroUsd,
                limit_amount: 25_000,
                time_zone: None,
                time_zone_source: None,
                reason: Some("raise to match".into()),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        fx.office
            .active_budget_holds(None, Some(&fx.task_b))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        fx.office
            .open_budget_inbox_notices()
            .unwrap()
            .iter()
            .filter(|n| n.notice.budget_id == b_task.id)
            .count(),
        1
    );
    let ext_op = next_uuid();
    let ext = fx
        .office
        .budget_extend_once(
            &fx.owner,
            BudgetExtendOnceParams {
                operation_id: ext_op.clone(),
                budget_id: b_task.id.clone(),
                reason: Some("finish PR".into()),
            },
        )
        .unwrap()
        .unwrap();
    assert!(!ext.replayed);
    assert_eq!(ext.budget.effective_limit, Some(30_000));
    assert!(ext.budget.extension_used);
    assert!(fx
        .office
        .active_budget_holds(None, Some(&fx.task_b))
        .unwrap()
        .is_empty());
    // Idempotent replay succeeds ONLY with identical payload; mutated payload or fresh extend fails with Conflict (`Pass 2 High #2`)!
    assert!(
        fx.office
            .budget_extend_once(
                &fx.owner,
                BudgetExtendOnceParams {
                    operation_id: ext_op.clone(),
                    budget_id: b_task.id.clone(),
                    reason: Some("finish PR".into()),
                },
            )
            .unwrap()
            .unwrap()
            .replayed
    );
    assert_eq!(
        fx.office
            .budget_extend_once(
                &fx.owner,
                BudgetExtendOnceParams {
                    operation_id: ext_op,
                    budget_id: b_task.id.clone(),
                    reason: None,
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        fx.office
            .budget_extend_once(
                &fx.owner,
                BudgetExtendOnceParams {
                    operation_id: next_uuid(),
                    budget_id: b_task.id.clone(),
                    reason: None,
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );

    // Stop cancels task_b.
    fx.office
        .budget_stop(
            &fx.pm_agent,
            BudgetStopParams {
                operation_id: next_uuid(),
                budget_id: b_task.id,
                reason: Some("stopping task".into()),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        fx.office
            .task_inspect(&fx.task_b)
            .unwrap()
            .unwrap()
            .task
            .status,
        TaskStatus::Cancelled
    );
}

fn run_l3_agent_daily_monthly_and_release_suite(fx: &CostBudgetFixture) {
    // Missing or invalid timezone on agent budget is refused (never silently falls back to UTC).
    assert_eq!(
        fx.office
            .budget_set(
                &fx.owner,
                BudgetSetParams {
                    operation_id: next_uuid(),
                    scope_kind: BudgetScopeKind::Agent,
                    task_id: None,
                    agent_id: Some(fx.worker_agent.clone()),
                    release_id: None,
                    period_kind: BudgetPeriodKind::Day,
                    unit: BudgetUnit::MicroUsd,
                    limit_amount: 10_000,
                    time_zone: None,
                    time_zone_source: None,
                    reason: None,
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        fx.office
            .budget_set(
                &fx.owner,
                BudgetSetParams {
                    operation_id: next_uuid(),
                    scope_kind: BudgetScopeKind::Agent,
                    task_id: None,
                    agent_id: Some(fx.worker_agent.clone()),
                    release_id: None,
                    period_kind: BudgetPeriodKind::Day,
                    unit: BudgetUnit::MicroUsd,
                    limit_amount: 10_000,
                    time_zone: Some("Not/AZone".into()),
                    time_zone_source: None,
                    reason: None,
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );

    // Independent daily AND monthly holds for worker_agent (already spent 45_000 today).
    let daily = fx
        .office
        .budget_set(
            &fx.owner,
            BudgetSetParams {
                operation_id: next_uuid(),
                scope_kind: BudgetScopeKind::Agent,
                task_id: None,
                agent_id: Some(fx.worker_agent.clone()),
                release_id: None,
                period_kind: BudgetPeriodKind::Day,
                unit: BudgetUnit::MicroUsd,
                limit_amount: 40_000,
                time_zone: Some("America/New_York".into()),
                time_zone_source: Some(BudgetTimeZoneSource::OwnerPerson),
                reason: None,
            },
        )
        .unwrap()
        .unwrap()
        .budget;
    let monthly = fx
        .office
        .budget_set(
            &fx.owner,
            BudgetSetParams {
                operation_id: next_uuid(),
                scope_kind: BudgetScopeKind::Agent,
                task_id: None,
                agent_id: Some(fx.worker_agent.clone()),
                release_id: None,
                period_kind: BudgetPeriodKind::Month,
                unit: BudgetUnit::MicroUsd,
                limit_amount: 40_000,
                time_zone: Some("America/New_York".into()),
                time_zone_source: Some(BudgetTimeZoneSource::OwnerPerson),
                reason: None,
            },
        )
        .unwrap()
        .unwrap()
        .budget;
    assert_eq!(daily.hold_state, Some(BudgetHoldState::StopPending));
    assert_eq!(monthly.hold_state, Some(BudgetHoldState::StopPending));
    assert_eq!(
        fx.office
            .active_budget_holds(Some(&fx.worker_agent), None)
            .unwrap()
            .len(),
        2
    );
    // Turn finishes -> transitions stop_pending -> held.
    assert!(fx
        .office
        .mark_budget_holds_stopped(&fx.worker_agent)
        .unwrap());
    assert!(fx
        .office
        .active_budget_holds(Some(&fx.worker_agent), None)
        .unwrap()
        .iter()
        .all(|h| h.state == BudgetHoldState::Held));

    // PM cannot see agent budgets at all; bound agent sees ONLY its own via `budget.agent_own`!
    assert!(fx
        .office
        .budget_list(
            &fx.pm_person,
            BudgetListParams {
                scope_kind: Some(BudgetScopeKind::Agent),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap()
        .budgets
        .is_empty());
    // Unassigned task is refused for `budget.agent_own`; `task_id: None` returns agent's own caps.
    assert_eq!(
        fx.office
            .budget_agent_own(
                &fx.worker_agent,
                BudgetAgentOwnParams {
                    task_id: Some(fx.task_b.clone()),
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    let own = fx
        .office
        .budget_agent_own(&fx.worker_agent, BudgetAgentOwnParams::default())
        .unwrap()
        .unwrap();
    assert_eq!(own.daily.as_ref().unwrap().limit_amount, Some(40_000));
    assert_eq!(own.monthly.as_ref().unwrap().limit_amount, Some(40_000));
    assert!(own.task.is_none());
    assert_eq!(
        fx.office
            .budget_agent_own(&fx.owner, BudgetAgentOwnParams::default())
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );

    // Clearing daily hold leaves monthly hold intact!
    let clear_op = next_uuid();
    fx.office
        .budget_clear(
            &fx.owner,
            BudgetClearParams {
                operation_id: clear_op.clone(),
                budget_id: daily.id.clone(),
                reason: Some("remove daily cap".into()),
            },
        )
        .unwrap()
        .unwrap();
    assert!(
        fx.office
            .budget_clear(
                &fx.owner,
                BudgetClearParams {
                    operation_id: clear_op,
                    budget_id: daily.id,
                    reason: Some("remove daily cap".into()),
                },
            )
            .unwrap()
            .unwrap()
            .replayed
    );
    assert_eq!(
        fx.office
            .active_budget_holds(Some(&fx.worker_agent), None)
            .unwrap()
            .len(),
        1
    );
    // Keep monthly paused retains BOTH the hold and the open owner Inbox item (`doc-92` §4).
    let keep_op = next_uuid();
    fx.office
        .budget_keep_paused(
            &fx.owner,
            BudgetKeepPausedParams {
                operation_id: keep_op.clone(),
                budget_id: monthly.id.clone(),
                reason: Some("wait for next month".into()),
            },
        )
        .unwrap()
        .unwrap();
    assert!(
        fx.office
            .budget_keep_paused(
                &fx.owner,
                BudgetKeepPausedParams {
                    operation_id: keep_op,
                    budget_id: monthly.id.clone(),
                    reason: Some("wait for next month".into()),
                },
            )
            .unwrap()
            .unwrap()
            .replayed
    );
    assert_eq!(
        fx.office
            .active_budget_holds(Some(&fx.worker_agent), None)
            .unwrap()
            .len(),
        1
    );
    assert!(fx
        .office
        .open_budget_inbox_notices()
        .unwrap()
        .iter()
        .any(|n| n.notice.budget_id == monthly.id));

    // Release warning cap fails closed to owner-only until L11 models release-to-project ownership.
    assert_eq!(
        fx.office
            .budget_set(
                &fx.pm_person,
                BudgetSetParams {
                    operation_id: next_uuid(),
                    scope_kind: BudgetScopeKind::Release,
                    task_id: None,
                    agent_id: None,
                    release_id: Some("rel-1".into()),
                    period_kind: BudgetPeriodKind::Lifetime,
                    unit: BudgetUnit::MicroUsd,
                    limit_amount: 10_000,
                    time_zone: None,
                    time_zone_source: None,
                    reason: None,
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    let rel = fx
        .office
        .budget_set(
            &fx.owner,
            BudgetSetParams {
                operation_id: next_uuid(),
                scope_kind: BudgetScopeKind::Release,
                task_id: None,
                agent_id: None,
                release_id: Some("rel-1".into()),
                period_kind: BudgetPeriodKind::Lifetime,
                unit: BudgetUnit::MicroUsd,
                limit_amount: 10_000,
                time_zone: None,
                time_zone_source: None,
                reason: None,
            },
        )
        .unwrap()
        .unwrap()
        .budget;
    assert_eq!(rel.state_summary, BudgetStateSummary::ReachedCap);
    assert_eq!(rel.hold_state, None);
    assert!(!rel.can_extend_once);
    assert_eq!(
        fx.office
            .budget_extend_once(
                &fx.owner,
                BudgetExtendOnceParams {
                    operation_id: next_uuid(),
                    budget_id: rel.id,
                    reason: None,
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );

    // Calendar reset into April 2026 closes March period & automatically releases monthly hold!
    let april_ms = org_proto::parse_rfc3339_ms("2026-04-01T05:00:00.000Z").unwrap();
    fx.clock.set(april_ms);
    assert!(fx
        .office
        .active_budget_holds(Some(&fx.worker_agent), None)
        .unwrap()
        .is_empty());
    let april_monthly = fx
        .office
        .budget_get(
            &fx.owner,
            BudgetGetParams {
                budget_id: Some(monthly.id),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap()
        .budget
        .unwrap();
    assert_eq!(april_monthly.period_key.as_deref(), Some("2026-04"));
    assert_eq!(april_monthly.used_amount, Some(0));
    assert_eq!(april_monthly.state, BudgetPolicyState::Active);
    assert_eq!(
        april_monthly.usage_status,
        Some(CompletenessStatus::Unavailable)
    );
}

fn run_pass1_security_and_retrigger_suite(fx: &CostBudgetFixture) {
    // 1. Explicit-ID price set replays idempotently (`#18`).
    let pid = next_uuid();
    let price_params = CostPriceSetParams {
        id: Some(pid),
        provider: "openai".into(),
        model: "gpt-5".into(),
        effective_from: "2026-03-01T00:00:00.000Z".into(),
        input_micro_usd_per_million: Some(5_000_000),
        output_micro_usd_per_million: Some(15_000_000),
        cache_read_micro_usd_per_million: Some(0),
        cache_write_micro_usd_per_million: Some(0),
    };
    let p_first = fx
        .office
        .cost_price_set(&fx.owner, price_params.clone())
        .unwrap()
        .unwrap();
    let p_replay = fx
        .office
        .cost_price_set(&fx.owner, price_params)
        .unwrap()
        .unwrap();
    assert_eq!(p_first.price.id, p_replay.price.id);

    // 2. Assigned task + auth-before-replay + cross-action Conflict + own-task visibility (`#2`, `#13`).
    let team_a = fx
        .office
        .task_inspect(&fx.task_a)
        .unwrap()
        .unwrap()
        .task
        .team_id;
    let task_c = fx
        .office
        .task_create(
            &fx.owner,
            TasksCreateParams {
                command_id: next_uuid(),
                task_id: next_uuid(),
                team_id: Some(team_a),
                project_id: Some(fx.project_a.clone()),
                assigned_team_id: None,
                assignee_id: Some(fx.worker_agent.clone()),
                task_type: Some("task".into()),
                work_type: Some(org_proto::tasks::WorkType::Build),
                title: "Assigned retrigger task".into(),
                body_markdown: "Test".into(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: None,
                labels: vec![],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                details: None,
            },
        )
        .unwrap()
        .unwrap()
        .task
        .id;
    let op_set = next_uuid();
    let set_params = BudgetSetParams {
        operation_id: op_set.clone(),
        scope_kind: BudgetScopeKind::Task,
        task_id: Some(task_c.clone()),
        agent_id: None,
        release_id: None,
        period_kind: BudgetPeriodKind::Lifetime,
        unit: BudgetUnit::MicroUsd,
        limit_amount: 10_000,
        time_zone: None,
        time_zone_source: None,
        reason: Some("assigned cap".into()),
    };
    let b_c = fx
        .office
        .budget_set(&fx.owner, set_params.clone())
        .unwrap()
        .unwrap()
        .budget;
    assert_eq!(
        fx.office
            .budget_set(&fx.other_person, set_params.clone())
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    assert!(
        fx.office
            .budget_set(&fx.owner, set_params.clone())
            .unwrap()
            .unwrap()
            .replayed
    );
    let mut mutated = set_params;
    mutated.limit_amount = 99_000;
    assert_eq!(
        fx.office
            .budget_set(&fx.owner, mutated)
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        fx.office
            .budget_clear(
                &fx.owner,
                BudgetClearParams {
                    operation_id: op_set,
                    budget_id: b_c.id.clone(),
                    reason: None,
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    let own_c = fx
        .office
        .budget_agent_own(
            &fx.worker_agent,
            BudgetAgentOwnParams {
                task_id: Some(task_c.clone()),
            },
        )
        .unwrap()
        .unwrap();
    let own_task = own_c.task.expect("assigned task budget visible");
    assert!(own_task.amounts_visible);
    assert_eq!(own_task.limit_amount, Some(10_000));

    // 3. Cross-project task mismatch refused (`#3`); correction replay (`#9`); hold re-trigger (`#7`).
    let u1 = fx
        .office
        .cost_usage_record(
            &fx.owner,
            usage_event(
                "evt-retrigger-1",
                &fx.worker_agent,
                "2026-04-02T10:00:00.000Z",
                (1_500, 0),
                Some(&task_c),
                Some(&fx.project_a),
                None,
            ),
        )
        .unwrap()
        .unwrap()
        .record;
    assert_eq!(
        fx.office
            .active_budget_holds(None, Some(&task_c))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        fx.office
            .cost_usage_correct(
                &fx.owner,
                CostUsageCorrectParams {
                    operation_id: next_uuid(),
                    supersedes_id: u1.id.clone(),
                    reason: "mismatched project".into(),
                    scope_kind: UsageScopeKind::Task,
                    task_id: Some(task_c.clone()),
                    project_id: Some(fx.project_b.clone()),
                    team_id: None,
                    feature_id: None,
                    milestone_id: None,
                    release_id: None,
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    let corr_params = CostUsageCorrectParams {
        operation_id: next_uuid(),
        supersedes_id: u1.id,
        reason: "move off task_c".into(),
        scope_kind: UsageScopeKind::Project,
        task_id: None,
        project_id: Some(fx.project_a.clone()),
        team_id: None,
        feature_id: None,
        milestone_id: None,
        release_id: None,
    };
    let c1 = fx
        .office
        .cost_usage_correct(&fx.owner, corr_params.clone())
        .unwrap()
        .unwrap();
    let c2 = fx
        .office
        .cost_usage_correct(&fx.owner, corr_params)
        .unwrap()
        .unwrap();
    assert_eq!(c1.record.id, c2.record.id);
    assert!(fx
        .office
        .active_budget_holds(None, Some(&task_c))
        .unwrap()
        .is_empty());

    // New spend on task_c crosses the SAME policy revision again -> MUST re-trigger hold & notice!
    fx.office
        .cost_usage_record(
            &fx.owner,
            usage_event(
                "evt-retrigger-2",
                &fx.worker_agent,
                "2026-04-02T11:00:00.000Z",
                (1_500, 0),
                Some(&task_c),
                Some(&fx.project_a),
                None,
            ),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        fx.office
            .active_budget_holds(None, Some(&task_c))
            .unwrap()
            .len(),
        1
    );
    assert!(fx
        .office
        .open_budget_inbox_notices()
        .unwrap()
        .iter()
        .any(|n| n.notice.budget_id == b_c.id));

    // 4. Stopping reached task_c clears its owner Inbox notice and subsequent reads NEVER resurrect it (`Pass 2 High #1`).
    fx.office
        .budget_stop(
            &fx.owner,
            BudgetStopParams {
                operation_id: next_uuid(),
                budget_id: b_c.id.clone(),
                reason: Some("stop after retrigger".into()),
            },
        )
        .unwrap()
        .unwrap();
    let _ = fx
        .office
        .budget_get(
            &fx.owner,
            BudgetGetParams {
                budget_id: Some(b_c.id.clone()),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
    assert!(!fx
        .office
        .open_budget_inbox_notices()
        .unwrap()
        .iter()
        .any(|n| n.notice.budget_id == b_c.id));
}

fn run_pass2_extended_reset_and_stop_suite(fx: &CostBudgetFixture) {
    // 1. `budget.stop` rejects release warning caps (`Pass 2 Low #5`).
    let rel = fx
        .office
        .budget_list(
            &fx.owner,
            BudgetListParams {
                scope_kind: Some(BudgetScopeKind::Release),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap()
        .budgets
        .into_iter()
        .next()
        .expect("release budget exists");
    assert_eq!(
        fx.office
            .budget_stop(
                &fx.owner,
                BudgetStopParams {
                    operation_id: next_uuid(),
                    budget_id: rel.id,
                    reason: None,
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );

    // 2. Lowering an already-extended task budget resets `extension_used` BEFORE evaluation:
    // zero phantom `Extended` holds and auto-blocks task cleanly (`Pass 2 High #3`).
    let team_a = fx
        .office
        .task_inspect(&fx.task_a)
        .unwrap()
        .unwrap()
        .task
        .team_id;
    let task_d = seed_task(
        &fx.office,
        &fx.owner,
        &team_a,
        &fx.project_a,
        "Extended reset task",
    );
    let rev = fx
        .office
        .task_inspect(&task_d)
        .unwrap()
        .unwrap()
        .task
        .revision;
    fx.office
        .task_transition(
            &fx.owner,
            TasksTransitionParams {
                command_id: next_uuid(),
                task: task_d.clone(),
                expected_revision: rev,
                status: TaskStatus::InProgress,
                blocked_reason: None,
                resolution: None,
                note: None,
            },
        )
        .unwrap()
        .unwrap();
    // Spend 15_000 on task_d, set limit 15_000, extend +20% -> effective 18_000 (`extension_used == true`).
    fx.office
        .cost_usage_record(
            &fx.owner,
            usage_event(
                "evt-ext-reset-1",
                &fx.worker_agent,
                "2026-04-02T12:00:00.000Z",
                (1_500, 0),
                Some(&task_d),
                Some(&fx.project_a),
                None,
            ),
        )
        .unwrap()
        .unwrap();
    let set_op_d = next_uuid();
    let b_d = fx
        .office
        .budget_set(
            &fx.owner,
            BudgetSetParams {
                operation_id: set_op_d.clone(),
                scope_kind: BudgetScopeKind::Task,
                task_id: Some(task_d.clone()),
                agent_id: None,
                release_id: None,
                period_kind: BudgetPeriodKind::Lifetime,
                unit: BudgetUnit::MicroUsd,
                limit_amount: 15_000,
                time_zone: None,
                time_zone_source: None,
                reason: None,
            },
        )
        .unwrap()
        .unwrap()
        .budget;
    // Cross-action reuse of `set_op_d` on `budget_extend_once` -> Conflict (`Pass 2 High #2`).
    assert_eq!(
        fx.office
            .budget_extend_once(
                &fx.owner,
                BudgetExtendOnceParams {
                    operation_id: set_op_d,
                    budget_id: b_d.id.clone(),
                    reason: None,
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    fx.office
        .budget_extend_once(
            &fx.owner,
            BudgetExtendOnceParams {
                operation_id: next_uuid(),
                budget_id: b_d.id.clone(),
                reason: None,
            },
        )
        .unwrap()
        .unwrap();
    // Move task_d back to in_progress, then lower limit to 14_000 (`14_000 <= 15_000 < extended(14_000)=16_800`).
    let rev2 = fx
        .office
        .task_inspect(&task_d)
        .unwrap()
        .unwrap()
        .task
        .revision;
    fx.office
        .task_transition(
            &fx.owner,
            TasksTransitionParams {
                command_id: next_uuid(),
                task: task_d.clone(),
                expected_revision: rev2,
                status: TaskStatus::InProgress,
                blocked_reason: None,
                resolution: None,
                note: None,
            },
        )
        .unwrap()
        .unwrap();
    let lowered_14 = fx
        .office
        .budget_set(
            &fx.owner,
            BudgetSetParams {
                operation_id: next_uuid(),
                scope_kind: BudgetScopeKind::Task,
                task_id: Some(task_d.clone()),
                agent_id: None,
                release_id: None,
                period_kind: BudgetPeriodKind::Lifetime,
                unit: BudgetUnit::MicroUsd,
                limit_amount: 14_000,
                time_zone: None,
                time_zone_source: None,
                reason: Some("lower below spend".into()),
            },
        )
        .unwrap()
        .unwrap()
        .budget;
    assert!(!lowered_14.extension_used);
    assert_eq!(
        fx.office
            .task_inspect(&task_d)
            .unwrap()
            .unwrap()
            .task
            .status,
        TaskStatus::Blocked
    );
    assert_eq!(
        fx.office
            .active_budget_holds(None, Some(&task_d))
            .unwrap()
            .len(),
        1
    );
    // Extend 14_000 -> 16_800, then lower to 10_000 (`15_000 >= extended(10_000)=12_000`): still ONLY 1 hold & 1 open notice!
    fx.office
        .budget_extend_once(
            &fx.owner,
            BudgetExtendOnceParams {
                operation_id: next_uuid(),
                budget_id: b_d.id.clone(),
                reason: None,
            },
        )
        .unwrap()
        .unwrap();
    let lowered_10 = fx
        .office
        .budget_set(
            &fx.owner,
            BudgetSetParams {
                operation_id: next_uuid(),
                scope_kind: BudgetScopeKind::Task,
                task_id: Some(task_d.clone()),
                agent_id: None,
                release_id: None,
                period_kind: BudgetPeriodKind::Lifetime,
                unit: BudgetUnit::MicroUsd,
                limit_amount: 10_000,
                time_zone: None,
                time_zone_source: None,
                reason: Some("lower far below spend".into()),
            },
        )
        .unwrap()
        .unwrap()
        .budget;
    assert!(!lowered_10.extension_used);
    assert_eq!(
        fx.office
            .active_budget_holds(None, Some(&task_d))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        fx.office
            .open_budget_inbox_notices()
            .unwrap()
            .iter()
            .filter(|n| n.notice.budget_id == b_d.id)
            .count(),
        1
    );

    // 3. Unknown/unpriced usage record (`cost_micro_usd == None`) increments `unpriced_records` (`Pass 2 Low #6`).
    let mut unknown_usage = usage_event(
        "evt-unpriced-unknown",
        &fx.worker_agent,
        "2026-04-02T12:30:00.000Z",
        (0, 0),
        Some(&task_d),
        Some(&fx.project_a),
        None,
    );
    unknown_usage.provider = None;
    unknown_usage.model = None;
    unknown_usage.input_tokens = None;
    unknown_usage.output_tokens = None;
    unknown_usage.cache_read_tokens = None;
    unknown_usage.cache_write_tokens = None;
    unknown_usage.reasoning_tokens = None;
    fx.office
        .cost_usage_record(&fx.owner, unknown_usage)
        .unwrap()
        .unwrap();
    let d_view = fx
        .office
        .budget_get(
            &fx.owner,
            BudgetGetParams {
                budget_id: Some(b_d.id),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap()
        .budget
        .unwrap();
    assert_eq!(d_view.unpriced_records, 1);
    assert_eq!(d_view.usage_status, Some(CompletenessStatus::Partial));
}

#[test]
fn l2_and_l3_conformance_on_memory_engine() {
    let fx = seed_fixture(EngineChoice::Memory);
    run_l2_accounting_suite(&fx);
    run_l3_task_and_agent_budget_suite(&fx);
    run_l3_agent_daily_monthly_and_release_suite(&fx);
    run_pass1_security_and_retrigger_suite(&fx);
    run_pass2_extended_reset_and_stop_suite(&fx);
}

#[test]
fn l2_and_l3_conformance_on_sqlite_with_protected_db_delete_and_rebuild() {
    let fx = seed_fixture(EngineChoice::Sqlite);
    run_l2_accounting_suite(&fx);
    run_l3_task_and_agent_budget_suite(&fx);
    run_l3_agent_daily_monthly_and_release_suite(&fx);
    run_pass1_security_and_retrigger_suite(&fx);
    run_pass2_extended_reset_and_stop_suite(&fx);

    let before_costs = fx
        .office
        .cost_query(&fx.owner, CostQueryParams::default())
        .unwrap()
        .unwrap();
    let before_budgets = fx
        .office
        .budget_list(&fx.owner, BudgetListParams::default())
        .unwrap()
        .unwrap();
    let office_id = fx.office.info().office_id.clone();
    let protected_dir = fx.roots.runtime.join(&office_id).join("protected");
    let acc_db = protected_dir.join("accounting.db");
    let bud_db = protected_dir.join("budgets.db");
    assert!(acc_db.is_file());
    assert!(bud_db.is_file());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&protected_dir)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&acc_db).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&bud_db).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    let CostBudgetFixture {
        _tmp,
        folder,
        roots,
        custody,
        clock,
        office,
        owner,
        pm_agent,
        ..
    } = fx;
    office.close().unwrap();

    // Delete BOTH isolated projections and verify full automatic rebuild from durable files!
    for base in [&acc_db, &bud_db] {
        for suffix in ["", "-wal", "-shm"] {
            let mut p = base.as_os_str().to_owned();
            p.push(suffix);
            let _ = std::fs::remove_file(std::path::Path::new(&p));
        }
    }
    let instance_id = custody.identity.instance_id.as_str().to_owned();
    let reopened = Office::open_with(
        folder,
        None,
        &instance_id,
        roots,
        custody,
        Options {
            engine: EngineChoice::Sqlite,
            clock,
            testing: Default::default(),
            before_final_checks: None,
        },
    )
    .expect("reopen after deleting protected accounting.db & budgets.db");

    let after_costs = reopened
        .cost_query(&owner, CostQueryParams::default())
        .unwrap()
        .unwrap();
    let after_budgets = reopened
        .budget_list(&owner, BudgetListParams::default())
        .unwrap()
        .unwrap();
    assert_eq!(
        after_costs.summary.known_cost_micro_usd,
        before_costs.summary.known_cost_micro_usd
    );
    assert_eq!(
        after_costs.summary.record_count,
        before_costs.summary.record_count
    );
    assert_eq!(after_budgets.budgets.len(), before_budgets.budgets.len());
    // Post-rebuild agent budget for `pm_agent` (who authored task_b before rebuild) upgrades mirror `'person' -> 'agent'` (`Pass 2 Med #4`).
    reopened
        .budget_set(
            &owner,
            BudgetSetParams {
                operation_id: next_uuid(),
                scope_kind: BudgetScopeKind::Agent,
                task_id: None,
                agent_id: Some(pm_agent),
                release_id: None,
                period_kind: BudgetPeriodKind::Day,
                unit: BudgetUnit::MicroUsd,
                limit_amount: 50_000,
                time_zone: Some("UTC".into()),
                time_zone_source: Some(BudgetTimeZoneSource::OwnerPerson),
                reason: None,
            },
        )
        .unwrap()
        .unwrap();
    reopened.close().unwrap();
}

fn seed_subscription_connection(office: &Office, owner: &str) -> String {
    use org_proto::hiring::{ProviderConnectionCreate, ProviderConnectionCreateParams};
    use org_proto::Request;
    let conn_id = next_uuid();
    let req = Request::new::<ProviderConnectionCreate>(
        1,
        &ProviderConnectionCreateParams {
            command_id: next_uuid(),
            connection_id: conn_id.clone(),
            default_profile_id: next_uuid(),
            key: "sub-claude-1".into(),
            name: "Claude Sub 1".into(),
            provider_type: "anthropic".into(),
            auth_mode: "subscription_cli".into(),
            subscription_identity: Some("sub-ident-1".into()),
            runtime_driver: "claude".into(),
            endpoint_url: None,
            max_concurrent_turns: 1,
        },
    )
    .unwrap();
    let resp = office.dispatch_hiring_rpc(Some(owner), &req).unwrap();
    assert!(matches!(resp.outcome, org_proto::Outcome::Result(_)));
    conn_id
}

fn sample_subscription_snapshot(
    office: &Office,
    conn_id: &str,
    now_ms: u64,
    attempt_seq: u64,
    daily_tokens: u64,
) -> crate::accounting::StoredSubscriptionUsageSnapshot {
    use org_proto::costs::{
        SubscriptionModelUsageView, SubscriptionUsageAuthSource, SubscriptionUsageStatus,
        SubscriptionUsageWindowView,
    };
    let ts = crate::time::format_rfc3339_ms(now_ms);
    crate::accounting::StoredSubscriptionUsageSnapshot {
        organization_id: office.info().org_id.clone(),
        office_id: office.info().office_id.clone(),
        connection_id: conn_id.to_owned(),
        attempt_seq,
        status: SubscriptionUsageStatus::Ok,
        auth_source: SubscriptionUsageAuthSource::DedicatedUsageCredential,
        collected_at: ts.clone(),
        last_ok_collected_at: Some(ts),
        fresh_until: Some(crate::time::format_rfc3339_ms(
            now_ms + crate::accounting::SUBSCRIPTION_USAGE_FRESHNESS_MS,
        )),
        issue_code: None,
        message: None,
        models: vec![SubscriptionModelUsageView {
            model: "claude-sonnet-4".into(),
            total: SubscriptionUsageWindowView {
                requests: Some(40),
                total_tokens: Some(250_000),
                ..Default::default()
            },
            short_term: None,
            daily: SubscriptionUsageWindowView {
                requests: Some(5),
                total_tokens: Some(daily_tokens),
                utilization_bps: Some(1_250),
                ..Default::default()
            },
            weekly: SubscriptionUsageWindowView {
                requests: Some(25),
                total_tokens: Some(140_000),
                utilization_bps: Some(4_500),
                ..Default::default()
            },
        }],
    }
}

fn verify_sqlite_subscription_usage_rebuild(fx: CostBudgetFixture) {
    use org_proto::costs::{CostSubscriptionUsageListParams, SubscriptionUsageStatus};
    let acc_db = fx
        .roots
        .runtime
        .join(&fx.office.info().office_id)
        .join("protected")
        .join("accounting.db");
    assert!(acc_db.exists());
    let CostBudgetFixture {
        _tmp,
        folder,
        roots,
        custody,
        clock,
        office,
        owner,
        ..
    } = fx;
    office.close().unwrap();
    for suffix in ["", "-wal", "-shm"] {
        let mut p = acc_db.as_os_str().to_owned();
        p.push(suffix);
        let _ = std::fs::remove_file(std::path::Path::new(&p));
    }
    let instance_id = custody.identity.instance_id.as_str().to_owned();
    let reopened = Office::open_with(
        folder,
        None,
        &instance_id,
        roots,
        custody,
        Options {
            engine: EngineChoice::Sqlite,
            clock,
            testing: Default::default(),
            before_final_checks: None,
        },
    )
    .expect("reopen after deleting protected/accounting.db");
    let rebuilt = reopened
        .cost_subscription_usage_list(&owner, CostSubscriptionUsageListParams::default())
        .unwrap()
        .unwrap();
    assert_eq!(rebuilt.snapshots.len(), 1);
    assert_eq!(rebuilt.snapshots[0].status, SubscriptionUsageStatus::Ok);
    assert_eq!(rebuilt.snapshots[0].models.len(), 1);
    assert_eq!(
        rebuilt.snapshots[0].models[0].daily.total_tokens,
        Some(30_000)
    );
    assert!(rebuilt.snapshots[0].stale);
    assert!(rebuilt.partial_totals);
    assert_eq!(rebuilt.excluded_snapshot_count, 1);
    assert!(rebuilt.totals_by_model.is_empty());

    // After restart, next_subscription_attempt_seq seeds above persisted high-water mark (200)
    // so new refreshes overwrite cleanly instead of being silently fenced out.
    let next_seq = reopened.next_subscription_attempt_seq();
    assert!(next_seq > 200);
    let refreshed = sample_subscription_snapshot(
        &reopened,
        &rebuilt.snapshots[0].connection_id,
        reopened.current_time_ms(),
        next_seq,
        42_000,
    );
    reopened
        .save_subscription_usage_snapshot(refreshed)
        .unwrap()
        .unwrap();
    let after_refresh = reopened
        .cost_subscription_usage_list(&owner, CostSubscriptionUsageListParams::default())
        .unwrap()
        .unwrap();
    assert!(!after_refresh.snapshots[0].stale);
    assert_eq!(
        after_refresh.totals_by_model[0].daily.total_tokens,
        Some(42_000)
    );
    reopened.close().unwrap();
}

#[test]
fn task79_subscription_usage_dual_engine_staleness_and_sqlite_rebuild() {
    use org_proto::costs::CostSubscriptionUsageListParams;
    for engine in [EngineChoice::Memory, EngineChoice::Sqlite] {
        let fx = seed_fixture(engine);
        let conn_id = seed_subscription_connection(&fx.office, &fx.owner);
        assert_eq!(
            fx.office
                .cost_subscription_usage_list(
                    &fx.pm_person,
                    CostSubscriptionUsageListParams::default()
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotAuthorized
        );
        let now_ms = fx.office.current_time_ms();
        let newer = sample_subscription_snapshot(&fx.office, &conn_id, now_ms, 200, 30_000);
        fx.office
            .save_subscription_usage_snapshot(newer)
            .unwrap()
            .unwrap();
        // Stale lower attempt_seq must be fenced out and NOT overwrite the newer snapshot!
        let older_attempt =
            sample_subscription_snapshot(&fx.office, &conn_id, now_ms, 199, 999_999);
        fx.office
            .save_subscription_usage_snapshot(older_attempt)
            .unwrap()
            .unwrap();
        let fresh = fx
            .office
            .cost_subscription_usage_list(&fx.owner, CostSubscriptionUsageListParams::default())
            .unwrap()
            .unwrap();
        assert_eq!(fresh.snapshots.len(), 1);
        assert!(!fresh.snapshots[0].stale);
        assert!(!fresh.is_api_equivalent_estimate);
        assert!(!fresh.partial_totals);
        assert_eq!(fresh.excluded_snapshot_count, 0);
        assert_eq!(fresh.totals_by_model[0].daily.total_tokens, Some(30_000));
        assert_eq!(fresh.totals_by_model[0].weekly.total_tokens, Some(140_000));

        fx.clock
            .set(now_ms + crate::accounting::SUBSCRIPTION_USAGE_FRESHNESS_MS + 1_000);
        let stale = fx
            .office
            .cost_subscription_usage_list(&fx.owner, CostSubscriptionUsageListParams::default())
            .unwrap()
            .unwrap();
        assert!(stale.snapshots[0].stale);
        assert!(stale.partial_totals);
        assert_eq!(stale.excluded_snapshot_count, 1);
        assert!(stale.totals_by_model.is_empty());

        if engine == EngineChoice::Sqlite {
            verify_sqlite_subscription_usage_rebuild(fx);
        }
    }
}
