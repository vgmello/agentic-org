//! Work events (an item's activity feed) and the `work.changed` event.

use serde::{Deserialize, Serialize};

use super::*;
use crate::events::EventKind;
use crate::org::Position;

/// A reference to one event: the item (or feature) and the version the event made.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventRef {
    pub target_id: String,
    pub version: u64,
}

/// One field a [`WorkEventKind::FieldsChanged`] changed, with its values before and after as
/// JSON (absent: no value).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldChange {
    pub field: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<serde_json::Value>,
}

/// One event: a committed change to one item or feature. Events are immutable; together they
/// are the item's activity feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkEvent {
    pub target_id: String,
    /// The version this event made.
    pub version: u64,
    pub at: String,
    /// The bound member that made the change. Absent: an automatic change, a system cascade or
    /// office bookkeeping (`WaitingStarted` / `WaitingEnded`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    /// An automatic change (it can be undone with `work.undo` for an hour).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto: bool,
    /// The triggering event (of an automatic change, or of office bookkeeping when there is
    /// one) or the undone one (of an `Undone`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<EventRef>,
    /// The organization position the change was checked at.
    pub org_position: Position,
    pub change: WorkEventKind,
}

/// What an event changed. Tagged `type`; an unknown kind decodes as `Unknown` (show
/// "changed"). Later phases add kinds (a bug's triage, verification and reopening, working
/// time, debt status, handovers).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum WorkEventKind {
    /// The item was created. It carries no fields: the item's state comes with the change as
    /// an [`WorkChange::ItemUpserted`].
    Created,
    FieldsChanged {
        fields: Vec<FieldChange>,
    },
    StatusChanged {
        from: ItemStatus,
        to: ItemStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolution: Option<Resolution>,
        /// The blocked reason, or another reason the move needed: free text, never an id (except
        /// in events recorded before `duplicate_of` existed, below). For a bug (phase 2): the
        /// note of `bug.mark_fixed`, or the reason of a won't fix or can't reproduce close.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        /// Done (or Fixed) with required steps left.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anyway: Option<DoneAnyway>,
        /// A bug closed as a duplicate (`resolution: duplicate`, phase 2): the id of the
        /// original bug. Absent on every other status change. Added 2026-11-08 (P2): an event
        /// recorded before it has the original's id in `reason` instead.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duplicate_of: Option<String>,
    },
    Assigned {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<String>,
    },
    /// A bug went from `new` to `triaged` (`bug.triage`): the area and severity are confirmed,
    /// the priority set, and the bug assigned when `assignee_id` is given (phase 2).
    Triaged {
        area_id: String,
        severity: Severity,
        priority: Priority,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        assignee_id: Option<String>,
    },
    /// A fixed bug was verified (`bug.verify`): `fixed` closes it as verified; `still_happens`
    /// sends it back to `triaged` with one more reopening (phase 2). `by_reporter` says
    /// whether the actor was the bug's reporter.
    Verified {
        outcome: VerifyOutcome,
        by_reporter: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// A closed bug was reopened (`bug.reopen`): to `triaged`, or to `new` when it had been
    /// closed as a duplicate, which also clears the bug's `duplicate_of` (phase 2). The item's
    /// new state, sent with the event as a [`WorkChange::ItemUpserted`], has it cleared.
    Reopened {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// A request's scoper changed, with its status, in one event. Recorded by `request.route`:
    /// a route or re-route (`scoper_to` the new scoper, `to: routed`) or an unroute
    /// (`scoper_to` absent, `to: new`); and by `request.accept` (`scoper_to` the caller,
    /// `to: scoping`). `scoper_from` is the scoper before (absent: none), `reason` the
    /// `route_reason`. Added 2026-11-09 (phase 3).
    Routed {
        from: ItemStatus,
        to: ItemStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scoper_from: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scoper_to: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        routed_by: Option<RoutedBy>,
    },
    Linked {
        link: Link,
    },
    Unlinked {
        link_id: String,
    },
    Comment {
        comment_id: String,
        body: String,
        comment_kind: CommentKind,
        /// A question addressed to this member.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        asks: Option<String>,
        /// The question (its `comment_id`) this answers.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        answers: Option<String>,
        /// The commenter also started watching the item (`work.comment { watch }`).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        watch: bool,
    },
    StepsAdded {
        steps: Vec<ChecklistStep>,
        /// Absent: at the end.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after_step_id: Option<String>,
    },
    StepEdited {
        step_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        required: Option<bool>,
    },
    StepChecked {
        step_id: String,
        done: bool,
    },
    StepMoved {
        step_id: String,
        /// Absent: to the top.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after_step_id: Option<String>,
    },
    StepRemoved {
        step_id: String,
    },
    StepMadeTask {
        step_id: String,
        task_id: String,
    },
    /// The step's task link ended (`checklist.unlink_task`, or its task was deleted): it is a
    /// plain step again.
    StepTaskUnlinked {
        step_id: String,
        task_id: String,
    },
    /// A span of waiting on a member (the owner) started, for one reason (§6). Office
    /// bookkeeping (§2.3), written in the triggering call's transaction (`work.comment`,
    /// `msg.ask`, `work.link` from phase 4, …) or by a cascade: a system event (`actor_id`
    /// absent, `auto` false, `cause` the triggering item event if there is one) whose only
    /// aspect, `waiting`, is one no call writes, so it never makes an `expected_version` stale
    /// and is never undone.
    WaitingStarted {
        on_member_id: String,
        reason: WaitReason,
    },
    /// The span keyed by (`on_member_id`, `reason`) ended. Bookkeeping, as `WaitingStarted`.
    WaitingEnded {
        on_member_id: String,
        reason: WaitReason,
    },
    Watched {
        member_id: String,
        watching: bool,
    },
    /// An automatic event was undone (`cause` names it); this restores the earlier values.
    Undone {
        event_version: u64,
    },
    /// The item became a tombstone.
    Deleted,
    #[serde(other)]
    Unknown,
}

/// Why an item waits on the owner (§6): the reason of a [`WorkEventKind::WaitingStarted`] or
/// [`WorkEventKind::WaitingEnded`]. A span is keyed by (`on_member_id`, `reason`), so
/// overlapping reasons are counted once. On the wire `{"type": "question", "comment_id": …}`,
/// `{"type": "message", "message_id": …}`, `{"type": "triage"}` or `{"type": "decision",
/// "doc_key": …}`; an unknown `type` decodes as `Unknown`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum WaitReason {
    /// A question to the owner (`work.comment { question, asks: owner }`); its answer ends it.
    Question { comment_id: String },
    /// An agent's `msg.ask` or `msg.send` to the owner carrying `meta.work_item` = the item;
    /// the owner's reply, or marking it done, ends it.
    Message { message_id: String },
    /// A `new` bug whose suggested triager is the owner (phase 2).
    Triage,
    /// The item is linked (`relates_to`) to, or named in the front matter `related` of, an
    /// ADR in `proposed` (phase 4); `doc_key` is the ADR's.
    Decision { doc_key: String },
    #[serde(other)]
    Unknown,
}

/// One change in a [`WorkChangedData`]. Tagged `type`; a kind this build doesn't know decodes
/// as `Unknown`: take a fresh `work.snapshot`. Phase 3 adds `feature_upserted { feature }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum WorkChange {
    /// An item, whole, after the change. Apply it only if it is newer than the copy held
    /// (`(generation, version)`).
    ItemUpserted { item: WorkItemSummary },
    /// An event, for live activity.
    EventAdded { event: WorkEvent },
    /// Phase 2: HQ numbered an item created elsewhere. Not an item version: it never changes
    /// the item's version or generation, and a later version never clears it.
    ItemNumbered { item_id: String, number: u64 },
    #[serde(other)]
    Unknown,
}

/// The data of a `work.changed` event: the changes one committed call made (its automatic
/// changes and cascades included), at `position`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkChangedData {
    pub position: WorkPosition,
    /// The write that made these changes. Absent for changes no client call made (a cascade).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_id: Option<String>,
    /// This event's part, from 1, of `parts`: a call whose changes don't fit in
    /// [`WORK_CHANGED_MAX_BYTES`] is split. Advance the saved position only after the last.
    pub part: u32,
    pub parts: u32,
    pub changes: Vec<WorkChange>,
    /// Who made the call (X-02 §4.2). Absent for a cascade, and from servers that predate it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<crate::principal::Actor>,
}

/// `work.changed`: sent to subscribers for every committed work call.
pub struct WorkChanged;

impl EventKind for WorkChanged {
    const NAME: &'static str = "work.changed";
    type Data = WorkChangedData;
}
