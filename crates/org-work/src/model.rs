//! What the office stores for a work item, and how reads are built from it.
//!
//! [`StoredItem`] is an item's whole current state, written by [`crate::WorkTxn::put_item`]
//! after every change. It serializes with serde, so the store may keep the columns it indexes
//! (see the design's §3.3 sketch) and the rest as JSON. Times are Unix milliseconds; events and
//! reads carry RFC 3339.
//!
//! It keeps removed steps and links as tombstones (their ids are never reused) and a reference
//! per comment, so it grows with an item's history. A store that writes it whole on every change
//! pays for that; one that keeps steps, links and comments as rows writes only the ones that
//! changed (compare with the stored copy).

use org_proto::work::{
    ChecklistCounts, ChecklistStep, CommentKind, DoneAnyway, ItemKind, ItemStatus, Link, LinkKind,
    LinkTarget, Priority, Resolution, Severity, Shortcut, WaitReason, WorkAccepted, WorkItem,
    WorkItemSummary,
};
use org_proto::ApiError;
use serde::{Deserialize, Serialize};

use crate::ids::short_id;
use crate::time::format_ms;

/// An item's whole state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredItem {
    pub id: String,
    pub kind: ItemKind,
    pub project_id: Option<String>,
    /// Assigned by HQ in the create's transaction (phase 1).
    pub number: Option<u64>,
    pub title: String,
    pub status: ItemStatus,
    /// Set exactly when the status is terminal.
    pub resolution: Option<Resolution>,
    /// Tasks and bugs.
    pub priority: Option<Priority>,
    pub assignee_id: Option<String>,
    pub created_by: String,
    pub created_at_ms: u64,
    /// The time of the item's last event. A new event is never dated earlier (§2.7).
    pub updated_at_ms: u64,
    pub owner_office_id: String,
    pub generation: u64,
    /// Equals the last event's version.
    pub version: u64,
    pub deleted: bool,
    #[serde(default)]
    pub task: TaskFields,
    #[serde(default)]
    pub bug: BugFields,
    /// In order. Removed steps stay as tombstones (their ids are never reused).
    #[serde(default)]
    pub checklist: Vec<StoredStep>,
    /// Links from this item. Removed links stay as tombstones.
    #[serde(default)]
    pub links: Vec<StoredLink>,
    #[serde(default)]
    pub watchers: Vec<String>,
    /// The item's comments, without their bodies (the bodies are in the `Comment` events).
    #[serde(default)]
    pub comments: Vec<StoredComment>,
    /// The waiting spans open now (§6): the office's bookkeeping, so a round's end can close
    /// each and a span is never opened twice.
    #[serde(default)]
    pub waiting: Vec<OpenWait>,
}

/// An open waiting span: who is waited on, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenWait {
    pub on_member_id: String,
    pub reason: WaitReason,
}

/// The fields only a task has.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskFields {
    pub description: Option<String>,
    pub definition_of_done: Option<String>,
    pub team_id: Option<String>,
    /// Phase 3.
    pub feature_id: Option<String>,
    /// Set exactly when the status is `blocked`.
    pub blocked_reason: Option<String>,
    /// Set when Done with required steps unticked; cleared on reopen.
    pub done_anyway: Option<DoneAnyway>,
    /// Set by the office when it makes the task's worktree; read-only to clients.
    pub branch: Option<String>,
    /// Phase 4.
    pub shortcut: Option<Shortcut>,
}

/// The fields only a bug has (§1.4).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BugFields {
    /// Always set on a bug: a live area of its project when it was set.
    pub area_id: Option<String>,
    /// Always set on a bug.
    pub severity: Option<Severity>,
    /// False until triage.
    pub severity_confirmed: bool,
    pub what_happened: String,
    pub expected: Option<String>,
    pub steps: Option<String>,
    pub environment: Option<String>,
    /// "What fixed it" (`bug.mark_fixed`) or the won't fix / can't reproduce reason; cleared
    /// when the bug leaves `fixed` or `closed`.
    pub closed_reason: Option<String>,
    /// Set exactly when closed as a duplicate.
    pub duplicate_of: Option<String>,
    /// How many times it was reopened or sent back by "still happens".
    pub reopen_count: u32,
    /// Phase 3.
    pub affects_feature_id: Option<String>,
    /// Set when Fixed by hand with required steps unticked; cleared when it leaves `fixed`.
    pub done_anyway: Option<DoneAnyway>,
}

/// One checklist step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredStep {
    pub id: String,
    pub text: String,
    pub required: bool,
    pub done: bool,
    /// The task this step was made into: the step then follows it.
    pub task_id: Option<String>,
    /// Who ticked or unticked it last (absent: automatically).
    pub checked_by: Option<String>,
    pub checked_at_ms: Option<u64>,
    /// Counts the events that touched this step (add, edit, check, move, make-task, unlink).
    pub version: u64,
    /// The step version of its last content change (add or edit): `checklist.edit` and
    /// `checklist.remove` conflict only with a content change after their `expected_version`.
    pub content_version: u64,
    /// The step version at which it was last made into a task (0: never):
    /// `checklist.remove` conflicts with a make-task after its `expected_version`.
    pub task_version: u64,
    pub removed: bool,
}

/// A link from an item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredLink {
    pub id: String,
    pub kind: LinkKind,
    pub to: LinkTarget,
    pub created_by: String,
    pub created_at_ms: u64,
    pub removed: bool,
}

/// A comment on an item, without its body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredComment {
    pub id: String,
    pub kind: CommentKind,
    /// A question addressed to this member.
    pub asks: Option<String>,
    /// A question that has an answer.
    pub answered: bool,
}

/// A stored answer to a `change_id` (§2.3): the params hash it was made for, and the answer
/// (a refusal included), kept until `expires_at_ms` (30 days after the change id's time).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredResult {
    pub params_hash: [u8; 32],
    pub answer: Result<WorkAccepted, ApiError>,
    pub expires_at_ms: u64,
}

impl StoredItem {
    /// The live checklist steps, in order.
    pub fn live_steps(&self) -> impl Iterator<Item = &StoredStep> {
        self.checklist.iter().filter(|s| !s.removed)
    }

    /// The live links from this item.
    pub fn live_links(&self) -> impl Iterator<Item = &StoredLink> {
        self.links.iter().filter(|l| !l.removed)
    }

    /// The item's live `part_of` link, if it was made from a checklist step.
    pub(crate) fn part_of(&self) -> Option<&StoredLink> {
        self.live_links().find(|l| l.kind == LinkKind::PartOf)
    }

    pub(crate) fn step_mut(&mut self, step_id: &str) -> Option<&mut StoredStep> {
        self.checklist.iter_mut().find(|s| s.id == step_id)
    }

    pub(crate) fn comment(&self, comment_id: &str) -> Option<&StoredComment> {
        self.comments.iter().find(|c| c.id == comment_id)
    }

    /// The live required steps that aren't ticked.
    pub fn required_left(&self) -> Vec<&StoredStep> {
        self.live_steps()
            .filter(|s| s.required && !s.done)
            .collect()
    }

    /// The item as lists, boards and live updates show it.
    pub fn summary(&self) -> WorkItemSummary {
        let live: Vec<&StoredStep> = self.live_steps().collect();
        WorkItemSummary {
            id: self.id.clone(),
            kind: self.kind,
            project_id: self.project_id.clone(),
            number: self.number,
            short_id: short_id(&self.id),
            title: self.title.clone(),
            status: self.status,
            resolution: self.resolution,
            priority: self.priority,
            assignee_id: self.assignee_id.clone(),
            created_by: self.created_by.clone(),
            created_at: format_ms(self.created_at_ms),
            updated_at: format_ms(self.updated_at_ms),
            team_id: self.task.team_id.clone(),
            feature_id: self.task.feature_id.clone(),
            blocked_reason: self.task.blocked_reason.clone(),
            checklist: ChecklistCounts {
                total: count(live.len()),
                done: count(live.iter().filter(|s| s.done).count()),
                required_left: count(live.iter().filter(|s| s.required && !s.done).count()),
            },
            done_anyway: self.task.done_anyway.is_some() || self.bug.done_anyway.is_some(),
            fixes: self.fixes(),
            owner_office_id: self.owner_office_id.clone(),
            generation: self.generation,
            version: self.version,
            deleted: self.deleted,
            area_id: self.bug.area_id.clone(),
            severity: self.bug.severity,
            severity_confirmed: self.bug.severity_confirmed,
            reopen_count: self.bug.reopen_count,
            duplicate_of: self.bug.duplicate_of.clone(),
            ..WorkItemSummary::default()
        }
    }

    /// The bugs this task fixes: the targets of its live `fixes` links.
    pub fn fixes(&self) -> Vec<String> {
        self.live_links()
            .filter(|l| l.kind == LinkKind::Fixes)
            .filter_map(|l| match &l.to {
                LinkTarget::Item { item_id } => Some(item_id.clone()),
                _ => None,
            })
            .collect()
    }

    /// [`Self::summary`] for a bug with its fix tasks (`fixed_by`): the tasks that have a live
    /// `fixes` link to it, which only the office's link index knows. The library fills it on
    /// the answers it builds ([`WorkReads::linked_to`](crate::WorkReads::linked_to) gives it
    /// for snapshot pages).
    pub fn summary_with_fixed_by(&self, fixed_by: Vec<String>) -> WorkItemSummary {
        WorkItemSummary {
            fixed_by,
            ..self.summary()
        }
    }

    /// The item's live links as the wire shows them. `is_deleted` says whether a target item is
    /// deleted (reads show "deleted").
    pub fn links_view(&self, is_deleted: &dyn Fn(&str) -> bool) -> Vec<Link> {
        self.live_links()
            .map(|l| link_view(&self.id, l, is_deleted))
            .collect()
    }

    /// The live checklist as the wire shows it.
    pub fn checklist_view(&self) -> Vec<ChecklistStep> {
        self.live_steps().map(StoredStep::view).collect()
    }
}

impl StoredStep {
    /// A new step, as `task.create` and `checklist.add` make it.
    pub(crate) fn new(id: String, text: String, required: bool) -> Self {
        Self {
            id,
            text,
            required,
            done: false,
            task_id: None,
            checked_by: None,
            checked_at_ms: None,
            version: 1,
            content_version: 1,
            task_version: 0,
            removed: false,
        }
    }

    pub fn view(&self) -> ChecklistStep {
        ChecklistStep {
            id: self.id.clone(),
            text: self.text.clone(),
            required: self.required,
            done: self.done,
            task_id: self.task_id.clone(),
            checked_by: self.checked_by.clone(),
            checked_at: self.checked_at_ms.map(format_ms),
            version: self.version,
        }
    }
}

pub(crate) fn link_view(from: &str, l: &StoredLink, is_deleted: &dyn Fn(&str) -> bool) -> Link {
    let target_deleted = match &l.to {
        LinkTarget::Item { item_id } => is_deleted(item_id),
        _ => false,
    };
    Link {
        id: l.id.clone(),
        from_item_id: from.to_owned(),
        kind: l.kind,
        to: l.to.clone(),
        created_by: l.created_by.clone(),
        created_at: format_ms(l.created_at_ms),
        target_deleted,
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The parts of a full item read that come from elsewhere than the item itself.
pub struct ItemReadParts {
    /// Live links to the item, at most [`org_proto::work::WORK_GET_MAX_LISTED`] + 1 (one more
    /// tells that there are more).
    pub backlinks: Vec<Link>,
    /// Whether a target of the item's own links is deleted.
    pub is_deleted: Box<dyn Fn(&str) -> bool>,
    /// The item's time figures (see [`crate::time_figures`]).
    pub time: org_proto::work::TimeFigures,
    /// A bug's fix tasks (ids of the live tasks with a `fixes` link to it).
    pub fixed_by: Vec<String>,
    /// A bug's suggested triager and verifier (§1.4), derived.
    pub suggested_triager: Option<String>,
    pub suggested_verifier: Option<String>,
}

/// The item in full (`work.get`): its summary and long fields, its checklist, links and
/// backlinks, at most [`org_proto::work::WORK_GET_MAX_LISTED`] backlinks and watchers.
pub fn work_item(item: &StoredItem, parts: ItemReadParts) -> WorkItem {
    use org_proto::work::WORK_GET_MAX_LISTED as MAX;
    let ItemReadParts {
        mut backlinks,
        is_deleted,
        time,
        fixed_by,
        suggested_triager,
        suggested_verifier,
    } = parts;
    let backlinks_more = backlinks.len() > MAX;
    backlinks.truncate(MAX);
    let watchers_more = item.watchers.len() > MAX;
    WorkItem {
        summary: item.summary_with_fixed_by(fixed_by),
        backlinks_more,
        watchers_more,
        description: item.task.description.clone(),
        definition_of_done: item.task.definition_of_done.clone(),
        branch: item.task.branch.clone(),
        done_anyway_detail: item
            .task
            .done_anyway
            .clone()
            .or(item.bug.done_anyway.clone()),
        shortcut: item.task.shortcut.clone(),
        what_happened: (item.kind == ItemKind::Bug).then(|| item.bug.what_happened.clone()),
        expected: item.bug.expected.clone(),
        steps: item.bug.steps.clone(),
        environment: item.bug.environment.clone(),
        closed_reason: item.bug.closed_reason.clone(),
        affects_feature_id: item.bug.affects_feature_id.clone(),
        suggested_triager,
        suggested_verifier,
        checklist: item.checklist_view(),
        links: item.links_view(is_deleted.as_ref()),
        backlinks,
        watchers: item.watchers.iter().take(MAX).cloned().collect(),
        time,
        ..WorkItem::default()
    }
}
