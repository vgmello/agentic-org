//! The built-in Chief of Staff's sessions (`TASK-80.1`): her preset is derived from who she is
//! (`OperatorSandboxed`), her operator spec turns Codex's shell on confined to `agent-os`, she
//! works only from her private home, and every run acts for the one caller that started it, in
//! that caller's own partition.

use std::collections::BTreeMap;

use org_agent_runtime::{
    validate_operator_shell, AgentIdentity, AgentName, PathPattern, SessionKind, ToolCapability,
};
use org_office::organization_family::records::BuiltinKey;

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
fn a_run_acts_for_the_caller_that_started_it_and_no_other() {
    let reg = RunBindingRegistry::new();
    reg.delegate_next_run("tess", Some("owner-1"));
    let run = reg.issue("tess", "run-1", "sess-1", 0).unwrap();
    assert_eq!(run.acting_for_caller_id.as_deref(), Some("owner-1"));
    assert_eq!(reg.acting_for_caller_id("tess").as_deref(), Some("owner-1"));
    assert_eq!(reg.acting_for_caller_id("someone-else"), None);
    // The delegation was taken by that run: the next one acts for nobody.
    reg.revoke("tess", None);
    assert_eq!(reg.acting_for_caller_id("tess"), None);
    let next = reg.issue("tess", "run-2", "sess-2", 0).unwrap();
    assert_eq!(next.acting_for_caller_id, None);
    // A cleared delegation is gone; another employee's never reaches her.
    reg.delegate_next_run("tess", Some("member-2"));
    reg.delegate_next_run("tess", None);
    reg.delegate_next_run("hr", Some("member-2"));
    assert_eq!(
        reg.issue("tess", "run-3", "sess-3", 0)
            .unwrap()
            .acting_for_caller_id,
        None
    );
    // An external lease never acts for anyone.
    let lease = reg.issue_external("tess", "ext-1", u64::MAX).unwrap();
    assert_eq!(lease.acting_for_caller_id, None);
    assert!(!format!("{run:?}").contains(run.expose_token()));
}

#[test]
fn each_caller_gets_a_partition_and_the_next_run_acts_for_them() {
    let f = fixture();
    let tess = f.office.chief_of_staff().unwrap().unwrap();
    let owner_turn = f
        .manager
        .enter_partition(&tess, Some(&f.owner))
        .unwrap()
        .expect("her partition");
    assert_eq!(owner_turn.key, PartitionKey::tess_ask(&f.owner));
    assert_eq!(
        (owner_turn.turn, owner_turn.resume_runtime_session_id),
        (1, None)
    );
    let run = f
        .manager
        .bindings()
        .issue(&f.tess, "run-1", "sess-1", 0)
        .unwrap();
    assert_eq!(run.acting_for_caller_id.as_deref(), Some(f.owner.as_str()));

    let member = "0190a5d2-7c3e-7a01-8b2c-3d4e5f60aaaa";
    let member_turn = f
        .manager
        .enter_partition(&tess, Some(member))
        .unwrap()
        .unwrap();
    assert_eq!(member_turn.key.as_str(), format!("tess:ask:{member}"));
    assert_eq!(member_turn.resume_runtime_session_id, None);
    let wake = f.manager.enter_partition(&tess, None).unwrap().unwrap();
    assert_eq!(wake.key, PartitionKey::tess_wake());

    // Anyone else has no partition, and their runs act for nobody.
    let hr = f.office.list_employees().unwrap();
    let hr = hr
        .iter()
        .find(|e| e.builtin_key == Some(BuiltinKey::Hr))
        .unwrap();
    assert_eq!(f.manager.enter_partition(hr, Some(&f.owner)).unwrap(), None);
    let hr_run = f
        .manager
        .bindings()
        .issue(hr.id.as_str(), "run-h", "sess-h", 0)
        .unwrap();
    assert_eq!(hr_run.acting_for_caller_id, None);
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
            },
        )
        .unwrap_err();
    assert_eq!(refused.code, org_proto::ErrorCode::NotAuthorized);
    assert!(refused.message.contains("only inside the office"));
}
