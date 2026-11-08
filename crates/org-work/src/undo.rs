//! `work.undo` (§2.4): reverts an automatic event within an hour, if no later event changed what
//! it changed. Phase 1's one automatic change is a made-task step ticking or unticking itself.

use org_proto::work::{EventRef, WorkEventKind, WorkUndoParams};

use crate::common::{load, not_deleted};
use crate::engine::{Ctx, Origin, Plan};
use crate::fail::{conflict, invalid, not_found, Fail};
use crate::time::parse_ms;

/// How long an automatic event can be undone.
pub const UNDO_WINDOW_MS: u64 = 60 * 60 * 1000;

pub(crate) fn undo(ctx: &mut Ctx, p: &WorkUndoParams) -> Result<Plan, Fail> {
    let item = load(ctx, &p.target_id)?;
    if p.event_version == 0 || p.event_version > item.version {
        return Err(not_found("no such event"));
    }
    let mut later = ctx.txn.events_since(&item.id, p.event_version - 1)?;
    if later.is_empty() || later[0].version != p.event_version {
        return Err(not_found("no such event"));
    }
    let target = later.remove(0);

    // Undo has its own order (§2.3 points to §2.4's rule; `org_proto::work::WorkUndo` lists
    // these errors): what the target event is, then its age, then whether it was undone
    // already, and only then whether a later change makes it stale. With `conflict` first,
    // "already undone" could never be answered: the `Undone` event has the same aspects.
    if !target.auto {
        return Err(invalid(if target.actor_id.is_none() {
            "a cascade or the office's bookkeeping can't be undone"
        } else {
            "only automatic changes can be undone"
        }));
    }
    let at = parse_ms(&target.at).unwrap_or(0);
    if ctx.now_ms.saturating_sub(at) > UNDO_WINDOW_MS {
        return Err(invalid("automatic changes can be undone for an hour"));
    }
    if later.iter().any(|e| {
        matches!(e.change, WorkEventKind::Undone { event_version } if event_version == p.event_version)
    }) {
        return Err(invalid("this change was already undone"));
    }
    let mut own = ctx
        .aspects(&item.id, std::slice::from_ref(&target))?
        .remove(0);
    // A tick that followed a task is stale once the step stopped following it.
    if let WorkEventKind::StepChecked { step_id, .. } = &target.change {
        own.push(crate::aspects::Aspect::StepTask(step_id.clone()));
    }
    for aspects in ctx.aspects(&item.id, &later)? {
        if aspects
            .iter()
            .any(|a| own.contains(a) || *a == crate::aspects::Aspect::Any)
        {
            return Err(conflict("a later change touched what this one changed"));
        }
    }
    not_deleted(&item)?;

    let WorkEventKind::StepChecked { step_id, done } = &target.change else {
        return Err(invalid("this build can't undo that kind of change"));
    };
    let caller = ctx.caller_id().to_owned();
    let mut draft = ctx.draft(item);
    let at = draft.at_ms();
    match draft.item.step_mut(step_id) {
        Some(s) if !s.removed => {
            s.done = !done;
            s.version += 1;
            s.checked_by = Some(caller);
            s.checked_at_ms = Some(at);
        }
        _ => return Err(conflict("the step is gone")),
    }
    draft.record(
        WorkEventKind::Undone {
            event_version: p.event_version,
        },
        Origin::Undo(EventRef {
            target_id: p.target_id.clone(),
            version: p.event_version,
        }),
    );
    Ok(Plan::of(vec![draft]))
}
