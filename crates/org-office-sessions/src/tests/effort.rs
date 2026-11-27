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
