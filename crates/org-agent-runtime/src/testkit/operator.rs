//! A fixture operator session (`OperatorSandboxed`, `TASK-80.1`) and the malformed variants
//! every adapter must refuse, so each adapter's conformance tests check the same specs.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::{
    AgentIdentity, AgentName, CommandPrefix, EnvName, LaunchContext, McpServer, McpServerName,
    PathPattern, PermissionMode, PermissionPolicy, Rule, RuleScope, RuleTool, RuntimeId,
    SandboxSpec, SessionId, SessionKind, SessionSpec, SpecPath, ToolCapability,
};

/// The operator's private home: the session's repo-less workspace.
pub const OPERATOR_HOME: &str = "/office/agents/tess/home";
/// The office socket, the only one the operator may reach.
pub const OPERATOR_SOCKET: &str = "/office/run/office.sock";
/// The office folders every agent is denied.
pub const OPERATOR_DENIED: &str = "/office/offices/*/**";

fn sp(path: &str) -> SpecPath {
    SpecPath::try_from(path).expect("fixture path")
}

fn cap_rule(cap: ToolCapability, scope: RuleScope) -> Rule {
    Rule {
        tool: cap.to_rule_tool(),
        scope,
    }
}

/// The `agent-os` prefix an operator's shell is confined to.
pub fn agent_os_prefix() -> CommandPrefix {
    CommandPrefix::try_from("agent-os").expect("static prefix")
}

/// The built-in Chief of Staff's home session on `runtime`, as the office plans it: plan
/// mode, read and search anywhere, the shell confined to `agent-os`, the office's folders
/// denied, the sandbox on with no writable root, only the office socket, no MCP server.
pub fn operator_spec(runtime: &str) -> SessionSpec {
    let denied = PathPattern::try_from(OPERATOR_DENIED).expect("fixture pattern");
    SessionSpec {
        runtime: RuntimeId::try_from(runtime).expect("runtime id"),
        kind: SessionKind::Home {},
        agent: AgentIdentity {
            name: AgentName::try_from("tess").expect("agent name"),
            description: "Tess (Chief of Staff)".to_owned(),
            prompt: "You are Tess (@tess).".to_owned(),
        },
        model: None,
        effort: None,
        workspace: sp(OPERATOR_HOME),
        read_only_dirs: Vec::new(),
        instructions: "Standing instructions.".to_owned(),
        repository_instructions: String::new(),
        permission_mode: PermissionMode::Plan,
        policy: PermissionPolicy {
            version: 1,
            allow: vec![
                cap_rule(ToolCapability::Read, RuleScope::Any),
                cap_rule(ToolCapability::Search, RuleScope::Any),
                cap_rule(
                    ToolCapability::Shell,
                    RuleScope::CommandPrefix(agent_os_prefix()),
                ),
            ],
            deny: vec![Rule {
                tool: RuleTool::try_from("*").expect("wildcard"),
                scope: RuleScope::Path(denied.clone()),
            }],
        },
        sandbox: SandboxSpec {
            enabled: true,
            writable_roots: Vec::new(),
            deny_write: vec![denied.clone()],
            deny_read: vec![denied],
            allowed_unix_sockets: vec![sp(OPERATOR_SOCKET)],
        },
        mcp_servers: BTreeMap::new(),
        tools: Some(vec![
            ToolCapability::Read.to_tool_name(),
            ToolCapability::Search.to_tool_name(),
            ToolCapability::Shell.to_tool_name(),
        ]),
        operator_shell_prefixes: vec![agent_os_prefix()],
    }
}

/// A launch context for [`operator_spec`], with the runtime at `runtime_path`.
pub fn operator_ctx(runtime_path: &str) -> LaunchContext {
    LaunchContext {
        runtime_path: PathBuf::from(runtime_path),
        runtime_version: None,
        runtime_dir: sp("/office/agents/tess/runtime"),
        tmpdir: sp("/private/tmp/agentic-org/tess-run"),
        office_env_names: vec![EnvName::try_from("AGENT_OS_SESSION_TOKEN").expect("env")],
        new_session_id: Some(
            SessionId::try_from("0193a5c0-0000-4000-8000-000000000080").expect("session id"),
        ),
        resume: None,
        config_dir: None,
    }
}

/// [`operator_spec`] broken one way at a time: every adapter refuses each.
pub fn malformed_operator_specs(runtime: &str) -> Vec<(&'static str, SessionSpec)> {
    let base = || operator_spec(runtime);
    let with = |what: &'static str, change: fn(&mut SessionSpec)| {
        let mut spec = base();
        change(&mut spec);
        (what, spec)
    };
    vec![
        with("sandbox off", |s| s.sandbox.enabled = false),
        with("a writable root", |s| {
            s.sandbox.writable_roots = vec![sp(OPERATOR_HOME)];
        }),
        with("an MCP server", |s| {
            s.mcp_servers.insert(
                McpServerName::try_from("office").expect("name"),
                McpServer {
                    command: sp("/office/bin/org-mcp"),
                    args: Vec::new(),
                },
            );
        }),
        with("accept edits", |s| {
            s.permission_mode = PermissionMode::AcceptEdits
        }),
        with("an edit allow", |s| {
            s.policy
                .allow
                .push(cap_rule(ToolCapability::Edit, RuleScope::Any));
        }),
        with("an unscoped shell", |s| {
            s.policy
                .allow
                .push(cap_rule(ToolCapability::Shell, RuleScope::Any));
        }),
        with("a wildcard allow", |s| {
            s.policy.allow.push(Rule {
                tool: RuleTool::try_from("*").expect("wildcard"),
                scope: RuleScope::Any,
            });
        }),
        with("another shell prefix", |s| {
            let git = CommandPrefix::try_from("git").expect("prefix");
            s.policy.allow.push(cap_rule(
                ToolCapability::Shell,
                RuleScope::CommandPrefix(git),
            ));
        }),
        with("every tool", |s| s.tools = None),
        with("no shell tool", |s| {
            s.tools = Some(vec![ToolCapability::Read.to_tool_name()]);
        }),
        with("an edit tool", |s| {
            if let Some(tools) = s.tools.as_mut() {
                tools.push(ToolCapability::Edit.to_tool_name());
            }
        }),
    ]
}
