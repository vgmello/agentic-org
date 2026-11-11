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

use serde::{Deserialize, Serialize};

use crate::calls::{Call, Empty};
use crate::events::EventKind;
use crate::org::Position;

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

/// A position in this office's work feed. `incarnation` changes if the office's work data is
/// restored or re-created: a client that sees another incarnation takes a fresh snapshot.
/// Positions compare only within one incarnation, by `seq`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkPosition {
    pub incarnation: u64,
    pub seq: u64,
}

/// A doc commit a work call made before answering (phase 3: `feature.create`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocWritten {
    pub project_id: String,
    pub path: String,
    pub branch: String,
    pub commit: String,
}

/// The answer to every work write. An error response means the owning office refused it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkAccepted {
    pub status: WorkStatus,
    /// Set when committed: the client feed position of this change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<WorkPosition>,
    /// Every item this call changed, whole, after the change: the item itself, items it
    /// created (`checklist.make_task`) and automatic changes. Empty when queued.
    #[serde(default)]
    pub items: Vec<WorkItemSummary>,
    /// Every feature record the call changed (phase 3).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<Feature>,
    /// Doc commits the call made before answering (phase 3).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub docs: Vec<DocWritten>,
}

/// Checklist progress, for lists and boards.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecklistCounts {
    #[serde(default)]
    pub total: u32,
    #[serde(default)]
    pub done: u32,
    /// Required steps not ticked: they guard Done (and a bug's Fixed).
    #[serde(default)]
    pub required_left: u32,
}

/// One work item as lists, boards and live updates need it: no long text. Fields that belong
/// to one kind are empty on the others.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkItemSummary {
    /// Client-made ULID; never changes. The only key writes use.
    pub id: String,
    pub kind: ItemKind,
    /// Required for tasks and bugs; optional for requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// Assigned by HQ (`T-12`, `B-7` per project; `REQ-14` per organization). Empty until
    /// then: the item shows "Pending · `short_id`". Always set in phase 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,
    /// The id's last 6 characters, lowercase: for display and typing.
    pub short_id: String,
    pub title: String,
    pub status: ItemStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<Resolution>,
    /// Tasks and bugs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    /// Tasks and bugs: who does it. Requests: who scopes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee_id: Option<String>,
    /// The bound member that created it (a bug's reporter, who raised a request).
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
    /// Tasks: one of the project's teams.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    /// Tasks: the feature it delivers (phase 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_id: Option<String>,
    /// Tasks: set exactly when the status is `blocked`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    /// Bugs (phase 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area_id: Option<String>,
    /// Bugs (phase 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
    /// Bugs: false until triage.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub severity_confirmed: bool,
    /// Bugs: how many times it was reopened or sent back.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub reopen_count: u32,
    /// Bugs: set exactly when closed as a duplicate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_of: Option<String>,
    /// Requests (phase 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_kind: Option<RequestKind>,
    /// A task: the bugs it fixes (phase 2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<String>,
    /// A bug: its fix tasks (phase 2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixed_by: Vec<String>,
    #[serde(default)]
    pub checklist: ChecklistCounts,
    /// Done (or Fixed) with required steps unticked, for a recorded reason.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub done_anyway: bool,
    /// The office that owns the item (HQ in phase 1).
    pub owner_office_id: String,
    /// Starts at 1; goes up on a handover or reclaim (phase 2).
    pub generation: u64,
    /// Starts at 1 (the create); +1 for each committed change; equals the last event's version.
    pub version: u64,
    /// A tombstone (`work.delete`): left out of reads unless asked for.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
}

// Summaries fill pages of up to `WORK_PAGE_MAX`, so they leave out what is empty.
fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// One item in full (`work.get`). Fields that belong to one kind are empty on the others.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkItem {
    pub summary: WorkItemSummary,
    /// More than [`WORK_GET_MAX_LISTED`] backlinks: `work.list { linked_to }` reaches the rest.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub backlinks_more: bool,
    /// More than [`WORK_GET_MAX_LISTED`] watchers.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub watchers_more: bool,
    /// Task: Markdown, at most [`WORK_DESCRIPTION_MAX_CHARS`] characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Task: the goal the agent works to, at most [`WORK_DOD_MAX_CHARS`] characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_of_done: Option<String>,
    /// Task: the branch the office made for its worktree. Read-only to clients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Task: why it was marked Done with required steps unticked, and which.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_anyway_detail: Option<DoneAnyway>,
    /// Task: the answer to "did you take a shortcut?" (phase 4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<Shortcut>,
    /// Bug (phase 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub what_happened: Option<String>,
    /// Bug (phase 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    /// Bug (phase 2): steps to reproduce.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<String>,
    /// Bug (phase 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    /// Bug (phase 2): the won't-fix or can't-reproduce reason, or what fixed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_reason: Option<String>,
    /// Bug (phase 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affects_feature_id: Option<String>,
    /// Request (phase 3): the original words, never edited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Request (phase 3): why this scoper.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_reason: Option<String>,
    /// Request (phase 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routed_by: Option<RoutedBy>,
    /// Request (phase 3): set when scoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<RequestOutcome>,
    /// Request (phase 3): set exactly when `merged`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_into: Option<String>,
    /// Request (phase 3): set exactly when `declined`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decline_reason: Option<String>,
    /// Tasks and bugs, in order.
    #[serde(default)]
    pub checklist: Vec<ChecklistStep>,
    /// Links from this item.
    #[serde(default)]
    pub links: Vec<Link>,
    /// Links to this item (at most [`WORK_GET_MAX_LISTED`]).
    #[serde(default)]
    pub backlinks: Vec<Link>,
    /// Member ids (at most [`WORK_GET_MAX_LISTED`]). The creator always watches.
    #[serde(default)]
    pub watchers: Vec<String>,
    #[serde(default)]
    pub time: TimeFigures,
    /// Bug, derived (phase 2): the area lead, else the area team's lead, else the owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_triager: Option<String>,
    /// Bug, derived (phase 2): the reporter if still an active member, else the triager.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_verifier: Option<String>,
}

/// Done (or Fixed) with required steps unticked: the recorded reason and the steps left.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoneAnyway {
    pub reason: String,
    /// The ids of the required steps that were unticked.
    #[serde(default)]
    pub steps: Vec<String>,
}

/// The answer to "did you take a shortcut?" when a task is done (phase 4). On the wire
/// `{"type": "none"}` or `{"type": "debt", "ids": ["DEBT-0012"]}`; an unknown `type` decodes
/// as `Unknown`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Shortcut {
    /// No shortcut taken.
    None,
    /// Shortcuts recorded as these debt docs.
    Debt { ids: Vec<String> },
    #[serde(other)]
    Unknown,
}

/// One checklist step. Tasks and bugs have checklists; requests don't.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecklistStep {
    /// Client-made ULID.
    pub id: String,
    /// 1 to [`WORK_STEP_TEXT_MAX_CHARS`] characters, one line.
    pub text: String,
    /// Required steps guard Done (and a bug's Fixed).
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub done: bool,
    /// Set when the step was made into a task: it then ticks when that task is Done
    /// (completed) and unticks when it's reopened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// From the latest check event; reads only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_by: Option<String>,
    /// From the latest check event; reads only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<String>,
    /// Counts the events that touched this step (edit, check, move, make-task).
    #[serde(default)]
    pub version: u64,
}

/// A new checklist step, as `task.create` and `checklist.add` take it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewStep {
    /// Client-made ULID.
    pub step_id: String,
    pub text: String,
    #[serde(default)]
    pub required: bool,
}

/// What a link points at. On the wire `{"type": "item", "item_id": …}` or `{"type": "doc",
/// "project_id": …, "doc_key": …}`; an unknown `type` decodes as `Unknown`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum LinkTarget {
    Item {
        item_id: String,
    },
    /// A doc in a project's repository: `doc_key` is its front-matter `id` when it has one
    /// (`ADR-0004`), else its path.
    Doc {
        project_id: String,
        doc_key: String,
    },
    #[serde(other)]
    Unknown,
}

/// A link from one item. It belongs to its `from` item, whose owner commits it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    /// Client-made ULID.
    pub id: String,
    pub from_item_id: String,
    pub kind: LinkKind,
    pub to: LinkTarget,
    pub created_by: String,
    pub created_at: String,
    /// The target item is deleted (reads show "deleted").
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub target_deleted: bool,
}

/// A scoped request's outcome (phase 3).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestOutcome {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature: Option<FeatureOutcome>,
    #[serde(default)]
    pub docs: Vec<DocRef>,
    /// At most [`WORK_MAX_DRAFT_TASKS`].
    #[serde(default)]
    pub draft_tasks: Vec<DraftTask>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimate: Option<Estimate>,
}

/// The feature a request's outcome makes or extends (phase 3). Tagged `type`; an unknown
/// `type` decodes as `Unknown`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum FeatureOutcome {
    New {
        feature_id: String,
    },
    Extends {
        feature_id: String,
    },
    #[serde(other)]
    Unknown,
}

/// A doc in a project, by key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocRef {
    pub project_id: String,
    pub doc_key: String,
}

/// A task a request's outcome proposes (phase 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftTask {
    pub draft_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
}

/// A request outcome's estimate (phase 3).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Estimate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd_cents_min: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd_cents_max: Option<u64>,
}

/// A feature record (phase 3): office data at HQ. The definition is a doc in the repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Feature {
    pub id: String,
    pub project_id: String,
    /// `FEAT-n`, from the doc.
    pub number: u32,
    pub path: String,
    pub title: String,
    pub status: FeatureStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced_by: Option<String>,
    #[serde(default)]
    pub tasks_total: u32,
    #[serde(default)]
    pub tasks_done: u32,
    pub version: u64,
}

/// An item's time figures, derived only from its events (§6). Durations in milliseconds.
/// `waiting_on_owner_ms + blocked_ms + rest_ms = lead_ms`. While sessions don't report working
/// time, `working_ms` and `idle_ms` are absent and `rest_ms` holds working and idle together:
/// show it as "the rest", never as idle (idle means nobody worked on it). When working time was
/// tracked for the item's whole lead time, `working_ms + idle_ms = rest_ms`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeFigures {
    /// The sum of each round's length; an open round runs to `as_of`.
    pub lead_ms: u64,
    /// From each round's first start to its end (or `as_of`). Absent when no round started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle_ms: Option<u64>,
    /// Present (with `idle_ms`) only when working time was tracked for the item's whole lead
    /// time (§6). Absent: "working time isn't tracked yet", never guessed. Always absent in
    /// phase 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_ms: Option<u64>,
    #[serde(default)]
    pub waiting_on_owner_ms: u64,
    #[serde(default)]
    pub blocked_ms: u64,
    /// Present only with `working_ms`: time nobody worked on it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_ms: Option<u64>,
    /// `lead_ms - waiting_on_owner_ms - blocked_ms`: working and idle together.
    #[serde(default)]
    pub rest_ms: u64,
    /// The last round is still open: the figures are "so far".
    #[serde(default)]
    pub open: bool,
    /// 1 + reopens.
    #[serde(default)]
    pub rounds: u32,
    pub as_of: String,
}

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
#[allow(clippy::large_enum_variant)] // short-lived wire values, as `OrgChange`
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

// ---- Reads ---------------------------------------------------------------------------------

/// `work.snapshot`: the office's items (and, phase 3, features), one page at a time, each page
/// at its own position. Subscribe from the first page's position. Open items always; closed
/// and done ones whose last terminal event is at or after `closed_since`, or all of them with
/// `all_closed`. Deleted items are left out. Errors: `not_found` (the project),
/// `invalid_request`.
pub struct WorkSnapshot;

/// Params of `work.snapshot`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkSnapshotParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_since: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all_closed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// At most [`WORK_PAGE_MAX`] (the default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Result of `work.snapshot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkSnapshotResult {
    /// The position this page was read at.
    pub position: WorkPosition,
    pub items: Vec<WorkItemSummary>,
    /// Phase 3.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<Feature>,
    /// Absent: this was the last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl Call for WorkSnapshot {
    const METHOD: &'static str = "work.snapshot";
    type Params = WorkSnapshotParams;
    type Result = WorkSnapshotResult;
}

/// `work.get`: one item in full. `item` is an id, a qualified number or a short id. Errors:
/// `not_found`, `invalid_request` (an ambiguous short id).
pub struct WorkGet;

/// Params of `work.get`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkGetParams {
    pub item: String,
}

/// Result of `work.get`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkGetResult {
    pub item: WorkItem,
}

impl Call for WorkGet {
    const METHOD: &'static str = "work.get";
    type Params = WorkGetParams;
    type Result = WorkGetResult;
}

/// `work.list`: items matching every filter given. Open items only unless `include_closed`.
/// Errors: `not_found`, `invalid_request`.
pub struct WorkList;

/// Params of `work.list`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<ItemKind>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub statuses: Vec<ItemStatus>,
    /// A member id, a handle, or `none` (unassigned).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_id: Option<String>,
    /// Items with a link to this item id (their backlinks to it), e.g. past
    /// [`WORK_GET_MAX_LISTED`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_to: Option<String>,
    /// With `linked_to`: only links of this kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_kind: Option<LinkKind>,
    /// Words in the title, at most 200 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub include_closed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// At most [`WORK_PAGE_MAX`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Result of `work.list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkListResult {
    pub items: Vec<WorkItemSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl Call for WorkList {
    const METHOD: &'static str = "work.list";
    type Params = WorkListParams;
    type Result = WorkListResult;
}

/// `work.times`: the time figures of up to [`WORK_PAGE_MAX`] items (list columns). Errors:
/// `invalid_request`.
pub struct WorkTimes;

/// Params of `work.times`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkTimesParams {
    pub item_ids: Vec<String>,
}

/// One item's time figures, in [`WorkTimesResult`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemTime {
    pub item_id: String,
    pub time: TimeFigures,
}

/// Result of `work.times`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkTimesResult {
    pub times: Vec<ItemTime>,
}

impl Call for WorkTimes {
    const METHOD: &'static str = "work.times";
    type Params = WorkTimesParams;
    type Result = WorkTimesResult;
}

/// `work.activity`: an item's (or feature's) events after `after_version`, oldest first.
/// Errors: `not_found`.
pub struct WorkActivity;

/// Params of `work.activity`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkActivityParams {
    pub target_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_version: Option<u64>,
    /// At most [`WORK_PAGE_MAX`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Result of `work.activity`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkActivityResult {
    pub events: Vec<WorkEvent>,
    /// Absent: no more events now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_after_version: Option<u64>,
}

impl Call for WorkActivity {
    const METHOD: &'static str = "work.activity";
    type Params = WorkActivityParams;
    type Result = WorkActivityResult;
}

// ---- Writes --------------------------------------------------------------------------------

macro_rules! write_call {
    ($(#[$doc:meta])* $call:ident, $method:literal, $params:ident { $($(#[$fattr:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        $(#[$doc])*
        pub struct $call;

        #[doc = concat!("Params of `", $method, "`.")]
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        pub struct $params {
            /// Client-made: an uppercase Crockford ULID, 26 characters. A retry with the same id
            /// and params gets the same answer; the same id with other params is
            /// `invalid_request`.
            pub change_id: String,
            $($(#[$fattr])* pub $field: $ty,)*
        }

        impl Call for $call {
            const METHOD: &'static str = $method;
            type Params = $params;
            type Result = WorkAccepted;
        }
    };
}

/// A `fixes` link `task.create` makes (phase 2): `link_id` is client-made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewFixLink {
    pub link_id: String,
    pub bug_id: String,
}

/// Where a bug was reported from (`bug.report { from }`): a `reported_from` link (client-made
/// `link_id`) to a task ("Report a bug from this task") or to a doc ("about this doc").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BugReportedFrom {
    pub link_id: String,
    pub to: LinkTarget,
}

write_call!(
    /// `task.create`: a task in `todo`, numbered `T-n` in its project. The project must exist
    /// and not be archived; `team_id` must be one of its teams; `assignee_id` a valid
    /// recipient (an agent that isn't offboarded, a person, or the owner). `feature_id` is
    /// phase 3 and `fixes` phase 2: a server before them refuses them with `invalid_request`.
    /// Errors: `not_found` (project, bug, feature), `invalid_request` (limits; archived
    /// project; team not the project's; a bug in another project), `unknown_recipient`,
    /// `already_exists` (an id).
    TaskCreate, "task.create", TaskCreateParams {
        item_id: String,
        project_id: String,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        definition_of_done: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        assignee_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        team_id: Option<String>,
        /// Absent: `normal`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        priority: Option<Priority>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        feature_id: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        fixes: Vec<NewFixLink>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        checklist: Vec<NewStep>,
    }
);
write_call!(
    /// `task.update`: only the fields given change; a `clear_` flag removes its field. A field
    /// with its `clear_` flag, or nothing to change, is `invalid_request`. `feature_id` and
    /// `clear_feature` are phase 3: a server before it refuses them with `invalid_request`.
    /// Errors: `not_found`, `invalid_request` (also limits), `conflict`.
    /// Updating an existing deleted item is `invalid_request`, not `not_found`.
    TaskUpdate, "task.update", TaskUpdateParams {
        item_id: String,
        expected_version: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        clear_description: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        definition_of_done: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        clear_definition_of_done: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        priority: Option<Priority>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        team_id: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        clear_team: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        feature_id: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        clear_feature: bool,
    }
);
write_call!(
    /// `task.assign`: `assignee_id` absent unassigns. Errors: `not_found`,
    /// `unknown_recipient`, `conflict`.
    TaskAssign, "task.assign", TaskAssignParams {
        item_id: String,
        expected_version: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        assignee_id: Option<String>,
    }
);
write_call!(
    /// `task.set_status`: a move in the task's state machine (`todo → in_progress ⇄ blocked →
    /// done`, and back; leaving `done` is a reopen). `blocked_reason` is given exactly with
    /// `blocked`; `resolution` (default `completed`), `anyway_reason` and `shortcut` only with
    /// `done`. `shortcut` is phase 4: a server before it refuses it with `invalid_request`. Done
    /// with required steps unticked is `checklist_incomplete` unless `anyway_reason` is given
    /// (then recorded as `done_anyway`); `cancelled` skips that guard.
    /// When the caller is the assignee agent, `in_progress` also makes it the agent's current
    /// task. Errors: `not_found`, `invalid_request` (wrong state; a reason missing or
    /// misplaced), `conflict`, `checklist_incomplete` (the message names the steps).
    TaskSetStatus, "task.set_status", TaskSetStatusParams {
        item_id: String,
        expected_version: u64,
        status: ItemStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blocked_reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolution: Option<Resolution>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anyway_reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shortcut: Option<Shortcut>,
    }
);

/// `work.set_current`: sets (or, with `item` absent, clears) the bound agent's current task.
/// It isn't an item change: no `change_id`, and its result is [`Empty`]. Errors: `not_found`,
/// `invalid_request` (not a task; the caller isn't an agent; the caller isn't the task's
/// assignee).
pub struct WorkSetCurrent;

/// Params of `work.set_current`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkSetCurrentParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
}

impl Call for WorkSetCurrent {
    const METHOD: &'static str = "work.set_current";
    type Params = WorkSetCurrentParams;
    type Result = Empty;
}

// ---- Bugs (phase 2) ------------------------------------------------------------------------
//
// A bug is `new → triaged → in_progress → fixed → closed`, and a closed bug can be reopened
// (design §1.10). A bug links to the tasks that fix it (`fixes`, from the task) and never turns
// into one; its status follows those tasks (§2.4). Each call below is one event on the bug.

write_call!(
    /// `bug.report`: a bug in `new`, numbered `B-n` in its project, reported by the caller and
    /// unassigned, with priority `normal`. `severity` is required (and unconfirmed until
    /// triage); `area_id` defaults to the project's General area and must be a live area of the
    /// project. `what_happened` is 1 to [`WORK_TEXT_MAX_CHARS`] characters; `expected` and
    /// `steps` up to [`WORK_TEXT_MAX_CHARS`], `environment` up to [`WORK_ENVIRONMENT_MAX_CHARS`].
    /// `from` makes a `reported_from` link. `affects_feature_id` is phase 3: a server before it
    /// refuses it with `invalid_request`. Errors: `not_found` (project, area, the linked task),
    /// `invalid_request` (limits; archived project; area not in the project or deleted; a
    /// severity that is `unknown`), `already_exists` (an id).
    BugReport, "bug.report", BugReportParams {
        item_id: String,
        project_id: String,
        title: String,
        what_happened: String,
        severity: Severity,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        area_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        steps: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        environment: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<BugReportedFrom>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        affects_feature_id: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        checklist: Vec<NewStep>,
    }
);
write_call!(
    /// `bug.update`: only the fields given change; a `clear_` flag removes an optional field.
    /// A field with its `clear_` flag, or nothing to change, is `invalid_request`. A new
    /// `area_id` on a `new` bug is a **reroute** (the suggested triager changes; nothing else
    /// does); a closed bug keeps its area. `affects_feature_id` and `clear_affects_feature` are
    /// phase 3: a server before it refuses them with `invalid_request`. Errors: `not_found`
    /// (also the area), `invalid_request`, `conflict`.
    BugUpdate, "bug.update", BugUpdateParams {
        item_id: String,
        expected_version: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        what_happened: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        clear_expected: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        steps: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        clear_steps: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        environment: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        clear_environment: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        severity: Option<Severity>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        priority: Option<Priority>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        area_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        affects_feature_id: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        clear_affects_feature: bool,
    }
);
write_call!(
    /// `bug.triage`: `new → triaged`. Confirms the area and severity, sets the priority and,
    /// when `assignee_id` is given, assigns the bug (absent: the assignee stays). One
    /// [`WorkEventKind::Triaged`] event. Errors: `not_found` (also the area), `invalid_request`
    /// (wrong state; the area isn't a live area of the project), `unknown_recipient`,
    /// `conflict` (two triages of the same bug: the second).
    BugTriage, "bug.triage", BugTriageParams {
        item_id: String,
        expected_version: u64,
        area_id: String,
        severity: Severity,
        priority: Priority,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        assignee_id: Option<String>,
    }
);
write_call!(
    /// `bug.assign`: `assignee_id` absent unassigns. A closed bug can't be assigned. Errors:
    /// `not_found`, `invalid_request` (wrong state), `unknown_recipient`, `conflict`.
    BugAssign, "bug.assign", BugAssignParams {
        item_id: String,
        expected_version: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        assignee_id: Option<String>,
    }
);
write_call!(
    /// `bug.mark_fixed`: `triaged` or `in_progress → fixed` by hand. `note` is "what fixed it"
    /// (1 to [`WORK_REASON_MAX_CHARS`] characters), kept as the bug's `closed_reason`. Required
    /// checklist steps unticked are `checklist_incomplete` unless `anyway_reason` is given
    /// (then recorded as `done_anyway`). Errors: `not_found`, `invalid_request` (wrong state),
    /// `conflict`, `checklist_incomplete`.
    BugMarkFixed, "bug.mark_fixed", BugMarkFixedParams {
        item_id: String,
        expected_version: u64,
        note: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anyway_reason: Option<String>,
    }
);
write_call!(
    /// `bug.verify`: on a `fixed` bug. `fixed` closes it as `verified` (the event records
    /// whether the actor was the reporter); `still_happens` sends it back to `triaged` with
    /// one more reopening, keeping its assignee and fix links, and needs a `note`. Errors:
    /// `not_found`, `invalid_request` (not `fixed`; the note missing), `conflict`.
    BugVerify, "bug.verify", BugVerifyParams {
        item_id: String,
        expected_version: u64,
        outcome: VerifyOutcome,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    }
);
write_call!(
    /// `bug.close`: closes an open bug, either as a duplicate (`duplicate_of`) or as
    /// `wont_fix` / `cant_reproduce` (`resolution` and `reason`, 1 to
    /// [`WORK_REASON_MAX_CHARS`] characters); exactly one of the two. The original of a
    /// duplicate is a bug of the same project, not this one, not deleted and not itself closed
    /// as a duplicate; the reporter starts watching it. Errors: `not_found` (the original),
    /// `invalid_request` (wrong state; the original in another project, itself, deleted or
    /// closed as a duplicate; the reason missing), `conflict`.
    BugClose, "bug.close", BugCloseParams {
        item_id: String,
        expected_version: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duplicate_of: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolution: Option<Resolution>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    }
);
write_call!(
    /// `bug.reopen`: `closed → triaged` (a bug closed as a duplicate goes to `new` instead,
    /// with `duplicate_of` cleared, so triage looks again). `note` is required; the reopening
    /// count goes up and a new round starts. Errors: `not_found`, `invalid_request` (not
    /// closed; the note missing), `conflict`.
    BugReopen, "bug.reopen", BugReopenParams {
        item_id: String,
        expected_version: u64,
        note: String,
    }
);

write_call!(
    /// `work.comment`: `body` is 1 to [`WORK_TEXT_MAX_CHARS`] characters; `comment_kind`
    /// defaults to `note`. `asks` (a member) only with a `question`; `answers` (a question's
    /// `comment_id` on the same item) only with an `answer`. A question asking the owner opens a
    /// "waiting on the owner" span, and its answer ends it. `watch` also makes the caller a
    /// watcher. Errors: `not_found` (the item, the question), `invalid_request`,
    /// `unknown_recipient`, `already_exists`.
    WorkComment, "work.comment", WorkCommentParams {
        item_id: String,
        comment_id: String,
        body: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment_kind: Option<CommentKind>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        asks: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        answers: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        watch: bool,
    }
);
write_call!(
    /// `work.watch`: start or stop watching an item as the caller. Errors: `not_found`.
    WorkWatch, "work.watch", WorkWatchParams {
        item_id: String,
        watching: bool,
    }
);
write_call!(
    /// `work.link`: `kind` is `relates_to` (phase 1), `fixes` (phase 2) or `pays_back`
    /// (phase 4); the other kinds are made only by their own calls. Errors: `not_found`,
    /// `invalid_request` (the kind's rule; too many links; an item to itself),
    /// `already_exists` (the id, or the same link).
    WorkLink, "work.link", WorkLinkParams {
        link_id: String,
        from_item_id: String,
        kind: LinkKind,
        to: LinkTarget,
    }
);
write_call!(
    /// `work.unlink`. `from_request` and `part_of` links can't be removed this way
    /// (`checklist.unlink_task` ends a `part_of`). Errors: `not_found`, `invalid_request`.
    WorkUnlink, "work.unlink", WorkUnlinkParams {
        link_id: String,
    }
);
write_call!(
    /// `work.undo`: undoes an automatic event (`event_version` of `target_id`) within an hour of
    /// it, if no later event changed what it changed. It records a new event restoring the
    /// earlier values, with the caller as actor. Errors: `not_found`, `invalid_request` (not
    /// automatic, a cascade, older than an hour, already undone), `conflict`.
    WorkUndo, "work.undo", WorkUndoParams {
        target_id: String,
        event_version: u64,
    }
);
write_call!(
    /// `work.delete`: **app-only** (it destroys history). The item becomes a tombstone: its
    /// record, events and number stay, links to it read as deleted, and the automatic rules and
    /// time figures ignore it. No undelete. Errors: `not_authorized`, `not_found`, `conflict`.
    WorkDelete, "work.delete", WorkDeleteParams {
        item_id: String,
        expected_version: u64,
    }
);
write_call!(
    /// `checklist.add`: 1 to [`WORK_MAX_STEPS`] steps after `after_step_id` (absent: at the
    /// end); an item has at most [`WORK_MAX_STEPS`]. Tasks and bugs only. Errors: `not_found`,
    /// `invalid_request` (a request; too many steps; limits), `already_exists`.
    /// An unknown or removed `after_step_id` is `invalid_request`.
    ChecklistAdd, "checklist.add", ChecklistAddParams {
        item_id: String,
        steps: Vec<NewStep>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after_step_id: Option<String>,
    }
);
write_call!(
    /// `checklist.edit`: `expected_version` is the step's; it conflicts only with a change to
    /// the step's text or `required`. Errors: `not_found`, `invalid_request`, `conflict`.
    ChecklistEdit, "checklist.edit", ChecklistEditParams {
        step_id: String,
        expected_version: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        required: Option<bool>,
    }
);
write_call!(
    /// `checklist.check`: tick or untick. Records an event only when the state changes. Errors:
    /// `not_found`, `invalid_request` (a step made into a task follows its task).
    ChecklistCheck, "checklist.check", ChecklistCheckParams {
        step_id: String,
        done: bool,
    }
);
write_call!(
    /// `checklist.move`: after `after_step_id` (absent: to the top). Errors: `not_found`,
    /// `invalid_request` (another item's step).
    ChecklistMove, "checklist.move", ChecklistMoveParams {
        step_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after_step_id: Option<String>,
    }
);
write_call!(
    /// `checklist.remove`: `expected_version` is the step's; it conflicts with a content change
    /// or the step having been made a task. Errors: `not_found`, `conflict`.
    ChecklistRemove, "checklist.remove", ChecklistRemoveParams {
        step_id: String,
        expected_version: u64,
    }
);
write_call!(
    /// `checklist.make_task`: creates task `task_id` in the item's project (and team, when the
    /// item is a task), titled `title` or the step's text, with a `part_of` link (`link_id`) to
    /// the item; the step then follows it. The answer carries both items. Errors: `not_found`,
    /// `invalid_request` (already made a task; the project is archived; the item isn't in a
    /// project), `already_exists`.
    ChecklistMakeTask, "checklist.make_task", ChecklistMakeTaskParams {
        step_id: String,
        task_id: String,
        link_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    }
);
write_call!(
    /// `checklist.unlink_task`: the step stops following its task and becomes a plain step; the
    /// task's `part_of` link ends. Errors: `not_found`, `invalid_request` (no task).
    ChecklistUnlinkTask, "checklist.unlink_task", ChecklistUnlinkTaskParams {
        step_id: String,
    }
);
