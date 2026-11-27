use org_proto::costs::{
    AttributionBasis, CompletenessStatus, CostBreakdownGroup, CostPriceListParams,
    CostPriceSetParams, CostQueryParams, CostUsageCorrectParams, CostUsageRecordParams,
    SubscriptionModelUsageView, SubscriptionUsageAuthSource, SubscriptionUsageStatus,
    SubscriptionUsageWindowView, UsageActorKind, UsageScopeKind,
};
use org_proto::ErrorCode;

use crate::memory::MemoryAccountingStore;
use crate::query::{execute_cost_query, export_cost_csv};
use crate::{
    aggregate_totals_by_model, build_attribution_correction, build_model_price,
    build_snapshot_view, build_usage_record, canonical_provider_namespace,
    StoredSubscriptionUsageSnapshot, SubscriptionConnectionMeta, SubscriptionCredentialPresence,
};
use crate::{AccountingStore, CostViewerScope, UsageBuildMeta};

const ORG: &str = "01900000-0000-7000-8000-000000000001";
const OFFICE: &str = "01900000-0000-7000-8000-000000000002";
const AGENT: &str = "01900000-0000-7000-8000-000000000003";
const TASK: &str = "01900000-0000-7000-8000-000000000004";
const PROJECT: &str = "01900000-0000-7000-8000-000000000005";

#[test]
fn memory_accounting_store_enforces_price_immutability_usage_replay_and_corrections() {
    let store = MemoryAccountingStore::new();
    let price = build_model_price(
        CostPriceSetParams {
            id: Some("01900000-0000-7000-8000-000000000010".into()),
            provider: "anthropic".into(),
            model: "claude-sonnet-4".into(),
            effective_from: "2026-01-01T00:00:00Z".into(),
            input_micro_usd_per_million: Some(3_000_000),
            output_micro_usd_per_million: Some(15_000_000),
            cache_read_micro_usd_per_million: Some(300_000),
            cache_write_micro_usd_per_million: Some(3_750_000),
        },
        "2026-01-01T00:00:00.000Z",
        "employee",
        None,
        "01900000-0000-7000-8000-000000000099".into(),
    )
    .expect("valid price");
    let saved = store.set_price(ORG, price.clone()).unwrap().unwrap();
    assert_eq!(store.set_price(ORG, saved.clone()).unwrap().unwrap(), saved);

    let mut mutated = price;
    mutated.input_micro_usd_per_million = Some(9_000_000);
    let conflict = store.set_price(ORG, mutated).unwrap().unwrap_err();
    assert_eq!(conflict.code, ErrorCode::Conflict);
    assert_eq!(
        store
            .list_prices(ORG, &CostPriceListParams::default())
            .unwrap()
            .len(),
        1
    );

    let eff = store
        .effective_price(ORG, "anthropic", "claude-sonnet-4", "2026-03-08T10:00:00Z")
        .unwrap()
        .expect("effective price");
    let rec = build_usage_record(
        UsageBuildMeta {
            org_id: ORG,
            origin_office_id: OFFICE,
            sequence: 1,
            recorded_at: "2026-03-08T10:00:01.000Z",
            recorded_by_kind: "system",
            recorded_by_id: None,
            default_id: "01900000-0000-7000-8000-000000000011".into(),
        },
        CostUsageRecordParams {
            id: None,
            producer_id: "prod-1".into(),
            source_event_id: "evt-1".into(),
            run_id: Some("run-1".into()),
            actor_kind: UsageActorKind::Agent,
            employee_id: Some(AGENT.into()),
            runtime: "claude_code".into(),
            provider: Some("anthropic".into()),
            model: Some("claude-sonnet-4".into()),
            occurred_at: "2026-03-08T10:00:00Z".into(),
            input_tokens: Some(1_000_000),
            output_tokens: Some(100_000),
            cache_read_tokens: Some(0),
            cache_write_tokens: Some(0),
            reasoning_tokens: Some(0),
            token_issue: None,
            runtime_cost_micro_usd: None,
            is_subscription: false,
            source_metadata_json: None,
            scope_kind: UsageScopeKind::Task,
            task_id: Some(TASK.into()),
            project_id: Some(PROJECT.into()),
            team_id: None,
            feature_id: None,
            milestone_id: None,
            release_id: None,
            task_type_snapshot: Some("task".into()),
            work_type: None,
            attribution_basis: AttributionBasis::CurrentTask,
        },
        Some(&eff),
    )
    .expect("valid usage record");
    assert_eq!(rec.token_status, CompletenessStatus::Complete);
    assert_eq!(rec.cost_micro_usd, Some(4_500_000));

    let (stored, replayed) = store.append_usage(ORG, rec.clone()).unwrap().unwrap();
    assert!(!replayed);
    assert_eq!(stored.source_sequence, 1);
    let (_, replayed_again) = store.append_usage(ORG, rec).unwrap().unwrap();
    assert!(replayed_again);

    let correction = build_attribution_correction(
        UsageBuildMeta {
            org_id: ORG,
            origin_office_id: OFFICE,
            sequence: 2,
            recorded_at: "2026-03-08T10:05:00.000Z",
            recorded_by_kind: "employee",
            recorded_by_id: Some(AGENT.into()),
            default_id: "01900000-0000-7000-8000-000000000012".into(),
        },
        &stored,
        &CostUsageCorrectParams {
            operation_id: "01900000-0000-7000-8000-000000000013".into(),
            supersedes_id: stored.id.clone(),
            scope_kind: UsageScopeKind::Project,
            task_id: None,
            project_id: None,
            team_id: None,
            feature_id: None,
            milestone_id: None,
            release_id: None,
            reason: "=reclassify to project overhead".into(),
        },
    )
    .expect("valid correction");
    assert_eq!(correction.project_id.as_deref(), Some(PROJECT));
    let (corr_stored, _) = store.append_usage(ORG, correction).unwrap().unwrap();
    assert!(store.is_superseded(ORG, &stored.id).unwrap());

    let effective = store.effective_records(ORG).unwrap();
    assert_eq!(effective.len(), 1);
    assert_eq!(effective[0].id, corr_stored.id);

    let cov = store.coverage_summary(ORG).unwrap();
    assert_eq!(cov.incomplete_streams, 0);
    let watermarks: Vec<crate::VerifiedStreamWatermark> =
        serde_json::from_str(cov.checkpoint_ref.as_deref().unwrap()).unwrap();
    for wm in &watermarks {
        assert!(store.stream_continues_watermark(ORG, wm).unwrap());
    }

    let query_res = execute_cost_query(
        &effective,
        &cov,
        &CostViewerScope::Owner,
        &CostQueryParams {
            group_by: Some(CostBreakdownGroup::Project),
            include_records: true,
            ..Default::default()
        },
        "UTC",
        1_772_964_600_000,
    )
    .unwrap();
    assert_eq!(query_res.summary.known_cost_micro_usd, Some(4_500_000));
    assert_eq!(query_res.breakdown.len(), 1);

    let csv = export_cost_csv(
        &effective,
        &cov,
        &CostViewerScope::Owner,
        &CostQueryParams::default(),
        "UTC",
        1_772_964_600_000,
    )
    .unwrap();
    assert_eq!(csv.row_count, 1);
    assert!(csv.csv.contains("\"'=reclassify to project overhead\""));
}

#[test]
fn subscription_usage_view_and_rollup_deduplicate_identities_and_handle_staleness() {
    assert_eq!(canonical_provider_namespace("claude-code"), "anthropic");
    assert_eq!(canonical_provider_namespace("codex-cli"), "openai");
    assert_eq!(canonical_provider_namespace("agy"), "google");

    let conn = SubscriptionConnectionMeta {
        id: "conn-1",
        key: "claude-main",
        name: "Claude Main",
        provider_type: "claude",
        subscription_identity: Some("acct@example.test"),
    };
    let stored = StoredSubscriptionUsageSnapshot {
        organization_id: ORG.into(),
        office_id: OFFICE.into(),
        connection_id: "conn-1".into(),
        attempt_seq: 1,
        status: SubscriptionUsageStatus::Ok,
        auth_source: SubscriptionUsageAuthSource::DedicatedUsageCredential,
        collected_at: "2026-03-08T10:00:00.000Z".into(),
        last_ok_collected_at: Some("2026-03-08T10:00:00.000Z".into()),
        fresh_until: Some("2026-03-08T10:15:00.000Z".into()),
        issue_code: None,
        message: None,
        models: vec![SubscriptionModelUsageView {
            model: "claude-sonnet-4".into(),
            total: SubscriptionUsageWindowView {
                requests: Some(12),
                total_tokens: Some(42_000),
                ..Default::default()
            },
            daily: SubscriptionUsageWindowView::default(),
            weekly: SubscriptionUsageWindowView::default(),
            short_term: None,
        }],
    };
    let fresh_now = org_proto::parse_rfc3339_ms("2026-03-08T10:05:00.000Z").unwrap();
    let cred = SubscriptionCredentialPresence {
        has_usage_credential: true,
        has_enrolled_credential: false,
        account_hint: Some("acct".into()),
    };
    let view1 = build_snapshot_view(conn, OFFICE, Some(&stored), &cred, fresh_now);
    assert!(!view1.stale);
    let view2 = view1.clone();
    let rollup = aggregate_totals_by_model(&[view1, view2]);
    assert_eq!(rollup.excluded_snapshot_count, 1);
    assert!(rollup.partial_totals);
    assert_eq!(rollup.totals_by_model.len(), 1);
    assert_eq!(rollup.totals_by_model[0].total.requests, Some(12));
}
