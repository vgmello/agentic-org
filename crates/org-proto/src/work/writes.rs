//! Work writes: the task calls, `work.set_current`, the bug calls (phase 2), the shared
//! item calls and the checklist calls. Each takes a `change_id` and answers [`WorkAccepted`].

use serde::{Deserialize, Serialize};

use super::items::{LinkTarget, NewStep, Shortcut, WorkAccepted};
use crate::call::{Call, Empty};
use crate::{CommentKind, ItemStatus, LinkKind, Priority, Resolution, Severity, VerifyOutcome};

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
/// assignee). Until work reads arrive, `item` must be a canonical uppercase ULID; references
/// and short ids are refused with `invalid_request`.
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
