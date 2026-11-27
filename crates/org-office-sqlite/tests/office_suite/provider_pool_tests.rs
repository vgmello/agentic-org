//! Dual-engine (`Memory` + `Sqlite` with delete+rebuild & coordinator restart) conformance tests
//! for `TASK-54` (`L4`: ordered provider connection pools, bindings, model discovery, readiness,
//! dual-authority permits, in-memory zero-write heartbeat renewal, cooldowns, and budget-hold
//! interaction; `FC-098`, `FC-099`, `doc-88`, `doc-89`, `TASK-2`).

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use org_proto::budgets::{
    BudgetPeriodKind, BudgetScopeKind, BudgetSetParams, BudgetTimeZoneSource, BudgetUnit,
};
use org_proto::costs::{
    AttributionBasis, CostUsageRecordParams, UsageActorKind, UsageScopeKind, WorkType,
};
use org_proto::hiring::{
    AgentExecutionSet, AgentExecutionSetParams, HireProposalWire, HrHire, HrHireParams,
    ProviderConnectionCreate, ProviderConnectionCreateParams,
};
use org_proto::providers::{
    AgentReadinessCode, AgentReadinessGetParams, ConnectionReadinessStatus,
    DiscoveredModelEntitlement, DiscoveredModelInput, ModelProfileCreateParams,
    ModelProfileModelsParams, ModelProfileUpdateParams, PermitAuthority, ProviderBindingListParams,
    ProviderBindingSetParams, ProviderConnectionListParams, ProviderConnectionRefreshParams,
    ProviderConnectionUpdateParams, ProviderCooldownRecordParams, ProviderQuotaStatusParams,
    TurnPermitState,
};
use org_proto::{ErrorCode, Outcome, Request};
use org_store::testkit::{identity, FakeCustody};
use org_store::{Clock, Timestamp};

use org_office::providers::PermitAcquireOutcome;
use org_office::testkit_support::office_harness::{places, record, request, CA_SEED};
use org_office::testkit_support::store_open::EngineChoice;
use org_office::testkit_support::Options;
use org_office::{Custody, DataRoots, Office};

static NEXT_ID: AtomicU64 = AtomicU64::new(140_000);
const BASE_MS: u64 = 1_775_000_000_000;

fn next_uuid() -> String {
    let n = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let ms = BASE_MS + (n % 10_000);
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
    fn advance(&self, delta_ms: u64) {
        self.0.fetch_add(delta_ms, Ordering::SeqCst);
    }
}
impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        Timestamp(self.0.load(Ordering::SeqCst))
    }
}

struct PoolFixture {
    _tmp: tempfile::TempDir,
    folder: std::path::PathBuf,
    roots: DataRoots,
    custody: Custody,
    clock: Arc<ManualClock>,
    office: Office,
    owner: String,
    agent_id: String,
    conn_a: String,
    conn_b: String,
    default_profile_a: String,
    pool_id: String,
}

fn create_conn(
    office: &Office,
    owner: &str,
    key: &str,
    sub: &str,
    endpoint_url: Option<&str>,
) -> (String, String) {
    let conn_id = next_uuid();
    let prof_id = next_uuid();
    let req = Request::new::<ProviderConnectionCreate>(
        1,
        &ProviderConnectionCreateParams {
            command_id: next_uuid(),
            connection_id: conn_id.clone(),
            default_profile_id: prof_id.clone(),
            key: key.into(),
            name: format!("Conn {key}"),
            provider_type: "anthropic".into(),
            auth_mode: "subscription_cli".into(),
            subscription_identity: Some(sub.into()),
            runtime_driver: "claude".into(),
            endpoint_url: endpoint_url.map(str::to_owned),
            max_concurrent_turns: 1,
        },
    )
    .unwrap();
    let resp = office.dispatch_hiring_rpc(Some(owner), &req).unwrap();
    assert!(matches!(resp.outcome, Outcome::Result(_)), "{resp:?}");
    (conn_id, prof_id)
}

fn seed_pool_fixture(engine: EngineChoice) -> PoolFixture {
    let clock = ManualClock::new(BASE_MS);
    let tmp = tempfile::tempdir().expect("tempdir");
    let (folder, roots) = places(tmp.path());
    std::fs::create_dir_all(&roots.durable).unwrap();
    std::fs::create_dir_all(&roots.runtime).unwrap();
    let custody = Custody {
        identity: identity(),
        authority: Arc::new(FakeCustody::new()),
    };
    let (new, binding) = request();
    let office = Office::provision_with(
        folder.clone(),
        &new,
        None,
        &binding,
        roots.clone(),
        custody.clone(),
        |_| record(CA_SEED),
        Options {
            backend: None,
            engine,
            clock: clock.clone(),
            testing: Default::default(),
            before_final_checks: None,
        },
    )
    .expect("provision office");
    let owner = office.owner_member_id().to_owned();
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();

    let agent_id = next_uuid();
    let hire_req = Request::new::<HrHire>(
        2,
        &HrHireParams {
            command_id: next_uuid(),
            request_id: next_uuid(),
            employee_id: agent_id.clone(),
            home_team_id: root_team,
            role_id: None,
            proposal: HireProposalWire {
                name: "Pool Agent".into(),
                handle: "poolbot".into(),
                title_in_team: Some("Engineer".into()),
                ..Default::default()
            },
            reason: "L4 pool test".into(),
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

    let (conn_a, default_profile_a) = create_conn(
        &office,
        &owner,
        "sub-a",
        "acct-sub-a",
        Some("https://api.a.test"),
    );
    let (conn_b, _) = create_conn(
        &office,
        &owner,
        "sub-b",
        "acct-sub-b",
        Some("http://127.0.0.1:9443"),
    );

    // Create manual ordered pool `[conn_a, conn_b]`.
    let pool_id = next_uuid();
    office
        .model_profile_create(
            &owner,
            ModelProfileCreateParams {
                command_id: next_uuid(),
                profile_id: pool_id.clone(),
                key: "primary-pool".into(),
                name: "Primary Pool".into(),
                description: "Failover across sub A and B".into(),
                connection_ids: vec![conn_a.clone(), conn_b.clone()],
            },
        )
        .unwrap()
        .unwrap();

    // Assign execution config to `agent_id`.
    let exec_req = Request::new::<AgentExecutionSet>(
        3,
        &AgentExecutionSetParams {
            command_id: next_uuid(),
            employee_id: agent_id.clone(),
            expected_version: None,
            model_profile_id: pool_id.clone(),
            model_id: "anthropic/claude-sonnet-4".into(),
            tools_config: vec![],
            permission_preset: Some("safe_ask".into()),
            enabled: true,
            system_prompt_markdown: "Follow instructions.".into(),
            effort: None,
        },
    )
    .unwrap();
    assert!(matches!(
        office
            .dispatch_hiring_rpc(Some(&owner), &exec_req)
            .unwrap()
            .outcome,
        Outcome::Result(_)
    ));

    PoolFixture {
        _tmp: tmp,
        folder,
        roots,
        custody,
        clock,
        office,
        owner,
        agent_id,
        conn_a,
        conn_b,
        default_profile_a,
        pool_id,
    }
}

fn run_pool_lifecycle_and_discovery_suite(fx: &PoolFixture) {
    // 1. Listing connections, updating endpoint_url & rejecting bad URLs / archiving in-use connection or profile.
    assert_eq!(
        fx.office
            .provider_connection_list(&fx.owner, ProviderConnectionListParams::default())
            .unwrap()
            .unwrap()
            .connections
            .len(),
        2
    );
    assert_eq!(
        fx.office
            .provider_connection_update(
                &fx.owner,
                ProviderConnectionUpdateParams {
                    command_id: next_uuid(),
                    connection_id: fx.conn_a.clone(),
                    expected_version: 1,
                    name: None,
                    endpoint_url: Some("http://insecure.example.com".into()),
                    clear_endpoint_url: false,
                    enabled: None,
                    archived: None,
                    max_concurrent_turns: None,
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        fx.office
            .provider_connection_update(
                &fx.owner,
                ProviderConnectionUpdateParams {
                    command_id: next_uuid(),
                    connection_id: fx.conn_a.clone(),
                    expected_version: 1,
                    name: None,
                    endpoint_url: None,
                    clear_endpoint_url: false,
                    enabled: None,
                    archived: Some(true),
                    max_concurrent_turns: None,
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    let archive_err = fx
        .office
        .model_profile_update(
            &fx.owner,
            ModelProfileUpdateParams {
                command_id: next_uuid(),
                profile_id: fx.pool_id.clone(),
                expected_version: 1,
                name: None,
                description: None,
                archived: Some(true),
                connection_ids: None,
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(archive_err.code, ErrorCode::Conflict);
    assert!(archive_err.message.contains("PROFILE_IN_USE"));

    // Converting auto-generated default profile to manual clears `is_auto_generated` & `generated_for_connection_id` (`doc-89`).
    fx.office
        .model_profile_update(
            &fx.owner,
            ModelProfileUpdateParams {
                command_id: next_uuid(),
                profile_id: fx.default_profile_a.clone(),
                expected_version: 1,
                name: Some("Manual A+B".into()),
                description: None,
                archived: None,
                connection_ids: Some(vec![fx.conn_a.clone(), fx.conn_b.clone()]),
            },
        )
        .unwrap()
        .unwrap();
    let converted = fx
        .office
        .load_model_profile(&fx.default_profile_a)
        .unwrap()
        .unwrap();
    assert!(!converted.is_auto_generated);
    assert_eq!(converted.generated_for_connection_id, None);
    assert_eq!(converted.connection_ids.len(), 2);

    // 2. Office-local bindings, subscription mismatch / unverified guards, model discovery & 300s TTL.
    let bind_op = next_uuid();
    let bind_params = ProviderBindingSetParams {
        operation_id: bind_op.clone(),
        connection_id: fx.conn_a.clone(),
        expected_revision: None,
        auth_context_ref: "vault:office-hq/sub-a".into(),
        runtime_binary_path: Some("/usr/local/bin/claude".into()),
        account_identity_ref: Some("acct-sub-a".into()),
    };
    let b1 = fx
        .office
        .provider_binding_set(&fx.owner, bind_params.clone())
        .unwrap()
        .unwrap();
    assert!(!b1.replayed);
    assert_eq!(b1.binding.revision, 1);
    assert!(
        fx.office
            .provider_binding_set(&fx.owner, bind_params)
            .unwrap()
            .unwrap()
            .replayed
    );

    // Subscription identity mismatch -> Unavailable with SUBSCRIPTION_IDENTITY_MISMATCH!
    let bad_refresh = fx
        .office
        .provider_connection_refresh(
            &fx.owner,
            ProviderConnectionRefreshParams {
                connection_id: fx.conn_a.clone(),
                driver_version: Some("2.1.311".into()),
                auth_signed_in: Some(true),
                observed_subscription_identity: Some("wrong-sub".into()),
                operator_attested_subscription: false,
                models: vec![],
                issue_code: None,
                ttl_seconds: Some(300),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        bad_refresh.state.status,
        ConnectionReadinessStatus::Unavailable
    );
    assert_eq!(
        bad_refresh.state.issue_code.as_deref(),
        Some("SUBSCRIPTION_IDENTITY_MISMATCH")
    );

    // Valid refresh on conn_a & conn_b discovers confirmed models.
    let ok_a = fx
        .office
        .provider_connection_refresh(
            &fx.owner,
            ProviderConnectionRefreshParams {
                connection_id: fx.conn_a.clone(),
                driver_version: Some("2.1.311".into()),
                auth_signed_in: Some(true),
                observed_subscription_identity: Some("acct-sub-a".into()),
                operator_attested_subscription: false,
                models: vec![DiscoveredModelInput {
                    model_id: "anthropic/claude-sonnet-4".into(),
                    driver_model_id: "claude-sonnet-4-20250514".into(),
                    entitlement: DiscoveredModelEntitlement::Confirmed,
                    capabilities_json: r#"{"tools":true}"#.into(),
                }],
                issue_code: None,
                ttl_seconds: Some(300),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(ok_a.state.status, ConnectionReadinessStatus::Ready);
    assert_eq!(ok_a.models.len(), 1);

    fx.office
        .provider_binding_set(
            &fx.owner,
            ProviderBindingSetParams {
                operation_id: next_uuid(),
                connection_id: fx.conn_b.clone(),
                expected_revision: None,
                auth_context_ref: "vault:office-hq/sub-b".into(),
                runtime_binary_path: Some("/usr/local/bin/claude".into()),
                account_identity_ref: Some("acct-sub-b".into()),
            },
        )
        .unwrap()
        .unwrap();
    fx.office
        .provider_connection_refresh(
            &fx.owner,
            ProviderConnectionRefreshParams {
                connection_id: fx.conn_b.clone(),
                driver_version: Some("2.1.311".into()),
                auth_signed_in: Some(true),
                observed_subscription_identity: Some("acct-sub-b".into()),
                operator_attested_subscription: false,
                models: vec![DiscoveredModelInput {
                    model_id: "anthropic/claude-sonnet-4".into(),
                    driver_model_id: "claude-sonnet-4-20250514".into(),
                    entitlement: DiscoveredModelEntitlement::Confirmed,
                    capabilities_json: "{}".into(),
                }],
                issue_code: None,
                ttl_seconds: Some(300),
            },
        )
        .unwrap()
        .unwrap();

    let union = fx
        .office
        .model_profile_models(
            &fx.owner,
            ModelProfileModelsParams {
                model_profile_id: fx.pool_id.clone(),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(union.models.len(), 1);
    assert_eq!(union.models[0].supporting_connections.len(), 2);
    assert_eq!(union.models[0].first_priority, 1);
    assert_eq!(union.models[0].ready_connections, 2);

    let ready = fx
        .office
        .agent_readiness_get(
            &fx.owner,
            AgentReadinessGetParams {
                employee_id: fx.agent_id.clone(),
                task_id: None,
            },
        )
        .unwrap()
        .unwrap();
    assert!(ready.ready);
    assert_eq!(ready.status, AgentReadinessCode::Ready);
    assert_eq!(ready.candidates.len(), 2);

    // Rebinding conn_a invalidates its prior readiness/models (`doc-88` §4)!
    fx.office
        .provider_binding_set(
            &fx.owner,
            ProviderBindingSetParams {
                operation_id: next_uuid(),
                connection_id: fx.conn_a.clone(),
                expected_revision: Some(1),
                auth_context_ref: "vault:office-hq/sub-a-v2".into(),
                runtime_binary_path: Some("/usr/local/bin/claude".into()),
                account_identity_ref: Some("acct-sub-a".into()),
            },
        )
        .unwrap()
        .unwrap();
    let listed = fx
        .office
        .provider_binding_list(
            &fx.owner,
            ProviderBindingListParams {
                connection_id: Some(fx.conn_a.clone()),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(listed.bindings[0].binding.revision, 2);
    assert!(listed.bindings[0].state.is_none());
    assert!(listed.bindings[0].models.is_empty());
}

fn run_permit_cooldown_and_budget_interaction_suite(fx: &PoolFixture) {
    // 1. Acquire `office_local` permit on conn_a (`max_concurrent_turns = 1`); second acquire hits CapacityFull.
    let PermitAcquireOutcome::Granted(p1) = fx
        .office
        .acquire_turn_permit(
            &fx.conn_a,
            &fx.agent_id,
            None,
            "anthropic/claude-sonnet-4",
            PermitAuthority::OfficeLocal,
            false,
        )
        .unwrap()
        .unwrap()
    else {
        panic!("expected permit granted");
    };
    assert_eq!(p1.authority, PermitAuthority::OfficeLocal);
    assert_eq!(
        fx.office
            .acquire_turn_permit(
                &fx.conn_a,
                &fx.agent_id,
                None,
                "anthropic/claude-sonnet-4",
                PermitAuthority::OfficeLocal,
                false,
            )
            .unwrap()
            .unwrap(),
        PermitAcquireOutcome::CapacityFull {
            occupied: 1,
            max: 1
        }
    );

    // 2. Heartbeat renewal is PURELY in-memory — ZERO durable writes (`TASK-2` / Non-Negotiable #18)!
    let writes_before = fx.office.provider_durable_write_count();
    fx.clock.advance(10_000);
    fx.office
        .renew_turn_permit(&p1.id, p1.coordinator_generation)
        .unwrap()
        .unwrap();
    assert_eq!(
        fx.office.provider_durable_write_count(),
        writes_before,
        "heartbeat renewal must perform zero durable writes"
    );

    // 3. Lease expiry transitions `active -> release_pending` and DOES NOT free capacity (`doc-88` §6)!
    fx.clock.advance(65_000);
    assert_eq!(
        fx.office
            .acquire_turn_permit(
                &fx.conn_a,
                &fx.agent_id,
                None,
                "anthropic/claude-sonnet-4",
                PermitAuthority::OfficeLocal,
                false,
            )
            .unwrap()
            .unwrap(),
        PermitAcquireOutcome::CapacityFull {
            occupied: 1,
            max: 1
        }
    );
    let q_status = fx
        .office
        .provider_quota_status(
            &fx.owner,
            ProviderQuotaStatusParams {
                connection_id: Some(fx.conn_a.clone()),
            },
        )
        .unwrap()
        .unwrap();
    assert!(q_status.active_permits.is_empty());
    assert_eq!(q_status.release_pending_permits.len(), 1);
    assert_eq!(
        q_status.release_pending_permits[0].state,
        TurnPermitState::ReleasePending
    );

    // Verified cleanup calls `release_turn_permit` -> frees the slot!
    assert!(fx.office.release_turn_permit(&p1.id).unwrap());

    // 4. Cooldown blocks conn_a (default 900s) while conn_b remains open!
    fx.office
        .provider_cooldown_record(
            &fx.owner,
            ProviderCooldownRecordParams {
                connection_id: fx.conn_a.clone(),
                model_id: None,
                cooldown_until: None,
                retry_after_seconds: Some(120),
            },
        )
        .unwrap()
        .unwrap();
    assert!(matches!(
        fx.office
            .acquire_turn_permit(
                &fx.conn_a,
                &fx.agent_id,
                None,
                "anthropic/claude-sonnet-4",
                PermitAuthority::HqPool,
                false,
            )
            .unwrap()
            .unwrap(),
        PermitAcquireOutcome::CoolingDown { .. }
    ));

    // 5. Budget hold blocks initial permit (`is_failover_switch = false`) with `BUDGET_HELD`,
    // while allowing §7 switch permit (`is_failover_switch = true`) for an already-running turn (`doc-88` §6 step 3)!
    fx.office
        .cost_usage_record(
            &fx.owner,
            CostUsageRecordParams {
                id: None,
                producer_id: "pool-test".into(),
                source_event_id: "evt-budget-1".into(),
                run_id: None,
                actor_kind: UsageActorKind::Agent,
                employee_id: Some(fx.agent_id.clone()),
                runtime: "claude".into(),
                provider: Some("anthropic".into()),
                model: Some("claude-sonnet-4".into()),
                occurred_at: org_office::format_rfc3339_ms(fx.clock.now().0),
                source_metadata_json: None,
                input_tokens: Some(100),
                output_tokens: Some(100),
                cache_read_tokens: Some(0),
                cache_write_tokens: Some(0),
                reasoning_tokens: Some(0),
                token_issue: None,
                runtime_cost_micro_usd: Some(50_000),
                is_subscription: false,
                scope_kind: UsageScopeKind::Organization,
                task_id: None,
                project_id: None,
                team_id: None,
                feature_id: None,
                milestone_id: None,
                release_id: None,
                task_type_snapshot: None,
                work_type: Some(WorkType::Build),
                attribution_basis: AttributionBasis::Unattributed,
            },
        )
        .unwrap()
        .unwrap();
    fx.office
        .budget_set(
            &fx.owner,
            BudgetSetParams {
                operation_id: next_uuid(),
                scope_kind: BudgetScopeKind::Agent,
                task_id: None,
                agent_id: Some(fx.agent_id.clone()),
                release_id: None,
                period_kind: BudgetPeriodKind::Day,
                unit: BudgetUnit::MicroUsd,
                limit_amount: 10_000,
                time_zone: Some("UTC".into()),
                time_zone_source: Some(BudgetTimeZoneSource::OwnerPerson),
                reason: None,
            },
        )
        .unwrap()
        .unwrap();
    let held_err = fx
        .office
        .acquire_turn_permit(
            &fx.conn_b,
            &fx.agent_id,
            None,
            "anthropic/claude-sonnet-4",
            PermitAuthority::HqPool,
            false,
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(held_err.code, ErrorCode::Conflict);
    assert!(held_err.message.starts_with("BUDGET_HELD:"));

    // §7 failover switch for an already-running turn STILL succeeds!
    let PermitAcquireOutcome::Granted(p_switch) = fx
        .office
        .acquire_turn_permit(
            &fx.conn_b,
            &fx.agent_id,
            None,
            "anthropic/claude-sonnet-4",
            PermitAuthority::HqPool,
            true,
        )
        .unwrap()
        .unwrap()
    else {
        panic!("expected §7 failover switch permit granted");
    };
    assert_eq!(p_switch.state, TurnPermitState::Active);
}

#[test]
fn l4_provider_pools_and_permits_on_memory_engine() {
    org_office_sqlite::install();
    let fx = seed_pool_fixture(EngineChoice::Memory);
    run_pool_lifecycle_and_discovery_suite(&fx);
    run_permit_cooldown_and_budget_interaction_suite(&fx);
}

#[test]
fn l4_provider_pools_and_permits_on_sqlite_with_rebuild_and_coordinator_restart() {
    org_office_sqlite::install();
    let fx = seed_pool_fixture(EngineChoice::Sqlite);
    run_pool_lifecycle_and_discovery_suite(&fx);
    run_permit_cooldown_and_budget_interaction_suite(&fx);

    let office_id = fx.office.info().office_id.clone();
    let db_path = fx
        .roots
        .runtime
        .join(&office_id)
        .join("protected")
        .join("providers.db");
    assert!(db_path.is_file());

    let gen_before = fx
        .office
        .provider_quota_status(&fx.owner, ProviderQuotaStatusParams::default())
        .unwrap()
        .unwrap()
        .coordinator_generation;

    let PoolFixture {
        _tmp,
        folder,
        roots,
        custody,
        clock,
        office,
        owner,
        conn_a,
        conn_b,
        ..
    } = fx;
    office.close().unwrap();

    // Delete `protected/providers.db` and reopen -> rebuilds bindings & cooldowns, increments
    // `coordinator_generation`, and converts prior `active` `p_switch` permit into `release_pending`!
    for suffix in ["", "-wal", "-shm"] {
        let mut p = db_path.as_os_str().to_owned();
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
            backend: None,
            engine: EngineChoice::Sqlite,
            clock,
            testing: Default::default(),
            before_final_checks: None,
        },
    )
    .expect("reopen after deleting protected/providers.db");

    let bindings = reopened
        .provider_binding_list(&owner, ProviderBindingListParams::default())
        .unwrap()
        .unwrap();
    assert_eq!(bindings.bindings.len(), 2);

    let quota = reopened
        .provider_quota_status(&owner, ProviderQuotaStatusParams::default())
        .unwrap()
        .unwrap();
    assert!(quota.coordinator_generation > gen_before);
    assert!(quota.active_permits.is_empty());
    assert_eq!(
        quota
            .release_pending_permits
            .iter()
            .filter(|p| p.connection_id == conn_b)
            .count(),
        1,
        "prior active permit on conn_b must be release_pending after coordinator restart"
    );
    assert!(quota.cooldowns.iter().any(|c| c.connection_id == conn_a));
    reopened.close().unwrap();
}

#[test]
fn pass1_hardening_ssrf_auth_boundaries_unknown_auth_dup_models_and_expired_renew() {
    org_office_sqlite::install();
    use org_office::organization_family::features::validate_endpoint_url;
    // Finding #6: SSRF / parser-confusion URLs refused
    assert!(validate_endpoint_url("https://api.example.com/v1"));
    assert!(validate_endpoint_url("https://api.example.com:8443/v1"));
    assert!(validate_endpoint_url("http://127.0.0.1:8080/v1"));
    assert!(validate_endpoint_url("http://[::1]:8080/v1"));
    assert!(validate_endpoint_url("http://localhost/v1"));
    assert!(!validate_endpoint_url("http://[::1].evil.com/v1"));
    assert!(!validate_endpoint_url("https://?evil"));
    assert!(!validate_endpoint_url("https://#evil"));
    assert!(!validate_endpoint_url("https://:443"));
    assert!(!validate_endpoint_url("http://localhost:notaport"));
    assert!(!validate_endpoint_url("http://127.0.0.1:80\\evil.com"));
    assert!(!validate_endpoint_url("http://169.254.169.254/latest"));

    let fx = seed_pool_fixture(EngineChoice::Memory);

    // Finding #14b: clear_endpoint_url + endpoint_url rejected
    let conflict_ep = fx
        .office
        .provider_connection_update(
            &fx.owner,
            ProviderConnectionUpdateParams {
                command_id: next_uuid(),
                connection_id: fx.conn_a.clone(),
                expected_version: 1,
                name: None,
                endpoint_url: Some("https://good.example.com".into()),
                clear_endpoint_url: true,
                enabled: None,
                archived: None,
                max_concurrent_turns: None,
            },
        )
        .unwrap();
    assert_eq!(conflict_ep.unwrap_err().code, ErrorCode::InvalidRequest);

    // Finding #5: agent cannot read provider.binding.list, provider.quota.status, another agent's readiness, or task_id readiness
    assert_eq!(
        fx.office
            .provider_binding_list(&fx.agent_id, ProviderBindingListParams::default())
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        fx.office
            .provider_quota_status(&fx.agent_id, ProviderQuotaStatusParams::default())
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        fx.office
            .agent_readiness_get(
                &fx.agent_id,
                AgentReadinessGetParams {
                    employee_id: fx.agent_id.clone(),
                    task_id: Some("task-secret".into()),
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    // Agent CAN read its own readiness without task_id
    assert!(fx
        .office
        .agent_readiness_get(
            &fx.agent_id,
            AgentReadinessGetParams {
                employee_id: fx.agent_id.clone(),
                task_id: None,
            },
        )
        .unwrap()
        .is_ok());

    // Bind conn_a so we can test refresh validation
    fx.office
        .provider_binding_set(
            &fx.owner,
            ProviderBindingSetParams {
                operation_id: next_uuid(),
                connection_id: fx.conn_a.clone(),
                expected_revision: None,
                auth_context_ref: "vault://conn-a".into(),
                runtime_binary_path: None,
                account_identity_ref: None,
            },
        )
        .unwrap()
        .unwrap();

    // Finding #10a: auth_signed_in: None stays Unknown (never Ready!)
    let unverified = fx
        .office
        .provider_connection_refresh(
            &fx.owner,
            ProviderConnectionRefreshParams {
                connection_id: fx.conn_a.clone(),
                driver_version: Some("1.0.0".into()),
                auth_signed_in: None,
                observed_subscription_identity: None,
                operator_attested_subscription: false,
                models: vec![],
                issue_code: None,
                ttl_seconds: Some(300),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(unverified.state.status, ConnectionReadinessStatus::Unknown);
    assert_eq!(
        unverified.state.issue_code.as_deref(),
        Some("AUTH_UNVERIFIED")
    );

    // Finding #11: duplicate model_id in refresh rejected with InvalidRequest
    let dup = fx
        .office
        .provider_connection_refresh(
            &fx.owner,
            ProviderConnectionRefreshParams {
                connection_id: fx.conn_a.clone(),
                driver_version: Some("1.0.0".into()),
                auth_signed_in: Some(true),
                observed_subscription_identity: Some("acct-sub-a".into()),
                operator_attested_subscription: false,
                models: vec![
                    DiscoveredModelInput {
                        model_id: "claude-sonnet-4".into(),
                        driver_model_id: "claude-sonnet-4-a".into(),
                        entitlement: DiscoveredModelEntitlement::Confirmed,
                        capabilities_json: "{}".into(),
                    },
                    DiscoveredModelInput {
                        model_id: "claude-sonnet-4".into(),
                        driver_model_id: "claude-sonnet-4-b".into(),
                        entitlement: DiscoveredModelEntitlement::Confirmed,
                        capabilities_json: "{}".into(),
                    },
                ],
                issue_code: None,
                ttl_seconds: Some(300),
            },
        )
        .unwrap();
    assert_eq!(dup.unwrap_err().code, ErrorCode::InvalidRequest);

    // Finding #2a: overdue permit cannot be resurrected by renew_turn_permit
    let granted = match fx
        .office
        .acquire_turn_permit(
            &fx.conn_a,
            &fx.agent_id,
            None,
            "claude-sonnet-4",
            PermitAuthority::OfficeLocal,
            false,
        )
        .unwrap()
        .unwrap()
    {
        PermitAcquireOutcome::Granted(p) => p,
        other => panic!("expected Granted, got {other:?}"),
    };
    fx.clock.advance(65_000);
    let renew_err = fx
        .office
        .renew_turn_permit(&granted.id, granted.coordinator_generation)
        .unwrap()
        .unwrap_err();
    assert_eq!(renew_err.code, ErrorCode::Conflict);
    assert!(renew_err.message.contains("PERMIT_EXPIRED"));
}
