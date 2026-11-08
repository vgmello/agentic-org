//! Checks most calls share.

use std::collections::HashSet;

use org_proto::work::{ItemKind, NewStep, Priority, WORK_MAX_STEPS, WORK_STEP_TEXT_MAX_CHARS};

use crate::engine::Ctx;
use crate::fail::{already_exists, invalid, not_found, Fail};
use crate::ids::is_ulid;
use crate::model::{StoredItem, StoredStep};
use crate::text::one_line;

/// The item, or `not_found`.
pub(crate) fn load(ctx: &mut Ctx, id: &str) -> Result<StoredItem, Fail> {
    ctx.txn
        .get(id)?
        .ok_or_else(|| not_found("no such work item"))
}

/// Refuses a call made on an item of another kind (§2.1).
pub(crate) fn expect_kind(item: &StoredItem, kind: ItemKind) -> Result<(), Fail> {
    if item.kind == kind {
        Ok(())
    } else {
        Err(invalid(format!(
            "this call is for a {}, and the item is a {}",
            kind_word(kind),
            kind_word(item.kind)
        )))
    }
}

fn kind_word(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Task => "task",
        ItemKind::Bug => "bug",
        ItemKind::Request => "request",
        _ => "work item",
    }
}

/// Refuses a change to a deleted item (a tombstone).
pub(crate) fn not_deleted(item: &StoredItem) -> Result<(), Fail> {
    if item.deleted {
        Err(invalid("the item is deleted"))
    } else {
        Ok(())
    }
}

/// Refuses a client-made id that isn't a ULID.
pub(crate) fn require_ulid(field: &str, id: &str) -> Result<(), Fail> {
    if is_ulid(id) {
        Ok(())
    } else {
        Err(invalid(format!("`{field}` must be a ULID")))
    }
}

/// Refuses the same client-made id twice in one call.
pub(crate) fn distinct<'a>(ids: impl IntoIterator<Item = &'a str>) -> Result<(), Fail> {
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(invalid(format!("the id `{id}` is given twice")));
        }
    }
    Ok(())
}

/// Refuses a client-made id that is taken (`already_exists`).
pub(crate) fn unused(ctx: &mut Ctx, ids: &[&str]) -> Result<(), Fail> {
    for id in ids {
        if ctx.txn.id_in_use(id)? {
            return Err(already_exists(format!("the id `{id}` is already used")));
        }
    }
    Ok(())
}

/// A priority a writer may send (never `Unknown`).
pub(crate) fn known_priority(priority: Option<Priority>) -> Result<Option<Priority>, Fail> {
    match priority {
        Some(Priority::Unknown) => Err(invalid("`priority` must be urgent, high, normal or low")),
        Some(p @ (Priority::Urgent | Priority::High | Priority::Normal | Priority::Low)) => {
            Ok(Some(p))
        }
        Some(_) => Err(invalid("`priority` must be urgent, high, normal or low")),
        None => Ok(None),
    }
}

/// New checklist steps, checked: ULID ids given once, text 1 to 200 characters on one line, and
/// at most [`WORK_MAX_STEPS`] live steps on the item afterwards (`live_now` there already).
pub(crate) fn new_steps(steps: &[NewStep], live_now: usize) -> Result<Vec<StoredStep>, Fail> {
    if live_now + steps.len() > WORK_MAX_STEPS {
        return Err(invalid(format!(
            "an item has at most {WORK_MAX_STEPS} checklist steps"
        )));
    }
    distinct(steps.iter().map(|s| s.step_id.as_str()))?;
    steps
        .iter()
        .map(|s| {
            require_ulid("step_id", &s.step_id)?;
            let text = one_line("text", &s.text, WORK_STEP_TEXT_MAX_CHARS)?;
            Ok(StoredStep::new(s.step_id.clone(), text, s.required))
        })
        .collect()
}
