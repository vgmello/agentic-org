//! Wire contracts for Chief of Staff (Tess) server-validated suggestions, structured UI
//! presentation blocks, Ask Tess turns, streaming events, and two-phase proposal apply
//! (`TASK-80.4` Parts A & B).
//!
//! All parameter types use `#[serde(deny_unknown_fields)]` so malformed or unexpected fields fail
//! closed at decode time.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::Call;

/// Maximum UTF-8 byte length of a suggestion summary.
pub const MAX_SUGGESTION_SUMMARY_BYTES: usize = 512;
/// Maximum UTF-8 byte length of a suggestion rationale.
pub const MAX_SUGGESTION_RATIONALE_BYTES: usize = 2_048;
/// Default suggestion TTL in seconds (15 minutes).
pub const DEFAULT_SUGGESTION_TTL_SECONDS: u64 = 900;
/// Minimum suggestion TTL in seconds.
pub const MIN_SUGGESTION_TTL_SECONDS: u64 = 30;
/// Maximum suggestion TTL in seconds (24 hours).
pub const MAX_SUGGESTION_TTL_SECONDS: u64 = 86_400;
/// Maximum follow-up prompts permitted in one [`UiBlock`].
pub const MAX_UI_FOLLOW_UPS: usize = 3;
/// Maximum verified entity links permitted in one [`UiBlock`].
pub const MAX_UI_ENTITY_LINKS: usize = 20;
/// Maximum metric items permitted in one [`UiBlock`].
pub const MAX_UI_METRICS: usize = 12;
/// Maximum navigation actions permitted in one [`UiBlock`].
pub const MAX_UI_NAVIGATION: usize = 6;
/// Maximum byte length of a UI block title, label, value, route, or prompt.
pub const MAX_UI_TEXT_BYTES: usize = 512;
/// Maximum UTF-8 byte length of an Ask Tess user prompt.
pub const MAX_TESS_PROMPT_BYTES: usize = 8_192;
/// Maximum UTF-8 byte length of a compact volatile UI viewport context envelope.
pub const MAX_VIEWPORT_CONTEXT_BYTES: usize = 800;
/// Maximum server-side wait window (ms) for one `tess.turn_events` poll call.
pub const MAX_TESS_EVENTS_WAIT_MS: u64 = 15_000;

/// Lifecycle state of a Chief of Staff proposed mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SuggestionStatus {
    Pending,
    Applied,
    Withdrawn,
    Dismissed,
    Expired,
    #[serde(other)]
    Unknown,
}

impl SuggestionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Applied => "applied",
            Self::Withdrawn => "withdrawn",
            Self::Dismissed => "dismissed",
            Self::Expired => "expired",
            Self::Unknown => "unknown",
        }
    }
}

/// Entity kinds that `ui.present` verifies against live office state and caller visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UiEntityKind {
    Task,
    Project,
    Team,
    Member,
    Role,
    Process,
    Channel,
    Office,
    Budget,
    HireRequest,
    #[serde(other)]
    Unknown,
}

impl UiEntityKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Project => "project",
            Self::Team => "team",
            Self::Member => "member",
            Self::Role => "role",
            Self::Process => "process",
            Self::Channel => "channel",
            Self::Office => "office",
            Self::Budget => "budget",
            Self::HireRequest => "hire_request",
            Self::Unknown => "unknown",
        }
    }
}

/// Caller-supplied entity reference for `ui.present`, resolved and authorized by the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiEntityLinkInput {
    pub kind: UiEntityKind,
    /// Entity UUIDv7, key (`T-A1`, `P-1`), handle, or canonical name depending on `kind`.
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Server-verified entity link returned inside a [`UiBlock`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiEntityLink {
    pub kind: UiEntityKind,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub label: String,
    pub route: String,
}

/// One compact metric card rendered in a [`UiBlock`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiMetric {
    pub label: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

/// One validated in-app navigation shortcut rendered in a [`UiBlock`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiNavigation {
    pub label: String,
    pub route: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_kind: Option<UiEntityKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
}

/// One suggested follow-up prompt (at most [`MAX_UI_FOLLOW_UPS`] per block).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiFollowUp {
    pub label: String,
    pub prompt: String,
}

/// Verified structured UI presentation payload returned by `ui.present` and `suggestion.create`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiBlock {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_links: Vec<UiEntityLink>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<UiMetric>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub navigation: Vec<UiNavigation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_ups: Vec<UiFollowUp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion_id: Option<String>,
}

/// Wire view of one caller-scoped proposed mutation stored by the office.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestionView {
    pub id: String,
    pub status: SuggestionStatus,
    /// Canonical `agent-os` command (`"<resource> <verb>"`).
    pub command: String,
    /// Underlying office RPC method declared by the command row.
    pub call: String,
    /// Validated input object for the target command.
    pub input: Map<String, Value>,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
    pub created_by_member_id: String,
    pub for_caller_id: String,
    pub preflight_ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preflight_note: Option<String>,
    pub created_at: String,
    pub expires_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_reason: Option<String>,
}

// ── suggestion.create ────────────────────────────────────────────────────────

/// `suggestion.create`: validate, preflight, and store a pending mutation proposal for the caller.
pub struct SuggestionCreate;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuggestionCreateParams {
    /// Target `agent-os` command (`"<resource> <verb>"`, e.g. `"task create"` or `"team update"`).
    pub command: String,
    /// Validated input fields matching the target command's schema.
    #[serde(default)]
    pub input: Map<String, Value>,
    /// Concise human-facing summary shown on the confirmation card.
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestionCreateResult {
    pub suggestion: SuggestionView,
    pub ui_block: UiBlock,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dry_run: bool,
}

impl Call for SuggestionCreate {
    const METHOD: &'static str = "suggestion.create";
    type Params = SuggestionCreateParams;
    type Result = SuggestionCreateResult;
}

// ── suggestion.list ──────────────────────────────────────────────────────────

/// `suggestion.list`: list pending or resolved suggestions scoped to the caller.
pub struct SuggestionList;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuggestionListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<SuggestionStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestionListResult {
    pub suggestions: Vec<SuggestionView>,
}

impl Call for SuggestionList {
    const METHOD: &'static str = "suggestion.list";
    type Params = SuggestionListParams;
    type Result = SuggestionListResult;
}

// ── suggestion.inspect ───────────────────────────────────────────────────────

/// `suggestion.inspect`: inspect one caller-scoped suggestion by ID.
pub struct SuggestionInspect;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuggestionInspectParams {
    pub suggestion_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestionInspectResult {
    pub suggestion: SuggestionView,
    pub ui_block: UiBlock,
}

impl Call for SuggestionInspect {
    const METHOD: &'static str = "suggestion.inspect";
    type Params = SuggestionInspectParams;
    type Result = SuggestionInspectResult;
}

// ── suggestion.withdraw ──────────────────────────────────────────────────────

/// `suggestion.withdraw`: withdraw a pending suggestion created for the current caller.
pub struct SuggestionWithdraw;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuggestionWithdrawParams {
    pub suggestion_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestionActionResult {
    pub suggestion: SuggestionView,
}

impl Call for SuggestionWithdraw {
    const METHOD: &'static str = "suggestion.withdraw";
    type Params = SuggestionWithdrawParams;
    type Result = SuggestionActionResult;
}

// ── suggestion.dismiss ───────────────────────────────────────────────────────

/// `suggestion.dismiss`: human caller dismisses a pending suggestion addressed to them.
pub struct SuggestionDismiss;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuggestionDismissParams {
    pub suggestion_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Call for SuggestionDismiss {
    const METHOD: &'static str = "suggestion.dismiss";
    type Params = SuggestionDismissParams;
    type Result = SuggestionActionResult;
}

// ── ui.present ───────────────────────────────────────────────────────────────

/// `ui.present`: validate entity references against live office state & caller visibility and
/// return a verified [`UiBlock`] for structured UI presentation.
pub struct UiPresent;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiPresentParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_links: Vec<UiEntityLinkInput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<UiMetric>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub navigation: Vec<UiNavigation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_ups: Vec<UiFollowUp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiPresentResult {
    pub ui_block: UiBlock,
}

impl Call for UiPresent {
    const METHOD: &'static str = "ui.present";
    type Params = UiPresentParams;
    type Result = UiPresentResult;
}

// ── suggestion.apply ─────────────────────────────────────────────────────────

/// Dual attribution metadata for a human-confirmed Tess suggestion application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestionAttribution {
    /// Verified active human caller whose authority executed the mutation (`via_grant: true`).
    pub actor_member_id: String,
    /// Chief of Staff member ID that proposed the suggestion.
    pub proposed_by_member_id: String,
}

/// Compact human-facing receipt returned after applying a suggestion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestionApplyReceipt {
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_href: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_label: Option<String>,
    pub applied_at: String,
}

/// `suggestion.apply`: two-phase human-authenticated HostChannel apply of a pending suggestion.
pub struct SuggestionApply;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuggestionApplyParams {
    pub suggestion_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub if_match: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestionApplyResult {
    pub suggestion: SuggestionView,
    pub applied_result: Value,
    pub attribution: SuggestionAttribution,
    pub receipt: SuggestionApplyReceipt,
}

impl Call for SuggestionApply {
    const METHOD: &'static str = "suggestion.apply";
    type Params = SuggestionApplyParams;
    type Result = SuggestionApplyResult;
}

// ── tess.ask ─────────────────────────────────────────────────────────────────

/// Status returned by [`TessAsk`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TessAskStatus {
    Streaming,
    Fallback,
    #[serde(other)]
    Unknown,
}

/// Compact (`<= 800` B) volatile UI viewport context envelope appended strictly LAST in the turn
/// input behind an explicit untrusted-data fence.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TessViewportContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    #[serde(default, alias = "routeLabel", skip_serializing_if = "Option::is_none")]
    pub route_label: Option<String>,
    #[serde(default, alias = "entityKind", skip_serializing_if = "Option::is_none")]
    pub entity_kind: Option<UiEntityKind>,
    #[serde(
        default,
        alias = "entityRef",
        alias = "entity_ref",
        alias = "entityId",
        skip_serializing_if = "Option::is_none"
    )]
    pub entity_id: Option<String>,
    #[serde(
        default,
        alias = "entityTitle",
        alias = "decisionTitle",
        alias = "decision_title",
        skip_serializing_if = "Option::is_none"
    )]
    pub entity_title: Option<String>,
    #[serde(default, alias = "decisionId", skip_serializing_if = "Option::is_none")]
    pub decision_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub filters: BTreeMap<String, String>,
}

/// `tess.ask`: start a caller-scoped Ask Tess turn in partition `tess:ask:<caller_id>`.
pub struct TessAsk;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TessAskParams {
    pub prompt: String,
    #[serde(
        default,
        alias = "context_ref",
        skip_serializing_if = "Option::is_none"
    )]
    pub viewport_context: Option<TessViewportContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TessAskResult {
    pub status: TessAskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub partition: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_answer: Option<String>,
}

impl Call for TessAsk {
    const METHOD: &'static str = "tess.ask";
    type Params = TessAskParams;
    type Result = TessAskResult;
}

// ── tess.turn_events ─────────────────────────────────────────────────────────

/// Coarse tool-progress state in an Ask Tess event stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TessToolState {
    Running,
    Done,
    Error,
    #[serde(other)]
    Unknown,
}

/// Terminal outcome of a finished Ask Tess turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TessTurnOutcome {
    Completed,
    Interrupted,
    Cancelled,
    #[serde(other)]
    Unknown,
}

/// Post-redaction event emitted for a caller-scoped Ask Tess turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum TessTurnEventKind {
    #[serde(rename = "turn.started")]
    TurnStarted { turn_id: String },
    #[serde(rename = "tool.progress")]
    ToolProgress {
        id: String,
        label: String,
        state: TessToolState,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phase: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        started_at: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
    },
    #[serde(rename = "text.delta")]
    TextDelta { delta: String },
    #[serde(rename = "ui.block")]
    UiBlock { ui_block: UiBlock },
    #[serde(rename = "suggestion.created")]
    SuggestionCreated {
        suggestion: Box<SuggestionView>,
        ui_block: UiBlock,
    },
    #[serde(rename = "turn.done")]
    TurnDone {
        turn_id: String,
        outcome: TessTurnOutcome,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        follow_ups: Vec<String>,
    },
    #[serde(rename = "turn.error")]
    TurnError { message: String },
    #[serde(other)]
    Unknown,
}

impl TessTurnEventKind {
    pub const fn event_name(&self) -> &'static str {
        match self {
            Self::TurnStarted { .. } => "turn.started",
            Self::ToolProgress { .. } => "tool.progress",
            Self::TextDelta { .. } => "text.delta",
            Self::UiBlock { .. } => "ui.block",
            Self::SuggestionCreated { .. } => "suggestion.created",
            Self::TurnDone { .. } => "turn.done",
            Self::TurnError { .. } => "turn.error",
            Self::Unknown => "unknown",
        }
    }

    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::TurnDone { .. } | Self::TurnError { .. })
    }
}

/// One monotonically sequenced frame from an Ask Tess turn stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TessTurnEvent {
    pub seq: u64,
    pub at: String,
    pub event: TessTurnEventKind,
}

/// `tess.turn_events`: poll or long-poll sequenced post-redaction events for a caller's turn.
pub struct TessTurnEvents;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TessTurnEventsParams {
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TessTurnEventsResult {
    pub turn_id: String,
    pub partition: String,
    pub events: Vec<TessTurnEvent>,
    pub head_seq: u64,
    pub done: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lagged: bool,
}

impl Call for TessTurnEvents {
    const METHOD: &'static str = "tess.turn_events";
    type Params = TessTurnEventsParams;
    type Result = TessTurnEventsResult;
}

// ── tess.turn_cancel ─────────────────────────────────────────────────────────

/// `tess.turn_cancel`: cancel the signed-in caller's own active Ask Tess turn.
pub struct TessTurnCancel;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TessTurnCancelParams {
    pub turn_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TessTurnCancelResult {
    pub turn_id: String,
    pub cancelled: bool,
    pub partition: String,
}

impl Call for TessTurnCancel {
    const METHOD: &'static str = "tess.turn_cancel";
    type Params = TessTurnCancelParams;
    type Result = TessTurnCancelResult;
}

// ── tess.session_reset ───────────────────────────────────────────────────────

/// `tess.session_reset`: reset only the signed-in caller's own `tess:ask:<caller_id>` partition.
pub struct TessSessionReset;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TessSessionResetParams {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TessSessionResetResult {
    pub reset: bool,
    pub stopped_live_session: bool,
    pub partition: String,
}

impl Call for TessSessionReset {
    const METHOD: &'static str = "tess.session_reset";
    type Params = TessSessionResetParams;
    type Result = TessSessionResetResult;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tess_params_reject_unknown_fields() {
        let bad_create = json!({
            "command": "task create",
            "input": {"title": "Test"},
            "summary": "Create task",
            "extra_bogus": true
        });
        assert!(serde_json::from_value::<SuggestionCreateParams>(bad_create).is_err());

        let bad_list = json!({"status": "pending", "unknown_field": 1});
        assert!(serde_json::from_value::<SuggestionListParams>(bad_list).is_err());

        let bad_inspect = json!({"suggestion_id": "id", "bogus": 2});
        assert!(serde_json::from_value::<SuggestionInspectParams>(bad_inspect).is_err());

        let bad_withdraw = json!({"suggestion_id": "id", "bogus": 2});
        assert!(serde_json::from_value::<SuggestionWithdrawParams>(bad_withdraw).is_err());

        let bad_dismiss = json!({"suggestion_id": "id", "bogus": 2});
        assert!(serde_json::from_value::<SuggestionDismissParams>(bad_dismiss).is_err());

        let bad_apply = json!({"suggestion_id": "id", "bogus": 2});
        assert!(serde_json::from_value::<SuggestionApplyParams>(bad_apply).is_err());

        let bad_ui = json!({"title": "Overview", "bogus": 3});
        assert!(serde_json::from_value::<UiPresentParams>(bad_ui).is_err());

        let bad_ask = json!({"prompt": "hi", "bogus": 1});
        assert!(serde_json::from_value::<TessAskParams>(bad_ask).is_err());

        let bad_vp = json!({"prompt": "hi", "viewport_context": {"route": "/", "bogus": 1}});
        assert!(serde_json::from_value::<TessAskParams>(bad_vp).is_err());

        let bad_events = json!({"turn_id": "t1", "bogus": 1});
        assert!(serde_json::from_value::<TessTurnEventsParams>(bad_events).is_err());

        let bad_cancel = json!({"turn_id": "t1", "bogus": 1});
        assert!(serde_json::from_value::<TessTurnCancelParams>(bad_cancel).is_err());

        let bad_reset = json!({"bogus": 1});
        assert!(serde_json::from_value::<TessSessionResetParams>(bad_reset).is_err());
    }

    #[test]
    fn tess_method_names_and_status_round_trip() {
        assert_eq!(SuggestionCreate::METHOD, "suggestion.create");
        assert_eq!(SuggestionList::METHOD, "suggestion.list");
        assert_eq!(SuggestionInspect::METHOD, "suggestion.inspect");
        assert_eq!(SuggestionWithdraw::METHOD, "suggestion.withdraw");
        assert_eq!(SuggestionDismiss::METHOD, "suggestion.dismiss");
        assert_eq!(SuggestionApply::METHOD, "suggestion.apply");
        assert_eq!(UiPresent::METHOD, "ui.present");
        assert_eq!(TessAsk::METHOD, "tess.ask");
        assert_eq!(TessTurnEvents::METHOD, "tess.turn_events");
        assert_eq!(TessTurnCancel::METHOD, "tess.turn_cancel");
        assert_eq!(TessSessionReset::METHOD, "tess.session_reset");

        assert_eq!(SuggestionStatus::Applied.as_str(), "applied");
        let unknown: SuggestionStatus = serde_json::from_str("\"future_state\"").unwrap();
        assert_eq!(unknown, SuggestionStatus::Unknown);
        let unknown_kind: UiEntityKind = serde_json::from_str("\"future_kind\"").unwrap();
        assert_eq!(unknown_kind, UiEntityKind::Unknown);
        let unknown_ask: TessAskStatus = serde_json::from_str("\"future\"").unwrap();
        assert_eq!(unknown_ask, TessAskStatus::Unknown);
        let unknown_ev: TessTurnEventKind =
            serde_json::from_value(json!({"type": "future.event", "x": 1})).unwrap();
        assert_eq!(unknown_ev, TessTurnEventKind::Unknown);

        let ask: TessAskParams = serde_json::from_value(json!({
            "prompt": "Summarize blockers",
            "context_ref": {
                "route": "/tasks/T-A1",
                "routeLabel": "Task T-A1",
                "entityRef": "T-A1"
            }
        }))
        .unwrap();
        assert_eq!(
            ask.viewport_context
                .as_ref()
                .and_then(|v| v.route.as_deref()),
            Some("/tasks/T-A1")
        );
        assert_eq!(
            ask.viewport_context
                .as_ref()
                .and_then(|v| v.entity_id.as_deref()),
            Some("T-A1")
        );
    }
}
