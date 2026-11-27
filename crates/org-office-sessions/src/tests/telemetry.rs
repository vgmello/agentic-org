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

#[test]
fn crashed_turn_before_turn_ended_is_finalized_as_failed_with_tree_rss_sampler() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingTreeSampler(AtomicUsize);
    impl crate::manager::ProcessMemorySampler for CountingTreeSampler {
        fn process_tree_rss_bytes(&self, _root_pid: u32) -> Option<u64> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Some(99 * 1024 * 1024)
        }
    }

    let _ = org_platform::process::harden_self();
    let bin_dir = tempfile::tempdir().unwrap();
    let sock_dir = tempfile::tempdir().unwrap();
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner = office.owner_member_id().to_owned();
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();

    let (agent_id, _conn_id) = hire_agent_with_runtime(
        &office,
        &owner,
        &root_team,
        "anthropic",
        "claude",
        "crash-bot",
        "claude-sonnet-4-6",
    );

    // Fake Claude binary: completes initialize handshake (`Ready` -> pump sends Turn 1 ->
    // `TurnStarted`), emits `system/init`, then exits immediately WITHOUT emitting a `result`
    // (`TurnEnded`) line so `finish_pump` must finalize the orphaned `InFlightTurn`.
    let fake_bin = write_fake_cli(
        bin_dir.path(),
        "claude",
        r#"#!/bin/sh
read init_line
echo '{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["crash-bot"],"current_permission_mode":"default","pid":1234,"version":"2.1.311"}}}'
read user_line
echo '{"type":"system","subtype":"init","session_id":"00000000-0000-4000-8000-0000000000aa","permissionMode":"default"}'
sleep 0.05
exit 1
"#,
    );

    let sampler = Arc::new(CountingTreeSampler(AtomicUsize::new(0)));
    let mut reg = RuntimeRegistry::production();
    reg.register(
        Arc::new(org_agent_runtime_claude::ClaudeRuntime::new()),
        Some(fake_bin),
    );
    let mut cfg = SessionManagerConfig::production(sock_dir.path().join("office.sock"));
    cfg.runtimes = reg;
    cfg.helper_dir = Some(bin_dir.path().to_path_buf());
    cfg.resource_limits.memory_sampler =
        Arc::clone(&sampler) as Arc<dyn crate::manager::ProcessMemorySampler>;
    let mgr = SessionManager::new(Arc::clone(&office), cfg);

    mgr.start_session(
        &owner,
        &org_proto::sessions::MemberSessionStartParams {
            command_id: new_uuid_v7(),
            employee_id: agent_id.clone(),
            instruction: Some("Do work then crash".into()),
            workspace: None,
            resume_runtime_session_id: None,
            fresh: false,
            model: None,
            effort: None,
        },
    )
    .expect("session starts");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(snap) = mgr.telemetry().snapshot_agent(&agent_id) {
            if snap.total_turn_count >= 1 {
                assert_eq!(snap.recent_turns.len(), 1);
                let sample = &snap.recent_turns[0];
                assert!(
                    !sample.succeeded,
                    "pre-TurnEnded crash must finalize orphaned turn as succeeded=false"
                );
                assert_eq!(sample.rss_bytes, Some(99 * 1024 * 1024));
                assert!(
                    sampler.0.load(Ordering::SeqCst) >= 1,
                    "turn-end telemetry must sample via process_tree_rss_bytes"
                );
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for crashed turn finalization in finish_pump"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    mgr.stop_all("test");
}
