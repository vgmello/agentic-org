//! A runtime's request to approve one tool call: [`ApprovalRequest`] and its parts.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::redact::{redact_opt, redact_value};
use crate::ToolCapability;

/// A runtime's request to approve one tool call. The office answers it from the agent's
/// policy, or asks a person in Needs you.
///
/// Only yes-or-no requests become approvals. A runtime request that needs **content** back
/// (Codex `tool/requestUserInput`, `mcpServer/elicitation/request`) has no answer in
/// [`crate::Decision`], so adapters **decline it themselves** in v1 and never raise it here.
///
/// **Escalations are never answered from policy.** [`escalation`](Self::escalation) says
/// whether allowing the request would widen the session beyond its spec:
/// - [`Escalation::SandboxBypass`]: the office **always denies** it. The fixed denies win,
///   with no policy rule and no owner override in phase 1.
/// - [`Escalation::PermissionChange`] and [`Escalation::Unknown`]: the office never answers
///   them from the policy, `allow_always` rules included. They always go to the owner in
///   Needs you, with a clear warning that allowing changes what the agent may do.
/// - [`Escalation::None`]: the office answers from the policy, or asks the owner.
///
/// The office acts on [`ApprovalRequest::effective_escalation`], never on the field alone, so
/// an adapter that leaves a permissions or unknown request at `None` still fails closed.
///
/// It's `#[non_exhaustive]` so it can grow: build one with [`ApprovalRequest::new`], then
/// set the optional fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ApprovalRequest {
    /// The request's id. Pass it back unchanged in [`crate::Control::Answer`].
    pub id: ApprovalId,
    /// What kind of action it is.
    pub kind: ApprovalKind,
    /// Whether allowing it would widen the session beyond its spec. **Required**: the adapter
    /// always sets it (see [`Escalation`]).
    pub escalation: Escalation,
    /// The tool.
    pub tool: String,
    /// Runtime-neutral capability class of the tool, when classified by the adapter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability: Option<ToolCapability>,
    /// The tool's name for people, where the runtime gives one.
    pub display_name: Option<String>,
    /// The tool's input, as structured JSON: what will actually run if allowed.
    pub input: Value,
    /// The agent's own explanation. **Untrusted**: the agent wrote it, so it may try to
    /// persuade. Show it labelled as such, and never decide on it.
    pub justification_untrusted: Option<String>,
    /// The path that triggered the request, where the runtime says.
    pub blocked_path: Option<String>,
    /// The tool call this is about, where the runtime says.
    pub tool_use_id: Option<String>,
}

impl ApprovalRequest {
    /// A request with its required fields; the optional ones start as `None`.
    pub fn new(
        id: ApprovalId,
        kind: ApprovalKind,
        escalation: Escalation,
        tool: impl Into<String>,
        input: Value,
    ) -> Self {
        ApprovalRequest {
            id,
            kind,
            escalation,
            tool: tool.into(),
            capability: None,
            display_name: None,
            input,
            justification_untrusted: None,
            blocked_path: None,
            tool_use_id: None,
        }
    }

    /// The escalation the office acts on: the adapter's, raised to what the request's kind
    /// or capability implies, so a wrong `None` fails closed. A [`ApprovalKind::Permissions`]
    /// request is at least a [`Escalation::PermissionChange`], an [`ApprovalKind::Unknown`]
    /// or [`ToolCapability::Unknown`] one is at least [`Escalation::Unknown`], and
    /// [`Escalation::SandboxBypass`] always stays.
    pub fn effective_escalation(&self) -> Escalation {
        if self.escalation == Escalation::None && self.capability == Some(ToolCapability::Unknown) {
            return Escalation::Unknown;
        }
        match (self.escalation, self.kind) {
            (Escalation::None, ApprovalKind::Permissions) => Escalation::PermissionChange,
            (Escalation::None, ApprovalKind::Unknown) => Escalation::Unknown,
            (escalation, _) => escalation,
        }
    }

    pub(super) fn redact(&mut self, f: &mut dyn FnMut(&mut String)) {
        let ApprovalRequest {
            id: _,
            kind: _,
            escalation: _,
            tool,
            capability: _,
            display_name,
            input,
            justification_untrusted,
            blocked_path,
            tool_use_id: _,
        } = self;
        f(tool);
        redact_opt(display_name, f);
        redact_value(input, f);
        redact_opt(justification_untrusted, f);
        redact_opt(blocked_path, f);
    }
}

/// What an [`ApprovalRequest`] asks to do. Unknown values decode as [`ApprovalKind::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ApprovalKind {
    /// Run a shell command.
    Command,
    /// Change files.
    FileChange,
    /// Widen the session's own permissions (e.g. Codex `item/permissions/requestApproval`).
    Permissions,
    /// Any other tool call (e.g. an MCP tool).
    Other,
    /// Not known to this build.
    #[serde(other)]
    Unknown,
}

/// Whether allowing an [`ApprovalRequest`] would widen the session beyond its spec. Unknown
/// values decode as [`Escalation::Unknown`], which the office treats like a permission change.
///
/// The adapter **must** set it from what its runtime asks:
/// - a command the runtime wants to run **outside its sandbox** (Codex's escalated or
///   unsandboxed commands): [`Escalation::SandboxBypass`];
/// - a request to change the session's own permissions or mode (Claude's `ExitPlanMode`, an
///   ACP mode switch, Codex's permissions requests): [`Escalation::PermissionChange`];
/// - a request it can't classify (an [`ApprovalKind::Unknown`]): [`Escalation::Unknown`];
/// - an ordinary tool call within the spec: [`Escalation::None`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Escalation {
    /// An ordinary tool call within the session's spec.
    None,
    /// Running outside the sandbox. Always denied.
    SandboxBypass,
    /// Changing the session's permissions or mode. Always asks the owner.
    PermissionChange,
    /// Not known to this build, or not classifiable. Always asks the owner.
    #[serde(other)]
    Unknown,
}

/// An approval request's id: **opaque** to the office.
///
/// The office stores it, compares it, and hands it back in [`crate::Control::Answer`]; it
/// never builds one or reads meaning into it. Adapters make it from their runtime's request
/// id ([`ApprovalId::new`]) and read it back ([`ApprovalId::as_str`]) to answer. It serializes
/// as a plain string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApprovalId(String);

impl ApprovalId {
    /// For adapters: wraps the runtime's own request id (as a string, whatever its type on
    /// the wire).
    pub fn new(id: impl Into<String>) -> Self {
        ApprovalId(id.into())
    }

    /// For adapters: the runtime's request id, to answer it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
