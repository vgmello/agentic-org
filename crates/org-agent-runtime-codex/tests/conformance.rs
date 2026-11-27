use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use org_agent_runtime::testkit::{self, Script, ScriptApproval};
use org_agent_runtime::{
    AgentIdentity, AgentName, AgentRuntime, ApprovalId, CommandPrefix, Control, ControlError,
    CostBasis, Decision, EnvName, Escalation, ExitInfo, LaunchContext, LimitStatus, McpServer,
    McpServerName, Output, PathPattern, PermissionMode, PermissionPolicy, PlainId, PlanError,
    Probe, ProbeContext, ProbeOutput, ProbeResult, Rule, RuleScope, RuleTool, RuntimeId,
    SandboxSpec, SessionEvent, SessionId, SessionKind, SessionSpec, SpecPath, StartFailure, Token,
    TurnOutcome,
};
use org_agent_runtime_codex::CodexRuntime;

fn sp(s: &str) -> SpecPath {
    SpecPath::try_from(s).unwrap()
}

fn pat(s: &str) -> PathPattern {
    PathPattern::try_from(s).unwrap()
}

fn sample_spec() -> SessionSpec {
    let mut mcp_servers = BTreeMap::new();
    mcp_servers.insert(
        McpServerName::try_from("office").unwrap(),
        McpServer {
            command: sp("/office/bin/org-mcp"),
            args: vec!["--stdio".to_owned()],
        },
    );
    SessionSpec {
        runtime: RuntimeId::try_from("codex").unwrap(),
        kind: SessionKind::Task {
            task_id: PlainId::try_from("t1").unwrap(),
        },
        agent: AgentIdentity {
            name: AgentName::try_from("ac-dev-gpt").unwrap(),
            description: "Backend engineer".to_owned(),
            prompt: "You are gpt.".to_owned(),
        },
        model: Some(Token::try_from("gpt-5.4").unwrap()),
        effort: Some(Token::try_from("high").unwrap()),
        workspace: sp("/work/t-1"),
        read_only_dirs: vec![sp("/homes/gpt")],
        instructions: "Follow workspace rules.".to_owned(),
        repository_instructions: String::new(),
        permission_mode: PermissionMode::AcceptEdits,
        policy: PermissionPolicy {
            version: 1,
            allow: vec![
                Rule {
                    tool: RuleTool::try_from("cap:edit").unwrap(),
                    scope: RuleScope::Any,
                },
                Rule {
                    tool: RuleTool::try_from("shell").unwrap(),
                    scope: RuleScope::CommandPrefix(CommandPrefix::try_from("git status").unwrap()),
                },
            ],
            deny: vec![Rule {
                tool: RuleTool::try_from("*").unwrap(),
                scope: RuleScope::Path(pat("/office/durable/**")),
            }],
        },
        sandbox: SandboxSpec {
            enabled: true,
            writable_roots: vec![sp("/work/t-1")],
            deny_write: vec![pat("/work/t-1/.git/hooks/**")],
            deny_read: vec![pat("/office/offices/*/**")],
            allowed_unix_sockets: vec![sp("/office/run/gpt.sock")],
        },
        mcp_servers,
        tools: None,
        operator_shell_prefixes: Vec::new(),
    }
}

fn sample_ctx() -> LaunchContext {
    LaunchContext {
        runtime_path: PathBuf::from("/opt/homebrew/bin/codex"),
        runtime_version: Some("0.169.0".to_owned()),
        runtime_dir: sp("/office/agents/gpt/runtime"),
        tmpdir: sp("/private/tmp/agentic-org/run-2"),
        office_env_names: vec![EnvName::try_from("AGENT_OS_SESSION_TOKEN").unwrap()],
        new_session_id: Some(SessionId::try_from("codex-thread-1").unwrap()),
        resume: None,
        config_dir: Some(sp("/office/agents/gpt/home/codex")),
    }
}

fn thread_ok_reply(id: &str) -> Vec<u8> {
    format!(
        r#"{{"jsonrpc":"2.0","id":2,"result":{{"thread":{{"id":"{id}"}},"activePermissionProfile":{{"id":"office","extends":null}},"sandbox":{{"type":"workspaceWrite","writableRoots":[]}},"approvalPolicy":{{"granular":{{"sandbox_approval":false,"rules":true,"mcp_elicitations":false,"request_permissions":false,"skill_approval":false}}}}}}}}"#
    )
    .into_bytes()
}

fn codex_script() -> Script {
    let init_ok =
        br#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"codex","version":"0.169.0"}}}"#
            .to_vec();
    let thread_ok = thread_ok_reply("thr_0193a5c0");
    let turn_started =
        br#"{"jsonrpc":"2.0","method":"turn/started","params":{"turn":{"id":"turn_1"}}}"#.to_vec();
    let msg_delta =
        br#"{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"delta":"Working."}}"#
            .to_vec();
    let usage = br#"{"jsonrpc":"2.0","method":"thread/tokenUsage/updated","params":{"tokenUsage":{"last":{"inputTokens":50,"outputTokens":25,"cachedInputTokens":10,"cacheWriteInputTokens":4,"reasoningOutputTokens":5}}}}"#.to_vec();
    let approval_req = br#"{"jsonrpc":"2.0","id":"apr-7","method":"item/commandExecution/requestApproval","params":{"itemId":"cmd_1","command":"echo hi","reason":"Check output"}}"#.to_vec();
    let approval_cancel =
        br#"{"jsonrpc":"2.0","method":"serverRequest/resolved","params":{"requestId":"apr-7"}}"#
            .to_vec();
    let turn_done = br#"{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"turn_1","status":"completed"}}}"#.to_vec();
    let steer_ack = br#"{"jsonrpc":"2.0","id":4,"result":{"turnId":"turn_1"}}"#.to_vec();

    let mut approval = ScriptApproval::new(approval_req, ApprovalId::new("apr-7"));
    approval.cancel = Some(approval_cancel);
    Script {
        opening: vec![init_ok, thread_ok],
        turn: vec![turn_started, msg_delta, usage],
        turn_end: vec![turn_done],
        stderr: vec![b"codex diagnostic".to_vec()],
        approval: Some(approval),
        mid_turn_step: Some(steer_ack),
    }
}

#[test]
fn passes_org_agent_runtime_testkit_conformance_and_plan_checks() {
    let runtime = CodexRuntime::new();
    let spec = sample_spec();
    let ctx = sample_ctx();
    let caps = runtime.capabilities();
    assert_eq!(
        caps.mid_turn_delivery,
        org_agent_runtime::MidTurnDelivery::NativeSteer
    );
    assert!(caps.supports_mid_turn_delivery());
    // A person's steer goes in natively as `turn/steer`, never as an interrupt (`TASK-103`).
    assert_eq!(caps.steer_route(), Ok(()));
    assert!(caps.supports_native_steer());
    assert_eq!(caps.cost, CostBasis::None);
    let (plan, _) = runtime.plan_session(&spec, &ctx).expect("plan_session");

    testkit::check_plan(&plan, &spec, &ctx, &caps);
    let script = codex_script();
    testkit::check_all(
        &mut || runtime.plan_session(&spec, &ctx).expect("plan_session").1,
        &script,
        &caps,
        Instant::now(),
    );
}

#[test]
fn plan_pins_shell_env_policy_clears_mcp_and_enforces_paths_and_take_over() {
    let runtime = CodexRuntime::new();
    let mut spec = sample_spec();
    spec.sandbox
        .writable_roots
        .push(sp("/office/agents/gpt/home/codex"));
    let ctx = sample_ctx();
    let (plan, _) = runtime.plan_session(&spec, &ctx).unwrap();
    assert_eq!(plan.args[0], "app-server");
    assert!(plan.args.contains(&"--stdio".to_owned()));
    assert!(!plan.args.iter().any(|a| a.contains("danger-full-access")));
    assert!(plan
        .args
        .iter()
        .any(|a| a.contains("/private/tmp/agentic-org/run-2")));
    // Named `permissions.office` profile replaces legacy `sandbox_mode` / `writable_roots`:
    // `"/"="read"` base, literal workspace `"write"`, `config_dir` `"read"` (with `auth.json="none"`),
    // fixed denies `"none"`/`"read"`, and office-socket-only network isolation.
    assert!(!plan.args.iter().any(|a| a.starts_with("sandbox_mode=")));
    assert!(!plan
        .args
        .iter()
        .any(|a| a.starts_with("sandbox_workspace_write.writable_roots=")));
    assert!(plan
        .args
        .contains(&"features.network_proxy=true".to_owned()));
    assert!(plan
        .args
        .contains(&"default_permissions=\"office\"".to_owned()));
    assert!(plan.args.iter().any(|a| a
        == "approval_policy={granular={sandbox_approval=false,rules=true,mcp_elicitations=false,request_permissions=false,skill_approval=false}}"));
    let fs_profile = plan
        .args
        .iter()
        .find(|a| a.starts_with("permissions.office.filesystem="))
        .expect("permissions.office.filesystem arg");
    assert!(fs_profile.contains("\"/\"=\"read\""), "{fs_profile}");
    assert!(
        fs_profile.contains("\"/work/t-1\"=\"write\""),
        "{fs_profile}"
    );
    assert!(
        fs_profile.contains("\"/office/offices\"=\"none\""),
        "{fs_profile}"
    );
    assert!(
        fs_profile.contains("\"/work/t-1/.git/hooks\"=\"read\""),
        "{fs_profile}"
    );
    assert!(
        fs_profile.contains("\"/office/agents/gpt/home/codex\"=\"read\""),
        "{fs_profile}"
    );
    assert!(
        !fs_profile.contains("\"/office/agents/gpt/home/codex\"=\"write\""),
        "{fs_profile}"
    );
    assert!(
        fs_profile.contains("\"/office/agents/gpt/home/codex/auth.json\"=\"none\""),
        "{fs_profile}"
    );
    let net_profile = plan
        .args
        .iter()
        .find(|a| a.starts_with("permissions.office.network="))
        .expect("permissions.office.network arg");
    assert!(
        net_profile.contains("unix_sockets={\"/office/run/gpt.sock\"=\"allow\"}"),
        "{net_profile}"
    );

    // Nested write grants under a write/read deny base are stripped so Codex cannot re-open
    // writes inside a denied subtree (`permissions.office.filesystem`).
    let mut nested_write_spec = sample_spec();
    nested_write_spec.sandbox.deny_write = vec![pat("/work/t-1/.git/**")];
    nested_write_spec
        .sandbox
        .writable_roots
        .push(sp("/work/t-1/.git/hooks"));
    let (nested_plan, _) = runtime.plan_session(&nested_write_spec, &ctx).unwrap();
    let nested_fs = nested_plan
        .args
        .iter()
        .find(|a| a.starts_with("permissions.office.filesystem="))
        .expect("permissions.office.filesystem arg");
    assert!(
        nested_fs.contains("\"/work/t-1/.git\"=\"read\""),
        "{nested_fs}"
    );
    assert!(
        !nested_fs.contains("\"/work/t-1/.git/hooks\"=\"write\""),
        "nested write grant must NOT override parent deny_write: {nested_fs}"
    );

    // Writable root or TMPDIR beneath `read_only_dirs` is stripped / refused.
    let mut ro_conflict_spec = sample_spec();
    ro_conflict_spec
        .sandbox
        .writable_roots
        .push(sp("/homes/gpt/sub"));
    let (ro_conflict_plan, _) = runtime.plan_session(&ro_conflict_spec, &ctx).unwrap();
    let ro_conflict_fs = ro_conflict_plan
        .args
        .iter()
        .find(|a| a.starts_with("permissions.office.filesystem="))
        .expect("permissions.office.filesystem arg");
    assert!(
        ro_conflict_fs.contains("\"/homes/gpt\"=\"read\""),
        "{ro_conflict_fs}"
    );
    assert!(
        !ro_conflict_fs.contains("\"/homes/gpt/sub\"=\"write\""),
        "writable root beneath read_only_dirs must be stripped: {ro_conflict_fs}"
    );
    let mut ro_tmpdir_ctx = sample_ctx();
    ro_tmpdir_ctx.tmpdir = sp("/homes/gpt/tmp");
    assert!(matches!(
        runtime.plan_session(&sample_spec(), &ro_tmpdir_ctx),
        Err(PlanError::InvalidSpec(_))
    ));

    // macOS root-symlink alias overlap (`/var/...` deny vs `/private/var/...` writable root,
    // and read-only `/private` ancestor above `/private/var` TMPDIR or `/var` writable root) fails closed
    // on macOS, while on non-macOS `/var` and `/private/var` remain disjoint literal paths.
    let mut private_ro_spec = sample_spec();
    private_ro_spec.read_only_dirs.push(sp("/private"));
    assert!(matches!(
        runtime.plan_session(&private_ro_spec, &ctx),
        Err(PlanError::InvalidSpec(_))
    ));
    let mut alias_spec = sample_spec();
    alias_spec.sandbox.deny_write.push(pat("/var/protected/**"));
    alias_spec
        .sandbox
        .writable_roots
        .push(sp("/private/var/protected/child"));
    let mut private_ro_alias_spec = sample_spec();
    private_ro_alias_spec.read_only_dirs.push(sp("/private"));
    private_ro_alias_spec
        .sandbox
        .writable_roots
        .push(sp("/var/nested"));
    let mut non_private_tmp_ctx = sample_ctx();
    non_private_tmp_ctx.tmpdir = sp("/work/tmp-run-2");
    if org_platform::fs::has_macos_private_root_aliases() {
        assert!(matches!(
            runtime.plan_session(&alias_spec, &ctx),
            Err(PlanError::InvalidSpec(_))
        ));
        assert!(matches!(
            runtime.plan_session(&private_ro_alias_spec, &non_private_tmp_ctx),
            Err(PlanError::InvalidSpec(_))
        ));
    } else {
        assert!(runtime.plan_session(&alias_spec, &ctx).is_ok());
        assert!(runtime
            .plan_session(&private_ro_alias_spec, &non_private_tmp_ctx)
            .is_ok());
    }

    // Literal paths with Codex glob metacharacters (`**`, `?`, `[...]`) fail closed before emitting profile.
    for bad_literal in ["/work/t-1/**", "/work/dir[1]", "/work/q?x"] {
        let mut glob_root_spec = sample_spec();
        glob_root_spec.sandbox.writable_roots.push(sp(bad_literal));
        assert!(
            matches!(
                runtime.plan_session(&glob_root_spec, &ctx),
                Err(PlanError::Unsupported(_))
            ),
            "expected Unsupported for literal path with glob metacharacter `{bad_literal}`"
        );
    }

    // Ancestor `"none"` deny covering `config_dir` prevents emitting a deeper `"read"` exception
    // for `config_dir` (`permissions.office.filesystem` respects the stricter ancestor deny).
    let mut ancestor_deny_spec = sample_spec();
    ancestor_deny_spec.policy.deny.push(Rule {
        tool: RuleTool::try_from("*").unwrap(),
        scope: RuleScope::Path(pat("/office/**")),
    });
    let (ancestor_plan, _) = runtime.plan_session(&ancestor_deny_spec, &ctx).unwrap();
    let ancestor_fs = ancestor_plan
        .args
        .iter()
        .find(|a| a.starts_with("permissions.office.filesystem="))
        .expect("permissions.office.filesystem arg");
    assert!(
        ancestor_fs.contains("\"/office\"=\"none\""),
        "{ancestor_fs}"
    );
    assert!(
        !ancestor_fs.contains("\"/office/agents/gpt/home/codex\"=\"read\""),
        "ancestor `none` deny must not be widened by a deeper config_dir `read` entry: {ancestor_fs}"
    );

    // SafeAsk (`PermissionMode::Ask`, empty allows) emits ZERO `"write"` entries in
    // `permissions.office.filesystem` (neither workspace nor TMPDIR is writable) and disables
    // shell pre-execution so Codex cannot auto-approve workspace patches or commands.
    let mut safe_ask_spec = sample_spec();
    safe_ask_spec.permission_mode = PermissionMode::Ask;
    safe_ask_spec.policy.allow.clear();
    let (safe_ask_plan, _) = runtime.plan_session(&safe_ask_spec, &ctx).unwrap();
    assert!(safe_ask_plan
        .args
        .contains(&"features.shell_tool=false".to_owned()));
    let safe_ask_fs = safe_ask_plan
        .args
        .iter()
        .find(|a| a.starts_with("permissions.office.filesystem="))
        .expect("permissions.office.filesystem arg");
    assert!(
        !safe_ask_fs.contains("=\"write\""),
        "SafeAsk must emit zero write roots so Codex cannot auto-approve patches: {safe_ask_fs}"
    );

    // Legacy path (`sandbox.enabled = false`) with any path/edit/read-only deny fails closed to
    // `sandbox_mode="read-only"` and `features.shell_tool=false` even when `AcceptEdits` and
    // unscoped `cap:edit`/`cap:shell` are present, because no named profile installs those denies.
    let mut legacy_unsandboxed = sample_spec();
    legacy_unsandboxed.sandbox.enabled = false;
    legacy_unsandboxed.policy.allow.push(Rule {
        tool: org_agent_runtime::ToolCapability::Shell.to_rule_tool(),
        scope: RuleScope::Any,
    });
    let (legacy_plan, _) = runtime.plan_session(&legacy_unsandboxed, &ctx).unwrap();
    assert!(
        legacy_plan
            .args
            .contains(&"sandbox_mode=\"read-only\"".to_owned()),
        "legacy unsandboxed plan with path denies must fail closed to read-only: {:?}",
        legacy_plan.args
    );
    assert!(
        legacy_plan
            .args
            .contains(&"features.shell_tool=false".to_owned()),
        "legacy unsandboxed plan with path denies must disable shell: {:?}",
        legacy_plan.args
    );

    // Leading-wildcard path deny patterns fail closed instead of silently dropping.
    let mut bad_wildcard = sample_spec();
    bad_wildcard.sandbox.deny_read.push(pat("/*/secrets/**"));
    assert!(matches!(
        runtime.plan_session(&bad_wildcard, &ctx),
        Err(PlanError::Unsupported(_))
    ));
    assert!(plan
        .args
        .contains(&"shell_environment_policy.ignore_default_excludes=true".to_owned()));
    assert!(plan.args.iter().any(|a| a
        == r#"shell_environment_policy.include_only=["PATH","HOME","TMPDIR","LANG","TERM","AGENT_OS_SESSION_TOKEN"]"#));
    assert!(plan.args.contains(&"mcp_servers={}".to_owned()));
    assert!(plan
        .env_add
        .iter()
        .any(|(k, v)| k.as_str() == "CODEX_HOME" && v == "/office/agents/gpt/home/codex"));

    let mut bad = sample_spec();
    bad.workspace = sp("/");
    assert!(matches!(
        runtime.plan_session(&bad, &ctx),
        Err(PlanError::InvalidSpec(_))
    ));

    // Take-over refuses missing resume and OneOff sessions.
    assert!(matches!(
        runtime.plan_take_over(&spec, &ctx, 80, 24),
        Err(PlanError::InvalidSpec(_))
    ));
    let mut resume_ctx = sample_ctx();
    resume_ctx.resume = Some(SessionId::try_from("thr_resume_9").unwrap());
    let mut one_off = sample_spec();
    one_off.kind = SessionKind::OneOff {
        run_id: PlainId::try_from("r1").unwrap(),
    };
    assert!(matches!(
        runtime.plan_take_over(&one_off, &resume_ctx, 80, 24),
        Err(PlanError::InvalidSpec(_))
    ));
}

#[test]
fn handshake_validates_sandbox_policy_resume_failure_and_empty_agents() {
    let runtime = CodexRuntime::new();
    let mut plan_spec = sample_spec();
    plan_spec.permission_mode = PermissionMode::Plan;
    let now = Instant::now();

    // Active permission profile MUST be `"office"` when sandbox is enabled (omitting legacy `sandbox` on thread/start)
    // and `initialize` must opt into `capabilities.experimentalApi: true` so Codex accepts granular `approvalPolicy`.
    let (_, mut s1) = runtime.plan_session(&plan_spec, &sample_ctx()).unwrap();
    let init_writes = s1.start(now);
    let init_text = init_writes
        .iter()
        .filter_map(|o| match o {
            Output::Write(l) => std::str::from_utf8(l.as_bytes()).ok(),
            _ => None,
        })
        .collect::<String>();
    assert!(
        init_text.contains(r#""experimentalApi":true"#),
        "initialize must opt into experimentalApi for granular approvalPolicy: {init_text}"
    );
    let open_writes = s1.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"version":"0.169.0"}}}"#,
        now,
    );
    let open_text = open_writes
        .iter()
        .filter_map(|o| match o {
            Output::Write(l) => std::str::from_utf8(l.as_bytes()).ok(),
            _ => None,
        })
        .collect::<String>();
    assert!(
        !open_text.contains(r#""sandbox":"#),
        "thread/start must NOT send legacy `sandbox` override when office profile is active: {open_text}"
    );
    assert!(
        open_text.contains(r#""permissions":"office""#),
        "thread/start must pin permissions:\"office\": {open_text}"
    );
    assert!(open_text.contains(r#""sandbox_approval":false"#));
    let out = s1.on_line(
        br#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_1"},"activePermissionProfile":{"id":"other"},"approvalPolicy":{"granular":{"sandbox_approval":false,"rules":true,"mcp_elicitations":false,"request_permissions":false,"skill_approval":false}}}}"#,
        now,
    );
    assert!(out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::ModeMismatch { .. }
        })
    )));

    // approvalPolicy="never" or granular sandbox_approval=true or plain "on-request" all fail closed with ModeMismatch.
    for bad_pol in [
        br#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_1"},"activePermissionProfile":{"id":"office"},"approvalPolicy":"never"}}"#.as_slice(),
        br#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_1"},"activePermissionProfile":{"id":"office"},"approvalPolicy":"on-request"}}"#.as_slice(),
        br#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_1"},"activePermissionProfile":{"id":"office"},"approvalPolicy":{"granular":{"sandbox_approval":true,"rules":true,"mcp_elicitations":false,"request_permissions":false}}}}"#.as_slice(),
    ] {
        let (_, mut s2) = runtime.plan_session(&sample_spec(), &sample_ctx()).unwrap();
        let _ = s2.start(now);
        let _ = s2.on_line(
            br#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"version":"0.169.0"}}}"#,
            now,
        );
        let out = s2.on_line(bad_pol, now);
        assert!(out.iter().any(|o| matches!(
            o,
            Output::Event(SessionEvent::StartFailed {
                reason: StartFailure::ModeMismatch { .. }
            })
        )));
    }

    // thread/resume error must emit StartFailure::ResumeFailed.
    let mut resume_ctx = sample_ctx();
    resume_ctx.resume = Some(SessionId::try_from("thr_old").unwrap());
    let (_, mut s3) = runtime.plan_session(&sample_spec(), &resume_ctx).unwrap();
    let _ = s3.start(now);
    let open_writes = s3.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"version":"0.169.0"}}}"#,
        now,
    );
    let thread_open_text = open_writes
        .iter()
        .filter_map(|o| match o {
            Output::Write(l) => std::str::from_utf8(l.as_bytes()).ok(),
            _ => None,
        })
        .collect::<String>();
    assert!(thread_open_text.contains(r#""approvalsReviewer":"user""#));
    assert!(
        thread_open_text.contains(r#""permissions":"office""#),
        "thread/resume must pin permissions:\"office\": {thread_open_text}"
    );
    assert!(!thread_open_text.contains(r#""sandbox":"#));
    let out = s3.on_line(
        br#"{"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"thread not found"}}"#,
        now,
    );
    assert!(out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::ResumeFailed { .. }
        })
    )));

    // Ready must report empty agents list for Codex and granular:rules mode.
    let (_, mut s4) = runtime.plan_session(&sample_spec(), &sample_ctx()).unwrap();
    let _ = s4.start(now);
    let _ = s4.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"version":"0.169.0"}}}"#,
        now,
    );
    let out = s4.on_line(&thread_ok_reply("thr_ok"), now);
    assert!(out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::Ready { agents, permission_mode, .. })
            if agents.is_empty() && permission_mode.as_deref() == Some("granular:rules")
    )));
    // turn/start must also pin permissions:"office", approvalsReviewer:"user", and granular approvalPolicy.
    let turn_writes = s4
        .control(
            Control::Send {
                text: "hello".into(),
                images: Vec::new(),
            },
            now,
        )
        .unwrap();
    let turn_text = turn_writes
        .iter()
        .filter_map(|o| match o {
            Output::Write(l) => std::str::from_utf8(l.as_bytes()).ok(),
            _ => None,
        })
        .collect::<String>();
    assert!(
        turn_text.contains(r#""permissions":"office""#),
        "turn/start must pin permissions:\"office\": {turn_text}"
    );
    assert!(
        turn_text.contains(r#""approvalsReviewer":"user""#),
        "turn/start must pin approvalsReviewer:\"user\": {turn_text}"
    );
    assert!(
        turn_text.contains(r#""sandbox_approval":false"#),
        "turn/start must pin granular approvalPolicy: {turn_text}"
    );

    // Missing approvalPolicy, missing activePermissionProfile, and object-form dangerFullAccess all fail closed.
    for bad_reply in [
        br#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_1"},"activePermissionProfile":{"id":"office"}}}"#
            .as_slice(),
        br#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_1"},"approvalPolicy":{"granular":{"sandbox_approval":false,"rules":true,"mcp_elicitations":false,"request_permissions":false}}}}"#
            .as_slice(),
        br#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_1"},"activePermissionProfile":{"id":"office"},"sandboxPolicy":{"type":"dangerFullAccess"},"approvalPolicy":{"granular":{"sandbox_approval":false,"rules":true,"mcp_elicitations":false,"request_permissions":false}}}}"#
            .as_slice(),
    ] {
        let (_, mut sx) = runtime.plan_session(&sample_spec(), &sample_ctx()).unwrap();
        let _ = sx.start(now);
        let _ = sx.on_line(
            br#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"version":"0.169.0"}}}"#,
            now,
        );
        let out = sx.on_line(bad_reply, now);
        assert!(
            out.iter().any(|o| matches!(
                o,
                Output::Event(SessionEvent::StartFailed {
                    reason: StartFailure::ModeMismatch { .. }
                })
            )),
            "expected ModeMismatch for {:?}",
            std::str::from_utf8(bad_reply)
        );
    }
}

#[test]
fn turn_error_recovery_multistep_usage_escalations_unknown_server_req_and_rate_limits() {
    let runtime = CodexRuntime::new();
    let spec = sample_spec();
    let now = Instant::now();
    let (_, mut session) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = session.start(now);
    assert!(session.start(now).is_empty());
    let _ = session.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"version":"0.169.0"}}}"#,
        now,
    );
    let _ = session.on_line(&thread_ok_reply("thr_ok"), now);

    // Idle Interrupt returns Ok(vec![]) without writing turn/interrupt.
    assert!(session.control(Control::Interrupt, now).unwrap().is_empty());

    // Idle usage notification does NOT leak as a phantom delta.
    assert!(session
        .on_line(
            br#"{"jsonrpc":"2.0","method":"thread/tokenUsage/updated","params":{"tokenUsage":{"last":{"inputTokens":999,"outputTokens":999}}}}"#,
            now
        )
        .is_empty());

    // turn/start RPC error unwinds Phase::Busy so next Send succeeds.
    let start_out = session
        .control(
            Control::Send {
                text: "fail first".into(),
                images: Vec::new(),
            },
            now,
        )
        .unwrap();
    assert!(start_out.iter().any(|o| match o {
        Output::Write(l) => std::str::from_utf8(l.as_bytes())
            .unwrap_or("")
            .contains(r#""sandbox_approval":false"#),
        _ => false,
    }));
    let err_out = session.on_line(
        br#"{"jsonrpc":"2.0","id":3,"error":{"code":-32000,"message":"rate limited"}}"#,
        now,
    );
    assert!(err_out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::TurnEnded {
            outcome: TurnOutcome::Failed { .. },
            ..
        })
    )));

    // Second turn succeeds and accumulates multi-step usage + cacheWriteInputTokens.
    let _ = session
        .control(
            Control::Send {
                text: "second turn".into(),
                images: Vec::new(),
            },
            now,
        )
        .unwrap();
    let _ = session.on_line(
        br#"{"jsonrpc":"2.0","method":"thread/tokenUsage/updated","params":{"tokenUsage":{"last":{"inputTokens":10,"outputTokens":5,"cacheWriteInputTokens":2}}}}"#,
        now,
    );
    let _ = session.on_line(
        br#"{"jsonrpc":"2.0","method":"thread/tokenUsage/updated","params":{"tokenUsage":{"last":{"inputTokens":20,"outputTokens":7,"cacheWriteInputTokens":3}}}}"#,
        now,
    );

    // Unknown server request sends JSON-RPC -32601 error reply so server never hangs.
    let unk = session.on_line(
        br#"{"jsonrpc":"2.0","id":99,"method":"unknown/serverMethod","params":{}}"#,
        now,
    );
    assert!(unk.iter().any(|o| match o {
        Output::Write(l) => std::str::from_utf8(l.as_bytes())
            .unwrap_or("")
            .contains("-32601"),
        _ => false,
    }));

    // Escalation classifier handles require_escalated, additionalPermissions, and blockedPath.
    let esc_sb = session.on_line(
        br#"{"jsonrpc":"2.0","id":"e1","method":"item/commandExecution/requestApproval","params":{"sandboxPermissions":"require_escalated","blockedPath":"/etc/hosts"}}"#,
        now,
    );
    let req_sb = esc_sb
        .iter()
        .find_map(|o| match o {
            Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
            _ => None,
        })
        .unwrap();
    assert_eq!(req_sb.escalation, Escalation::SandboxBypass);
    assert_eq!(req_sb.blocked_path.as_deref(), Some("/etc/hosts"));

    // Deny { stop_turn: true } sends {"decision":"cancel"}.
    let deny_out = session
        .control(
            Control::Answer {
                request_id: ApprovalId::new("e1"),
                decision: Decision::Deny {
                    message: Some("no".into()),
                    stop_turn: true,
                },
            },
            now,
        )
        .unwrap();
    assert!(deny_out.iter().any(|o| match o {
        Output::Write(l) => std::str::from_utf8(l.as_bytes())
            .unwrap_or("")
            .contains(r#""decision":"cancel""#),
        _ => false,
    }));

    // Rate limits parse both RFC 3339 strings and numeric Unix epoch seconds/ms as RFC 3339 UTC.
    let rl = session.on_line(
        br#"{"jsonrpc":"2.0","method":"account/rateLimits/updated","params":{"rateLimits":{"primary":{"usedPercent":85.0,"resetsAt":1795359600},"secondary":{"usedPercent":50.0,"resetsAt":1795359600000}}}}"#,
        now,
    );
    assert!(rl.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::RateLimit(r))
            if r.status == LimitStatus::Warning && r.resets_at.as_deref() == Some("2026-11-22T15:00:00Z")
    )));
    assert!(rl.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::RateLimit(r))
            if r.status == LimitStatus::Ok && r.resets_at.as_deref() == Some("2026-11-22T15:00:00Z")
    )));

    // Completing turn flushes accumulated usage (10+20=30 input, 5+7=12 output, 2+3=5 cache_write).
    let done = session.on_line(
        br#"{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"t2","status":"completed"}}}"#,
        now,
    );
    assert!(done.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::Usage(u))
            if u.input == 30 && u.output == 12 && u.cache_write == 5
    )));

    // Truncated emits SessionEvent::Truncated.
    assert!(matches!(
        session.on_truncated(4096, now).as_slice(),
        [Output::Event(SessionEvent::Truncated { len: 4096 })]
    ));

    // Mid-turn crash emits TurnOutcome::Failed.
    let _ = session
        .control(
            Control::Send {
                text: "crash".into(),
                images: Vec::new(),
            },
            now,
        )
        .unwrap();
    let crashed = session.on_exit(
        ExitInfo {
            code: Some(1),
            signal: None,
        },
        now,
    );
    assert!(crashed.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::TurnEnded {
            outcome: TurnOutcome::Failed { .. },
            ..
        })
    )));

    // Probes: reject root runtime_dir and check version range.
    let mut pctx = ProbeContext::new(PathBuf::from("/opt/homebrew/bin/codex"));
    pctx.runtime_dir = Some(sp("/"));
    assert!(runtime.plan_probe(Probe::Version, &pctx).is_none());
    let out_v = ProbeOutput::new(b"codex-cli 0.169.0\n".to_vec(), Vec::new(), Some(0));
    assert_eq!(
        runtime.parse_probe(Probe::Version, &out_v),
        ProbeResult::Version {
            raw: "codex-cli 0.169.0".to_owned(),
            in_tested_range: Some(true),
        }
    );
    let out_v171 = ProbeOutput::new(b"codex-cli 0.171.0\n".to_vec(), Vec::new(), Some(0));
    assert_eq!(
        runtime.parse_probe(Probe::Version, &out_v171),
        ProbeResult::Version {
            raw: "codex-cli 0.171.0".to_owned(),
            in_tested_range: Some(true),
        }
    );
    assert_eq!(
        session.control(Control::Close, now),
        Err(ControlError::Closed)
    );
}

#[test]
fn codex_shell_env_policy_preserves_posix_path_and_excludes_secrets_in_real_shell() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let runtime = CodexRuntime::new();
    let spec = sample_spec();
    let mut ctx = sample_ctx();
    // Even if a caller accidentally passes a secret name, Codex's plan excludes it.
    ctx.office_env_names
        .push(EnvName::try_from("OPENAI_API_KEY").unwrap());
    let (plan, _) = runtime.plan_session(&spec, &ctx).unwrap();

    let include_arg = plan
        .args
        .iter()
        .find_map(|a| a.strip_prefix("shell_environment_policy.include_only="))
        .expect("include_only arg present");
    let include_only: Vec<String> =
        serde_json::from_str(include_arg).expect("include_only is valid JSON string array");
    for required in [
        "PATH",
        "HOME",
        "TMPDIR",
        "LANG",
        "TERM",
        "AGENT_OS_SESSION_TOKEN",
    ] {
        assert!(
            include_only.iter().any(|n| n == required),
            "include_only missing `{required}`: {include_only:?}"
        );
    }
    assert!(
        !include_only.iter().any(|n| n == "OPENAI_API_KEY"),
        "secret env must not appear in include_only: {include_only:?}"
    );

    // Create a temporary bin dir with an executable `agent-os` stub and verify a real `/bin/sh`
    // receiving ONLY the variables permitted by `shell_environment_policy.include_only` resolves
    // `agent-os` on PATH, sees AGENT_OS_SESSION_TOKEN, and does NOT see ambient secrets.
    let unique = format!(
        "codex-shell-env-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let root = std::env::temp_dir().join(unique);
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let stub = bin_dir.join("agent-os");
    fs::write(&stub, "#!/bin/sh\necho ok\n").unwrap();
    let mut perms = fs::metadata(&stub).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&stub, perms).unwrap();

    let ambient = [
        ("PATH", format!("{}:/usr/bin:/bin", bin_dir.display())),
        ("HOME", root.display().to_string()),
        ("TMPDIR", root.display().to_string()),
        ("LANG", "C.UTF-8".to_owned()),
        ("TERM", "xterm-256color".to_owned()),
        ("AGENT_OS_SESSION_TOKEN", "tok-live-conformance".to_owned()),
        (
            "OPENAI_API_KEY",
            "sk-secret-ambient-must-not-leak".to_owned(),
        ),
    ];

    let mut cmd = Command::new("/bin/sh");
    cmd.env_clear();
    for (k, v) in &ambient {
        if include_only.iter().any(|allowed| allowed == k) {
            cmd.env(k, v);
        }
    }
    let output = cmd
        .arg("-c")
        .arg("command -v agent-os && printf '%s|%s' \"$AGENT_OS_SESSION_TOKEN\" \"${OPENAI_API_KEY:-}\"")
        .output()
        .expect("spawn /bin/sh");
    let _ = fs::remove_dir_all(&root);

    assert!(
        output.status.success(),
        "real shell failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "unexpected stdout: {stdout:?}");
    assert!(
        lines[0].ends_with("/agent-os"),
        "expected agent-os on PATH, got {}",
        lines[0]
    );
    assert_eq!(lines[1], "tok-live-conformance|");
}

#[test]
fn codex_capability_approvals_subagents_and_unknown_capability_guards() {
    use org_agent_runtime::{ToolCapability, ToolName};

    let runtime = CodexRuntime::new();

    // Three-preset matrix (`ReadOnly`, `WorkspaceEdits`, `AutonomousSandboxed`):
    // every preset pins `default_permissions="office"` + granular `approval_policy`
    // (`sandbox_approval=false,rules=true,mcp_elicitations=false,request_permissions=false,skill_approval=false`)
    // and omits legacy `sandbox_mode`.
    let granular_arg = "approval_policy={granular={sandbox_approval=false,rules=true,mcp_elicitations=false,request_permissions=false,skill_approval=false}}";
    let mut ro_spec = sample_spec();
    ro_spec.permission_mode = PermissionMode::Plan;
    ro_spec.tools = Some(vec![
        ToolCapability::Read.to_tool_name(),
        ToolCapability::Search.to_tool_name(),
        ToolCapability::Shell.to_tool_name(),
    ]);
    ro_spec.policy.allow = vec![
        Rule {
            tool: ToolCapability::Read.to_rule_tool(),
            scope: RuleScope::Any,
        },
        Rule {
            tool: ToolCapability::Search.to_rule_tool(),
            scope: RuleScope::Any,
        },
        Rule {
            tool: ToolCapability::Shell.to_rule_tool(),
            scope: RuleScope::CommandPrefix(CommandPrefix::try_from("agent-os").unwrap()),
        },
    ];
    let now = Instant::now();
    let (ro_plan, mut ro_session) = runtime.plan_session(&ro_spec, &sample_ctx()).unwrap();
    assert!(!ro_plan.args.iter().any(|a| a.starts_with("sandbox_mode=")));
    assert!(ro_plan
        .args
        .contains(&"default_permissions=\"office\"".to_owned()));
    assert!(ro_plan.args.contains(&granular_arg.to_owned()));
    // Codex cannot natively gate scoped `CommandPrefix("agent-os")` shell allowlists, so
    // `ReadOnly` fails closed by disabling `features.shell_tool=false` pre-execution.
    assert!(ro_plan
        .args
        .contains(&"features.shell_tool=false".to_owned()));
    assert!(ro_plan.args.iter().any(|a| {
        a.starts_with("permissions.office.filesystem=")
            && a.contains("\"/\"=\"read\"")
            && a.contains("\"/work/t-1\"=\"read\"")
            && !a.contains("=\"write\"")
    }));
    let _ = ro_session.start(now);
    let _ = ro_session.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"version":"0.169.0"}}}"#,
        now,
    );
    let ro_ready = ro_session.on_line(&thread_ok_reply("thr_ro"), now);
    assert!(ro_ready
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::Ready { .. }))));

    for (label, extra_shell_scope, expect_shell_disabled) in [
        (
            "WorkspaceEdits",
            RuleScope::CommandPrefix(CommandPrefix::try_from("agent-os").unwrap()),
            true,
        ),
        ("AutonomousSandboxed", RuleScope::Any, false),
    ] {
        let mut s = sample_spec();
        s.permission_mode = PermissionMode::AcceptEdits;
        s.tools = None;
        s.policy.allow = vec![
            Rule {
                tool: ToolCapability::Read.to_rule_tool(),
                scope: RuleScope::Any,
            },
            Rule {
                tool: ToolCapability::Edit.to_rule_tool(),
                scope: RuleScope::Any,
            },
            Rule {
                tool: ToolCapability::Shell.to_rule_tool(),
                scope: extra_shell_scope,
            },
        ];
        let (p, mut preset_session) = runtime.plan_session(&s, &sample_ctx()).unwrap();
        assert_eq!(
            p.args.contains(&"features.shell_tool=false".to_owned()),
            expect_shell_disabled,
            "{label} shell_tool fail-closed expectation mismatch"
        );
        assert!(
            !p.args.iter().any(|a| a.starts_with("sandbox_mode=")),
            "{label} should use named permissions.office profile rather than legacy sandbox_mode"
        );
        assert!(
            p.args.iter().any(|a| {
                a.starts_with("permissions.office.filesystem=")
                    && a.contains("\"/work/t-1\"=\"write\"")
            }),
            "{label} should grant workspace write inside permissions.office"
        );
        assert!(
            p.args.contains(&granular_arg.to_owned()),
            "{label} should use granular approval_policy"
        );
        let _ = preset_session.start(now);
        let _ = preset_session.on_line(
            br#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"version":"0.171.0"}}}"#,
            now,
        );
        let ready = preset_session.on_line(&thread_ok_reply("thr_preset"), now);
        assert!(
            ready
                .iter()
                .any(|o| matches!(o, Output::Event(SessionEvent::Ready { .. }))),
            "{label} preset session must reach Ready"
        );
        if !expect_shell_disabled {
            // In AutonomousSandboxed (where shell is enabled), a Codex 0.171.0 command approval
            // carrying optional `proposedExecpolicyAmendment` still classifies as `Escalation::None`
            // because `CodexSession` only responds with plain `{"decision":"accept"}`.
            let cmd_apr = preset_session.on_line(
                br#"{"jsonrpc":"2.0","id":"c-auto","method":"item/commandExecution/requestApproval","params":{"itemId":"cmd_auto","threadId":"thr_preset","turnId":"t1","command":"/bin/zsh -lc 'agent-os inbox'","proposedExecpolicyAmendment":["agent-os","inbox"]}}"#,
                now,
            );
            assert!(
                cmd_apr.iter().any(|o| matches!(
                    o,
                    Output::Event(SessionEvent::ApprovalRequested(r))
                        if r.capability == Some(ToolCapability::Shell) && r.escalation == Escalation::None
                )),
                "{label} session must classify ordinary command approval as Shell + None"
            );
        }
        let _ = preset_session.on_line(
            br#"{"jsonrpc":"2.0","method":"item/started","params":{"item":{"id":"fc_p","type":"fileChange","status":"inProgress","changes":[{"path":"/work/t-1/src/lib.rs","kind":"update"}]}}}"#,
            now,
        );
        let patch_apr = preset_session.on_line(
            br#"{"jsonrpc":"2.0","id":"p-preset","method":"item/fileChange/requestApproval","params":{"itemId":"fc_p","threadId":"thr_preset","turnId":"t1"}}"#,
            now,
        );
        assert!(
            patch_apr.iter().any(|o| matches!(
                o,
                Output::Event(SessionEvent::ApprovalRequested(r))
                    if r.capability == Some(ToolCapability::Edit)
                        && r.escalation == Escalation::None
                        && r.input["changes"][0]["path"] == serde_json::json!("/work/t-1/src/lib.rs")
            )),
            "{label} preset session must correlate fileChange targets"
        );
    }

    // Empty `spec.tools` disables shell tool and narrows workspace to read-only in permissions.office.
    let mut no_tools_spec = sample_spec();
    no_tools_spec.tools = Some(vec![]);
    let (no_tools_plan, _) = runtime.plan_session(&no_tools_spec, &sample_ctx()).unwrap();
    assert!(no_tools_plan
        .args
        .contains(&"features.shell_tool=false".to_owned()));
    assert!(no_tools_plan.args.iter().any(|a| {
        a.starts_with("permissions.office.filesystem=") && a.contains("\"/work/t-1\"=\"read\"")
    }));

    // `cap:shell` Any or CommandPrefix deny disables shell tool pre-execution even when broad shell is allowed.
    for deny_scope in [
        RuleScope::Any,
        RuleScope::CommandPrefix(CommandPrefix::try_from("rm -rf").unwrap()),
    ] {
        let mut deny_shell_spec = sample_spec();
        deny_shell_spec.policy.allow.push(Rule {
            tool: ToolCapability::Shell.to_rule_tool(),
            scope: RuleScope::Any,
        });
        deny_shell_spec.policy.deny.push(Rule {
            tool: ToolCapability::Shell.to_rule_tool(),
            scope: deny_scope,
        });
        let (deny_shell_plan, _) = runtime
            .plan_session(&deny_shell_spec, &sample_ctx())
            .unwrap();
        assert!(deny_shell_plan
            .args
            .contains(&"features.shell_tool=false".to_owned()));
    }

    // Version-matched Codex wire protocol tests (0.169.0+):
    let (_, mut session) = runtime.plan_session(&sample_spec(), &sample_ctx()).unwrap();
    let _ = session.start(now);
    let _ = session.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"version":"0.169.0"}}}"#,
        now,
    );
    let _ = session.on_line(&thread_ok_reply("thr_cap"), now);

    // Schema-accurate Codex 0.169.0+ string-serialized shell command approval classifies as
    // `ToolCapability::Shell` and `Escalation::None` (sandbox overrides banned at handshake).
    let cmd_out = session.on_line(
        br#"{"jsonrpc":"2.0","id":"c1","method":"item/commandExecution/requestApproval","params":{"itemId":"cmd_1","threadId":"thr_cap","turnId":"t1","command":"/bin/zsh -lc 'agent-os inbox'"}}"#,
        now,
    );
    let cmd_req = cmd_out
        .iter()
        .find_map(|o| match o {
            Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
            _ => None,
        })
        .unwrap();
    assert_eq!(cmd_req.capability, Some(ToolCapability::Shell));
    assert_eq!(cmd_req.escalation, Escalation::None);

    // Network approval -> SandboxBypass; `kind: "humanCommand"` -> PermissionChange;
    // optional `proposedExecpolicyAmendment` alone stays `Escalation::None` because
    // `CodexSession` never replies with `acceptWithExecpolicyAmendment`.
    let net_out = session.on_line(
        br#"{"jsonrpc":"2.0","id":"c-net","method":"item/commandExecution/requestApproval","params":{"itemId":"cmd_n","command":"curl https://example.com","networkApprovalContext":{"host":"example.com","protocol":"https"}}}"#,
        now,
    );
    assert!(net_out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ApprovalRequested(r)) if r.escalation == Escalation::SandboxBypass
    )));
    let amend_out = session.on_line(
        br#"{"jsonrpc":"2.0","id":"c-am","method":"item/commandExecution/requestApproval","params":{"itemId":"cmd_a","command":"/bin/zsh -lc 'agent-os inbox'","proposedExecpolicyAmendment":["agent-os","inbox"]}}"#,
        now,
    );
    assert!(amend_out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ApprovalRequested(r)) if r.escalation == Escalation::None
    )));
    let human_cmd_out = session.on_line(
        br#"{"jsonrpc":"2.0","id":"c-hc","method":"item/commandExecution/requestApproval","params":{"itemId":"cmd_hc","kind":"humanCommand","command":"/bin/zsh -lc 'agent-os inbox'"}}"#,
        now,
    );
    assert!(human_cmd_out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ApprovalRequested(r)) if r.escalation == Escalation::PermissionChange
    )));

    // Conflicting bypass aliases must NOT mask `unsandboxed: true`.
    let mask_out = session.on_line(
        br#"{"jsonrpc":"2.0","id":"c2","method":"item/commandExecution/requestApproval","params":{"itemId":"cmd_2","command":"/bin/zsh -lc 'agent-os inbox'","withEscalatedPermissions":false,"unsandboxed":true}}"#,
        now,
    );
    let mask_req = mask_out
        .iter()
        .find_map(|o| match o {
            Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
            _ => None,
        })
        .unwrap();
    assert_eq!(mask_req.escalation, Escalation::SandboxBypass);

    // FileChange targets sent in `item/started` are correlated onto `item/fileChange/requestApproval`
    // even when the approval payload carries only `itemId`.
    let _ = session.on_line(
        br#"{"jsonrpc":"2.0","method":"item/started","params":{"item":{"id":"patch_1","type":"fileChange","status":"inProgress","changes":[{"path":"/office/durable/state/org.sqlite","kind":"update"}]}}}"#,
        now,
    );
    let patch_out = session.on_line(
        br#"{"jsonrpc":"2.0","id":"p1","method":"item/fileChange/requestApproval","params":{"itemId":"patch_1","threadId":"thr_cap","turnId":"t1","startedAtMs":1700000000000}}"#,
        now,
    );
    let patch_req = patch_out
        .iter()
        .find_map(|o| match o {
            Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
            _ => None,
        })
        .unwrap();
    assert_eq!(patch_req.capability, Some(ToolCapability::Edit));
    assert_eq!(
        patch_req.input["changes"][0]["path"],
        serde_json::json!("/office/durable/state/org.sqlite")
    );

    // Legacy `applyPatchApproval` normalizes `fileChanges` map onto `changes` before policy evaluation.
    let legacy_patch_out = session.on_line(
        br#"{"jsonrpc":"2.0","id":"p-legacy","method":"applyPatchApproval","params":{"callId":"patch_legacy","conversationId":"thr_cap","fileChanges":{"/office/durable/state/org.sqlite":{"type":"update"}}}}"#,
        now,
    );
    let legacy_patch_req = legacy_patch_out
        .iter()
        .find_map(|o| match o {
            Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
            _ => None,
        })
        .unwrap();
    assert_eq!(legacy_patch_req.capability, Some(ToolCapability::Edit));
    assert!(
        legacy_patch_req.input["changes"]["/office/durable/state/org.sqlite"].is_object(),
        "legacy applyPatchApproval must normalize fileChanges onto changes: {}",
        legacy_patch_req.input
    );

    // `grantRoot` widens session permissions -> Escalation::PermissionChange.
    let grant_out = session.on_line(
        br#"{"jsonrpc":"2.0","id":"p2","method":"item/fileChange/requestApproval","params":{"itemId":"patch_2","threadId":"thr_cap","turnId":"t1","grantRoot":"/etc"}}"#,
        now,
    );
    let grant_req = grant_out
        .iter()
        .find_map(|o| match o {
            Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
            _ => None,
        })
        .unwrap();
    assert_eq!(grant_req.escalation, Escalation::PermissionChange);

    // Collab sub-agent lifecycle tracks child thread identity and terminal child state:
    // 1) `spawnAgent` completion registers child `thr_child_1` while still running (NO ToolFinished yet).
    let spawn_done = session.on_line(
        br#"{"jsonrpc":"2.0","method":"item/completed","params":{"item":{"id":"call_spawn_1","type":"collabAgentToolCall","tool":"spawnAgent","status":"completed","senderThreadId":"thr_cap","receiverThreadIds":["thr_child_1"],"agentsStates":{"thr_child_1":{"status":"running"}}}}}"#,
        now,
    );
    assert!(spawn_done.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ToolStarted {
            tool_use_id,
            capability: Some(ToolCapability::Subagent),
            ..
        }) if tool_use_id == "thr_child_1"
    )));
    assert!(
        !spawn_done
            .iter()
            .any(|o| matches!(o, Output::Event(SessionEvent::ToolFinished { .. }))),
        "running child must NOT emit ToolFinished on spawnAgent completion: {spawn_done:?}"
    );

    // 2) `sendInput` does NOT emit ToolCapability::Subagent (must not double-count child).
    let send_start = session.on_line(
        br#"{"jsonrpc":"2.0","method":"item/started","params":{"item":{"id":"call_send_1","type":"collabAgentToolCall","tool":"sendInput","status":"inProgress","senderThreadId":"thr_cap","receiverThreadIds":["thr_child_1"],"agentsStates":{"thr_child_1":{"status":"running"}}}}}"#,
        now,
    );
    assert!(!send_start.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ToolStarted {
            capability: Some(ToolCapability::Subagent),
            ..
        })
    )));

    // 3) `wait` completion with terminal `agentsStates["thr_child_1"].status == "completed"`
    // emits ToolFinished for child `thr_child_1`.
    let wait_done = session.on_line(
        br#"{"jsonrpc":"2.0","method":"item/completed","params":{"item":{"id":"call_wait_1","type":"collabAgentToolCall","tool":"wait","status":"completed","senderThreadId":"thr_cap","receiverThreadIds":["thr_child_1"],"agentsStates":{"thr_child_1":{"status":"completed"}}}}}"#,
        now,
    );
    assert!(wait_done.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ToolFinished {
            tool_use_id,
            is_error: false,
        }) if tool_use_id == "thr_child_1"
    )));
    // Subsequent duplicate `subAgentActivity` completed for `thr_child_1` is deduplicated.
    let dup_activity = session.on_line(
        br#"{"jsonrpc":"2.0","method":"item/completed","params":{"item":{"id":"sa_dup","type":"subAgentActivity","agentPath":"worker","agentThreadId":"thr_child_1","kind":"completed"}}}"#,
        now,
    );
    assert!(dup_activity.is_empty());

    // 4) Standalone `subAgentActivity` lifecycle completes a child without any parent `wait` call.
    let sa_start = session.on_line(
        br#"{"jsonrpc":"2.0","method":"item/started","params":{"item":{"id":"sa_1","type":"subAgentActivity","agentPath":"scout","agentThreadId":"thr_child_2","kind":"started"}}}"#,
        now,
    );
    assert!(sa_start.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ToolStarted {
            tool_use_id,
            capability: Some(ToolCapability::Subagent),
            ..
        }) if tool_use_id == "thr_child_2"
    )));
    let sa_done = session.on_line(
        br#"{"jsonrpc":"2.0","method":"item/completed","params":{"item":{"id":"sa_2","type":"subAgentActivity","agentPath":"scout","agentThreadId":"thr_child_2","kind":"completed"}}}"#,
        now,
    );
    assert!(sa_done.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ToolFinished {
            tool_use_id,
            is_error: false,
        }) if tool_use_id == "thr_child_2"
    )));

    // Unknown cap:* tokens fail closed in tools and deny rules.
    let mut bad_tools = sample_spec();
    bad_tools.tools = Some(vec![ToolName::try_from("cap:network").unwrap()]);
    assert!(matches!(
        runtime.plan_session(&bad_tools, &sample_ctx()),
        Err(PlanError::Unsupported(_))
    ));
    let mut bad_deny = sample_spec();
    bad_deny.policy.deny.push(Rule {
        tool: RuleTool::try_from("cap:network").unwrap(),
        scope: RuleScope::Any,
    });
    assert!(matches!(
        runtime.plan_session(&bad_deny, &sample_ctx()),
        Err(PlanError::Unsupported(_))
    ));
}

#[test]
fn task115_codex_native_steer_gates_active_turn_id_and_tracks_rpc_ack_or_error() {
    let runtime = CodexRuntime::new();
    let (_, mut session) = runtime.plan_session(&sample_spec(), &sample_ctx()).unwrap();
    let now = Instant::now();

    let _ = session.start(now);
    let _ = session.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"codex","version":"0.172.0"}}}"#,
        now,
    );
    let _ = session.on_line(&thread_ok_reply("thr_steer_1"), now);

    // Idle DeliverMidTurn -> InvalidState.
    assert_eq!(
        session.control(
            Control::DeliverMidTurn {
                delivery_id: "d-idle".into(),
                text: "msg".into(),
                images: Vec::new(),
            },
            now
        ),
        Err(ControlError::InvalidState)
    );

    // Start turn; BEFORE `turn/started` arrives, active_turn_id is None -> NotReady (no write!).
    let _ = session
        .control(
            Control::Send {
                text: "go".into(),
                images: Vec::new(),
            },
            now,
        )
        .unwrap();
    assert_eq!(
        session.control(
            Control::DeliverMidTurn {
                delivery_id: "d-early".into(),
                text: "too early".into(),
                images: Vec::new(),
            },
            now
        ),
        Err(ControlError::NotReady)
    );

    // After `turn/started`, DeliverMidTurn writes `turn/steer` with `expectedTurnId` and tracks req id.
    let _ = session.on_line(
        br#"{"jsonrpc":"2.0","method":"turn/started","params":{"turn":{"id":"turn_42"}}}"#,
        now,
    );
    let out = session
        .control(
            Control::DeliverMidTurn {
                delivery_id: "d-ok".into(),
                text: "peer steer".into(),
                images: Vec::new(),
            },
            now,
        )
        .unwrap();
    let Output::Write(line) = &out[0] else {
        panic!("expected turn/steer write")
    };
    let req: serde_json::Value = serde_json::from_slice(line.as_bytes()).unwrap();
    assert_eq!(req["method"], "turn/steer");
    assert_eq!(req["params"]["expectedTurnId"], "turn_42");
    let rpc_id = req["id"].as_u64().unwrap();

    // RPC success -> MidTurnDelivered.
    let ack = session.on_line(
        format!(r#"{{"jsonrpc":"2.0","id":{rpc_id},"result":{{"turnId":"turn_42"}}}}"#).as_bytes(),
        now,
    );
    assert_eq!(
        ack,
        vec![Output::Event(SessionEvent::MidTurnDelivered {
            delivery_id: "d-ok".into(),
        })]
    );

    // Second steer RPC error -> MidTurnDeliveryFailed.
    let out2 = session
        .control(
            Control::DeliverMidTurn {
                delivery_id: "d-err".into(),
                text: "stale".into(),
                images: Vec::new(),
            },
            now,
        )
        .unwrap();
    let Output::Write(line2) = &out2[0] else {
        panic!("expected turn/steer write")
    };
    let rpc_id2 = serde_json::from_slice::<serde_json::Value>(line2.as_bytes()).unwrap()["id"]
        .as_u64()
        .unwrap();
    let err_out = session.on_line(
        format!(
            r#"{{"jsonrpc":"2.0","id":{rpc_id2},"error":{{"code":-32000,"message":"expectedTurnId mismatch"}}}}"#
        )
        .as_bytes(),
        now,
    );
    assert_eq!(
        err_out,
        vec![Output::Event(SessionEvent::MidTurnDeliveryFailed {
            delivery_id: "d-err".into(),
            reason: "expectedTurnId mismatch".into(),
        })]
    );

    // Third steer still unacked when turn/completed arrives -> MidTurnDeliveryFailed BEFORE TurnEnded.
    let _ = session
        .control(
            Control::DeliverMidTurn {
                delivery_id: "d-race".into(),
                text: "race".into(),
                images: Vec::new(),
            },
            now,
        )
        .unwrap();
    let end = session.on_line(
        br#"{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"turn_42","status":"completed"}}}"#,
        now,
    );
    let fail_idx = end.iter().position(|o| {
        matches!(
            o,
            Output::Event(SessionEvent::MidTurnDeliveryFailed { delivery_id, .. })
                if delivery_id == "d-race"
        )
    });
    let end_idx = end
        .iter()
        .position(|o| matches!(o, Output::Event(SessionEvent::TurnEnded { .. })));
    assert!(matches!((fail_idx, end_idx), (Some(f), Some(e)) if f < e));
}

/// `TASK-181`: the office's five effort levels on Codex. `low`..`xhigh` are Codex's own names;
/// `max` runs as `xhigh`, Codex's highest. The same value goes on the command line and into
/// `thread/start`; one Codex does not know refuses the spec instead of failing at start.
#[test]
fn every_office_effort_level_maps_onto_codex_reasoning_effort() {
    let runtime = CodexRuntime::new();
    let now = Instant::now();
    for (level, codex) in [
        ("low", "low"),
        ("medium", "medium"),
        ("high", "high"),
        ("xhigh", "xhigh"),
        ("max", "xhigh"),
        ("minimal", "minimal"),
    ] {
        let mut spec = sample_spec();
        spec.effort = Some(Token::try_from(level).unwrap());
        let (plan, mut session) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
        let overrides: Vec<&String> = plan
            .args
            .iter()
            .filter(|a| a.starts_with("model_reasoning_effort="))
            .collect();
        assert_eq!(
            overrides,
            [&format!("model_reasoning_effort=\"{codex}\"")],
            "{level}"
        );
        let _ = session.start(now);
        let thread_start = session
            .on_line(
                br#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"version":"0.169.0"}}}"#,
                now,
            )
            .iter()
            .filter_map(|o| match o {
                Output::Write(l) => std::str::from_utf8(l.as_bytes()).ok().map(str::to_owned),
                _ => None,
            })
            .collect::<String>();
        assert!(
            thread_start.contains(&format!(r#""reasoningEffort":"{codex}""#)),
            "{level}: {thread_start}"
        );
    }

    let mut spec = sample_spec();
    spec.effort = None;
    let (plan, _) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    assert!(!plan
        .args
        .iter()
        .any(|a| a.starts_with("model_reasoning_effort=")));

    for unknown in ["turbo", "XHIGH", "ultra"] {
        spec.effort = Some(Token::try_from(unknown).unwrap());
        assert!(
            matches!(
                runtime.plan_session(&spec, &sample_ctx()),
                Err(PlanError::InvalidSpec(_))
            ),
            "{unknown}"
        );
    }
}

#[test]
fn codex_emits_local_image_items_on_turn_start_and_turn_steer_with_read_only_dir() {
    use org_agent_runtime::{ImageInputMode, TurnImage};

    let runtime = CodexRuntime::new();
    assert_eq!(
        runtime.capabilities().image_input,
        ImageInputMode::LocalPath
    );

    let mut spec = sample_spec();
    let att_dir = SpecPath::try_from("/office/agents/ada/runtime/attachments").unwrap();
    spec.read_only_dirs.push(att_dir.clone());
    let (plan, mut session) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    assert!(
        plan.args
            .iter()
            .any(|arg| arg.contains(att_dir.as_str()) && arg.contains("read")),
        "Codex permissions profile must mount read_only_dirs as read: {:?}",
        plan.args
    );

    let now = Instant::now();
    let _ = session.start(now);
    let _ = session.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"codex","version":"0.172.0"}}}"#,
        now,
    );
    let _ = session.on_line(&thread_ok_reply("thr_img_1"), now);

    let img_path = SpecPath::try_from("/office/agents/ada/runtime/attachments/t1/a1.png").unwrap();
    let out = session
        .control(
            Control::Send {
                text: "Check image".into(),
                images: vec![TurnImage::local_path("image/png", img_path.clone())],
            },
            now,
        )
        .expect("Codex turn/start accepts localImage");
    let Output::Write(line) = &out[0] else {
        panic!("expected turn/start write")
    };
    let req: serde_json::Value = serde_json::from_slice(line.as_bytes()).unwrap();
    assert_eq!(req["method"], "turn/start");
    assert_eq!(
        req["params"]["input"],
        serde_json::json!([
            { "type": "text", "text": "Check image" },
            { "type": "localImage", "path": img_path.as_str() }
        ])
    );
    assert!(
        !String::from_utf8_lossy(line.as_bytes()).contains("base64"),
        "Codex must never emit base64 in turn/start"
    );

    // Emit turn/started so turn/steer is accepted with localImage too.
    let _ = session.on_line(
        br#"{"jsonrpc":"2.0","method":"turn/started","params":{"turn":{"id":"turn_img_1"}}}"#,
        now,
    );
    let steer_path =
        SpecPath::try_from("/office/agents/ada/runtime/attachments/mt1/a2.jpg").unwrap();
    let steer_out = session
        .control(
            Control::DeliverMidTurn {
                delivery_id: "steer-img-1".into(),
                text: "Mid-turn image".into(),
                images: vec![TurnImage::local_path("image/jpeg", steer_path.clone())],
            },
            now,
        )
        .expect("Codex turn/steer accepts localImage");
    let Output::Write(steer_line) = &steer_out[0] else {
        panic!("expected turn/steer write")
    };
    let steer_req: serde_json::Value = serde_json::from_slice(steer_line.as_bytes()).unwrap();
    assert_eq!(steer_req["method"], "turn/steer");
    assert_eq!(
        steer_req["params"]["input"],
        serde_json::json!([
            { "type": "text", "text": "Mid-turn image" },
            { "type": "localImage", "path": steer_path.as_str() }
        ])
    );

    // InlineBase64 on turn/steer is refused cleanly without mutating state.
    assert_eq!(
        session.control(
            Control::DeliverMidTurn {
                delivery_id: "steer-bad".into(),
                text: "bad".into(),
                images: vec![TurnImage::inline_bytes("image/png", b"raw")],
            },
            now
        ),
        Err(ControlError::Unsupported)
    );
}
