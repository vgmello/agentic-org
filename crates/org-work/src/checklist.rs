//! Checklist calls (§4.7): add, edit, check, move, remove, make a step into a task, and end that
//! link. Steps have three aspects (content, done, position) and their own version, so a tick or
//! a move never makes a text edit stale.

use org_proto::work::{
    ChecklistAddParams, ChecklistCheckParams, ChecklistEditParams, ChecklistMakeTaskParams,
    ChecklistMoveParams, ChecklistRemoveParams, ChecklistUnlinkTaskParams, ItemKind, LinkKind,
    LinkTarget, Priority, WorkEventKind, WORK_STEP_TEXT_MAX_CHARS, WORK_TITLE_MAX_CHARS,
};

use crate::common::{distinct, load, new_steps, not_deleted, require_ulid, unused};
use crate::engine::{Ctx, Draft, Origin, Plan};
use crate::fail::{conflict, invalid, not_found, Fail};
use crate::model::{StoredItem, StoredLink};
use crate::tasks::new_task;
use crate::text::{cut, one_line};

/// The item holding a live step, or `not_found`.
fn load_step_item(ctx: &mut Ctx, step_id: &str) -> Result<StoredItem, Fail> {
    match ctx.txn.item_of_step(step_id)? {
        Some(item) if item.live_steps().any(|s| s.id == step_id) => Ok(item),
        _ => Err(not_found("no such checklist step")),
    }
}

fn has_checklist(item: &StoredItem) -> Result<(), Fail> {
    match item.kind {
        ItemKind::Task | ItemKind::Bug => Ok(()),
        _ => Err(invalid("only tasks and bugs have checklists")),
    }
}

/// The index in `item.checklist` after which a step goes: `after` must be a live step of the
/// item, other than `moving`. `None` (the top) is index 0.
fn position_after(
    item: &StoredItem,
    after: Option<&str>,
    moving: Option<&str>,
) -> Result<usize, Fail> {
    let Some(after) = after else { return Ok(0) };
    if Some(after) == moving {
        return Err(invalid("a step can't go after itself"));
    }
    item.checklist
        .iter()
        .position(|s| s.id == after && !s.removed)
        .map(|i| i + 1)
        .ok_or_else(|| invalid("`after_step_id` isn't a step of this item"))
}

/// `checklist.add`: 1 to 100 steps after `after_step_id` (absent: at the end).
pub(crate) fn add(ctx: &mut Ctx, p: &ChecklistAddParams) -> Result<Plan, Fail> {
    let item = load(ctx, &p.item_id)?;
    has_checklist(&item)?;
    if p.steps.is_empty() {
        return Err(invalid("give at least one step"));
    }
    let steps = new_steps(&p.steps, item.live_steps().count())?;
    let at = match p.after_step_id.as_deref() {
        Some(after) => position_after(&item, Some(after), None)?,
        None => item.checklist.len(),
    };
    not_deleted(&item)?;
    let ids: Vec<&str> = steps.iter().map(|s| s.id.as_str()).collect();
    unused(ctx, &ids)?;

    let mut draft = ctx.draft(item);
    let views = steps.iter().map(|s| s.view()).collect();
    draft.item.checklist.splice(at..at, steps);
    draft.record(
        WorkEventKind::StepsAdded {
            steps: views,
            after_step_id: p.after_step_id.clone(),
        },
        Origin::Caller,
    );
    Ok(Plan::of(vec![draft]))
}

/// `checklist.edit`: the step's text or `required`. Conflicts only with a content change after
/// the step's `expected_version`.
pub(crate) fn edit(ctx: &mut Ctx, p: &ChecklistEditParams) -> Result<Plan, Fail> {
    let item = load_step_item(ctx, &p.step_id)?;
    if p.text.is_none() && p.required.is_none() {
        return Err(invalid("nothing to change"));
    }
    let text = p
        .text
        .as_deref()
        .map(|t| one_line("text", t, WORK_STEP_TEXT_MAX_CHARS))
        .transpose()?;
    let step = item
        .live_steps()
        .find(|s| s.id == p.step_id)
        .expect("loaded with the step");
    if p.expected_version > step.version {
        return Err(invalid("`expected_version` is ahead of the step's version"));
    }
    if step.content_version > p.expected_version {
        return Err(conflict(
            "someone changed this step's text since you last saw it",
        ));
    }
    not_deleted(&item)?;

    let mut draft = ctx.draft(item);
    let step = draft.item.step_mut(&p.step_id).expect("checked above");
    if let Some(t) = &text {
        step.text = t.clone();
    }
    if let Some(r) = p.required {
        step.required = r;
    }
    step.version += 1;
    step.content_version = step.version;
    draft.record(
        WorkEventKind::StepEdited {
            step_id: p.step_id.clone(),
            text,
            required: p.required,
        },
        Origin::Caller,
    );
    Ok(Plan::of(vec![draft]))
}

/// `checklist.check`: tick or untick. Records an event only when the state changes. A step made
/// into a task follows its task and can't be ticked by hand.
pub(crate) fn check(ctx: &mut Ctx, p: &ChecklistCheckParams) -> Result<Plan, Fail> {
    let item = load_step_item(ctx, &p.step_id)?;
    not_deleted(&item)?;
    let step = item
        .live_steps()
        .find(|s| s.id == p.step_id)
        .expect("loaded with the step");
    if step.task_id.is_some() {
        return Err(invalid(
            "this step follows its task: it ticks when the task is done",
        ));
    }
    if step.done == p.done {
        return Ok(Plan::unchanged(item));
    }
    let mut draft = ctx.draft(item);
    let at = draft.at_ms();
    let caller = ctx.caller_id().to_owned();
    let step = draft.item.step_mut(&p.step_id).expect("checked above");
    step.done = p.done;
    step.version += 1;
    step.checked_by = Some(caller);
    step.checked_at_ms = Some(at);
    draft.record(
        WorkEventKind::StepChecked {
            step_id: p.step_id.clone(),
            done: p.done,
        },
        Origin::Caller,
    );
    Ok(Plan::of(vec![draft]))
}

/// `checklist.move`: after `after_step_id` (absent: to the top). A move to where the step
/// already is records nothing.
pub(crate) fn move_step(ctx: &mut Ctx, p: &ChecklistMoveParams) -> Result<Plan, Fail> {
    let item = load_step_item(ctx, &p.step_id)?;
    let mut target = position_after(&item, p.after_step_id.as_deref(), Some(&p.step_id))?;
    not_deleted(&item)?;

    let from = item
        .checklist
        .iter()
        .position(|s| s.id == p.step_id)
        .expect("loaded with the step");
    let mut order = item.checklist.clone();
    let step = order.remove(from);
    if target > from {
        target -= 1;
    }
    order.insert(target, step);
    let live = |steps: &[crate::model::StoredStep]| -> Vec<String> {
        steps
            .iter()
            .filter(|s| !s.removed)
            .map(|s| s.id.clone())
            .collect()
    };
    if live(&order) == live(&item.checklist) {
        return Ok(Plan::unchanged(item));
    }
    let mut draft = ctx.draft(item);
    draft.item.checklist = order;
    draft.item.step_mut(&p.step_id).expect("moved").version += 1;
    draft.record(
        WorkEventKind::StepMoved {
            step_id: p.step_id.clone(),
            after_step_id: p.after_step_id.clone(),
        },
        Origin::Caller,
    );
    Ok(Plan::of(vec![draft]))
}

/// `checklist.remove`: conflicts with a content change, or the step having been made a task,
/// after the step's `expected_version`. A made-task step's task keeps going, without its
/// `part_of` link.
pub(crate) fn remove(ctx: &mut Ctx, p: &ChecklistRemoveParams) -> Result<Plan, Fail> {
    let item = load_step_item(ctx, &p.step_id)?;
    let step = item
        .live_steps()
        .find(|s| s.id == p.step_id)
        .expect("loaded with the step")
        .clone();
    if p.expected_version > step.version {
        return Err(invalid("`expected_version` is ahead of the step's version"));
    }
    if step.content_version > p.expected_version || step.task_version > p.expected_version {
        return Err(conflict("someone changed this step since you last saw it"));
    }
    not_deleted(&item)?;

    let item_id = item.id.clone();
    let mut draft = ctx.draft(item);
    let s = draft.item.step_mut(&p.step_id).expect("checked above");
    s.removed = true;
    s.version += 1;
    draft.record(
        WorkEventKind::StepRemoved {
            step_id: p.step_id.clone(),
        },
        Origin::Caller,
    );
    let mut drafts = vec![draft];
    if let Some(task_id) = &step.task_id {
        if let Some(task) = end_part_of(ctx, task_id, &item_id)? {
            drafts.push(task);
        }
    }
    Ok(Plan::of(drafts))
}

/// Ends the task's live `part_of` link to `parent_id`, if it has one: an `Unlinked` event on the
/// task, made by the caller.
fn end_part_of(ctx: &mut Ctx, task_id: &str, parent_id: &str) -> Result<Option<Draft>, Fail> {
    let Some(task) = ctx.txn.get(task_id)? else {
        return Ok(None);
    };
    let Some(link_id) = task
        .live_links()
        .find(|l| {
            l.kind == LinkKind::PartOf
                && matches!(&l.to, LinkTarget::Item { item_id } if item_id == parent_id)
        })
        .map(|l| l.id.clone())
    else {
        return Ok(None);
    };
    let mut draft = ctx.draft(task);
    if let Some(l) = draft.item.links.iter_mut().find(|l| l.id == link_id) {
        l.removed = true;
    }
    draft.record(WorkEventKind::Unlinked { link_id }, Origin::Caller);
    Ok(Some(draft))
}

/// `checklist.make_task`: creates task `task_id` in the item's project (and team, when the item
/// is a task), titled `title` or the step's text cut to 120 characters, with a `part_of` link to
/// the item; the step then follows it (unticked until the task is Done · Completed).
pub(crate) fn make_task(ctx: &mut Ctx, p: &ChecklistMakeTaskParams) -> Result<Plan, Fail> {
    let item = load_step_item(ctx, &p.step_id)?;
    let project_id = item
        .project_id
        .clone()
        .ok_or_else(|| invalid("the item isn't in a project"))?;
    let project = ctx
        .org
        .project(&project_id)?
        .ok_or_else(|| not_found("no such project"))?;
    require_ulid("task_id", &p.task_id)?;
    require_ulid("link_id", &p.link_id)?;
    distinct([p.task_id.as_str(), p.link_id.as_str()])?;
    let step = item
        .live_steps()
        .find(|s| s.id == p.step_id)
        .expect("loaded with the step")
        .clone();
    let title = match p.title.as_deref() {
        Some(t) => one_line("title", t, WORK_TITLE_MAX_CHARS)?,
        None => cut(&step.text, WORK_TITLE_MAX_CHARS),
    };
    if project.archived {
        return Err(invalid("the project is archived: it takes no new items"));
    }
    not_deleted(&item)?;
    if step.task_id.is_some() {
        return Err(invalid("this step was already made into a task"));
    }
    unused(ctx, &[p.task_id.as_str(), p.link_id.as_str()])?;

    let mut task = new_task(ctx, &p.task_id, &project.id, title, Priority::Normal);
    if item.kind == ItemKind::Task {
        task.task.team_id = item.task.team_id.clone();
    }
    task.links.push(StoredLink {
        id: p.link_id.clone(),
        kind: LinkKind::PartOf,
        to: LinkTarget::Item {
            item_id: item.id.clone(),
        },
        created_by: ctx.caller_id().to_owned(),
        created_at_ms: 0,
        removed: false,
    });
    let mut task = ctx.draft_new(task);
    let at = task.at_ms();
    task.item.links[0].created_at_ms = at;
    task.record(WorkEventKind::Created, Origin::Caller);

    let mut parent = ctx.draft(item);
    let s = parent.item.step_mut(&p.step_id).expect("checked above");
    s.task_id = Some(p.task_id.clone());
    // From now on the step follows its task, which is open: it ticks only when the task is
    // Done · Completed (§1.6), so a tick it had ends here.
    s.done = false;
    s.checked_by = None;
    s.checked_at_ms = None;
    s.version += 1;
    s.task_version = s.version;
    parent.record(
        WorkEventKind::StepMadeTask {
            step_id: p.step_id.clone(),
            task_id: p.task_id.clone(),
        },
        Origin::Caller,
    );
    Ok(Plan::of(vec![parent, task]))
}

/// `checklist.unlink_task`: the step stops following its task and becomes a plain step; the
/// task's `part_of` link ends.
pub(crate) fn unlink_task(ctx: &mut Ctx, p: &ChecklistUnlinkTaskParams) -> Result<Plan, Fail> {
    let item = load_step_item(ctx, &p.step_id)?;
    not_deleted(&item)?;
    let Some(task_id) = item
        .live_steps()
        .find(|s| s.id == p.step_id)
        .and_then(|s| s.task_id.clone())
    else {
        return Err(invalid("this step wasn't made into a task"));
    };

    let item_id = item.id.clone();
    let mut parent = ctx.draft(item);
    let s = parent.item.step_mut(&p.step_id).expect("checked above");
    s.task_id = None;
    s.version += 1;
    parent.record(
        WorkEventKind::StepTaskUnlinked {
            step_id: p.step_id.clone(),
            task_id: task_id.clone(),
        },
        Origin::Caller,
    );
    let mut drafts = vec![parent];
    if let Some(task) = end_part_of(ctx, &task_id, &item_id)? {
        drafts.push(task);
    }
    Ok(Plan::of(drafts))
}
