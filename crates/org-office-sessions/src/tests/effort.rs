//! `TASK-181`: an agent's reasoning effort reaches the session spec and the runtime's command
//! line, and a changed effort recycles a warm session through the route digest.

use org_agent_runtime::Token;
use org_proto::hiring::{AgentExecutionSet, AgentExecutionSetParams};

use super::*;
use crate::instructions::StandingInstructions;
use crate::manager::{build_session_spec, ModelChoice};
use crate::router::{
    acquire_warm_permit_for_office, select_and_admit_route, AdmitRouteRequest, AdmittedRoute,
    WarmPermitOutcome,
};

const MODEL: &str = "claude-sonnet-4-6";

struct Agent {
    office: Arc<Office>,
    owner: String,
    agent: String,
    connection: String,
    profile: String,
}

fn rpc<C: org_proto::Call>(office: &Office, owner: &str, params: &C::Params) {
    let request = Request::new::<C>(1, params).unwrap();
    let outcome = office
        .dispatch_hiring_rpc(Some(owner), &request)
        .unwrap()
        .outcome;
    assert!(
        matches!(outcome, org_proto::Outcome::Result(_)),
        "{}: {outcome:?}",
        C::METHOD
    );
}

/// A ready Claude connection, and an agent hired onto it with `effort`.
fn hired_agent(effort: Option<&str>) -> Agent {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner = office.owner_member_id().to_owned();
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();
    let connection = new_uuid_v7();
    let profile = new_uuid_v7();
    rpc::<ProviderConnectionCreate>(
        &office,
        &owner,
        &ProviderConnectionCreateParams {
            command_id: new_uuid_v7(),
            connection_id: connection.clone(),
            default_profile_id: profile.clone(),
            key: "claude-effort".into(),
            name: "Claude Effort".into(),
            provider_type: "anthropic".into(),
            auth_mode: "subscription_cli".into(),
            subscription_identity: Some("dev@example.com".into()),
            runtime_driver: "claude-code".into(),
            endpoint_url: None,
            max_concurrent_turns: 2,
        },
    );
    office
        .provider_binding_set(
            &owner,
            org_proto::providers::ProviderBindingSetParams {
                operation_id: new_uuid_v7(),
                connection_id: connection.clone(),
                expected_revision: None,
                auth_context_ref: "subscription://local".into(),
                runtime_binary_path: None,
                account_identity_ref: Some("dev@example.com".into()),
            },
        )
        .unwrap()
        .unwrap();
    office
        .provider_connection_refresh(
            &owner,
            org_proto::providers::ProviderConnectionRefreshParams {
                connection_id: connection.clone(),
                driver_version: Some("2.1.323".into()),
                auth_signed_in: Some(true),
                observed_subscription_identity: Some("dev@example.com".into()),
                operator_attested_subscription: true,
                models: vec![org_proto::providers::DiscoveredModelInput {
                    model_id: MODEL.into(),
                    driver_model_id: MODEL.into(),
                    entitlement: org_proto::providers::DiscoveredModelEntitlement::Confirmed,
                    capabilities_json: "{}".into(),
                }],
                issue_code: None,
                ttl_seconds: Some(300),
            },
        )
        .unwrap()
        .unwrap();
    let agent = new_uuid_v7();
    rpc::<HrHire>(
        &office,
        &owner,
        &HrHireParams {
            command_id: new_uuid_v7(),
            request_id: new_uuid_v7(),
            employee_id: agent.clone(),
            home_team_id: root_team,
            role_id: None,
            proposal: HireProposalWire {
                name: "Deep Thinker".into(),
                handle: "deep-thinker".into(),
                title_in_team: Some("Reviewer".into()),
                reports_to_employee_id: Some(owner.clone()),
                model_profile_id: Some(profile.clone()),
                model_id: Some(MODEL.into()),
                permission_preset: Some("workspace_edits".into()),
                system_prompt_markdown: Some("Review carefully.".into()),
                effort: effort.map(str::to_owned),
                ..HireProposalWire::default()
            },
            reason: "Effort test".into(),
        },
    );
    Agent {
        office,
        owner,
        agent,
        connection,
        profile,
    }
}

impl Agent {
    /// Admits a route the way a cold start does, and gives its permit straight back.
    fn admit(&self) -> AdmittedRoute {
        let registry = RuntimeRegistry::production();
        let info = self.office.info();
        let vault = CredentialVault::new(
            self.office.durable_root().to_path_buf(),
            info.org_id.clone(),
            info.office_id.clone(),
            Some([0x22u8; 32]),
        );
        vault
            .store_wrapped(
                &self.connection,
                "anthropic",
                b"sk-ant-oat01-effort-test-token",
                None,
                None,
            )
            .unwrap();
        let excluded = std::collections::HashSet::new();
        let admitted = select_and_admit_route(AdmitRouteRequest {
            office: &self.office,
            vault: &vault,
            runtimes: &registry,
            hq_permits: None,
            oauth_refresh: None,
            employee_id: &self.agent,
            task_id: None,
            is_failover_switch: false,
            excluded_connections: &excluded,
            turn_override: None,
        })
        .unwrap();
        if let Some((permit, _)) = &admitted.permit {
            let _ = self.office.release_turn_permit(permit);
        }
        admitted
    }

    fn set_effort(&self, effort: Option<&str>) {
        rpc::<AgentExecutionSet>(
            &self.office,
            &self.owner,
            &AgentExecutionSetParams {
                command_id: new_uuid_v7(),
                employee_id: self.agent.clone(),
                expected_version: None,
                model_profile_id: self.profile.clone(),
                model_id: MODEL.into(),
                tools_config: Vec::new(),
                permission_preset: Some("workspace_edits".into()),
                effort: effort.map(str::to_owned),
                enabled: true,
                system_prompt_markdown: "Review carefully.".into(),
            },
        );
    }

    fn warm(&self, digest: &str) -> WarmPermitOutcome {
        let outcome = acquire_warm_permit_for_office(
            &self.office,
            None,
            &self.connection,
            &self.agent,
            None,
            Some(digest),
        );
        if let WarmPermitOutcome::Granted((permit, _)) = &outcome {
            let _ = self.office.release_turn_permit(permit);
        }
        outcome
    }

    /// The spec a launch on `runtime` builds from `admitted`, as `launch.rs` builds it.
    fn spec(&self, admitted: &AdmittedRoute, runtime: &str) -> SessionSpec {
        let employee = self.office.load_employee(&self.agent).unwrap().unwrap();
        let workspace = SpecPath::try_from("/work/effort").unwrap();
        let preset = build_session_policy(
            PermissionPreset::WorkspaceEdits,
            true,
            &workspace,
            None,
            &[],
            1,
        )
        .unwrap();
        build_session_spec(
            RuntimeId::try_from(runtime).unwrap(),
            &employee,
            ModelChoice {
                model: admitted.model_token.clone(),
                effort: admitted.effort.clone(),
            },
            workspace,
            Vec::new(),
            StandingInstructions {
                instructions: "Review carefully.".into(),
                repository_instructions: String::new(),
            },
            &preset,
        )
        .unwrap()
    }
}

fn launch_ctx() -> LaunchContext {
    LaunchContext {
        runtime_path: PathBuf::from("/opt/bin/runtime"),
        runtime_version: None,
        runtime_dir: SpecPath::try_from("/office/agents/deep/runtime").unwrap(),
        tmpdir: SpecPath::try_from("/tmp/agentic-org/run-effort").unwrap(),
        office_env_names: Vec::new(),
        new_session_id: Some(SessionId::try_from("0193a5c0-0000-4000-8000-000000000001").unwrap()),
        resume: None,
        config_dir: None,
    }
}

/// The value right after `flag` in a planned command line, if the flag is there.
fn after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let at = args.iter().position(|a| a == flag)?;
    args.get(at + 1).map(String::as_str)
}

fn claude_args(spec: &SessionSpec) -> Vec<String> {
    let claude = org_agent_runtime_claude::ClaudeRuntime::new();
    claude.plan_session(spec, &launch_ctx()).unwrap().0.args
}

fn codex_effort(spec: &SessionSpec) -> Option<String> {
    let codex = org_agent_runtime_codex::CodexRuntime::new();
    let args = codex.plan_session(spec, &launch_ctx()).unwrap().0.args;
    let effort = args
        .iter()
        .filter_map(|a| a.strip_prefix("model_reasoning_effort="))
        .collect::<Vec<_>>();
    assert!(effort.len() <= 1, "{args:?}");
    effort.first().map(|v| v.trim_matches('"').to_owned())
}

#[test]
fn the_agents_effort_reaches_the_spec_and_the_runtimes_command_line() {
    let agent = hired_agent(Some("xhigh"));
    let admitted = agent.admit();
    assert_eq!(admitted.effort, Some(Token::try_from("xhigh").unwrap()));

    let spec = agent.spec(&admitted, "claude");
    assert_eq!(spec.effort.as_ref().map(Token::as_str), Some("xhigh"));
    let args = claude_args(&spec);
    assert_eq!(after(&args, "--effort"), Some("xhigh"), "{args:?}");
    assert_eq!(after(&args, "--model"), Some(MODEL), "{args:?}");
    let spec = agent.spec(&admitted, "codex");
    assert_eq!(codex_effort(&spec).as_deref(), Some("xhigh"));

    // Every level reaches Claude as it is; Codex tops out at `xhigh`.
    for (level, codex) in [
        ("low", "low"),
        ("medium", "medium"),
        ("high", "high"),
        ("xhigh", "xhigh"),
        ("max", "xhigh"),
    ] {
        agent.set_effort(Some(level));
        let admitted = agent.admit();
        let args = claude_args(&agent.spec(&admitted, "claude"));
        assert_eq!(after(&args, "--effort"), Some(level), "{args:?}");
        let spec = agent.spec(&admitted, "codex");
        assert_eq!(codex_effort(&spec).as_deref(), Some(codex), "{level}");
    }

    // No effort: none in the spec, and no flag, so each runtime keeps its default.
    agent.set_effort(None);
    let admitted = agent.admit();
    assert_eq!(admitted.effort, None);
    let spec = agent.spec(&admitted, "claude");
    assert_eq!(spec.effort, None);
    assert!(!claude_args(&spec).iter().any(|a| a == "--effort"));
    assert_eq!(codex_effort(&agent.spec(&admitted, "codex")), None);
}

#[test]
fn changing_the_effort_changes_the_route_digest_and_recycles_a_warm_session() {
    let agent = hired_agent(Some("high"));
    let high = agent.admit().route_digest_suffix;
    assert!(
        matches!(agent.warm(&high), WarmPermitOutcome::Granted(_)),
        "an unchanged effort keeps the warm session"
    );

    agent.set_effort(Some("max"));
    assert!(
        matches!(agent.warm(&high), WarmPermitOutcome::StaleWarm),
        "a changed effort must recycle the warm session"
    );
    let max = agent.admit().route_digest_suffix;
    assert_ne!(max, high);
    // The warm path computes the digest the way admission does, effort included.
    assert!(matches!(agent.warm(&max), WarmPermitOutcome::Granted(_)));

    agent.set_effort(None);
    assert!(matches!(agent.warm(&max), WarmPermitOutcome::StaleWarm));
    let none = agent.admit().route_digest_suffix;
    assert!(matches!(agent.warm(&none), WarmPermitOutcome::Granted(_)));
    assert_ne!(none, max);
}

#[test]
fn the_route_digest_holds_the_effort_by_value_and_none_keeps_the_earlier_digest() {
    use org_office::organization_family::records::ReasoningEffort;

    let agent = hired_agent(None);
    let conn = agent
        .office
        .load_provider_connection(&agent.connection)
        .unwrap()
        .unwrap();
    let digest =
        |effort| crate::router::compute_route_digest_with_wt(&conn, 3, MODEL, effort, 7, "wt");
    // The same revision with another effort is another route: a warm session never carries
    // one effort into a turn configured with another.
    let all: std::collections::BTreeSet<String> = ReasoningEffort::ALL
        .into_iter()
        .map(|e| digest(Some(e)))
        .chain([digest(None)])
        .collect();
    assert_eq!(all.len(), ReasoningEffort::ALL.len() + 1);
    // No effort hashes exactly what it did before efforts existed, so upgrading the office
    // recycles no warm session of an agent without one.
    let before = format!(
        "conn={}:rev={}:bind=3:endpoint=:model={MODEL}:exec=7:wt=wt",
        conn.id.as_str(),
        conn.meta.revision
    );
    let sha = ring::digest::digest(&ring::digest::SHA256, before.as_bytes());
    let hex: String = sha.as_ref()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(digest(None), hex);
}

// ---------------------------------------------------------------------------
// TASK-136 AC #3: Per-task / per-turn model & effort override routing
// ---------------------------------------------------------------------------

impl Agent {
    fn refresh_models(&self, model_ids: &[&str]) {
        self.office
            .provider_connection_refresh(
                &self.owner,
                org_proto::providers::ProviderConnectionRefreshParams {
                    connection_id: self.connection.clone(),
                    driver_version: Some("2.1.323".into()),
                    auth_signed_in: Some(true),
                    observed_subscription_identity: Some("dev@example.com".into()),
                    operator_attested_subscription: true,
                    models: model_ids
                        .iter()
                        .map(|m| org_proto::providers::DiscoveredModelInput {
                            model_id: (*m).to_owned(),
                            driver_model_id: (*m).to_owned(),
                            entitlement:
                                org_proto::providers::DiscoveredModelEntitlement::Confirmed,
                            capabilities_json: "{}".into(),
                        })
                        .collect(),
                    issue_code: None,
                    ttl_seconds: Some(300),
                },
            )
            .unwrap()
            .unwrap();
    }

    fn admit_with_override_result(
        &self,
        turn_override: &crate::router::TurnModelOverride,
    ) -> Result<AdmittedRoute, org_proto::ApiError> {
        let registry = RuntimeRegistry::production();
        let info = self.office.info();
        let vault = CredentialVault::new(
            self.office.durable_root().to_path_buf(),
            info.org_id.clone(),
            info.office_id.clone(),
            Some([0x22u8; 32]),
        );
        vault
            .store_wrapped(
                &self.connection,
                "anthropic",
                b"sk-ant-oat01-effort-test-token",
                None,
                None,
            )
            .unwrap();
        let excluded = std::collections::HashSet::new();
        let admitted = select_and_admit_route(AdmitRouteRequest {
            office: &self.office,
            vault: &vault,
            runtimes: &registry,
            hq_permits: None,
            oauth_refresh: None,
            employee_id: &self.agent,
            task_id: None,
            is_failover_switch: false,
            excluded_connections: &excluded,
            turn_override: Some(turn_override),
        })?;
        if let Some((permit, _)) = &admitted.permit {
            let _ = self.office.release_turn_permit(permit);
        }
        Ok(admitted)
    }

    fn admit_with_override(
        &self,
        turn_override: &crate::router::TurnModelOverride,
    ) -> AdmittedRoute {
        self.admit_with_override_result(turn_override).unwrap()
    }

    fn warm_with_override(
        &self,
        digest: &str,
        turn_override: &crate::router::TurnModelOverride,
    ) -> WarmPermitOutcome {
        let outcome = crate::router::acquire_warm_permit_for_office_with_override(
            &self.office,
            None,
            &self.connection,
            &self.agent,
            None,
            Some(digest),
            Some(turn_override),
        );
        if let WarmPermitOutcome::Granted((permit, _)) = &outcome {
            let _ = self.office.release_turn_permit(permit);
        }
        outcome
    }
}

fn codex_model(spec: &SessionSpec) -> Option<String> {
    let codex = org_agent_runtime_codex::CodexRuntime::new();
    let args = codex.plan_session(spec, &launch_ctx()).unwrap().0.args;
    args.iter()
        .find_map(|a| a.strip_prefix("model="))
        .map(|v| v.trim_matches('"').to_owned())
}

#[test]
fn per_turn_model_and_effort_override_flows_across_claude_codex_and_agy() {
    use crate::router::{validate_turn_override, TurnModelOverride};

    // Profile default is `MODEL` + effort `medium`.
    let agent = hired_agent(Some("medium"));

    // 1. Empty override preserves profile defaults, and an unentitled model override fails closed.
    let default_admitted = agent.admit_with_override(&TurnModelOverride::default());
    assert_eq!(
        default_admitted.model_token.as_ref().map(Token::as_str),
        Some(MODEL)
    );
    assert_eq!(
        default_admitted.effort.as_ref().map(Token::as_str),
        Some("medium")
    );
    let unentitled = validate_turn_override(Some("unentitled-model-x"), None).unwrap();
    let err = agent
        .admit_with_override_result(&unentitled)
        .err()
        .expect("unentitled model must fail closed");
    assert_eq!(err.code, org_proto::ErrorCode::Conflict);
    assert!(err.message.contains("MODEL_UNVERIFIED"), "{err:?}");

    // Confirm additional models on the connection so per-turn overrides can route to them.
    agent.refresh_models(&[
        MODEL,
        "claude-haiku-4-5",
        "gpt-5.4-mini",
        "gemini-4-neon-low",
    ]);

    // 2. Per-turn override changes both model and effort without mutating the agent profile.
    let override_spec =
        validate_turn_override(Some("claude-haiku-4-5"), Some("low")).expect("valid override");
    let admitted = agent.admit_with_override(&override_spec);
    assert_eq!(
        admitted.model_token.as_ref().map(Token::as_str),
        Some("claude-haiku-4-5")
    );
    assert_eq!(admitted.effort.as_ref().map(Token::as_str), Some("low"));

    // Claude receives both `--model` and `--effort`.
    let claude_spec = agent.spec(&admitted, "claude");
    let c_args = claude_args(&claude_spec);
    assert_eq!(after(&c_args, "--model"), Some("claude-haiku-4-5"));
    assert_eq!(after(&c_args, "--effort"), Some("low"));

    // Codex receives both `model=` and `model_reasoning_effort=` (with `max` clamped to `xhigh`).
    let codex_override =
        validate_turn_override(Some("gpt-5.4-mini"), Some("max")).expect("valid codex override");
    let codex_admitted = agent.admit_with_override(&codex_override);
    let codex_spec = agent.spec(&codex_admitted, "codex");
    assert_eq!(codex_model(&codex_spec).as_deref(), Some("gpt-5.4-mini"));
    assert_eq!(codex_effort(&codex_spec).as_deref(), Some("xhigh"));

    // Agy ACP consumes `spec.model` via ACP `session/set_model`, and fails closed on `spec.effort`
    // because Agy encodes effort in the model identifier.
    let agy = org_agent_runtime_agy::AgyRuntime::new();
    let agy_effort_spec = agent.spec(&admitted, "agy");
    assert!(
        matches!(
            agy.plan_session(&agy_effort_spec, &launch_ctx()),
            Err(PlanError::Unsupported("effort"))
        ),
        "Agy must fail closed when a separate effort flag is supplied"
    );

    agent.set_effort(None);
    let agy_override = validate_turn_override(Some("gemini-4-neon-low"), None).unwrap();
    let agy_admitted = agent.admit_with_override(&agy_override);
    assert_eq!(agy_admitted.effort, None);
    let agy_spec = agent.spec(&agy_admitted, "agy");
    let (_plan, mut session) = agy.plan_session(&agy_spec, &launch_ctx()).unwrap();
    let now = Instant::now();
    let _init = session.start(now);
    let open = session.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"agy","version":"1.0.0"}}}"#,
        now,
    );
    assert!(!open.is_empty());
    let after_new = session.on_line(
        br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"0193a5c0-0000-4000-8000-000000000099","modes":{"currentModeId":"default","availableModes":[{"id":"default"}]}}}"#,
        now,
    );
    let set_model_wire = after_new
        .iter()
        .find_map(|o| match o {
            Output::Write(line) => String::from_utf8(line.as_bytes().to_vec()).ok(),
            _ => None,
        })
        .expect("Agy must emit ACP session/set_model for overridden model");
    assert!(
        set_model_wire.contains("session/set_model")
            && set_model_wire.contains("gemini-4-neon-low"),
        "expected ACP session/set_model with override model, got: {set_model_wire}"
    );

    // 3. Warm permit digest changes when override changes, forcing clean warm recycle.
    let default_ov = TurnModelOverride::default();
    let low_ov = validate_turn_override(None, Some("low")).unwrap();
    let default_digest = agent.admit_with_override(&default_ov).route_digest_suffix;
    let low_digest = agent.admit_with_override(&low_ov).route_digest_suffix;
    assert_ne!(default_digest, low_digest);
    assert!(matches!(
        agent.warm_with_override(&default_digest, &default_ov),
        WarmPermitOutcome::Granted(_)
    ));
    assert!(matches!(
        agent.warm_with_override(&default_digest, &low_ov),
        WarmPermitOutcome::StaleWarm
    ));
    assert!(matches!(
        agent.warm_with_override(&low_digest, &low_ov),
        WarmPermitOutcome::Granted(_)
    ));
}

#[test]
fn invalid_effort_and_model_overrides_fail_closed_and_task_wake_persists_valid_overrides() {
    use crate::router::{parse_task_routing_labels, validate_turn_override};
    use org_proto::ErrorCode;

    // All canonical effort tiers succeed.
    for tier in org_proto::hiring::EFFORT_LEVELS {
        let ov = validate_turn_override(Some("claude-sonnet-4-6"), Some(tier)).unwrap();
        assert_eq!(ov.effort_str(), Some(tier));
        assert_eq!(ov.model.as_deref(), Some("claude-sonnet-4-6"));
    }

    // Invalid effort tiers, uppercase, whitespace, control chars, and flag injection fail closed.
    for bad_effort in ["", "turbo", "HIGH", "high\n", "high ", "--effort", "\x00"] {
        let err = validate_turn_override(None, Some(bad_effort)).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidRequest, "effort={bad_effort:?}");
    }

    // Invalid models (empty, leading '-', whitespace, control chars) fail closed.
    for bad_model in [
        "",
        "-m",
        "--dangerously-skip-permissions",
        "claude\n--evil",
        "claude sonnet",
        "model\x00x",
    ] {
        let err = validate_turn_override(Some(bad_model), None).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidRequest, "model={bad_model:?}");
    }

    // Task routing labels parse valid overrides and reject malformed routing labels.
    let parsed = parse_task_routing_labels(&[
        "backend".into(),
        "effort:xhigh".into(),
        "model:claude-opus-4-6".into(),
    ])
    .unwrap();
    assert_eq!(parsed.effort_str(), Some("xhigh"));
    assert_eq!(parsed.model.as_deref(), Some("claude-opus-4-6"));
    assert_eq!(
        parse_task_routing_labels(&["effort:ultra".into()])
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        parse_task_routing_labels(&["model:--inject".into()])
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );

    // SessionManager::start_session refuses invalid effort/model before admission.
    let tmp = tempfile::tempdir().unwrap();
    let agent = hired_agent(Some("medium"));
    let mgr = SessionManager::new(
        Arc::clone(&agent.office),
        SessionManagerConfig::production(tmp.path().join("office.sock")),
    );
    let err = mgr
        .start_session(
            &agent.owner,
            &MemberSessionStartParams {
                command_id: new_uuid_v7(),
                employee_id: agent.agent.clone(),
                instruction: Some("hi".into()),
                workspace: None,
                resume_runtime_session_id: None,
                fresh: false,
                model: None,
                effort: Some("--effort=bad".into()),
            },
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);

    // Task wake with overrides persists validated model/effort on the queued wake item.
    mgr.set_budget_hold(Some("test_queue_only".into()));
    mgr.on_task_assigned_with_overrides(
        &agent.agent,
        &agent.owner,
        "Owner",
        true,
        "T-99",
        "Quick triage",
        Vec::new(),
        parsed,
    );
    let wakes = mgr
        .store
        .read_pending_wakes(&agent.agent)
        .unwrap()
        .expect("pending wakes");
    assert_eq!(wakes.items.len(), 1);
    assert_eq!(
        wakes.items[0].model_override.as_deref(),
        Some("claude-opus-4-6")
    );
    assert_eq!(wakes.items[0].effort_override.as_deref(), Some("xhigh"));
}
