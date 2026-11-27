use org_accounting::UsageCoverageSummary;
use org_proto::budgets::{
    BudgetHoldState, BudgetPeriodKind, BudgetPolicyState, BudgetScopeKind, BudgetStateSummary,
    BudgetTimeZoneSource, BudgetUnit,
};
use org_proto::costs::{
    AttributionBasis, CompletenessStatus, UsageActorKind, UsageCostBasis, UsageRecordView,
    UsageScopeKind,
};
use org_proto::ErrorCode;

use crate::eval::{apply_extend_once, evaluate_budget, ExtendOnceRequest};
use crate::memory::MemoryBudgetStore;
use crate::{
    BudgetAggregate, BudgetPolicyVersion, BudgetRecord, BudgetStore, SatelliteReadOnlyBudgetStore,
};

const ORG: &str = "01900000-0000-7000-8000-000000000001";
const TASK: &str = "01900000-0000-7000-8000-000000000004";
const OWNER: &str = "01900000-0000-7000-8000-000000000005";

fn sample_task_budget(limit: u64) -> BudgetAggregate {
    BudgetAggregate {
        budget: BudgetRecord {
            organization_id: ORG.into(),
            id: "01900000-0000-7000-8000-000000000020".into(),
            current_revision: 1,
            scope_kind: BudgetScopeKind::Task,
            task_id: Some(TASK.into()),
            agent_id: None,
            release_id: None,
            period_kind: BudgetPeriodKind::Lifetime,
            time_zone: "UTC".into(),
            time_zone_source: BudgetTimeZoneSource::Hq,
            time_zone_subject_ref: None,
            created_at: "2026-03-08T09:00:00.000Z".into(),
        },
        versions: vec![BudgetPolicyVersion {
            organization_id: ORG.into(),
            budget_id: "01900000-0000-7000-8000-000000000020".into(),
            revision: 1,
            state: BudgetPolicyState::Active,
            unit: BudgetUnit::MicroUsd,
            limit_amount: Some(limit),
            time_zone: "UTC".into(),
            time_zone_source: BudgetTimeZoneSource::Hq,
            time_zone_subject_ref: None,
            changed_by_id: OWNER.into(),
            changed_at: "2026-03-08T09:00:00.000Z".into(),
            reason: Some("initial cap".into()),
        }],
        periods: Vec::new(),
        events: Vec::new(),
        notices: Vec::new(),
        holds: Vec::new(),
        pending_stop: None,
    }
}

fn task_usage(id: &str, cost_micro_usd: u64) -> UsageRecordView {
    UsageRecordView {
        id: id.into(),
        origin_office_id: "01900000-0000-7000-8000-000000000002".into(),
        producer_id: "prod-1".into(),
        source_event_id: format!("evt-{id}"),
        source_sequence: 1,
        execution_office_id: None,
        run_id: None,
        actor_kind: UsageActorKind::Agent,
        employee_id: Some("01900000-0000-7000-8000-000000000003".into()),
        runtime: "claude_code".into(),
        provider: Some("anthropic".into()),
        model: Some("claude-sonnet-4".into()),
        occurred_at: "2026-03-08T10:00:00.000Z".into(),
        recorded_at: "2026-03-08T10:00:01.000Z".into(),
        recorded_by_kind: "system".into(),
        recorded_by_id: None,
        normalization_version: 1,
        source_metadata_json: "{}".into(),
        payload_sha256: "0".repeat(64),
        input_tokens: Some(100),
        output_tokens: Some(50),
        cache_read_tokens: Some(0),
        cache_write_tokens: Some(0),
        reasoning_tokens: Some(0),
        token_status: CompletenessStatus::Complete,
        token_issue: None,
        price_id: None,
        cost_micro_usd: Some(cost_micro_usd),
        cost_basis: UsageCostBasis::TokenEstimate,
        cost_status: CompletenessStatus::Complete,
        cost_issue: None,
        scope_kind: UsageScopeKind::Task,
        task_id: Some(TASK.into()),
        project_id: None,
        team_id: None,
        feature_id: None,
        milestone_id: None,
        release_id: None,
        task_type_snapshot: Some("task".into()),
        work_type: None,
        attribution_basis: AttributionBasis::CurrentTask,
        supersedes_id: None,
        correction_kind: None,
        correction_reason: None,
    }
}

#[test]
fn evaluate_budget_triggers_80_and_100_holds_extends_once_and_strips_hidden_amounts() {
    let mut agg = sample_task_budget(10_000);
    let mut seq = 100u64;
    let mut next_id = || {
        seq += 1;
        format!("01900000-0000-7000-8000-{seq:012x}")
    };
    let cov = UsageCoverageSummary::default();
    let usage = [task_usage("u1", 10_000)];
    let out = evaluate_budget(
        &mut agg,
        &usage,
        &cov,
        1_772_964_001_000,
        "2026-03-08T10:00:01.000Z",
        &mut |_| Ok(true),
        &mut next_id,
    )
    .expect("eval succeeds");
    assert!(out.changed);
    assert_eq!(out.newly_reached_task_id.as_deref(), Some(TASK));
    assert_eq!(
        agg.active_hold().unwrap().state,
        BudgetHoldState::StopPending
    );
    assert_eq!(agg.notices.len(), 2);

    let hidden = agg.to_view(false);
    assert_eq!(hidden.limit_amount, None);
    assert_eq!(hidden.used_amount, None);
    assert_eq!(hidden.state_summary, BudgetStateSummary::StopPending);

    apply_extend_once(
        &mut agg,
        ExtendOnceRequest {
            operation_id: "op-ext-1",
            actor_id: OWNER,
            req_hash: "hash-1",
            reason: Some("finish task".into()),
            incomplete_streams: 0,
            now_utc: "2026-03-08T10:01:00.000Z",
        },
        &mut next_id,
    )
    .expect("extend once succeeds");
    assert!(agg.active_hold().is_none());
    let visible = agg.to_view(true);
    assert_eq!(visible.effective_limit, Some(12_000));
    assert_eq!(visible.remaining_amount, Some(2_000));
    assert!(!visible.can_extend_once);
}

#[test]
fn satellite_read_only_budget_store_rejects_writes_and_delegates_reads() {
    let mem = MemoryBudgetStore::new();
    let agg = sample_task_budget(5_000);
    mem.save_aggregate(&agg, &[]).unwrap().unwrap();
    let ro = SatelliteReadOnlyBudgetStore::new(mem);
    let refused = ro.save_aggregate(&agg, &[]).unwrap().unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotAuthorized);
    assert_eq!(ro.list_all(ORG).unwrap().len(), 1);
    assert!(ro.get_by_id(ORG, &agg.budget.id).unwrap().is_some());
}
