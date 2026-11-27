//! The built-in Chief of Staff's sessions (`TASK-80.1`): her preset is derived from who she is
//! (`OperatorSandboxed`), her operator spec turns Codex's shell on confined to `agent-os`, she
//! works only from her private home, and every run acts for the one caller that started it, in
//! that caller's own partition.

use std::collections::BTreeMap;

use org_agent_runtime::{
    validate_operator_shell, AgentIdentity, AgentName, PathPattern, SessionKind, ToolCapability,
};
use org_office::organization_family::records::BuiltinKey;
use org_proto::ErrorCode;

use super::*;
use crate::partition::PartitionKey;

struct Fixture {
    office: Arc<Office>,
    manager: SessionManager,
    owner: String,
    tess: String,
    _sock: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let snap = office.org_snapshot().unwrap();
    let owner = snap
        .members
        .iter()
        .find(|m| m.owner)
        .expect("owner")
        .id
        .clone();
    let tess = office
        .chief_of_staff()
        .unwrap()
        .expect("Tess")
        .id
        .to_string();
    let sock = tempfile::tempdir().unwrap();
    let cfg = SessionManagerConfig::production(sock.path().join("office.sock"));
    let manager = SessionManager::new(Arc::clone(&office), cfg);
    Fixture {
        office,
        manager,
        owner,
        tess,
        _sock: sock,
    }
}

#[test]
fn her_preset_is_derived_from_who_she_is_and_never_stored() {
    use org_office::organization_family::records::PermissionPreset as P;
    for stored in P::ALL {
        assert_eq!(
            P::effective_for(Some(BuiltinKey::ChiefOfStaff), stored),
            P::OperatorSandboxed
        );
        assert_eq!(P::effective_for(Some(BuiltinKey::Hr), stored), stored);
        assert_eq!(P::effective_for(None, stored), stored);
    }
    // Nobody else ever runs it, whatever reaches them.
    assert_eq!(P::effective_for(None, P::OperatorSandboxed), P::SafeAsk);
    assert_eq!(P::parse("operator_sandboxed"), None);
    assert!(parse_preset("operator_sandboxed").is_err());
    assert!(serde_json::from_str::<P>("\"operator_sandboxed\"").is_err());
    assert!(serde_json::to_string(&P::OperatorSandboxed).is_err());
    assert!(P::OperatorSandboxed.is_read_only_fs() && P::ReadOnly.is_read_only_fs());
    assert!(!P::WorkspaceEdits.is_read_only_fs());

    let f = fixture();
    let grants = f.manager.list_grants(&f.owner, Some(&f.tess)).unwrap();
    assert_eq!(grants.permission_preset, "operator_sandboxed");
    assert_eq!(grants.permission_mode, "plan");
    assert!(grants.sandbox_enabled);
    assert!(
        grants.writable_roots.is_empty(),
        "{:?}",
        grants.writable_roots
    );
    assert!(
        !grants.denied_read_patterns.is_empty(),
        "office folders stay denied"
    );
    let allows: Vec<_> = grants
        .rules
        .iter()
        .filter(|r| r.effect == "allow")
        .collect();
    assert!(
        allows.iter().all(|r| r.tool != "cap:edit" && r.tool != "*"),
        "{allows:?}"
    );
    assert!(
        allows
            .iter()
            .any(|r| r.tool == "cap:shell" && r.pattern.as_deref() == Some("agent-os")),
        "{allows:?}"
    );
    // The HR lead keeps its stored (default) preset.
    let hr = f.office.list_employees().unwrap();
    let hr = hr
        .iter()
        .find(|e| e.builtin_key == Some(BuiltinKey::Hr))
        .unwrap();
    let hr_grants = f
        .manager
        .list_grants(&f.owner, Some(hr.id.as_str()))
        .unwrap();
    assert_eq!(hr_grants.permission_preset, "safe_ask");
}

#[test]
fn the_operator_preset_makes_an_operator_spec_that_codex_runs_with_a_confined_shell() {
    let ws = SpecPath::try_from("/office/agents/tess/home").unwrap();
    let sock = SpecPath::try_from("/office/run/office.sock").unwrap();
    let glob = PathPattern::try_from("/office/offices/*/**").unwrap();
    let cfg = build_session_policy(
        PermissionPreset::OperatorSandboxed,
        true,
        &ws,
        Some(&sock),
        std::slice::from_ref(&glob),
        1,
    )
    .unwrap();
    assert_eq!(cfg.permission_mode, PermissionMode::Plan);
    assert!(cfg.sandbox.writable_roots.is_empty());
    assert_eq!(cfg.sandbox.allowed_unix_sockets, vec![sock.clone()]);
    assert_eq!(cfg.sandbox.deny_read, vec![glob.clone()]);
    assert_eq!(
        cfg.operator_shell_prefixes
            .iter()
            .map(|p| p.as_str())
            .collect::<Vec<_>>(),
        ["agent-os"]
    );
    let read_only = build_session_policy(
        PermissionPreset::ReadOnly,
        true,
        &ws,
        Some(&sock),
        std::slice::from_ref(&glob),
        1,
    )
    .unwrap();
    assert!(
        read_only.operator_shell_prefixes.is_empty(),
        "plain read-only is no operator"
    );
    assert_eq!(read_only.policy, cfg.policy, "the same rules as read-only");

    let spec = SessionSpec {
        runtime: RuntimeId::try_from("codex").unwrap(),
        kind: SessionKind::Home {},
        agent: AgentIdentity {
            name: AgentName::try_from("tess").unwrap(),
            description: "Tess (Chief of Staff)".into(),
            prompt: "You are Tess (@tess).".into(),
        },
        model: None,
        effort: None,
        workspace: ws,
        read_only_dirs: Vec::new(),
        instructions: String::new(),
        repository_instructions: String::new(),
        permission_mode: cfg.permission_mode,
        policy: cfg.policy.clone(),
        sandbox: cfg.sandbox.clone(),
        mcp_servers: BTreeMap::new(),
        tools: cfg.allowed_tools.clone(),
        operator_shell_prefixes: cfg.operator_shell_prefixes.clone(),
    };
    assert_eq!(validate_operator_shell(&spec), Ok(true));
    assert!(spec
        .tools
        .as_ref()
        .is_some_and(|t| t.contains(&ToolCapability::Shell.to_tool_name())));
    let ctx = LaunchContext {
        runtime_path: PathBuf::from("/usr/local/bin/codex"),
        runtime_version: None,
        runtime_dir: SpecPath::try_from("/office/agents/tess/runtime").unwrap(),
        tmpdir: SpecPath::try_from("/tmp/tess-run").unwrap(),
        office_env_names: Vec::new(),
        new_session_id: None,
        resume: None,
        config_dir: None,
    };
    let (plan, _) = org_agent_runtime_codex::CodexRuntime
        .plan_session(&spec, &ctx)
        .expect("Codex plans her operator session");
    assert!(!plan.args.iter().any(|a| a == "features.shell_tool=false"));
    // Her plain read-only twin keeps Codex's shell off, as every read-only agent does.
    let plain = SessionSpec {
        operator_shell_prefixes: Vec::new(),
        ..spec
    };
    let (plan, _) = org_agent_runtime_codex::CodexRuntime
        .plan_session(&plain, &ctx)
        .unwrap();
    assert!(plan.args.iter().any(|a| a == "features.shell_tool=false"));
}

#[test]
fn she_works_only_from_her_private_home() {
    let f = fixture();
    let elsewhere = tempfile::tempdir().unwrap();
    let refused = f
        .manager
        .start_session(
            &f.owner,
            &MemberSessionStartParams {
                command_id: new_uuid_v7(),
                employee_id: f.tess.clone(),
                instruction: Some("Summarise the week.".into()),
                workspace: Some(elsewhere.path().display().to_string()),
                resume_runtime_session_id: None,
                fresh: false,
            },
        )
        .unwrap_err();
    assert_eq!(refused.code, org_proto::ErrorCode::InvalidRequest);
    assert!(
        refused.message.contains("private home"),
        "{}",
        refused.message
    );
}

#[test]
fn a_run_acts_for_exactly_the_caller_its_launch_names() {
    let reg = RunBindingRegistry::new();
    let run = reg
        .issue("tess", "run-1", "sess-1", 0, Some("owner-1"))
        .unwrap();
    assert_eq!(run.acting_for_caller_id.as_deref(), Some("owner-1"));
    assert_eq!(reg.acting_for_caller_id("tess").as_deref(), Some("owner-1"));
    assert_eq!(reg.acting_for_caller_id("someone-else"), None);
    // Each run carries its own caller: a later run of another partition acts for its own, and
    // nothing a start records for one run can reach another.
    reg.revoke("tess", None);
    assert_eq!(reg.acting_for_caller_id("tess"), None);
    let next = reg
        .issue("tess", "run-2", "sess-2", 0, Some("member-2"))
        .unwrap();
    assert_eq!(next.acting_for_caller_id.as_deref(), Some("member-2"));
    let wake = reg.issue("tess", "run-3", "sess-3", 0, None).unwrap();
    assert_eq!(wake.acting_for_caller_id, None);
    assert_eq!(reg.acting_for_caller_id("tess"), None);
    // An external lease never acts for anyone.
    let lease = reg.issue_external("tess", "ext-1", u64::MAX).unwrap();
    assert_eq!(lease.acting_for_caller_id, None);
    assert!(!format!("{run:?}").contains(run.expose_token()));
}

#[test]
fn each_caller_gets_a_partition_and_no_one_else_has_one() {
    let f = fixture();
    let tess = f.office.chief_of_staff().unwrap().unwrap();
    let owner = SessionManager::turn_partition(&tess, Some(&f.owner)).expect("hers");
    assert_eq!(owner, PartitionKey::tess_ask(&f.owner));
    assert_eq!(owner.caller_id(), Some(f.owner.as_str()));
    let member = "0190a5d2-7c3e-7a01-8b2c-3d4e5f60aaaa";
    let theirs = SessionManager::turn_partition(&tess, Some(member)).unwrap();
    assert_eq!(theirs.as_str(), format!("tess:ask:{member}"));
    assert_eq!(
        SessionManager::turn_partition(&tess, None),
        Some(PartitionKey::tess_wake())
    );
    // Anyone else has no partition, and so their runs act for nobody.
    let hr = f.office.list_employees().unwrap();
    let hr = hr
        .iter()
        .find(|e| e.builtin_key == Some(BuiltinKey::Hr))
        .unwrap();
    assert_eq!(SessionManager::turn_partition(hr, Some(&f.owner)), None);
}

#[test]
fn she_never_signs_in_from_an_external_workspace() {
    let f = fixture();
    let refused = f
        .manager
        .external_login_request(
            &f.tess,
            &org_proto::external_auth::AuthLoginRequestParams {
                member: f.tess.clone(),
                pubkey_ed25519_b64: "11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=".into(),
                workspace_label: "laptop".into(),
                hostname: None,
                workspace_path: None,
            },
        )
        .unwrap_err();
    assert_eq!(refused.code, org_proto::ErrorCode::NotAuthorized);
    assert!(refused.message.contains("only inside the office"));

    // No later step of a sign-in is ever hers either, even with a login on record for her.
    let only_inside = |err: org_proto::ApiError| {
        err.code == org_proto::ErrorCode::NotAuthorized
            && err.message.contains("only inside the office")
    };
    let challenge = f
        .manager
        .external_challenge(
            &f.tess,
            "sess-1",
            org_proto::external_auth::ChallengePurpose::Exchange,
        )
        .unwrap_err();
    assert!(only_inside(challenge));
    let exchange = f
        .manager
        .external_exchange(
            &f.tess,
            &org_proto::external_auth::SignedChallengeParams {
                member: f.tess.clone(),
                session_id: "sess-1".into(),
                nonce: "n".into(),
                signature_ed25519_b64: "s".into(),
            },
        )
        .unwrap_err();
    assert!(only_inside(exchange));
    let (_, code) = f
        .manager
        .external
        .pending
        .lock()
        .unwrap()
        .insert(crate::external::PendingInsert {
            request_id: new_uuid_v7(),
            employee_id: &f.tess,
            pubkey_ed25519_b64: "11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=",
            workspace_label: "laptop",
            hostname: None,
            workspace_path: None,
            now_ms: crate::credentials::now_ms(),
        })
        .unwrap();
    let approve = f
        .manager
        .external_approve(
            &f.owner,
            &org_proto::external_auth::MemberExternalApproveParams {
                command_id: None,
                code: code.into(),
                ttl_secs: None,
                takeover: false,
            },
        )
        .unwrap_err();
    assert!(only_inside(approve));
    assert!(f.manager.external_lease(&f.tess).is_none());
}

fn add_person(office: &Office, owner: &str, name: &str) -> String {
    use org_office::rules::OrgCommand;
    use org_proto::org::MemberAddPersonParams;

    let id = new_uuid_v7();
    let team = office.org_snapshot().unwrap().teams[0].id.clone();
    office
        .submit_org(
            owner,
            OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: new_uuid_v7(),
                member_id: id.clone(),
                name: name.to_owned(),
                handle: Some(name.to_lowercase()),
                home_team_id: Some(team),
                contact: None,
            }),
        )
        .unwrap()
        .unwrap();
    id
}

/// Inspect, list, transcript and stop RPCs naming `target`, each with a fresh `command_id`.
fn session_rpcs(target: &str) -> Vec<Request> {
    use org_proto::sessions::{
        MemberSessionInspect, MemberSessionInspectParams, MemberSessionList,
        MemberSessionListParams, MemberSessionStop, MemberSessionStopParams,
        MemberSessionTranscript, MemberSessionTranscriptParams,
    };

    let employee_id = Some(target.to_owned());
    vec![
        Request::new::<MemberSessionInspect>(
            1,
            &MemberSessionInspectParams {
                employee_id: employee_id.clone(),
            },
        )
        .unwrap(),
        Request::new::<MemberSessionList>(
            2,
            &MemberSessionListParams {
                employee_id: employee_id.clone(),
                limit: None,
            },
        )
        .unwrap(),
        Request::new::<MemberSessionTranscript>(
            3,
            &MemberSessionTranscriptParams {
                employee_id,
                ..Default::default()
            },
        )
        .unwrap(),
        Request::new::<MemberSessionStop>(
            4,
            &MemberSessionStopParams {
                command_id: new_uuid_v7(),
                employee_id: target.to_owned(),
                reason: None,
            },
        )
        .unwrap(),
    ]
}

fn rpc_error(manager: &SessionManager, caller: &str, request: &Request) -> Option<ErrorCode> {
    let response = manager
        .dispatch_rpc(Some(caller), request)
        .expect("a session RPC");
    match response.outcome {
        org_proto::Outcome::Error(err) => Some(err.code),
        org_proto::Outcome::Result(_) => None,
    }
}

/// Another active person may manage ordinary agents' sessions, but never Tess's: inspecting,
/// listing, reading or stopping them would show the owner's replies and transcripts or end the
/// owner's turn. The owner keeps all four (`TASK-80.1`).
#[test]
fn only_the_owner_may_inspect_list_read_or_stop_her_sessions() {
    let f = fixture();
    let bob = add_person(&f.office, &f.owner, "Bob");
    for request in session_rpcs(&f.tess) {
        assert_eq!(
            rpc_error(&f.manager, &bob, &request),
            Some(ErrorCode::NotAuthorized),
            "a non-owner person is refused `{}` on Tess",
            request.method
        );
    }
    for request in session_rpcs(&f.tess) {
        // The owner passes authorization (her idle session has no transcript yet).
        let owner = rpc_error(&f.manager, &f.owner, &request);
        assert!(
            owner.is_none() || owner == Some(ErrorCode::NotFound),
            "the owner may `{}` on Tess: {owner:?}",
            request.method
        );
    }

    // An ordinary agent's sessions stay open to every active person.
    let root = f.office.org_snapshot().unwrap().teams[0].id.clone();
    let (agent, _) = hire_agent_with_runtime(
        &f.office,
        &f.owner,
        &root,
        "anthropic",
        "claude",
        "ada",
        "claude-sonnet-4",
    );
    for request in session_rpcs(&agent) {
        let bob_call = rpc_error(&f.manager, &bob, &request);
        assert!(
            bob_call.is_none() || bob_call == Some(ErrorCode::NotFound),
            "a person may `{}` on an ordinary agent: {bob_call:?}",
            request.method
        );
    }
}
