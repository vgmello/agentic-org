//! Permission presets, default-on sandbox, and escalation policy (`ADR-0032` §3, `ENROLL-7`, `FC-189`).

use std::fmt;

use org_agent_runtime::{
    ApprovalKind, ApprovalRequest, CommandPrefix, Escalation, PathPattern, PermissionMode,
    PermissionPolicy, Rule, RuleScope, RuleTool, SandboxSpec, SpecPath, ToolCapability, ToolName,
};
use org_office::organization_family::records::PermissionPreset;
use org_proto::sessions::{GrantRuleView, SessionGrantsResult};

mod paths;
mod policy;
mod shell;
mod targets;

use paths::{all_targets_within_writable_roots, build_deny_strings};
use policy::{evaluate_allow_policy, matches_any_deny, request_in_allowed_tools};
use targets::{collect_json_strings, extract_concrete_file_targets};

/// Effective configuration derived from a [`PermissionPreset`] and sandbox toggle.
#[derive(Debug, Clone)]
pub struct PresetConfig {
    pub preset: PermissionPreset,
    pub permission_mode: PermissionMode,
    pub policy: PermissionPolicy,
    pub sandbox: SandboxSpec,
    pub allowed_tools: Option<Vec<ToolName>>,
    /// The prefixes an operator session's shell is confined to
    /// ([`PermissionPreset::OperatorSandboxed`]): `agent-os`. Empty for every other preset.
    pub operator_shell_prefixes: Vec<CommandPrefix>,
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
    // No trailing whitespace so Claude Code emits `Bash(agent-os *)` instead of dropping the rule.
    let agent_os_prefix =
        CommandPrefix::try_from("agent-os").expect("static agent-os command prefix is valid");
    let (permission_mode, allow_rules, allowed_tools) = match preset {
        PermissionPreset::SafeAsk => (PermissionMode::Ask, Vec::new(), None),
        // The Chief of Staff's preset is the read-only one, its shell confined to `agent-os`
        // natively on every runtime (`operator_shell_prefixes`, `TASK-80.1`).
        PermissionPreset::ReadOnly | PermissionPreset::OperatorSandboxed => (
            PermissionMode::Plan,
            vec![
                cap_rule(ToolCapability::Read, RuleScope::Any),
                cap_rule(ToolCapability::Search, RuleScope::Any),
                cap_rule(
                    ToolCapability::Shell,
                    RuleScope::CommandPrefix(agent_os_prefix.clone()),
                ),
            ],
            Some(vec![
                ToolCapability::Read.to_tool_name(),
                ToolCapability::Search.to_tool_name(),
                ToolCapability::Shell.to_tool_name(),
            ]),
        ),
        PermissionPreset::WorkspaceEdits => (
            PermissionMode::AcceptEdits,
            vec![
                cap_rule(ToolCapability::Read, RuleScope::Any),
                cap_rule(ToolCapability::Search, RuleScope::Any),
                cap_rule(ToolCapability::Edit, RuleScope::Any),
                cap_rule(
                    ToolCapability::Shell,
                    RuleScope::CommandPrefix(agent_os_prefix.clone()),
                ),
            ],
            None,
        ),
        PermissionPreset::AutonomousSandboxed => (
            PermissionMode::AcceptEdits,
            vec![
                cap_rule(ToolCapability::Read, RuleScope::Any),
                cap_rule(ToolCapability::Search, RuleScope::Any),
                cap_rule(ToolCapability::Edit, RuleScope::Any),
                cap_rule(ToolCapability::Shell, RuleScope::Any),
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

    let writable_roots = if preset.is_read_only_fs() {
        Vec::new()
    } else {
        vec![workspace.clone()]
    };
    let operator_shell_prefixes = match preset {
        PermissionPreset::OperatorSandboxed => vec![agent_os_prefix.clone()],
        _ => Vec::new(),
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
        operator_shell_prefixes,
    })
}

/// Extends a [`PresetConfig`] with agent-owned task worktree writable roots and bare-cache
/// write-only denies (`doc-35` / `FC-103` / `TASK-126`) without denying read access to shared
/// Git metadata (`HEAD`, `config`, `packed-refs`, `refs/remotes`).
pub fn apply_worktree_sandbox_rules(
    cfg: &mut PresetConfig,
    extra_writable_roots: &[SpecPath],
    extra_write_denies: &[PathPattern],
) {
    if !cfg.preset.is_read_only_fs() {
        for root in extra_writable_roots {
            if !cfg.sandbox.writable_roots.contains(root) {
                cfg.sandbox.writable_roots.push(root.clone());
            }
        }
    }
    let edit_tool = RuleTool::try_from(ToolCapability::Edit.as_rule_token())
        .expect("static edit capability token is valid");
    for pat in extra_write_denies {
        if !cfg.sandbox.deny_write.contains(pat) {
            cfg.sandbox.deny_write.push(pat.clone());
        }
        let deny_rule = Rule {
            tool: edit_tool.clone(),
            scope: RuleScope::Path(pat.clone()),
        };
        if !cfg.policy.deny.contains(&deny_rule) {
            cfg.policy.deny.push(deny_rule);
        }
    }
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

/// Evaluates an [`ApprovalRequest`] against policy and ADR-0032 escalation rules.
pub fn decide_escalation(
    request: &ApprovalRequest,
    policy: &PermissionPolicy,
    preset: PermissionPreset,
) -> EscalationDisposition {
    decide_escalation_with_tools(request, policy, preset, None)
}

pub fn decide_escalation_with_tools(
    request: &ApprovalRequest,
    policy: &PermissionPolicy,
    preset: PermissionPreset,
    allowed_tools: Option<&[ToolName]>,
) -> EscalationDisposition {
    decide_escalation_with_roots(request, policy, preset, allowed_tools, None)
}

/// Evaluates an [`ApprovalRequest`] against policy, optional `allowed_tools`, and `writable_roots`.
pub fn decide_escalation_with_roots(
    request: &ApprovalRequest,
    policy: &PermissionPolicy,
    preset: PermissionPreset,
    allowed_tools: Option<&[ToolName]>,
    writable_roots: Option<&[SpecPath]>,
) -> EscalationDisposition {
    let escalation = request.effective_escalation();
    if escalation == Escalation::SandboxBypass {
        return EscalationDisposition::AutoDeny {
            reason: "Sandbox bypass requests are refused by office security policy.".to_owned(),
        };
    }
    let mut strings = Vec::new();
    collect_json_strings(&request.input, &mut strings);
    if let Some(blocked_path) = &request.blocked_path {
        strings.push(blocked_path.clone());
    }
    let (concrete_targets, has_opaque_targets) = extract_concrete_file_targets(request);
    for target in &concrete_targets {
        if !strings.contains(target) {
            strings.push(target.clone());
        }
    }
    let deny_strings = build_deny_strings(&strings, &concrete_targets);
    if matches_any_deny(&policy.deny, request, &deny_strings) {
        return EscalationDisposition::AutoDeny {
            reason: format!(
                "Tool `{}` is denied by office security policy.",
                request.tool
            ),
        };
    }
    if let Some(allowlist) = allowed_tools {
        if !request_in_allowed_tools(request, allowlist) {
            return EscalationDisposition::AutoDeny {
                reason: format!(
                    "Tool `{}` is outside this session's allowed tools.",
                    request.tool
                ),
            };
        }
    }
    if let Some(roots) = writable_roots {
        if is_edit_request(request)
            && (has_opaque_targets || !all_targets_within_writable_roots(&concrete_targets, roots))
        {
            return EscalationDisposition::AutoDeny {
                reason: format!(
                    "Write target for tool `{}` is outside this session's writable roots.",
                    request.tool
                ),
            };
        }
    }
    match escalation {
        Escalation::PermissionChange => EscalationDisposition::EscalateToOwner {
            warning: format!(
                "Agent requested a permission mode change for tool `{}`; owner decision required.",
                request.tool
            ),
        },
        Escalation::None => evaluate_allow_policy(request, policy, preset, &strings),
        _ => EscalationDisposition::EscalateToOwner {
            warning: format!(
                "Agent raised an unrecognized escalation for tool `{}`; owner decision required.",
                request.tool
            ),
        },
    }
}

fn is_edit_request(request: &ApprovalRequest) -> bool {
    request.kind == ApprovalKind::FileChange || request.capability == Some(ToolCapability::Edit)
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
    let map_spec = |v: &[SpecPath]| v.iter().map(|p| p.as_str().to_owned()).collect();
    let map_pat = |v: &[PathPattern]| v.iter().map(|p| p.as_str().to_owned()).collect();
    SessionGrantsResult {
        employee_id: employee_id.to_owned(),
        permission_preset: config.preset.as_str().to_owned(),
        permission_mode: mode_name(config.permission_mode).to_owned(),
        sandbox_enabled: config.sandbox.enabled,
        writable_roots: map_spec(&config.sandbox.writable_roots),
        read_only_dirs: map_spec(read_only_dirs),
        denied_write_patterns: map_pat(&config.sandbox.deny_write),
        denied_read_patterns: map_pat(&config.sandbox.deny_read),
        allowed_tools: config
            .allowed_tools
            .as_ref()
            .map(|list| list.iter().map(|t| t.as_str().to_owned()).collect()),
        rules,
    }
}

fn cap_rule(cap: ToolCapability, scope: RuleScope) -> Rule {
    Rule {
        tool: cap.to_rule_tool(),
        scope,
    }
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
