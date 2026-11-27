//! Integration test for `office.metrics` RPC authorization and aggregation (`TASK-136` AC #2).

use std::sync::Arc;
use std::time::{Duration, Instant};

use org_agent_runtime::{Usage, UsageScope};
use org_office::rules::OrgCommand;
use org_office::{Office, TestOffice};
use org_proto::hiring::{
    HireProposalWire, HrHire, HrHireParams, ProviderConnectionCreate,
    ProviderConnectionCreateParams,
};
use org_proto::org::MemberAddPersonParams;
use org_proto::telemetry::{OfficeMetrics, OfficeMetricsParams, OfficeMetricsResult};
use org_proto::{ErrorCode, Outcome, Request};

use super::*;

#[test]
fn office_metrics_rpc_enforces_owner_only_and_aggregates_telemetry() {
    let tmp = tempfile::tempdir().unwrap();
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner = office.owner_member_id().to_owned();
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();

    // Add a non-owner person.
    let person_id = new_uuid_v7();
    office
        .submit_org(
            &owner,
            OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: new_uuid_v7(),
                member_id: person_id.clone(),
                name: "Teammate".into(),
                handle: Some("teammate".into()),
                home_team_id: Some(root_team.clone()),
                contact: None,
            }),
        )
        .unwrap()
        .unwrap();

    // Hire an agent.
    let conn_id = new_uuid_v7();
    let prof_id = new_uuid_v7();
    let conn_req = Request::new::<ProviderConnectionCreate>(
        1,
        &ProviderConnectionCreateParams {
            command_id: new_uuid_v7(),
            connection_id: conn_id,
            default_profile_id: prof_id.clone(),
            key: "claude-metrics".into(),
            name: "Claude Metrics".into(),
            provider_type: "anthropic".into(),
            auth_mode: "api_key".into(),
            subscription_identity: None,
            runtime_driver: "claude-code".into(),
            endpoint_url: None,
            max_concurrent_turns: 2,
        },
    )
    .unwrap();
    assert!(matches!(
        office
            .dispatch_hiring_rpc(Some(&owner), &conn_req)
            .unwrap()
            .outcome,
        Outcome::Result(_)
    ));

    let agent_id = new_uuid_v7();
    let hire_req = Request::new::<HrHire>(
        2,
        &HrHireParams {
            command_id: new_uuid_v7(),
            request_id: new_uuid_v7(),
            employee_id: agent_id.clone(),
            home_team_id: root_team,
            role_id: None,
            proposal: HireProposalWire {
                name: "Metrics Bot".into(),
                handle: "metrics-bot".into(),
                title_in_team: Some("Engineer".into()),
                reports_to_employee_id: Some(owner.clone()),
                model_profile_id: Some(prof_id),
                model_id: Some("claude-sonnet-4-6".into()),
                permission_preset: Some("workspace_edits".into()),
                effort: Some("medium".into()),
                ..HireProposalWire::default()
            },
            reason: "Telemetry test".into(),
        },
    )
    .unwrap();
    assert!(matches!(
        office
            .dispatch_hiring_rpc(Some(&owner), &hire_req)
            .unwrap()
            .outcome,
        Outcome::Result(_)
    ));

    let mgr = SessionManager::new(
        Arc::clone(&office),
        SessionManagerConfig::production(tmp.path().join("office.sock")),
    );

    // Seed two turns on the collector for `agent_id`.
    let t0 = Instant::now();
    mgr.telemetry().record_session_started(
        &agent_id,
        Some("claude"),
        Some("claude-sonnet-4-6"),
        Some("high"),
    );
    for (lat_ms, in_tok, cache_tok, out_tok, denials) in [
        (100u64, 1_000u64, 3_000u64, 200u64, 1u64),
        (300u64, 500u64, 1_500u64, 100u64, 0u64),
    ] {
        mgr.telemetry()
            .record_turn_started(&agent_id, Some("claude-sonnet-4-6"), Some("high"), t0);
        let mut usage = Usage::new(UsageScope::TurnDelta, UsageScope::TurnDelta);
        usage.model = Some("claude-sonnet-4-6".into());
        usage.input = in_tok;
        usage.output = out_tok;
        usage.cache_read = cache_tok;
        usage.reasoning = 50;
        mgr.telemetry()
            .record_usage(&agent_id, "run-metrics", &usage);
        mgr.telemetry().record_sandbox_denial(&agent_id, denials);
        mgr.telemetry().record_turn_finished(
            &agent_id,
            true,
            Some(64 * 1024 * 1024),
            t0 + Duration::from_millis(lat_ms),
            1_700_000_000_000 + lat_ms,
        );
    }

    let req = Request::new::<OfficeMetrics>(99, &OfficeMetricsParams::default()).unwrap();

    // Unauthenticated, non-owner person, and agent MUST fail closed with NotAuthorized.
    for (label, caller) in [
        ("unauthenticated", None),
        ("non-owner person", Some(person_id.as_str())),
        ("agent", Some(agent_id.as_str())),
    ] {
        let resp = mgr.dispatch_rpc(caller, &req).expect("handled");
        assert!(
            matches!(resp.outcome, Outcome::Error(ref e) if e.code == ErrorCode::NotAuthorized),
            "{label} must be refused with NotAuthorized, got: {:?}",
            resp.outcome
        );
    }

    // Owner succeeds and sees expected aggregate & per-agent metrics.
    let resp = mgr.dispatch_rpc(Some(&owner), &req).expect("handled");
    let metrics: OfficeMetricsResult = match resp.outcome {
        Outcome::Result(val) => serde_json::from_value(val).unwrap(),
        Outcome::Error(e) => panic!("owner office.metrics failed: {e:?}"),
    };
    assert_eq!(metrics.aggregate.recent_turn_count, 2);
    assert_eq!(metrics.aggregate.total_turn_count, 2);
    assert_eq!(metrics.aggregate.p50_wall_latency_ms, Some(100));
    assert_eq!(metrics.aggregate.p95_wall_latency_ms, Some(300));
    assert_eq!(metrics.aggregate.avg_wall_latency_ms, Some(200));
    assert_eq!(metrics.aggregate.input_tokens, 1_500);
    assert_eq!(metrics.aggregate.output_tokens, 300);
    assert_eq!(metrics.aggregate.cached_read_tokens, 4_500);
    assert_eq!(metrics.aggregate.thought_tokens, 100);
    assert!((metrics.aggregate.cache_hit_ratio.unwrap() - 0.75).abs() < 1e-9);
    assert_eq!(metrics.aggregate.sandbox_denial_count, 1);

    let bot = metrics
        .agents
        .iter()
        .find(|a| a.employee_id == agent_id)
        .expect("agent row present");
    assert_eq!(bot.handle, "metrics-bot");
    assert_eq!(bot.active_model.as_deref(), Some("claude-sonnet-4-6"));
    assert_eq!(bot.active_effort.as_deref(), Some("high"));
    assert_eq!(bot.peak_rss_bytes, Some(64 * 1024 * 1024));
    assert_eq!(bot.sandbox_denial_count, 1);
}
