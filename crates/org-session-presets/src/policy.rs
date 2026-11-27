//! Allow/deny rule matching of an approval request against the session policy.

use org_agent_runtime::{
    ApprovalKind, ApprovalRequest, PermissionPolicy, Rule, RuleScope, ToolCapability, ToolName,
};
use org_office::organization_family::records::PermissionPreset;

use super::paths::{path_matches_glob, path_matches_glob_deny};
use super::shell::{
    command_value_matches_safe_prefix, decoded_argv_matches_deny_prefix,
    decoded_str_matches_deny_prefix, extract_shell_command, is_shell_binary, is_shell_cmd_flag,
    shell_command_is_unambiguously_decoded,
};
use super::targets::{has_concrete_file_target, is_file_target_request, is_shell_request};
use super::EscalationDisposition;

pub(super) fn evaluate_allow_policy(
    request: &ApprovalRequest,
    policy: &PermissionPolicy,
    preset: PermissionPreset,
    strings: &[String],
) -> EscalationDisposition {
    if request.capability == Some(ToolCapability::Unknown) {
        return EscalationDisposition::EscalateToOwner {
            warning: format!(
                "Tool `{}` reported an unrecognized capability class; owner decision required.",
                request.tool
            ),
        };
    }
    if has_path_deny(&policy.deny, request) {
        if is_shell_request(request)
            && (strings.is_empty() || extract_shell_command(&request.input).is_none())
        {
            return EscalationDisposition::EscalateToOwner {
                warning: format!(
                    "Shell tool `{}` with opaque input cannot be proven outside denied office paths.",
                    request.tool
                ),
            };
        }
        if is_file_target_request(request) && !has_concrete_file_target(request) {
            return EscalationDisposition::EscalateToOwner {
                warning: format!(
                    "File tool `{}` with opaque input cannot be proven outside denied office paths.",
                    request.tool
                ),
            };
        }
    }
    if has_command_prefix_deny(&policy.deny, request)
        && is_shell_request(request)
        && !shell_command_is_unambiguously_decoded(&request.input)
    {
        return EscalationDisposition::EscalateToOwner {
            warning: format!(
                "Shell tool `{}` with compound or undecodable command cannot be proven outside denied command prefixes.",
                request.tool
            ),
        };
    }
    if preset != PermissionPreset::SafeAsk && matches_any_allow(&policy.allow, request, strings) {
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

pub(super) fn request_in_allowed_tools(request: &ApprovalRequest, allowlist: &[ToolName]) -> bool {
    if request.capability == Some(ToolCapability::Unknown) {
        return false;
    }
    allowlist.iter().any(|allowed| {
        let raw = allowed.as_str();
        if ToolCapability::is_capability_token(raw) {
            ToolCapability::parse_rule_token(raw).is_some_and(|cap| request.capability == Some(cap))
        } else {
            raw == request.tool
        }
    })
}

pub(super) fn rule_matches_tool(rule: &Rule, request: &ApprovalRequest, is_deny: bool) -> bool {
    let rule_tool = rule.tool.as_str();
    if rule_tool == "*" {
        return true;
    }
    if ToolCapability::is_capability_token(rule_tool) {
        return match ToolCapability::parse_rule_token(rule_tool) {
            Some(cap) => {
                request.capability == Some(cap)
                    || (is_deny
                        && cap == ToolCapability::Edit
                        && request.kind == ApprovalKind::FileChange)
            }
            // Fail closed on unrecognized capability tokens: always match denies, never allows.
            None => is_deny,
        };
    }
    rule_tool == request.tool
}

pub(super) fn has_path_deny(rules: &[Rule], request: &ApprovalRequest) -> bool {
    rules.iter().any(|rule| {
        rule_matches_tool(rule, request, true) && matches!(rule.scope, RuleScope::Path(_))
    })
}

pub(super) fn has_command_prefix_deny(rules: &[Rule], request: &ApprovalRequest) -> bool {
    rules.iter().any(|rule| {
        rule_matches_tool(rule, request, true) && matches!(rule.scope, RuleScope::CommandPrefix(_))
    })
}

pub(super) fn matches_command_prefix_deny(input: &serde_json::Value, prefix: &str) -> bool {
    let Some(cmd_val) = extract_shell_command(input) else {
        return false;
    };
    match cmd_val {
        serde_json::Value::String(s) => decoded_str_matches_deny_prefix(s, prefix),
        serde_json::Value::Array(items) => {
            let Some(argv) = items
                .iter()
                .map(|v| v.as_str().map(ToOwned::to_owned))
                .collect::<Option<Vec<String>>>()
            else {
                return false;
            };
            if argv.len() >= 3
                && is_shell_binary(&argv[0])
                && is_shell_cmd_flag(&argv[1])
                && decoded_str_matches_deny_prefix(&argv[2], prefix)
            {
                return true;
            }
            decoded_argv_matches_deny_prefix(&argv, prefix)
        }
        _ => false,
    }
}

pub(super) fn matches_any_deny(
    rules: &[Rule],
    request: &ApprovalRequest,
    strings: &[String],
) -> bool {
    rules.iter().any(|rule| {
        if !rule_matches_tool(rule, request, true) {
            return false;
        }
        match &rule.scope {
            RuleScope::Any => true,
            RuleScope::Path(pattern) => strings
                .iter()
                .any(|s| path_matches_glob_deny(s, pattern.as_str())),
            RuleScope::CommandPrefix(prefix) => {
                matches_command_prefix_deny(&request.input, prefix.as_str())
            }
            RuleScope::McpServer(server) => {
                request
                    .tool
                    .starts_with(&format!("mcp__{}__", server.as_str()))
                    || request
                        .tool
                        .starts_with(&format!("mcp_{}_", server.as_str()))
            }
            _ => true, // Unknown deny scope fails closed.
        }
    })
}

pub(super) fn matches_any_allow(
    rules: &[Rule],
    request: &ApprovalRequest,
    strings: &[String],
) -> bool {
    rules.iter().any(|rule| {
        if !rule_matches_tool(rule, request, false) {
            return false;
        }
        match &rule.scope {
            RuleScope::Any => true,
            RuleScope::Path(pattern) => strings
                .iter()
                .any(|s| path_matches_glob(s, pattern.as_str())),
            RuleScope::CommandPrefix(prefix) => extract_shell_command(&request.input)
                .is_some_and(|cmd| command_value_matches_safe_prefix(cmd, prefix.as_str())),
            RuleScope::McpServer(server) => {
                request
                    .tool
                    .starts_with(&format!("mcp__{}__", server.as_str()))
                    || request
                        .tool
                        .starts_with(&format!("mcp_{}_", server.as_str()))
            }
            _ => false, // Unknown allow scope is dropped.
        }
    })
}
