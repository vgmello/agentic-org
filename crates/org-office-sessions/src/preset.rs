//! Permission presets, default-on sandbox, and escalation policy (`ADR-0032` §3, `ENROLL-7`, `FC-189`).

use std::fmt;

use org_agent_runtime::{
    ApprovalRequest, Escalation, PathPattern, PermissionMode, PermissionPolicy, Rule, RuleScope,
    RuleTool, SandboxSpec, SpecPath, ToolName,
};
use org_office::organization_family::records::PermissionPreset;
use org_proto::sessions::{GrantRuleView, SessionGrantsResult};

/// Effective configuration derived from a [`PermissionPreset`] and sandbox toggle.
#[derive(Debug, Clone)]
pub struct PresetConfig {
    pub preset: PermissionPreset,
    pub permission_mode: PermissionMode,
    pub policy: PermissionPolicy,
    pub sandbox: SandboxSpec,
    pub allowed_tools: Option<Vec<ToolName>>,
}

/// Errors building a permission/sandbox configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetError {
    BypassRefused(&'static str),
    InvalidPath(String),
}

impl fmt::Display for PresetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BypassRefused(mode) => {
                write!(
                    f,
                    "bypass permission mode `{mode}` is unconditionally refused"
                )
            }
            Self::InvalidPath(detail) => write!(f, "invalid sandbox path: {detail}"),
        }
    }
}

impl std::error::Error for PresetError {}

/// Parses a wire/API preset name, unconditionally refusing any bypass/unrestricted mode (`ENROLL-7`).
pub fn parse_preset(name: &str) -> Result<PermissionPreset, PresetError> {
    let trimmed = name.trim();
    if matches!(
        trimmed,
        "bypass"
            | "bypassPermissions"
            | "bypass_permissions"
            | "dangerously_skip_permissions"
            | "unrestricted"
    ) {
        return Err(PresetError::BypassRefused("bypassPermissions"));
    }
    PermissionPreset::parse(trimmed).ok_or(PresetError::BypassRefused("unknown permission preset"))
}

/// Builds the runtime's [`PermissionMode`], [`PermissionPolicy`], [`SandboxSpec`], and tool list
/// from `preset`, `sandbox_enabled` (default-on), workspace, office socket, and denied roots.
pub fn build_session_policy(
    preset: PermissionPreset,
    sandbox_enabled: bool,
    workspace: &SpecPath,
    office_socket: Option<&SpecPath>,
    denied_office_globs: &[PathPattern],
    policy_version: u64,
) -> Result<PresetConfig, PresetError> {
    let (permission_mode, allow_rules, allowed_tools) = match preset {
        PermissionPreset::SafeAsk => (PermissionMode::Ask, Vec::new(), None),
        PermissionPreset::ReadOnly => (
            PermissionMode::Plan,
            vec![
                rule("Read", RuleScope::Any),
                rule("Glob", RuleScope::Any),
                rule("Grep", RuleScope::Any),
            ],
            Some(vec![tool("Read"), tool("Glob"), tool("Grep"), tool("Bash")]),
        ),
        PermissionPreset::WorkspaceEdits => (
            PermissionMode::AcceptEdits,
            vec![
                rule("Read", RuleScope::Any),
                rule("Glob", RuleScope::Any),
                rule("Grep", RuleScope::Any),
                rule("Edit", RuleScope::Any),
                rule("Write", RuleScope::Any),
            ],
            None,
        ),
        PermissionPreset::AutonomousSandboxed => (
            PermissionMode::AcceptEdits,
            vec![
                rule("Read", RuleScope::Any),
                rule("Glob", RuleScope::Any),
                rule("Grep", RuleScope::Any),
                rule("Edit", RuleScope::Any),
                rule("Write", RuleScope::Any),
                rule("Bash", RuleScope::Any),
            ],
            None,
        ),
    };

    let mut deny_rules = Vec::new();
    let mut deny_read = Vec::new();
    let mut deny_write = Vec::new();
    for office_glob in denied_office_globs {
        deny_rules.push(Rule {
            tool: RuleTool::try_from("*").expect("wildcard rule tool"),
            scope: RuleScope::Path(office_glob.clone()),
        });
        deny_read.push(office_glob.clone());
        deny_write.push(office_glob.clone());
    }
    let hooks_glob = format!("{}/.git/hooks/**", workspace.as_str());
    if let Ok(git_hooks) = PathPattern::try_from(hooks_glob.as_str()) {
        deny_rules.push(Rule {
            tool: RuleTool::try_from("*").expect("wildcard rule tool"),
            scope: RuleScope::Path(git_hooks.clone()),
        });
        deny_write.push(git_hooks);
    }

    let writable_roots = match preset {
        PermissionPreset::ReadOnly => Vec::new(),
        _ => vec![workspace.clone()],
    };
    let allowed_unix_sockets = office_socket.into_iter().cloned().collect();

    Ok(PresetConfig {
        preset,
        permission_mode,
        policy: PermissionPolicy {
            version: policy_version.max(1),
            allow: allow_rules,
            deny: deny_rules,
        },
        sandbox: SandboxSpec {
            enabled: sandbox_enabled,
            writable_roots,
            deny_write,
            deny_read,
            allowed_unix_sockets,
        },
        allowed_tools,
    })
}

/// How the office answers an incoming [`ApprovalRequest`] from a runtime session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscalationDisposition {
    /// Allowed automatically by the session's active policy (only when `Escalation::None`).
    AllowByPolicy,
    /// Denied automatically without bothering the owner (fixed deny or `SandboxBypass`).
    AutoDeny { reason: String },
    /// Escalated to the organization owner with an explicit warning (never answered from policy).
    EscalateToOwner { warning: String },
}

/// Evaluates an [`ApprovalRequest`] against the session's policy and ADR-0032 escalation rules.
/// - `Escalation::SandboxBypass` is ALWAYS auto-denied (fixed denies win; no override in v1).
/// - `Escalation::PermissionChange` and `Escalation::Unknown` ALWAYS go to the owner.
/// - `Escalation::None` checks deny rules first (deny beats allow, including scoped Path /
///   CommandPrefix / McpServer rules; fails closed on opaque shell invocations when path denies exist).
pub fn decide_escalation(
    request: &ApprovalRequest,
    policy: &PermissionPolicy,
    preset: PermissionPreset,
) -> EscalationDisposition {
    match request.escalation {
        Escalation::SandboxBypass => EscalationDisposition::AutoDeny {
            reason: "Sandbox bypass requests are refused by office security policy.".to_owned(),
        },
        Escalation::PermissionChange => EscalationDisposition::EscalateToOwner {
            warning: format!(
                "Agent requested a permission mode change for tool `{}`; owner decision required.",
                request.tool
            ),
        },
        Escalation::None => evaluate_policy_rules(request, policy, preset),
        _ => EscalationDisposition::EscalateToOwner {
            warning: format!(
                "Agent raised an unrecognized escalation for tool `{}`; owner decision required.",
                request.tool
            ),
        },
    }
}

fn evaluate_policy_rules(
    request: &ApprovalRequest,
    policy: &PermissionPolicy,
    preset: PermissionPreset,
) -> EscalationDisposition {
    let mut strings = Vec::new();
    collect_json_strings(&request.input, &mut strings);
    if matches_any_deny(&policy.deny, &request.tool, &strings) {
        return EscalationDisposition::AutoDeny {
            reason: format!(
                "Tool `{}` is denied by office security policy.",
                request.tool
            ),
        };
    }
    if has_path_deny(&policy.deny, &request.tool)
        && is_shell_tool(&request.tool)
        && strings.is_empty()
    {
        return EscalationDisposition::EscalateToOwner {
            warning: format!(
                "Shell tool `{}` with opaque input cannot be proven outside denied office paths.",
                request.tool
            ),
        };
    }
    if preset != PermissionPreset::SafeAsk
        && matches_any_allow(&policy.allow, &request.tool, &strings)
    {
        return EscalationDisposition::AllowByPolicy;
    }
    EscalationDisposition::EscalateToOwner {
        warning: format!(
            "Approval required for tool `{}` under `{}` preset.",
            request.tool,
            preset.as_str()
        ),
    }
}

/// Projects a [`PresetConfig`] into the wire [`SessionGrantsResult`] served by `agent-os grant list`.
pub fn build_grants_summary(
    employee_id: &str,
    config: &PresetConfig,
    read_only_dirs: &[SpecPath],
) -> SessionGrantsResult {
    let mut rules = Vec::new();
    for rule in &config.policy.deny {
        rules.push(rule_to_view("deny", rule));
    }
    for rule in &config.policy.allow {
        rules.push(rule_to_view("allow", rule));
    }
    SessionGrantsResult {
        employee_id: employee_id.to_owned(),
        permission_preset: config.preset.as_str().to_owned(),
        permission_mode: mode_name(config.permission_mode).to_owned(),
        sandbox_enabled: config.sandbox.enabled,
        writable_roots: config
            .sandbox
            .writable_roots
            .iter()
            .map(|p| p.as_str().to_owned())
            .collect(),
        read_only_dirs: read_only_dirs
            .iter()
            .map(|p| p.as_str().to_owned())
            .collect(),
        denied_write_patterns: config
            .sandbox
            .deny_write
            .iter()
            .map(|p| p.as_str().to_owned())
            .collect(),
        denied_read_patterns: config
            .sandbox
            .deny_read
            .iter()
            .map(|p| p.as_str().to_owned())
            .collect(),
        allowed_tools: config
            .allowed_tools
            .as_ref()
            .map(|list| list.iter().map(|t| t.as_str().to_owned()).collect()),
        rules,
    }
}

fn rule(tool_name: &str, scope: RuleScope) -> Rule {
    Rule {
        tool: RuleTool::try_from(tool_name).expect("static rule tool is valid"),
        scope,
    }
}

fn tool(name: &str) -> ToolName {
    ToolName::try_from(name).expect("static tool name is valid")
}

fn collect_json_strings(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_json_strings(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            for v in map.values() {
                collect_json_strings(v, out);
            }
        }
        _ => {}
    }
}

fn is_shell_tool(tool_name: &str) -> bool {
    matches!(tool_name, "Bash" | "Shell" | "sh")
}

fn has_path_deny(rules: &[Rule], tool_name: &str) -> bool {
    rules.iter().any(|rule| {
        (rule.tool.as_str() == "*" || rule.tool.as_str() == tool_name)
            && matches!(rule.scope, RuleScope::Path(_))
    })
}

fn matches_any_deny(rules: &[Rule], tool_name: &str, strings: &[String]) -> bool {
    rules.iter().any(|rule| {
        if rule.tool.as_str() != "*" && rule.tool.as_str() != tool_name {
            return false;
        }
        match &rule.scope {
            RuleScope::Any => true,
            RuleScope::Path(pattern) => strings
                .iter()
                .any(|s| path_matches_glob(s, pattern.as_str())),
            RuleScope::CommandPrefix(prefix) => strings
                .iter()
                .any(|s| s.trim_start().starts_with(prefix.as_str())),
            RuleScope::McpServer(server) => {
                tool_name.starts_with(&format!("mcp__{}__", server.as_str()))
            }
            _ => true, // Unknown deny scope fails closed.
        }
    })
}

fn matches_any_allow(rules: &[Rule], tool_name: &str, strings: &[String]) -> bool {
    rules.iter().any(|rule| {
        if rule.tool.as_str() != "*" && rule.tool.as_str() != tool_name {
            return false;
        }
        match &rule.scope {
            RuleScope::Any => true,
            RuleScope::Path(pattern) => strings
                .iter()
                .any(|s| path_matches_glob(s, pattern.as_str())),
            RuleScope::CommandPrefix(prefix) => strings
                .iter()
                .any(|s| s.trim_start().starts_with(prefix.as_str())),
            RuleScope::McpServer(server) => {
                tool_name.starts_with(&format!("mcp__{}__", server.as_str()))
            }
            _ => false, // Unknown allow scope is dropped.
        }
    })
}

fn path_matches_glob(candidate: &str, pattern: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix("/**") {
        let with_slash = format!("{prefix}/");
        return candidate == prefix
            || candidate.starts_with(&with_slash)
            || candidate.contains(&with_slash)
            || candidate.contains(prefix);
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return candidate.starts_with(prefix) || candidate.contains(prefix);
    }
    candidate == pattern || candidate.contains(pattern)
}

fn mode_name(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Ask => "ask",
        PermissionMode::AcceptEdits => "accept_edits",
        PermissionMode::Plan => "plan",
        _ => "unknown",
    }
}

fn rule_to_view(effect: &str, rule: &Rule) -> GrantRuleView {
    let (scope, pattern) = match &rule.scope {
        RuleScope::Any => ("any", None),
        RuleScope::Path(p) => ("path", Some(p.as_str().to_owned())),
        RuleScope::CommandPrefix(c) => ("command_prefix", Some(c.as_str().to_owned())),
        RuleScope::McpServer(m) => ("mcp_server", Some(m.as_str().to_owned())),
        _ => ("unknown", None),
    };
    GrantRuleView {
        effect: effect.to_owned(),
        tool: rule.tool.as_str().to_owned(),
        scope: scope.to_owned(),
        pattern,
    }
}
