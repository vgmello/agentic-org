//! Wire contracts for Chief of Staff (Tess) server-validated suggestions and structured UI
//! presentation blocks (`TASK-80.4` Part A).
//!
//! All parameter types use `#[serde(deny_unknown_fields)]` so malformed or unexpected fields fail
//! closed at decode time.

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

/// Lifecycle state of a Chief of Staff proposed mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SuggestionStatus {
    Pending,
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

        let bad_ui = json!({"title": "Overview", "bogus": 3});
        assert!(serde_json::from_value::<UiPresentParams>(bad_ui).is_err());
    }

    #[test]
    fn tess_method_names_and_status_round_trip() {
        assert_eq!(SuggestionCreate::METHOD, "suggestion.create");
        assert_eq!(SuggestionList::METHOD, "suggestion.list");
        assert_eq!(SuggestionInspect::METHOD, "suggestion.inspect");
        assert_eq!(SuggestionWithdraw::METHOD, "suggestion.withdraw");
        assert_eq!(SuggestionDismiss::METHOD, "suggestion.dismiss");
        assert_eq!(UiPresent::METHOD, "ui.present");
        let unknown: SuggestionStatus = serde_json::from_str("\"future_state\"").unwrap();
        assert_eq!(unknown, SuggestionStatus::Unknown);
        let unknown_kind: UiEntityKind = serde_json::from_str("\"future_kind\"").unwrap();
        assert_eq!(unknown_kind, UiEntityKind::Unknown);
    }
}
