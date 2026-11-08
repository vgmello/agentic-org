//! What organization changes mean for open bugs (§2.5), reconciled from **current state** so it
//! is idempotent: running it twice gives the same state, and it needs no knowledge of which
//! organization event came first.
//!
//! - **A deleted area**: the area's open bugs (not closed) move to the area's replacement
//!   (`replaced_by`, followed to a live area). Closed bugs keep the old area.
//! - **The suggested triager changed** (an area's lead or team, a team's lead, membership):
//!   each `new` bug gets or loses its triage waiting span (§6), so a bug whose triager is now
//!   (or no longer) the owner waits on the owner (or stops).
//!
//! Each change is a system event: no actor, not automatic (no Undo). The whole run is one
//! committed call (one origin row and one feed row, no change id). The office runs it inside
//! [`crate::WorkStore::transact`] after it applies an organization event, as the rest of §2.5's
//! cascades; the org cursor and the catch-up inside every write belong to that layer.

use org_proto::org::Position;
use org_proto::work::{FieldChange, ItemStatus, WorkAccepted, WorkEventKind, WorkStatus};
use serde_json::Value;

use crate::bugs::sync_triage;
use crate::call::Caller;
use crate::engine::{write, Ctx, Origin, Plan, TxnOutput};
use crate::ports::{Clock, OrgView, StoreError, WorkTxn};

/// The live area a deleted area's bugs belong in: its replacement, followed through further
/// deletions. `None` when there is no live one to go to (a broken chain).
fn live_area(org: &dyn OrgView, area_id: &str) -> Result<Option<String>, StoreError> {
    let mut id = area_id.to_owned();
    for _ in 0..8 {
        let Some(area) = org.area(&id)? else {
            return Ok(None);
        };
        if !area.deleted {
            return Ok(Some(area.id));
        }
        match area.replaced_by {
            Some(next) => id = next,
            None => return Ok(None),
        }
    }
    Ok(None)
}

/// Moves open bugs out of deleted areas and brings every open bug's triage span in line with
/// its suggested triager. Commits nothing (and answers with no position) when nothing needs
/// to change.
pub fn reconcile_bugs(
    org: &dyn OrgView,
    txn: &mut dyn WorkTxn,
    clock: &dyn Clock,
    office_id: &str,
) -> Result<TxnOutput, StoreError> {
    // No call, so no caller: the cascade's events have no actor (`Origin::Cascade`), and the
    // context only needs something to hold.
    let caller = Caller::agent("");
    let org_position: Position = org.applied_position()?;
    let mut ctx = Ctx {
        org,
        txn,
        now_ms: clock.now_ms(),
        office_id,
        caller: &caller,
        org_position,
    };
    let mut drafts = vec![];
    for bug in ctx.txn.open_bugs(None)? {
        let area_id = bug.bug.area_id.clone();
        let target = match &area_id {
            Some(id) => live_area(org, id)?,
            None => None,
        };
        let mut draft = ctx.draft(bug);
        if let (Some(from), Some(to)) = (area_id, target) {
            if from != to {
                draft.item.bug.area_id = Some(to.clone());
                draft.record(
                    WorkEventKind::FieldsChanged {
                        fields: vec![FieldChange {
                            field: "area_id".to_owned(),
                            from: Some(Value::String(from)),
                            to: Some(Value::String(to)),
                        }],
                    },
                    Origin::Cascade,
                );
            }
        }
        if draft.item.status == ItemStatus::New || !draft.item.waiting.is_empty() {
            sync_triage(org, &mut draft, Origin::Cascade).map_err(|f| match f {
                crate::fail::Fail::Store(e) => e,
                crate::fail::Fail::Refuse(e) => StoreError::new(e.message),
            })?;
        }
        if !draft.events.is_empty() {
            drafts.push(draft);
        }
    }
    if drafts.is_empty() {
        return Ok(TxnOutput::Committed {
            accepted: WorkAccepted {
                status: WorkStatus::Committed,
                position: None,
                items: vec![],
                features: vec![],
                docs: vec![],
            },
            changed: vec![],
        });
    }
    let (accepted, changed) = write(&mut ctx, Plan::of(drafts), None)?;
    Ok(TxnOutput::Committed { accepted, changed })
}
