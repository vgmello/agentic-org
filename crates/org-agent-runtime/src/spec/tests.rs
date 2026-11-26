use std::collections::BTreeMap;

use serde_json::Value;

use super::fnv1a_64;
use crate::{
    AgentIdentity, AgentName, Capabilities, CommandPrefix, McpServer, McpServerName, PathPattern,
    PermissionMode, PermissionPolicy, PlainId, Rule, RuleScope, RuleTool, RuntimeId, SandboxSpec,
    SessionKind, SessionSpec, SpecPath, Token, ToolCapability, ToolName,
};

fn p(path: &str) -> SpecPath {
    SpecPath::try_from(path).unwrap()
}

fn pat(pattern: &str) -> PathPattern {
    PathPattern::try_from(pattern).unwrap()
}

fn tok(token: &str) -> Token {
    Token::try_from(token).unwrap()
}

fn mcp(name: &str) -> McpServerName {
    McpServerName::try_from(name).unwrap()
}

fn sample_spec() -> SessionSpec {
    let mut mcp_servers = BTreeMap::new();
    mcp_servers.insert(
        mcp("office"),
        McpServer {
            command: p("/office/bin/org-mcp"),
            args: vec!["--stdio".to_owned()],
        },
    );
    SessionSpec {
        runtime: RuntimeId::try_from("claude").unwrap(),
        kind: SessionKind::Task {
            task_id: PlainId::try_from("t1").unwrap(),
        },
        agent: AgentIdentity {
            name: AgentName::try_from("ac-dev-ada").unwrap(),
            description: "Staff engineer".to_owned(),
            prompt: "You are ada.".to_owned(),
        },
        model: Some(tok("opus")),
        effort: None,
        workspace: p("/work/t-1"),
        read_only_dirs: vec![p("/homes/ada")],
        instructions: "Do the task.\nThen stop.".to_owned(),
        permission_mode: PermissionMode::AcceptEdits,
        policy: PermissionPolicy {
            version: 3,
            allow: vec![Rule {
                tool: RuleTool::try_from("Bash").unwrap(),
                scope: RuleScope::CommandPrefix(CommandPrefix::try_from("git ").unwrap()),
            }],
            deny: vec![Rule {
                tool: RuleTool::try_from("*").unwrap(),
                scope: RuleScope::Path(pat("/office/**")),
            }],
        },
        sandbox: SandboxSpec {
            enabled: true,
            writable_roots: vec![p("/work/t-1")],
            deny_write: vec![pat("/work/t-1/.git/hooks/**")],
            deny_read: vec![pat("/office/offices/*/**")],
            allowed_unix_sockets: vec![p("/office/run/ada.sock")],
        },
        mcp_servers,
        tools: Some(vec![
            ToolName::try_from("Bash").unwrap(),
            ToolName::try_from("Edit").unwrap(),
        ]),
    }
}

#[test]
fn canonical_json_is_compact_with_sorted_keys() {
    let spec = sample_spec();
    let json = String::from_utf8(spec.canonical_json()).unwrap();
    // The exact form is part of the contract (it feeds every stored hash).
    let expected = concat!(
        r#"{"agent":{"description":"Staff engineer","name":"ac-dev-ada","prompt":"You are ada."},"#,
        r#""effort":null,"instructions":"Do the task.\nThen stop.","kind":{"task_id":"t1","type":"task"},"#,
        r#""mcp_servers":{"office":{"args":["--stdio"],"command":"/office/bin/org-mcp"}},"#,
        r#""model":"opus","permission_mode":"accept_edits","#,
        r#""policy":{"allow":[{"scope":{"type":"command_prefix","value":"git "},"tool":"Bash"}],"#,
        r#""deny":[{"scope":{"type":"path","value":"/office/**"},"tool":"*"}],"version":3},"#,
        r#""read_only_dirs":["/homes/ada"],"runtime":"claude","#,
        r#""sandbox":{"allowed_unix_sockets":["/office/run/ada.sock"],"#,
        r#""deny_read":["/office/offices/*/**"],"deny_write":["/work/t-1/.git/hooks/**"],"#,
        r#""enabled":true,"writable_roots":["/work/t-1"]},"tools":["Bash","Edit"],"#,
        r#""workspace":"/work/t-1"}"#
    );
    assert_eq!(json, expected);
    // The canonical form is itself valid JSON that decodes to the same spec.
    let back: SessionSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back, spec);
}

#[test]
fn spec_hash_is_pinned() {
    // Pins the algorithm and the canonical form: if this changes, every stored hash
    // changes, which is a contract change.
    let spec = sample_spec();
    // The sample's hash, computed independently (FNV-1a 64 over the golden canonical JSON
    // in `canonical_json_is_compact_with_sorted_keys`).
    assert_eq!(spec.spec_hash(), "4ca16808942e59dc");
    assert_eq!(
        spec.spec_hash(),
        format!("{:016x}", fnv1a_64(&spec.canonical_json()))
    );
    assert_eq!(spec.spec_hash().len(), 16);
    assert!(spec
        .spec_hash()
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    // FNV-1a 64 reference vectors.
    assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fnv1a_64(b"a"), 0xaf63_dc4c_8601_ec8c);
    assert_eq!(fnv1a_64(b"foobar"), 0x8594_4171_f739_67e8);
}

#[test]
fn map_insertion_order_does_not_matter() {
    let mut a = sample_spec();
    let mut b = sample_spec();
    let server = McpServer {
        command: p("/x"),
        args: vec![],
    };
    a.mcp_servers.clear();
    b.mcp_servers.clear();
    for name in ["zeta", "alpha", "mid"] {
        a.mcp_servers.insert(mcp(name), server.clone());
    }
    for name in ["mid", "zeta", "alpha"] {
        b.mcp_servers.insert(mcp(name), server.clone());
    }
    assert_eq!(a.canonical_json(), b.canonical_json());
    assert_eq!(a.spec_hash(), b.spec_hash());
}

#[test]
fn canonical_form_does_not_depend_on_the_input_key_order() {
    // A spec decoded from JSON with shuffled keys and whitespace hashes the same.
    let spec = sample_spec();
    let mut value = serde_json::to_value(&spec).unwrap();
    let shuffled: serde_json::Map<String, Value> = value
        .as_object_mut()
        .unwrap()
        .iter()
        .rev()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let pretty = serde_json::to_string_pretty(&Value::Object(shuffled)).unwrap();
    let back: SessionSpec = serde_json::from_str(&pretty).unwrap();
    assert_eq!(back.spec_hash(), spec.spec_hash());
    assert_eq!(back.canonical_json(), spec.canonical_json());
}

#[test]
fn spec_hash_is_stable_across_clone_and_serde_round_trip() {
    let spec = sample_spec();
    let hash = spec.spec_hash();
    assert_eq!(spec.clone().spec_hash(), hash);
    let stored = serde_json::to_string(&spec).unwrap();
    let back: SessionSpec = serde_json::from_str(&stored).unwrap();
    assert_eq!(back, spec);
    assert_eq!(back.spec_hash(), hash);
    // Twice, to be sure it isn't something the first decode normalized.
    let again: SessionSpec = serde_json::from_str(&serde_json::to_string(&back).unwrap()).unwrap();
    assert_eq!(again.spec_hash(), hash);
}

#[test]
fn spec_hash_changes_when_any_field_changes() {
    let base = sample_spec();
    let base_hash = base.spec_hash();
    // One mutation per field, listed by an exhaustive destructuring so a new field
    // can't be added without a case here.
    let SessionSpec {
        runtime: _,
        kind: _,
        agent: _,
        model: _,
        effort: _,
        workspace: _,
        read_only_dirs: _,
        instructions: _,
        permission_mode: _,
        policy: _,
        sandbox: _,
        mcp_servers: _,
        tools: _,
    } = &base;
    type Mutation = (&'static str, fn(&mut SessionSpec));
    let mutations: Vec<Mutation> = vec![
        ("runtime", |s| {
            s.runtime = RuntimeId::try_from("codex").unwrap()
        }),
        ("kind", |s| s.kind = SessionKind::Home {}),
        ("kind.task_id", |s| {
            s.kind = SessionKind::Task {
                task_id: PlainId::try_from("t2").unwrap(),
            }
        }),
        ("kind one-off", |s| {
            s.kind = SessionKind::OneOff {
                run_id: PlainId::try_from("t1").unwrap(),
            }
        }),
        ("agent.name", |s| {
            s.agent.name = AgentName::try_from("ac-dev-ada2").unwrap()
        }),
        ("agent.description", |s| s.agent.description.push('x')),
        ("agent.prompt", |s| s.agent.prompt.push('x')),
        ("model", |s| s.model = None),
        ("effort", |s| s.effort = Some(tok("high"))),
        ("workspace", |s| s.workspace = p("/work/t-2")),
        ("read_only_dirs", |s| s.read_only_dirs.clear()),
        ("instructions", |s| s.instructions.push('!')),
        ("permission_mode", |s| {
            s.permission_mode = PermissionMode::Plan
        }),
        ("policy.version", |s| s.policy.version += 1),
        ("policy.allow", |s| s.policy.allow.clear()),
        ("policy.deny", |s| s.policy.deny.clear()),
        ("policy rule scope", |s| {
            s.policy.allow[0].scope = RuleScope::Path(pat("/git"))
        }),
        ("sandbox.enabled", |s| s.sandbox.enabled = false),
        ("sandbox.writable_roots", |s| {
            s.sandbox.writable_roots.push(p("/tmp/x"))
        }),
        ("sandbox.deny_write", |s| s.sandbox.deny_write.clear()),
        ("sandbox.deny_read", |s| s.sandbox.deny_read.clear()),
        ("sandbox.allowed_unix_sockets", |s| {
            s.sandbox.allowed_unix_sockets.clear()
        }),
        ("mcp_servers", |s| s.mcp_servers.clear()),
        ("mcp_servers.args", |s| {
            s.mcp_servers.get_mut(&mcp("office")).unwrap().args.clear()
        }),
        ("tools", |s| s.tools = None),
        ("tools list", |s| {
            s.tools = Some(vec![ToolName::try_from("Bash").unwrap()])
        }),
        ("list order", |s| {
            s.read_only_dirs.push(p("/a"));
            s.read_only_dirs.reverse();
        }),
    ];
    let mut seen = std::collections::BTreeSet::new();
    seen.insert(base_hash.clone());
    for (name, mutate) in mutations {
        let mut spec = base.clone();
        mutate(&mut spec);
        assert_ne!(spec, base, "{name}: the mutation changed nothing");
        let hash = spec.spec_hash();
        assert_ne!(hash, base_hash, "{name}: hash didn't change");
        assert!(seen.insert(hash), "{name}: hash collides with another case");
    }
}

fn caps(deny_rules: bool, deny_write: bool, deny_read: bool, tools: bool) -> Capabilities {
    Capabilities {
        mid_turn_delivery: crate::MidTurnDelivery::None,
        interrupt_in_band: true,
        idle_interrupt_ends_turn: false,
        approval_bridge: true,
        deny_message_reaches_model: true,
        runtime_cancels_approvals: false,
        unsolicited_turns: false,
        session_id_known_up_front: true,
        resume_ignores_cwd: false,
        resume_applies_instructions: true,
        reads_workspace_instruction_files: false,
        tools_detach_from_group: false,
        take_over: true,
        enforces_writable_roots: true,
        enforces_deny_write: deny_write,
        enforces_deny_read: deny_read,
        enforces_unix_socket_allowlist: true,
        enforces_tool_allowlist: tools,
        enforces_deny_rules: deny_rules,
        usage_scope: crate::UsageScope::TurnDelta,
        cost: crate::CostBasis::None,
        tested_versions: crate::VersionRange {
            min: "1".into(),
            max: "2".into(),
        },
    }
}

/// The sample with every deny-like restriction empty or off.
fn no_denies() -> SessionSpec {
    let mut spec = sample_spec();
    spec.policy.deny.clear();
    spec.sandbox.deny_write.clear();
    spec.sandbox.deny_read.clear();
    spec.tools = None;
    spec
}

#[test]
fn deny_wins_over_allow_for_each_restriction_on_its_own() {
    let all = caps(true, true, true, true);
    let empty = no_denies();
    assert!(empty.keeps_allow_rules(&all));
    // Nothing to drop: no capability matters, sandbox on or off.
    assert!(empty.keeps_allow_rules(&caps(false, false, false, false)));
    let mut off = empty.clone();
    off.sandbox.enabled = false;
    assert!(off.keeps_allow_rules(&caps(false, false, false, false)));

    type Add = fn(&mut SessionSpec);
    // (what, how to make only it non-empty, the caps that drop only it, whether turning
    // the sandbox off drops it)
    let cases: [(&str, Add, Capabilities, bool); 4] = [
        (
            "deny rules",
            |s| s.policy.deny = sample_spec().policy.deny,
            caps(false, true, true, true),
            false,
        ),
        (
            "deny_write",
            |s| s.sandbox.deny_write = sample_spec().sandbox.deny_write,
            caps(true, false, true, true),
            true,
        ),
        (
            "deny_read",
            |s| s.sandbox.deny_read = sample_spec().sandbox.deny_read,
            caps(true, true, false, true),
            true,
        ),
        (
            "tools",
            |s| s.tools = Some(vec![]),
            caps(true, true, true, false),
            false,
        ),
    ];
    for (what, add, drops_it, sandbox_off_drops_it) in cases.clone() {
        let mut spec = no_denies();
        add(&mut spec);
        assert!(spec.keeps_allow_rules(&all), "{what}: enforced");
        assert!(!spec.keeps_allow_rules(&drops_it), "{what}: dropped");
        assert_eq!(
            spec.effective_permission_mode(&drops_it),
            PermissionMode::Ask,
            "{what}: accept_edits must narrow"
        );
        assert_eq!(
            spec.effective_permission_mode(&all),
            PermissionMode::AcceptEdits
        );
        // Dropping only the others changes nothing.
        for (other, _, other_caps, _) in &cases {
            if *other != what {
                assert!(spec.keeps_allow_rules(other_caps), "{what} vs {other}");
            }
        }
        let mut off = spec.clone();
        off.sandbox.enabled = false;
        assert_eq!(
            !off.keeps_allow_rules(&all),
            sandbox_off_drops_it,
            "{what}: sandbox off"
        );
    }
}

#[test]
fn effective_permission_mode_only_narrows() {
    let dropped = caps(false, true, true, true);
    let mut spec = sample_spec();
    for mode in [
        PermissionMode::Ask,
        PermissionMode::AcceptEdits,
        PermissionMode::Plan,
    ] {
        spec.permission_mode = mode;
        let expected = if mode == PermissionMode::AcceptEdits {
            PermissionMode::Ask
        } else {
            mode
        };
        assert_eq!(spec.effective_permission_mode(&dropped), expected);
        assert_eq!(
            spec.effective_permission_mode(&caps(true, true, true, true)),
            mode
        );
    }
}

#[test]
fn permission_mode_has_no_bypass() {
    // Compile-level: an exhaustive match inside the crate. Adding a variant (a bypass
    // one included) breaks this test, so it can't happen without a deliberate edit here.
    fn runtime_checks_permissions(mode: PermissionMode) -> bool {
        match mode {
            PermissionMode::Ask | PermissionMode::AcceptEdits | PermissionMode::Plan => true,
        }
    }
    for mode in [
        PermissionMode::Ask,
        PermissionMode::AcceptEdits,
        PermissionMode::Plan,
    ] {
        assert!(runtime_checks_permissions(mode));
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(serde_json::from_str::<PermissionMode>(&json).unwrap(), mode);
    }
    assert_eq!(
        serde_json::to_string(&PermissionMode::AcceptEdits).unwrap(),
        r#""accept_edits""#
    );
    // Serde refuses every spelling of a bypass request, and anything unknown.
    for bad in [
        r#""bypassPermissions""#,
        r#""bypass_permissions""#,
        r#""bypass""#,
        r#""dangerously_skip_permissions""#,
        r#""danger-full-access""#,
        r#""unknown""#,
        r#""""#,
    ] {
        assert!(
            serde_json::from_str::<PermissionMode>(bad).is_err(),
            "{bad} decoded"
        );
    }
    // And a stored spec carrying one doesn't decode at all.
    let mut value = serde_json::to_value(sample_spec()).unwrap();
    value["permission_mode"] = Value::String("bypassPermissions".to_owned());
    assert!(serde_json::from_value::<SessionSpec>(value).is_err());
}

#[test]
fn unknown_spec_fields_fail_to_decode_at_every_level() {
    let base = serde_json::to_value(sample_spec()).unwrap();
    assert!(serde_json::from_value::<SessionSpec>(base.clone()).is_ok());
    for pointer in [
        "",
        "/kind",
        "/agent",
        "/policy",
        "/policy/allow/0",
        "/policy/allow/0/scope",
        "/sandbox",
        "/mcp_servers/office",
    ] {
        let mut value = base.clone();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("future_restriction".to_owned(), Value::Bool(true));
        assert!(
            serde_json::from_value::<SessionSpec>(value).is_err(),
            "an unknown field at {pointer:?} decoded"
        );
    }
    // Beside a fieldless variant's tag too.
    let mut value = base.clone();
    value["kind"] = serde_json::json!({"type": "home", "future_restriction": true});
    assert!(serde_json::from_value::<SessionSpec>(value).is_err());
    let mut value = base;
    value["kind"] = serde_json::json!({"type": "home"});
    assert_eq!(
        serde_json::from_value::<SessionSpec>(value).unwrap().kind,
        SessionKind::Home {}
    );
}

#[test]
fn v1_option_keys_must_be_present() {
    let base = serde_json::to_value(sample_spec()).unwrap();
    for key in ["tools", "model", "effort"] {
        let mut value = base.clone();
        value.as_object_mut().unwrap().remove(key);
        let err = serde_json::from_value::<SessionSpec>(value).unwrap_err();
        assert!(err.to_string().contains("missing field"), "{key}: {err}");
        // Present as null is fine: that's None.
        let mut value = base.clone();
        value[key] = Value::Null;
        assert!(
            serde_json::from_value::<SessionSpec>(value).is_ok(),
            "{key}"
        );
    }
}

#[test]
fn office_written_enums_fail_closed_on_unknown_values() {
    assert!(serde_json::from_str::<SessionKind>(r#"{"type":"shared"}"#).is_err());
    assert!(serde_json::from_str::<RuleScope>(r#"{"type":"regex","value":".*"}"#).is_err());
}

#[test]
fn spec_parts_round_trip() {
    for kind in [
        SessionKind::Home {},
        SessionKind::Task {
            task_id: PlainId::try_from("t").unwrap(),
        },
        SessionKind::OneOff {
            run_id: PlainId::try_from("r").unwrap(),
        },
    ] {
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(serde_json::from_str::<SessionKind>(&json).unwrap(), kind);
    }
    assert_eq!(
        serde_json::to_string(&SessionKind::Home {}).unwrap(),
        r#"{"type":"home"}"#
    );
    for scope in [
        RuleScope::Any,
        RuleScope::Path(pat("/a/**")),
        RuleScope::CommandPrefix(CommandPrefix::try_from("git ").unwrap()),
        RuleScope::McpServer(mcp("office")),
    ] {
        let json = serde_json::to_string(&scope).unwrap();
        assert_eq!(serde_json::from_str::<RuleScope>(&json).unwrap(), scope);
    }
    assert_eq!(
        serde_json::to_string(&RuleScope::Any).unwrap(),
        r#"{"type":"any"}"#
    );
}

#[test]
fn a_stored_spec_with_a_bad_value_does_not_decode() {
    let base = serde_json::to_value(sample_spec()).unwrap();
    for (pointer, bad) in [
        ("/workspace", "work/t-1"),
        ("/read_only_dirs/0", "/homes/ada/../x"),
        ("/sandbox/writable_roots/0", "/work//t-1"),
        ("/sandbox/allowed_unix_sockets/0", "/office/run/"),
        ("/mcp_servers/office/command", "org-mcp"),
        ("/sandbox/deny_write/0", "~/.claude/**"),
        ("/sandbox/deny_read/0", "/office/offices/*/*.db"),
        ("/workspace", "/"),
        ("/sandbox/writable_roots/0", "/"),
        ("/policy/deny/0/scope/value", "/office/**/x*"),
        ("/policy/allow/0/scope/value", "git (status)"),
        ("/agent/name", "Ada"),
        ("/model", "opus --dangerously-skip-permissions"),
        ("/model", "-x"),
        ("/tools/0", "Bash(git *)"),
        ("/kind/task_id", "t-1"),
        ("/policy/allow/0/tool", "Bash) Read(/etc"),
        ("/policy/deny/0/scope/value", "/work/[draft]"),
        ("/sandbox/deny_read/0", "/work/a?"),
    ] {
        let mut value = base.clone();
        *value.pointer_mut(pointer).unwrap() = Value::from(bad);
        assert!(
            serde_json::from_value::<SessionSpec>(value).is_err(),
            "{pointer} = {bad:?} decoded"
        );
    }
    // Map keys are checked too.
    let mut value = base;
    let server = value["mcp_servers"]["office"].clone();
    value["mcp_servers"]["my server"] = server;
    assert!(serde_json::from_value::<SessionSpec>(value).is_err());
}

#[test]
fn tool_capability_tokens_and_serde_round_trip() {
    for (cap, wire, token) in [
        (ToolCapability::Read, "read", "cap:read"),
        (ToolCapability::Search, "search", "cap:search"),
        (ToolCapability::Edit, "edit", "cap:edit"),
        (ToolCapability::Shell, "shell", "cap:shell"),
        (ToolCapability::Subagent, "subagent", "cap:subagent"),
    ] {
        assert_eq!(cap.as_str(), wire);
        assert_eq!(cap.as_rule_token(), token);
        assert!(ToolCapability::is_capability_token(token));
        assert_eq!(ToolCapability::parse_rule_token(token), Some(cap));
        assert_eq!(cap.to_rule_tool().as_str(), token);
        assert_eq!(cap.to_tool_name().as_str(), token);
        let encoded = serde_json::to_string(&cap).unwrap();
        assert_eq!(encoded, format!("\"{wire}\""));
        assert_eq!(
            serde_json::from_str::<ToolCapability>(&encoded).unwrap(),
            cap
        );
    }
    assert!(ToolCapability::is_capability_token("cap:unknown"));
    assert!(ToolCapability::is_capability_token("cap:network"));
    assert_eq!(ToolCapability::parse_rule_token("cap:unknown"), None);
    assert_eq!(ToolCapability::parse_rule_token("cap:network"), None);
    assert!(!ToolCapability::is_capability_token("Bash"));
    assert_eq!(ToolCapability::parse_rule_token("Bash"), None);
    assert_eq!(
        serde_json::from_str::<ToolCapability>("\"network\"").unwrap(),
        ToolCapability::Unknown
    );

    // Capability tokens fit existing RuleTool/ToolName grammar and round-trip inside SessionSpec.
    let mut spec = sample_spec();
    spec.policy.allow = vec![Rule {
        tool: ToolCapability::Shell.to_rule_tool(),
        scope: RuleScope::CommandPrefix(CommandPrefix::try_from("agent-os").unwrap()),
    }];
    spec.tools = Some(vec![
        ToolCapability::Read.to_tool_name(),
        ToolCapability::Search.to_tool_name(),
        ToolCapability::Shell.to_tool_name(),
    ]);
    let json = serde_json::to_string(&spec).unwrap();
    let back: SessionSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back, spec);
    assert_eq!(back.spec_hash(), spec.spec_hash());
}
