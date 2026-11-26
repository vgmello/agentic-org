//! Work items: tasks (and later bugs and feature requests), their checklists, links, comments
//! and events (see `docs/design/tasks-projects-docs.md` §1, §2 and §4).
//!
//! Work items are **office data**, office-first: the assignee's office owns each one, and every
//! change is an immutable [`WorkEvent`], which is also the item's activity feed and the only
//! source of its [`TimeFigures`]. In phase 1 there is one office (HQ), which owns every item.
//! Projects and areas are organization data: see [`crate::org`].
//!
//! This module holds phase 1's calls (the reads, the task calls, the shared item calls and the
//! checklist calls) and phase 2's bug calls (`bug.*`). Requests, features and docs add their
//! calls later, additively. The
//! record types ([`WorkItemSummary`], [`WorkItem`], [`Feature`], …) already carry every kind's
//! fields, so those phases add calls and event kinds, not fields.
//!
//! ## Conventions
//! - Calls go to the client's **own office**, on a **bound connection** (ADR-0003 `msg.bind`):
//!   an agent bound as itself, or the app bound as the owner or a person. An unbound connection
//!   gets `not_bound`. **The actor is always the bound member**: no call carries an actor,
//!   reporter or `created_by`.
//! - **No permissions in phase 1**: any bound member may make any call here, except the
//!   **app-only** ones, which any other connection gets `not_authorized` for, before any check
//!   but binding. In this module: `work.delete`. (Every organization command is app-only too.)
//! - Every write's params start with **`change_id`**, a client-made ULID (26 **uppercase**
//!   Crockford base32 characters, the first `0` to `7`; `invalid_request` otherwise), and its
//!   result is [`WorkAccepted`]. The owning office keeps `change_id → (params hash, result)` for
//!   30 days: a retry with the same `change_id` and params gets the stored answer (refusals
//!   included, except `storage_error`, `unavailable`, `not_bound`, `not_authorized`,
//!   `too_large`, a `change_id` that isn't a ULID, `expired` and `clock_skew`, which are never
//!   stored); the same `change_id` with other params is `invalid_request` ("change id
//!   reused"). A `change_id` dated more than 29 days ago is `expired`, more than 5 minutes
//!   ahead of the office's clock `clock_skew`.
//! - Client-made ids (`item_id`, `step_id`, `link_id`, `comment_id`, `task_id`, …) are ULIDs
//!   (`invalid_request` otherwise) not already used (`already_exists`).
//! - **`expected_version`** is required on every call that changes an existing item, except
//!   the commutative ones: `checklist.add`, `checklist.check`, `checklist.move`,
//!   `checklist.make_task`, `checklist.unlink_task`, `work.comment`, `work.watch`, `work.link`
//!   and `work.unlink`. A value above the current version is `invalid_request`. **Conflicts are
//!   per field**: a call is refused with `conflict` only when an event after `expected_version`
//!   changed a field the call writes (or the status, when it changes the status). So a comment
//!   or a tick never makes a title edit stale. A checklist step has its own `version` and three
//!   aspects (content, done, position); `checklist.edit` and `checklist.remove` conflict only
//!   with a content change (or, for `remove`, the step having been made a task).
//! - **One call, one event per item it changes.** Automatic changes (a made-task step ticking
//!   itself) are separate events, marked `auto`, in the same commit; `work.undo` reverts one
//!   within an hour.
//! - Item references: `item_id` where the app writes; `item` (an id, a qualified number such
//!   as `chk-T-12` or `REQ-14`, or a short id) where an agent may type it. A short id that
//!   matches more than one item is `invalid_request` ("ambiguous").
//! - Times are RFC 3339 UTC with milliseconds (`2026-11-08T02:10:00.000Z`), from the owning
//!   office's clock, never earlier than the item's previous event.
//! - Enum values this build doesn't know decode as `Unknown`, in any shape; tagged types
//!   ([`WorkEventKind`], [`WorkChange`], [`LinkTarget`], …) decode an unknown `type` as
//!   `Unknown`, so one new value never fails a whole message. Writers never send `Unknown`
//!   (`invalid_request`).
//!
//! ## When several errors apply
//! The first that applies: `not_bound`, `not_authorized`, `expired`, `clock_skew`, `not_found`,
//! `too_large`, `invalid_request` (the params alone: shape, limits, references),
//! `invalid_name`, `unknown_recipient`, `not_a_member`, `owner_protected`, `conflict`,
//! `invalid_request` (the current state doesn't allow it), `checklist_incomplete`,
//! `already_exists`, then `unavailable` and `storage_error`. Stale beats wrong-state on purpose
//! ("someone changed it a moment ago" is the useful answer); the organization commands keep
//! their own order (see [`crate::org`]).
//!
//! ## Sizes
//! Every request and response is one frame ([`crate::MAX_FRAME_BYTES`]).
//! - A write whose encoded params exceed [`WORK_PARAMS_MAX_BYTES`] is refused with `too_large`.
//! - Paged reads (`work.snapshot`, `work.list`, `work.activity`) treat `limit` as a maximum
//!   and stop early so the encoded response stays under [`WORK_RESPONSE_MAX_BYTES`], returning
//!   a cursor to go on from.
//! - `work.get` returns at most [`WORK_GET_MAX_LISTED`] backlinks and as many watchers, with
//!   `backlinks_more` / `watchers_more`. `work.list { linked_to }` reaches the rest of the
//!   backlinks; phase 1 has no call that lists the rest of the watchers.
//! - A `work.changed` event is at most [`WORK_CHANGED_MAX_BYTES`]: a call whose changes don't
//!   fit is sent as several events with the same `change_id` and position, numbered by `part`
//!   and `parts`.
//!
//! ## Snapshot, then subscribe
//! `work.snapshot` pages are read independently, each at its own [`WorkPosition`]. The client
//! subscribes with `subscribe { from_work_position }` from the **first** page's position (see
//! [`crate::SubscribeParams`]) and applies a change only if it is newer than what it holds:
//! newer is `(generation, version)`, compared in that order. A client advances its saved
//! position only after the last part of a `work.changed`. Deleted items are left out of the
//! snapshot; their deletion arrives as a change.

mod events;
mod items;
mod reads;
mod writes;

pub use events::{
    EventRef, FieldChange, WaitReason, WorkChange, WorkChanged, WorkChangedData, WorkEvent,
    WorkEventKind,
};
pub use items::{
    ChecklistCounts, ChecklistStep, DocRef, DocWritten, DoneAnyway, DraftTask, Estimate, Feature,
    FeatureOutcome, Link, LinkTarget, NewStep, RequestOutcome, Shortcut, TimeFigures, WorkAccepted,
    WorkItem, WorkItemSummary, WorkPosition,
};
pub use reads::{
    ItemTime, WorkActivity, WorkActivityParams, WorkActivityResult, WorkGet, WorkGetParams,
    WorkGetResult, WorkList, WorkListParams, WorkListResult, WorkSnapshot, WorkSnapshotParams,
    WorkSnapshotResult, WorkTimes, WorkTimesParams, WorkTimesResult,
};
pub use writes::{
    BugAssign, BugAssignParams, BugClose, BugCloseParams, BugMarkFixed, BugMarkFixedParams,
    BugReopen, BugReopenParams, BugReport, BugReportParams, BugReportedFrom, BugTriage,
    BugTriageParams, BugUpdate, BugUpdateParams, BugVerify, BugVerifyParams, ChecklistAdd,
    ChecklistAddParams, ChecklistCheck, ChecklistCheckParams, ChecklistEdit, ChecklistEditParams,
    ChecklistMakeTask, ChecklistMakeTaskParams, ChecklistMove, ChecklistMoveParams,
    ChecklistRemove, ChecklistRemoveParams, ChecklistUnlinkTask, ChecklistUnlinkTaskParams,
    NewFixLink, TaskAssign, TaskAssignParams, TaskCreate, TaskCreateParams, TaskSetStatus,
    TaskSetStatusParams, TaskUpdate, TaskUpdateParams, WorkComment, WorkCommentParams, WorkDelete,
    WorkDeleteParams, WorkLink, WorkLinkParams, WorkSetCurrent, WorkSetCurrentParams, WorkUndo,
    WorkUndoParams, WorkUnlink, WorkUnlinkParams, WorkWatch, WorkWatchParams,
};

use serde::{Deserialize, Serialize};

/// The longest item title, in characters (one line).
pub const WORK_TITLE_MAX_CHARS: usize = 120;
/// The longest general text field (a comment body, a bug's `what_happened`, `expected` and
/// `steps`), in characters.
pub const WORK_TEXT_MAX_CHARS: usize = 10_000;
/// The longest task description (Markdown), in characters.
pub const WORK_DESCRIPTION_MAX_CHARS: usize = 20_000;
/// The longest request text, in characters.
pub const WORK_REQUEST_TEXT_MAX_CHARS: usize = 20_000;
/// The longest reason (done anyway, a bug's close reason, a decline), in characters.
pub const WORK_REASON_MAX_CHARS: usize = 1_000;
/// The longest `blocked_reason`, in characters (one line).
pub const WORK_BLOCKED_REASON_MAX_CHARS: usize = 200;
/// The longest `definition_of_done`, in characters.
pub const WORK_DOD_MAX_CHARS: usize = 1_000;
/// The longest bug `environment`, in characters.
pub const WORK_ENVIRONMENT_MAX_CHARS: usize = 2_000;
/// The longest checklist step text, in characters (one line).
pub const WORK_STEP_TEXT_MAX_CHARS: usize = 200;
/// The most checklist steps one item may have.
pub const WORK_MAX_STEPS: usize = 100;
/// The most links one item may have.
pub const WORK_MAX_LINKS: usize = 100;
/// The most draft tasks a request's outcome may have.
pub const WORK_MAX_DRAFT_TASKS: usize = 50;
/// The largest `limit` of a paged read, and the most ids `work.times` takes.
pub const WORK_PAGE_MAX: u32 = 500;
/// The largest encoded params a work write may have (`too_large` otherwise).
pub const WORK_PARAMS_MAX_BYTES: usize = 768 * 1024;
/// The size a paged read's encoded response stays under (it stops early and returns a cursor).
pub const WORK_RESPONSE_MAX_BYTES: usize = 768 * 1024;
/// The largest encoded `work.changed` event; a call's changes beyond it are split into parts.
pub const WORK_CHANGED_MAX_BYTES: usize = 512 * 1024;
/// The most backlinks, and the most watchers, `work.get` returns for one item.
pub const WORK_GET_MAX_LISTED: usize = 200;

/// A string enum that never fails to decode: an unknown value, in any shape, is `Unknown`.
/// Like `org`'s `AgentStatus`, but one wire name per variant for both directions.
macro_rules! wire_enum {
    (
        $(#[$doc:meta])*
        $name:ident { $($(#[$vdoc:meta])* $variant:ident = $wire:literal),* $(,)? }
    ) => {
        $(#[$doc])*
        ///
        /// A value this build doesn't know, in any shape, decodes as `Unknown`; writers never
        /// send `Unknown` (`invalid_request`).
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize)]
        #[non_exhaustive]
        pub enum $name {
            $($(#[$vdoc])* #[serde(rename = $wire)] $variant,)*
            /// Missing or not known to this build.
            #[default]
            #[serde(rename = "unknown")]
            Unknown,
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                // Read the shape first, then decide: this never fails on a well-formed document.
                let value = serde_json::Value::deserialize(d)?;
                Ok(match value.as_str() {
                    $(Some($wire) => $name::$variant,)*
                    _ => $name::Unknown,
                })
            }
        }
    };
}

wire_enum!(
    /// What happened to a work write.
    WorkStatus {
        /// The owning office committed it; [`WorkAccepted::position`] says where.
        Committed = "committed",
        /// Phase 2: another office owns the item, so the call was sent to it as a change
        /// request (whether or not that office is reachable now). The outcome arrives later,
        /// matched by `change_id`. It doesn't mean "offline".
        Queued = "queued",
    }
);

wire_enum!(
    /// The kind of a work item. Fixed at create.
    ItemKind {
        Task = "task",
        Bug = "bug",
        Request = "request",
    }
);

wire_enum!(
    /// A work item's status. The values valid for each kind, and the moves between them, are
    /// the design's state machines (§1.10): a task is `todo`, `in_progress`, `blocked` or
    /// `done`; a bug `new`, `triaged`, `in_progress`, `fixed` or `closed`; a request `new`,
    /// `routed`, `scoping`, `scoped`, `approved`, `declined` or `merged`. A move the current
    /// status doesn't allow is `invalid_request` ("wrong state").
    ItemStatus {
        Todo = "todo",
        InProgress = "in_progress",
        Blocked = "blocked",
        Done = "done",
        New = "new",
        Triaged = "triaged",
        Fixed = "fixed",
        Closed = "closed",
        Routed = "routed",
        Scoping = "scoping",
        Scoped = "scoped",
        Approved = "approved",
        Declined = "declined",
        Merged = "merged",
    }
);

wire_enum!(
    /// How a terminal status ended: a task's `done` is `completed` (the default) or
    /// `cancelled`; a bug's `closed` is `verified`, `duplicate`, `wont_fix` or
    /// `cant_reproduce`. Set exactly when the status is terminal.
    Resolution {
        Completed = "completed",
        Cancelled = "cancelled",
        Verified = "verified",
        Duplicate = "duplicate",
        WontFix = "wont_fix",
        CantReproduce = "cant_reproduce",
    }
);

wire_enum!(
    /// A task's or bug's priority. Absent at create, the server uses `normal`
    /// (`Priority::default()` is `Unknown`, for decoding).
    Priority {
        Urgent = "urgent",
        High = "high",
        Normal = "normal",
        Low = "low",
    }
);

wire_enum!(
    /// A bug's severity: required at report, confirmed at triage.
    Severity {
        Critical = "critical",
        Major = "major",
        Minor = "minor",
        Trivial = "trivial",
    }
);

wire_enum!(
    /// What a feature request asks for: suggested, changeable.
    RequestKind {
        Feature = "feature",
        Change = "change",
        Technical = "technical",
        Question = "question",
    }
);

wire_enum!(
    /// Where a request's routing suggestion came from.
    RoutedBy {
        Person = "person",
        Rule = "rule",
        Copilot = "copilot",
    }
);

wire_enum!(
    /// What a link means (§1.7). `fixes`: task → bug, same project. `part_of`: a task made
    /// from a checklist step → the step's item. `from_request`: task → request, made only by
    /// approval. `reported_from`: bug → task or doc. `relates_to`: any item → item or doc.
    /// `pays_back`: task → a debt doc.
    LinkKind {
        Fixes = "fixes",
        PartOf = "part_of",
        FromRequest = "from_request",
        ReportedFrom = "reported_from",
        RelatesTo = "relates_to",
        PaysBack = "pays_back",
    }
);

wire_enum!(
    /// A feature record's status.
    FeatureStatus {
        Proposed = "proposed",
        Planned = "planned",
        InProgress = "in_progress",
        Delivered = "delivered",
        Retired = "retired",
    }
);

wire_enum!(
    /// What a comment is. A `question` may be addressed to a member (`asks`); an `answer`
    /// names the question it answers.
    CommentKind {
        Note = "note",
        Question = "question",
        Answer = "answer",
    }
);

wire_enum!(
    /// What a bug's verification found (`bug.verify`): the fix works (`fixed`, the bug is
    /// closed as verified), or the bug still happens (`still_happens`, it goes back to
    /// `triaged`).
    VerifyOutcome {
        Fixed = "fixed",
        StillHappens = "still_happens",
    }
);

#[cfg(test)]
mod tests;
