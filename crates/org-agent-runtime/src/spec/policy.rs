//! The permission vocabulary of a spec: [`PermissionMode`], [`PermissionPolicy`], its
//! [`Rule`]s and [`RuleScope`]s, and the runtime-neutral [`ToolCapability`] classes.

use serde::{Deserialize, Serialize};

use crate::{CommandPrefix, McpServerName, PathPattern, RuleTool, ToolName};

#[cfg(doc)]
use super::SessionSpec;

/// How the runtime asks before acting. Each mode maps to every runtime's own setting
/// (Claude `--permission-mode`, Codex `approvalPolicy`, Antigravity's mode).
///
/// **There is no bypass variant**, and a stored or sent bypass value (e.g.
/// `"bypassPermissions"`) fails to decode, so no code path can ask a runtime to skip its
/// permission checks. Like every boundary enum it's `#[non_exhaustive]`: an adapter that
/// meets a mode it doesn't know refuses the spec ([`crate::PlanError::Unsupported`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PermissionMode {
    /// Ask before anything the policy doesn't allow (Claude's "manual" / "default").
    Ask,
    /// Accept file edits inside the writable roots without asking; ask for the rest. An
    /// implicit allow, so adapters apply it only through
    /// [`SessionSpec::effective_permission_mode`], which turns it into `Ask` when a deny was
    /// dropped.
    AcceptEdits,
    /// Plan only: read and propose, never change anything.
    Plan,
}

/// The rules the runtime applies itself, before anything reaches the office's approval
/// bridge. The office answers bridged approvals from the same policy.
///
/// **A deny beats a matching allow, whatever the order of the rules**: the lists aren't
/// ordered rule chains, and an adapter whose runtime is first-match-wins writes every deny
/// before every allow.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionPolicy {
    /// The policy's version in the office (every change, `allow_always` included, bumps it).
    pub version: u64,
    /// What the runtime may do without asking. Grants: an adapter drops one its runtime can't
    /// express exactly (never widens it), and drops them all when it drops the deny rules
    /// (see [`crate::Capabilities`]).
    pub allow: Vec<Rule>,
    /// What the runtime must never do. Deny wins over allow. A droppable restriction, per
    /// [`crate::Capabilities::enforces_deny_rules`], never narrowed.
    pub deny: Vec<Rule>,
}

/// Canonical prefix for runtime-neutral tool capability tokens carried inside [`ToolName`]
/// and [`RuleTool`] values (e.g. `cap:read`, `cap:shell`).
pub const CAPABILITY_PREFIX: &str = "cap:";

/// Runtime-neutral tool capability classes translated by each runtime adapter.
///
/// The office builds preset allowlists and policy rules using canonical `cap:<name>` tokens
/// (`cap:read`, `cap:search`, `cap:edit`, `cap:shell`, `cap:subagent`) instead of hardcoding
/// one runtime's concrete tool names (`Read`, `Bash`, `run_command`, `apply_patch`). Each
/// adapter translates those tokens into its own concrete tool names when planning a session
/// and classifies incoming tool events and approval requests back into `ToolCapability`.
///
/// Unknown capability values on incoming runtime events decode as [`ToolCapability::Unknown`],
/// which fails closed during policy evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ToolCapability {
    /// Read files or inspect file contents without modifying them.
    Read,
    /// Search, list, or glob workspace files and contents without modifying them.
    Search,
    /// Create, edit, patch, or delete files inside writable roots.
    Edit,
    /// Run shell commands inside the runtime sandbox.
    Shell,
    /// Spawn or invoke a delegated sub-agent.
    Subagent,
    /// A capability class not known to this build. Fails closed in policy evaluation.
    #[serde(other)]
    Unknown,
}

impl ToolCapability {
    /// Wire/display name of this capability class (`"read"`, `"search"`, `"edit"`, `"shell"`,
    /// `"subagent"`, or `"unknown"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Search => "search",
            Self::Edit => "edit",
            Self::Shell => "shell",
            Self::Subagent => "subagent",
            Self::Unknown => "unknown",
        }
    }

    /// Canonical `cap:<name>` token string used in [`ToolName`] and [`RuleTool`] fields.
    pub const fn as_rule_token(self) -> &'static str {
        match self {
            Self::Read => "cap:read",
            Self::Search => "cap:search",
            Self::Edit => "cap:edit",
            Self::Shell => "cap:shell",
            Self::Subagent => "cap:subagent",
            Self::Unknown => "cap:unknown",
        }
    }

    /// Returns `true` if `raw` uses the reserved `cap:` capability-token namespace.
    ///
    /// Adapters use this to distinguish an unknown future capability token (which must fail
    /// closed on denies and tool allowlists) from a concrete runtime tool name.
    pub fn is_capability_token(raw: &str) -> bool {
        raw.starts_with(CAPABILITY_PREFIX)
    }

    /// Parses a known canonical `cap:<name>` token into its [`ToolCapability`].
    ///
    /// Returns `None` for concrete tool names, `"*"`, `"cap:unknown"`, and unrecognized
    /// `cap:*` tokens so callers fail closed on unknown capabilities.
    pub fn parse_rule_token(raw: &str) -> Option<Self> {
        match raw {
            "cap:read" => Some(Self::Read),
            "cap:search" => Some(Self::Search),
            "cap:edit" => Some(Self::Edit),
            "cap:shell" => Some(Self::Shell),
            "cap:subagent" => Some(Self::Subagent),
            _ => None,
        }
    }

    /// Converts this capability into a validated [`RuleTool`] token (`cap:<name>`).
    pub fn to_rule_tool(self) -> RuleTool {
        RuleTool::try_from(self.as_rule_token()).expect("static capability rule token is valid")
    }

    /// Converts this capability into a validated [`ToolName`] token (`cap:<name>`).
    pub fn to_tool_name(self) -> ToolName {
        ToolName::try_from(self.as_rule_token()).expect("static capability tool token is valid")
    }
}

/// One permission rule: a tool plus what it applies to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// A runtime-neutral capability token (`cap:read`, `cap:search`, `cap:edit`, `cap:shell`,
    /// `cap:subagent`), a concrete runtime tool name (e.g. `Bash`, `Edit`, `mcp__office__...`),
    /// or `*` for any tool.
    pub tool: RuleTool,
    /// What the rule covers.
    pub scope: RuleScope,
}

/// What a [`Rule`] covers. Written by the office, so an unknown scope fails to decode.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
#[non_exhaustive]
pub enum RuleScope {
    /// Every use of the tool.
    Any,
    /// Uses on paths matching this pattern.
    Path(PathPattern),
    /// Shell commands that start with this prefix, for shell tools.
    CommandPrefix(CommandPrefix),
    /// Tools of this MCP server.
    McpServer(McpServerName),
}
