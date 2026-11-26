//! Work item records: summaries, full items, checklist steps, links, request outcomes,
//! features and time figures.

use serde::{Deserialize, Serialize};

use super::*;

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
