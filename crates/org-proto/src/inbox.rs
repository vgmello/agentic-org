//! HQ's owner Inbox read contract (IA-10 rev 5.3). Reads never perform an action.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{Call, EventKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum InboxKind {
    Approval,
    Question,
    Decision,
    Problem,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum InboxClears {
    Placed,
    ModelSet,
    Answered,
    Accepted,
    LeavesFixed,
    AssignedOrClosed,
    Unblocked,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum InboxState {
    Open,
    Handled,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum OpenTarget {
    Member,
    WorkItem,
    Bug,
    Message,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Open {
    pub target: OpenTarget,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionInput {
    pub field: String,
    pub required: bool,
    pub max_chars: u32,
}

/// Clients drop unknown actions and map Open targets to their own routes. Fresh ids live
/// for one unchanged input, including retries after a lost answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Action {
    Call {
        label: String,
        op: String,
        params: Map<String, Value>,
        fresh: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input: Option<ActionInput>,
    },
    Open {
        label: String,
        open: Open,
    },
    #[serde(other)]
    Unknown,
}

/// Unknown `source` strings are retained. Unknown kinds/clearing rules render Open only.
/// Text caps count JSON-encoded bytes: summary512, context2048, recommendation1024,
/// options8x128. Writers truncate at a character boundary with an ellipsis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxItem {
    #[serde(rename = "ref")]
    pub reference: String,
    pub source: String,
    pub kind: InboxKind,
    pub member: Option<String>,
    pub project: Option<String>,
    pub team: Option<String>,
    pub summary: String,
    pub context: Option<String>,
    pub recommendation: Option<String>,
    pub options: Option<Vec<String>>,
    pub created_at_ms: Option<u64>,
    pub expires_at_ms: Option<u64>,
    pub state: InboxState,
    pub handled_at_ms: Option<u64>,
    pub outcome: Option<String>,
    pub clears: InboxClears,
    pub dismissable: bool,
    pub actions: Vec<Action>,
}

impl InboxItem {
    pub fn bound_text(&mut self) {
        self.summary = bounded(&self.summary, 512);
        self.context = self.context.as_ref().map(|s| bounded(s, 2048));
        self.recommendation = self.recommendation.as_ref().map(|s| bounded(s, 1024));
        if let Some(options) = &mut self.options {
            options.truncate(8);
            for option in options {
                *option = bounded(option, 128);
            }
        }
    }
}

fn bounded(text: &str, max: usize) -> String {
    if crate::msg::json_string_len(text) <= max {
        return text.to_owned();
    }
    let mut result = String::new();
    let mut used = 5; // JSON quotes plus the UTF-8 ellipsis; every field cap is at least 128.
    for c in text.chars() {
        let cost = crate::msg::json_string_len(c.encode_utf8(&mut [0; 4])) - 2;
        if used + cost > max {
            break;
        }
        result.push(c);
        used += cost;
    }
    result.push('…');
    result
}

/// Unfiltered open totals, independent of source item caps. `total` includes future kinds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxCounts {
    pub approvals: u64,
    pub questions: u64,
    pub decisions: u64,
    pub problems: u64,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxPosition {
    pub epoch: String,
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxListParams {
    pub state: InboxState,
    pub kinds: Option<Vec<InboxKind>>,
    pub project: Option<String>,
    pub team: Option<String>,
    /// Opaque: echo it unchanged. Sources receive the decoded keyset before their cap.
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxListResult {
    pub items: Vec<InboxItem>,
    pub next_cursor: Option<String>,
    pub counts: InboxCounts,
    pub as_of: InboxPosition,
    pub degraded: Vec<String>,
}

pub struct InboxList;
impl Call for InboxList {
    const METHOD: &'static str = "inbox.list";
    type Params = InboxListParams;
    type Result = InboxListResult;
}

/// Shared list/hint payload types; a1 defines the wire shape, a2 serves the hint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxChangedData {
    pub counts: InboxCounts,
    pub as_of: InboxPosition,
    pub degraded: Vec<String>,
}
pub struct InboxChanged;
impl EventKind for InboxChanged {
    const NAME: &'static str = "inbox.changed";
    type Data = InboxChangedData;
}

#[cfg(test)]
mod tests;
