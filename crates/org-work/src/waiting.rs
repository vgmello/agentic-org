//! The office's waiting bookkeeping (§2.3, §6): `WaitingStarted` / `WaitingEnded` system events
//! (no actor, not automatic, `cause` = the triggering event; their only aspect is `waiting`, so
//! they never make a caller's `expected_version` stale and are never undone). One span per
//! `(member, reason)`; the time figures count overlapping spans once.
//!
//! Phase 1 writes one reason, a question to the owner (`work.comment { question, asks: owner }`),
//! closed by its first answer. A round's terminal transition ends every open span, and a reopen
//! opens again the spans whose reasons still hold, so a span is never opened twice.

use org_proto::work::{CommentKind, EventRef, WaitReason, WorkEventKind};

use crate::engine::{Draft, Origin};
use crate::model::OpenWait;
use crate::states::is_terminal;

fn in_round(draft: &Draft) -> bool {
    !is_terminal(draft.item.kind, draft.item.status)
}

fn question(comment_id: &str) -> WaitReason {
    WaitReason::Question {
        comment_id: comment_id.to_owned(),
    }
}

/// Opens a span, unless the item is outside a round or the span is already open.
fn start(draft: &mut Draft, on_member_id: &str, reason: WaitReason, cause: EventRef) {
    let open = OpenWait {
        on_member_id: on_member_id.to_owned(),
        reason,
    };
    if !in_round(draft) || draft.item.waiting.contains(&open) {
        return;
    }
    draft.item.waiting.push(open.clone());
    draft.record(
        WorkEventKind::WaitingStarted {
            on_member_id: open.on_member_id,
            reason: open.reason,
        },
        Origin::System(cause),
    );
}

/// Ends a span if it is open.
fn end(draft: &mut Draft, open: &OpenWait, cause: EventRef) {
    let before = draft.item.waiting.len();
    draft.item.waiting.retain(|w| w != open);
    if draft.item.waiting.len() == before {
        return;
    }
    draft.record(
        WorkEventKind::WaitingEnded {
            on_member_id: open.on_member_id.clone(),
            reason: open.reason.clone(),
        },
        Origin::System(cause),
    );
}

/// A question to the owner was asked (its comment is in `draft`).
pub(crate) fn question_asked(draft: &mut Draft, owner_id: &str, comment_id: &str, cause: EventRef) {
    start(draft, owner_id, question(comment_id), cause);
}

/// A question to the owner got its first answer.
pub(crate) fn question_answered(
    draft: &mut Draft,
    owner_id: &str,
    comment_id: &str,
    cause: EventRef,
) {
    let open = OpenWait {
        on_member_id: owner_id.to_owned(),
        reason: question(comment_id),
    };
    end(draft, &open, cause);
}

/// A move into a terminal status: every open span ends with the round.
pub(crate) fn round_closed(draft: &mut Draft, cause: &EventRef) {
    for open in draft.item.waiting.clone() {
        end(draft, &open, cause.clone());
    }
}

/// A reopen: opens a span again for each reason that still holds (every unanswered question to
/// the owner).
pub(crate) fn round_reopened(draft: &mut Draft, owner_id: &str, cause: &EventRef) {
    let open: Vec<String> = draft
        .item
        .comments
        .iter()
        .filter(|c| {
            c.kind == CommentKind::Question && !c.answered && c.asks.as_deref() == Some(owner_id)
        })
        .map(|c| c.id.clone())
        .collect();
    for comment_id in open {
        start(draft, owner_id, question(&comment_id), cause.clone());
    }
}
