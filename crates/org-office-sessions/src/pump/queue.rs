//! Queued turn payload and attachment-ID coalescing for the session pump (`TASK-98.3`).
//!
//! Leaf submodule inside `pump/`: holds only lightweight attachment identifiers (never raw bytes
//! or base64 data) and imports zero sibling `pump` modules.

use std::collections::HashSet;

/// One turn queued on the live pump channel or waiting in `PumpLoopState::pending_turns`.
///
/// Carries only lightweight `attachment_ids` (never bytes or base64), so durable failover,
/// warm-handle recycling, and history records stay compact and adapter-agnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QueuedTurn {
    pub text: String,
    pub attachment_ids: Vec<String>,
    /// Set on a pre-side-effect provider image-rejection retry so the follow-up turn degrades
    /// fail-soft to staged-path text delivery (`TASK-98.3` AC #4).
    pub force_text_only: bool,
}

impl QueuedTurn {
    pub(crate) fn text_only(text: String) -> Self {
        Self {
            text,
            attachment_ids: Vec::new(),
            force_text_only: false,
        }
    }

    pub(crate) fn new(text: String, attachment_ids: Vec<String>) -> Self {
        Self {
            text,
            attachment_ids: dedup_attachment_ids(attachment_ids),
            force_text_only: false,
        }
    }
}

/// Deduplicates non-empty trimmed attachment identifiers while preserving first-seen order.
pub(crate) fn dedup_attachment_ids(ids: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for raw in ids {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let owned = trimmed.to_owned();
        if seen.insert(owned.clone()) {
            out.push(owned);
        }
    }
    out
}

/// Coalesces multiple queued turns into one combined text objective and deduplicated attachment
/// ID list (used on stale warm recycle and pre-side-effect pool failover).
pub(crate) fn merge_queued_turns(turns: Vec<QueuedTurn>) -> (String, Vec<String>) {
    let mut texts = Vec::with_capacity(turns.len());
    let mut all_ids = Vec::new();
    for turn in turns {
        if !turn.text.is_empty() {
            texts.push(turn.text);
        }
        all_ids.extend(turn.attachment_ids);
    }
    (texts.join("\n\n"), dedup_attachment_ids(all_ids))
}
