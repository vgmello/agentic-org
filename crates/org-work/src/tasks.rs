//! Task calls (§4.4): `task.create`, `task.update`, `task.assign`, `task.set_status`, and the
//! automatic change a task's status makes (its made-task step ticks or unticks, §2.4).

use org_proto::work::{
    DoneAnyway, EventRef, FieldChange, ItemKind, ItemStatus, LinkKind, LinkTarget, Priority,
    Resolution, TaskAssignParams, TaskCreateParams, TaskSetStatusParams, TaskUpdateParams,
    WorkEventKind, WORK_BLOCKED_REASON_MAX_CHARS, WORK_DESCRIPTION_MAX_CHARS, WORK_DOD_MAX_CHARS,
    WORK_MAX_LINKS, WORK_REASON_MAX_CHARS, WORK_TITLE_MAX_CHARS,
};
use org_proto::ErrorCode;
use serde_json::Value;

use crate::aspects::Aspect;
use crate::common::{
    distinct, expect_kind, known_priority, load, new_steps, not_deleted, require_ulid, unused,
};
use crate::engine::{Ctx, Draft, Origin, Plan};
use crate::fail::{invalid, not_found, refuse, Fail};
use crate::fixes::{self, Edge};
use crate::model::{BugFields, StoredItem, StoredLink, TaskFields};
use crate::states::{is_task_status, is_terminal, task_move_allowed};
use crate::text::{multi_line, one_line};
use crate::waiting;

/// `task.create`: a task in `todo`, numbered `T-n` in its project.
pub(crate) fn create(ctx: &mut Ctx, p: &TaskCreateParams) -> Result<Plan, Fail> {
    let project = ctx
        .org
        .project(&p.project_id)?
        .ok_or_else(|| not_found("no such project"))?;
    if p.fixes.len() > WORK_MAX_LINKS {
        return Err(invalid(format!(
            "an item has at most {WORK_MAX_LINKS} links"
        )));
    }
    let mut fix_bugs = vec![];
    for fix in &p.fixes {
        fix_bugs.push(
            ctx.txn
                .get(&fix.bug_id)?
                .ok_or_else(|| not_found("no such bug to fix"))?,
        );
    }

    require_ulid("item_id", &p.item_id)?;
    let title = one_line("title", &p.title, WORK_TITLE_MAX_CHARS)?;
    let description = optional(p.description.as_deref(), |d| {
        multi_line("description", d, WORK_DESCRIPTION_MAX_CHARS)
    })?;
    let definition_of_done = optional(p.definition_of_done.as_deref(), |d| {
        multi_line("definition_of_done", d, WORK_DOD_MAX_CHARS)
    })?;
    let priority = known_priority(p.priority)?.unwrap_or(Priority::Normal);
    if p.feature_id.is_some() {
        return Err(invalid("`feature_id` comes with features (a later phase)"));
    }
    let checklist = new_steps(&p.checklist, 0)?;
    distinct(
        std::iter::once(p.item_id.as_str())
            .chain(checklist.iter().map(|s| s.id.as_str()))
            .chain(p.fixes.iter().map(|f| f.link_id.as_str())),
    )?;
    distinct(p.fixes.iter().map(|f| f.bug_id.as_str()))?;
    for (fix, bug) in p.fixes.iter().zip(&fix_bugs) {
        require_ulid("link_id", &fix.link_id)?;
        crate::bugs::check_fixable(&project.id, bug)?;
    }
    if let Some(team) = &p.team_id {
        check_team(ctx, &project.id, team)?;
    }
    if project.archived {
        return Err(invalid("the project is archived: it takes no new items"));
    }
    if let Some(assignee) = &p.assignee_id {
        ctx.check_recipient(assignee)?;
    }
    let mut ids = vec![p.item_id.as_str()];
    ids.extend(checklist.iter().map(|s| s.id.as_str()));
    ids.extend(p.fixes.iter().map(|f| f.link_id.as_str()));
    unused(ctx, &ids)?;

    let item = new_task(ctx, &p.item_id, &project.id, title, priority);
    let links = p
        .fixes
        .iter()
        .map(|f| StoredLink {
            id: f.link_id.clone(),
            kind: LinkKind::Fixes,
            to: LinkTarget::Item {
                item_id: f.bug_id.clone(),
            },
            created_by: ctx.caller_id().to_owned(),
            created_at_ms: 0,
            removed: false,
        })
        .collect();
    let mut draft = ctx.draft_new(StoredItem {
        links,
        assignee_id: p.assignee_id.clone(),
        task: TaskFields {
            description,
            definition_of_done,
            team_id: p.team_id.clone(),
            ..TaskFields::default()
        },
        checklist,
        ..item
    });
    let at = draft.at_ms();
    for l in &mut draft.item.links {
        l.created_at_ms = at;
    }
    draft.record(WorkEventKind::Created, Origin::Caller);
    Ok(Plan::of(vec![draft]))
}

/// A new task's state before its `Created` event (version 0): `todo`, the caller its creator
/// and first watcher, owned by this office.
pub(crate) fn new_task(
    ctx: &Ctx,
    id: &str,
    project_id: &str,
    title: String,
    priority: Priority,
) -> StoredItem {
    StoredItem {
        id: id.to_owned(),
        kind: ItemKind::Task,
        project_id: Some(project_id.to_owned()),
        number: None,
        title,
        status: ItemStatus::Todo,
        resolution: None,
        priority: Some(priority),
        assignee_id: None,
        created_by: ctx.caller_id().to_owned(),
        created_at_ms: 0,
        updated_at_ms: 0,
        owner_office_id: ctx.office_id.to_owned(),
        generation: 1,
        version: 0,
        deleted: false,
        task: TaskFields::default(),
        bug: BugFields::default(),
        checklist: vec![],
        links: vec![],
        watchers: vec![ctx.caller_id().to_owned()],
        comments: vec![],
        waiting: vec![],
    }
}

fn optional<T>(
    value: Option<&str>,
    check: impl Fn(&str) -> Result<T, Fail>,
) -> Result<Option<T>, Fail> {
    value.map(check).transpose()
}

fn check_team(ctx: &Ctx, project_id: &str, team_id: &str) -> Result<(), Fail> {
    if ctx
        .org
        .project_teams(project_id)?
        .iter()
        .any(|t| t == team_id)
    {
        Ok(())
    } else {
        Err(invalid("the team isn't one of the project's teams"))
    }
}

fn load_task(ctx: &mut Ctx, id: &str) -> Result<StoredItem, Fail> {
    let item = load(ctx, id)?;
    expect_kind(&item, ItemKind::Task)?;
    Ok(item)
}

/// `task.update`: only the fields given change; a `clear_` flag removes its field.
pub(crate) fn update(ctx: &mut Ctx, p: &TaskUpdateParams) -> Result<Plan, Fail> {
    let item = load_task(ctx, &p.item_id)?;

    let given = p.title.is_some()
        || p.description.is_some()
        || p.clear_description
        || p.definition_of_done.is_some()
        || p.clear_definition_of_done
        || p.priority.is_some()
        || p.team_id.is_some()
        || p.clear_team
        || p.feature_id.is_some()
        || p.clear_feature;
    if !given {
        return Err(invalid("nothing to change"));
    }
    for (value, clear, name) in [
        (p.description.is_some(), p.clear_description, "description"),
        (
            p.definition_of_done.is_some(),
            p.clear_definition_of_done,
            "definition_of_done",
        ),
        (p.team_id.is_some(), p.clear_team, "team_id"),
    ] {
        if value && clear {
            return Err(invalid(format!("`{name}` is given with its `clear_` flag")));
        }
    }
    if p.feature_id.is_some() || p.clear_feature {
        return Err(invalid("`feature_id` comes with features (a later phase)"));
    }
    let title = optional(p.title.as_deref(), |t| {
        one_line("title", t, WORK_TITLE_MAX_CHARS)
    })?;
    let description = optional(p.description.as_deref(), |d| {
        multi_line("description", d, WORK_DESCRIPTION_MAX_CHARS)
    })?;
    let dod = optional(p.definition_of_done.as_deref(), |d| {
        multi_line("definition_of_done", d, WORK_DOD_MAX_CHARS)
    })?;
    let priority = known_priority(p.priority)?;
    if let (Some(team), Some(project)) = (&p.team_id, &item.project_id) {
        check_team(ctx, project, team)?;
    }

    // The fields this call writes, in a fixed order, with their new values.
    let mut changes: Vec<(&str, Option<String>)> = vec![];
    if let Some(t) = title {
        changes.push(("title", Some(t)));
    }
    if description.is_some() || p.clear_description {
        changes.push(("description", description));
    }
    if dod.is_some() || p.clear_definition_of_done {
        changes.push(("definition_of_done", dod));
    }
    if let Some(pr) = priority {
        changes.push(("priority", Some(priority_word(pr).to_owned())));
    }
    if p.team_id.is_some() || p.clear_team {
        changes.push(("team_id", p.team_id.clone()));
    }
    let names: Vec<&str> = changes.iter().map(|(n, _)| *n).collect();
    ctx.check_expected(
        &item,
        p.expected_version,
        |a| matches!(a, Aspect::Field(f) if names.contains(&f.as_str())),
    )?;
    not_deleted(&item)?;

    let mut draft = ctx.draft(item);
    let mut fields = vec![];
    for (name, value) in changes {
        let it = &mut draft.item;
        let from = match name {
            "title" => Some(std::mem::replace(
                &mut it.title,
                value.clone().unwrap_or_default(),
            )),
            "description" => std::mem::replace(&mut it.task.description, value.clone()),
            "definition_of_done" => {
                std::mem::replace(&mut it.task.definition_of_done, value.clone())
            }
            "priority" => {
                let before = it.priority.map(|p| priority_word(p).to_owned());
                it.priority = priority;
                before
            }
            "team_id" => std::mem::replace(&mut it.task.team_id, value.clone()),
            _ => unreachable!("only the fields above are written"),
        };
        fields.push(FieldChange {
            field: name.to_owned(),
            from: from.map(Value::String),
            to: value.map(Value::String),
        });
    }
    draft.record(WorkEventKind::FieldsChanged { fields }, Origin::Caller);
    Ok(Plan::of(vec![draft]))
}

pub(crate) fn priority_word(p: Priority) -> &'static str {
    match p {
        Priority::Urgent => "urgent",
        Priority::High => "high",
        Priority::Normal => "normal",
        Priority::Low => "low",
        _ => "unknown",
    }
}

/// `task.assign`: `assignee_id` absent unassigns.
pub(crate) fn assign(ctx: &mut Ctx, p: &TaskAssignParams) -> Result<Plan, Fail> {
    let item = load_task(ctx, &p.item_id)?;
    if let Some(assignee) = &p.assignee_id {
        ctx.check_recipient(assignee)?;
    }
    ctx.check_expected(&item, p.expected_version, |a| *a == Aspect::Assignee)?;
    not_deleted(&item)?;

    let mut draft = ctx.draft(item);
    let from = std::mem::replace(&mut draft.item.assignee_id, p.assignee_id.clone());
    draft.record(
        WorkEventKind::Assigned {
            from,
            to: p.assignee_id.clone(),
        },
        Origin::Caller,
    );
    Ok(Plan::of(vec![draft]))
}

/// Parent completion is never overridable; cancellation retains its separate semantics.
fn require_closed_subtasks(ctx: &mut Ctx, parent: &StoredItem) -> Result<(), Fail> {
    let mut open: Vec<_> = ctx
        .txn
        .linked_to(&parent.id, LinkKind::PartOf)?
        .into_iter()
        .filter(|child| {
            child.kind == ItemKind::Task && !child.deleted && !is_terminal(child.kind, child.status)
        })
        .collect();
    open.sort_by(|a, b| a.number.cmp(&b.number).then_with(|| a.id.cmp(&b.id)));
    if open.is_empty() {
        return Ok(());
    }
    let keys: Vec<_> = open.iter().map(task_key).collect();
    let count = open.len();
    Err(invalid(format!(
        "{} has {count} open subtask{}: {}. Finish or cancel them first.",
        task_key(parent),
        if count == 1 { "" } else { "s" },
        keys.join(", ")
    )))
}

fn task_key(item: &StoredItem) -> String {
    item.number
        .map_or_else(|| crate::short_id(&item.id), |n| format!("T-{n}"))
}

/// `task.set_status`: a move in the task's state machine, with Done's step guard (§1.10).
pub(crate) fn set_status(ctx: &mut Ctx, p: &TaskSetStatusParams) -> Result<Plan, Fail> {
    let item = load_task(ctx, &p.item_id)?;

    let to = p.status;
    if !is_task_status(to) {
        return Err(invalid(
            "`status` must be todo, in_progress, blocked or done",
        ));
    }
    let blocked_reason = match (to, p.blocked_reason.as_deref()) {
        (ItemStatus::Blocked, Some(r)) => Some(one_line(
            "blocked_reason",
            r,
            WORK_BLOCKED_REASON_MAX_CHARS,
        )?),
        (ItemStatus::Blocked, None) => return Err(invalid("blocking a task needs a reason")),
        (_, Some(_)) => return Err(invalid("`blocked_reason` goes only with `blocked`")),
        (_, None) => None,
    };
    let done = to == ItemStatus::Done;
    let resolution = match (done, p.resolution) {
        (true, None | Some(Resolution::Completed)) => Some(Resolution::Completed),
        (true, Some(Resolution::Cancelled)) => Some(Resolution::Cancelled),
        (true, Some(_)) => return Err(invalid("a task's resolution is completed or cancelled")),
        (false, Some(_)) => return Err(invalid("`resolution` goes only with `done`")),
        (false, None) => None,
    };
    let anyway_reason = match (done, p.anyway_reason.as_deref()) {
        (true, Some(r)) => Some(one_line("anyway_reason", r, WORK_REASON_MAX_CHARS)?),
        (false, Some(_)) => return Err(invalid("`anyway_reason` goes only with `done`")),
        (_, None) => None,
    };
    if p.shortcut.is_some() {
        return Err(invalid("`shortcut` comes with debt (a later phase)"));
    }
    ctx.check_expected(&item, p.expected_version, |a| *a == Aspect::Status)?;
    not_deleted(&item)?;
    let from = item.status;
    if !task_move_allowed(from, to) {
        return Err(invalid(format!(
            "wrong state: a task can't go from {} to {}",
            status_word(from),
            status_word(to)
        )));
    }
    // Done (completed) with required steps unticked needs a reason; cancelled skips the guard.
    let mut anyway = None;
    if resolution == Some(Resolution::Completed) {
        require_closed_subtasks(ctx, &item)?;
        let left = item.required_left();
        if !left.is_empty() {
            let Some(reason) = anyway_reason else {
                let names: Vec<String> = left.iter().map(|s| format!("\"{}\"", s.text)).collect();
                let n = left.len();
                return Err(refuse(
                    ErrorCode::ChecklistIncomplete,
                    format!(
                        "{n} required step{} {} done: {}",
                        if n == 1 { "" } else { "s" },
                        if n == 1 { "isn't" } else { "aren't" },
                        names.join(", ")
                    ),
                ));
            };
            anyway = Some(DoneAnyway {
                reason,
                steps: left.iter().map(|s| s.id.clone()).collect(),
            });
        }
    }

    let reopen = from == ItemStatus::Done;
    let owner_id = ctx.org.owner_id()?;
    let mut draft = ctx.draft(item);
    {
        let it = &mut draft.item;
        it.status = to;
        it.resolution = resolution;
        it.task.blocked_reason = blocked_reason.clone();
        it.task.done_anyway = anyway.clone();
    }
    let cause = draft.record(
        WorkEventKind::StatusChanged {
            from,
            to,
            resolution,
            reason: blocked_reason,
            anyway,
            duplicate_of: None,
        },
        Origin::Caller,
    );
    if done {
        waiting::round_closed(&mut draft, &cause);
    } else if reopen {
        waiting::round_reopened(&mut draft, &owner_id, &cause);
    }
    let mut drafts = vec![];
    let completed = resolution == Some(Resolution::Completed);
    if completed || reopen {
        if let Some(parent) = step_follows_task(ctx, &draft.item, completed, cause.clone())? {
            drafts.push(parent);
        }
    }
    // A bug follows the tasks that fix it (§2.4).
    let mut edges = vec![];
    if reopen {
        edges.push(Edge::Reopened);
    }
    match to {
        ItemStatus::InProgress => edges.push(Edge::Started),
        ItemStatus::Done => edges.push(Edge::Done),
        _ => {}
    }
    fixes::follow_task(ctx, &mut drafts, &draft.item, &edges, &cause)?;
    drafts.insert(0, draft);
    Ok(Plan::of(drafts))
}

/// Step follows its task (CHK-6, §2.4): when a made task goes Done · Completed its step ticks,
/// and when it's reopened its step unticks. An automatic event on the step's item, unless the
/// item is deleted, the step is gone, or it already shows that state.
fn step_follows_task(
    ctx: &mut Ctx,
    task: &StoredItem,
    done: bool,
    cause: EventRef,
) -> Result<Option<Draft>, Fail> {
    let Some(LinkTarget::Item { item_id }) = task.part_of().map(|l| l.to.clone()) else {
        return Ok(None);
    };
    let Some(parent) = ctx.txn.get(&item_id)? else {
        return Ok(None);
    };
    if parent.deleted {
        return Ok(None);
    }
    let Some(step) = parent
        .live_steps()
        .find(|s| s.task_id.as_deref() == Some(task.id.as_str()))
    else {
        return Ok(None);
    };
    if step.done == done {
        return Ok(None);
    }
    let step_id = step.id.clone();
    let mut draft = ctx.draft(parent);
    let at = draft.at_ms();
    if let Some(step) = draft.item.step_mut(&step_id) {
        step.done = done;
        step.version += 1;
        step.checked_by = None;
        step.checked_at_ms = Some(at);
    }
    draft.record(
        WorkEventKind::StepChecked { step_id, done },
        Origin::Auto(cause),
    );
    Ok(Some(draft))
}

pub(crate) fn status_word(s: ItemStatus) -> &'static str {
    match s {
        ItemStatus::Todo => "to do",
        ItemStatus::InProgress => "in progress",
        ItemStatus::Blocked => "blocked",
        ItemStatus::Done => "done",
        ItemStatus::New => "new",
        ItemStatus::Triaged => "triaged",
        ItemStatus::Fixed => "fixed",
        ItemStatus::Closed => "closed",
        ItemStatus::Routed => "routed",
        ItemStatus::Scoping => "scoping",
        ItemStatus::Scoped => "scoped",
        ItemStatus::Approved => "approved",
        ItemStatus::Declined => "declined",
        ItemStatus::Merged => "merged",
        _ => "an unknown status",
    }
}
