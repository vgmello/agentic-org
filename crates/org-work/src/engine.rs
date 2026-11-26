//! One work write, start to finish, inside one [`crate::WorkStore::transact`]: the grant-only
//! check, the change-id lookup, the age checks, then **decide** (read, check every rule and
//! compute the whole outcome, automatic changes included) and only then **write** (§3.2). A
//! refusal has nothing to undo: it writes only its change-result row. Running it again on the
//! same state gives the same answer, so the writer may re-run it.

use org_proto::org::{AgentStatus, MemberKind};
use org_proto::work::{
    EventRef, ItemKind, LinkKind, WorkAccepted, WorkChangedData, WorkItemSummary, WorkStatus,
    WORK_PARAMS_MAX_BYTES,
};
use org_proto::{ApiError, ErrorCode};

use crate::call::{Caller, WorkCall};
use crate::decision::{Ctx, Draft, Plan};
use crate::fail::Fail;
use crate::hash::{canonical_json, params_hash};
use crate::ids::ulid_time_ms;
use crate::model::{StoredItem, StoredResult};
use crate::ports::TxnOutput;
use crate::ports::{Clock, OrgView, StoreError, WorkTxn};
use crate::{bugs, checklist, feed, items, tasks, undo};

const MINUTE_MS: u64 = 60_000;
const DAY_MS: u64 = 24 * 60 * MINUTE_MS;
/// A change id dated more than this long ago is `expired` (unless its answer is stored).
pub const CHANGE_ID_MAX_AGE_MS: u64 = 29 * DAY_MS;
/// A change id dated more than this far ahead of the office's clock is `clock_skew`.
pub const CHANGE_ID_MAX_AHEAD_MS: u64 = 5 * MINUTE_MS;
/// How long an answer is kept, after its change id's time.
pub const CHANGE_RESULT_KEEP_MS: u64 = 30 * DAY_MS;

/// Error codes whose answers are never stored: a later retry may succeed (§2.3).
fn never_stored(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::StorageError
            | ErrorCode::Unavailable
            | ErrorCode::NotBound
            | ErrorCode::TooLarge
            | ErrorCode::Expired
            | ErrorCode::ClockSkew
            | ErrorCode::NotAuthorized
    )
}

/// Read-only authorization and original-result prefix, shared with native replay.
/// `None` alone permits a new decision. No clock, allocation or write is performed.
pub fn lookup_existing(
    org: &dyn OrgView,
    caller: &Caller,
    call: &WorkCall,
    lookup: impl FnOnce(&str) -> Result<Option<StoredResult>, StoreError>,
) -> Result<Option<TxnOutput>, StoreError> {
    if call.grant_only() && !caller.via_grant {
        return Ok(Some(TxnOutput::Refused(WorkCall::grant_only_refusal(
            call.method(),
        ))));
    }
    // An agent's connection acts as an agent that isn't offboarded, and only a grant-bound
    // caller acts as the owner or a person (§2.2). The office binds connections; this keeps the rule true
    // inside the library too.
    let member = org.member(&caller.member_id)?;
    let may_act = match &member {
        Some(m) if caller.via_grant => m.owner || m.kind == MemberKind::Person,
        Some(m) => {
            !m.owner
                && m.kind == MemberKind::Agent
                && m.agent
                    .as_ref()
                    .is_some_and(|a| matches!(a.status, AgentStatus::Active | AgentStatus::Paused))
        }
        None => false,
    };
    if !may_act {
        return Ok(Some(TxnOutput::Refused(ApiError::new(
            ErrorCode::NotAuthorized,
            "this connection can't act as that member",
        ))));
    }
    let params = call.params_json();
    let hash = params_hash(call.method(), &caller.member_id, &params);
    let change_id = call.change_id();
    if let Some(stored) = lookup(change_id)? {
        if stored.params_hash == hash {
            return Ok(Some(TxnOutput::Replayed(stored.answer)));
        }
        return Ok(Some(TxnOutput::Refused(ApiError::new(
            ErrorCode::InvalidRequest,
            "change id reused with other params",
        ))));
    }
    Ok(None)
}

/// Runs one work write as the body of [`crate::WorkStore::transact`]: decide, then write.
/// `office_id` is the office running it (the owner of what it creates, phase 1).
///
/// The order (§2.3): `not_authorized` (grant-only calls on a connection that isn't grant-bound);
/// the change-id lookup (the same id and params hash get the stored answer without
/// re-validating; the same id with other params is `invalid_request`); `expired`; `clock_skew`; then the call's own rules
/// in the design's error order, with `too_large` right after `not_found`.
pub fn execute(
    org: &dyn OrgView,
    txn: &mut dyn WorkTxn,
    clock: &dyn Clock,
    office_id: &str,
    caller: &Caller,
    call: &WorkCall,
) -> Result<TxnOutput, StoreError> {
    if let Some(output) = lookup_existing(org, caller, call, |id| txn.change_result(id))? {
        return Ok(output);
    }
    let params = call.params_json();
    let hash = params_hash(call.method(), &caller.member_id, &params);
    let change_id = call.change_id();
    let Some(made_ms) = ulid_time_ms(change_id) else {
        return Ok(TxnOutput::Refused(ApiError::new(
            ErrorCode::InvalidRequest,
            "`change_id` must be a ULID",
        )));
    };
    let now_ms = clock.now_ms();
    if now_ms.saturating_sub(made_ms) > CHANGE_ID_MAX_AGE_MS {
        return Ok(TxnOutput::Refused(ApiError::new(
            ErrorCode::Expired,
            "the change id is too old",
        )));
    }
    if made_ms > now_ms.saturating_add(CHANGE_ID_MAX_AHEAD_MS) {
        return Ok(TxnOutput::Refused(ApiError::new(
            ErrorCode::ClockSkew,
            "the change id is dated ahead of the office's clock",
        )));
    }

    let mut ctx = Ctx {
        org,
        txn,
        now_ms,
        office_id,
        caller,
        org_position: org.applied_position()?,
    };
    let decided = match decide(&mut ctx, call) {
        Ok(plan) => Ok(plan),
        Err(Fail::Store(e)) => return Err(e),
        Err(Fail::Refuse(e)) => Err(e),
    };
    let decided = match decided {
        Err(e) if e.code == ErrorCode::NotFound => Err(e),
        _ if canonical_json(&params).len() > WORK_PARAMS_MAX_BYTES => Err(ApiError::new(
            ErrorCode::TooLarge,
            "the params are over the size limit",
        )),
        other => other,
    };
    let stored = |answer| StoredResult {
        params_hash: hash,
        answer,
        expires_at_ms: made_ms.saturating_add(CHANGE_RESULT_KEEP_MS),
    };
    match decided {
        Err(e) => {
            if !never_stored(e.code) {
                ctx.txn
                    .put_change_result(change_id, &stored(Err(e.clone())))?;
            }
            Ok(TxnOutput::Refused(e))
        }
        Ok(plan) => {
            let (accepted, changed) = write(&mut ctx, plan, Some(change_id))?;
            ctx.txn
                .put_change_result(change_id, &stored(Ok(accepted.clone())))?;
            Ok(TxnOutput::Committed { accepted, changed })
        }
    }
}

fn decide(ctx: &mut Ctx, call: &WorkCall) -> Result<Plan, Fail> {
    let plan = match call {
        WorkCall::TaskCreate(p) => tasks::create(ctx, p),
        WorkCall::TaskUpdate(p) => tasks::update(ctx, p),
        WorkCall::TaskAssign(p) => tasks::assign(ctx, p),
        WorkCall::TaskSetStatus(p) => tasks::set_status(ctx, p),
        WorkCall::Comment(p) => items::comment(ctx, p),
        WorkCall::Watch(p) => items::watch(ctx, p),
        WorkCall::Link(p) => items::link(ctx, p),
        WorkCall::Unlink(p) => items::unlink(ctx, p),
        WorkCall::Delete(p) => items::delete(ctx, p),
        WorkCall::Undo(p) => undo::undo(ctx, p),
        WorkCall::ChecklistAdd(p) => checklist::add(ctx, p),
        WorkCall::ChecklistEdit(p) => checklist::edit(ctx, p),
        WorkCall::ChecklistCheck(p) => checklist::check(ctx, p),
        WorkCall::ChecklistMove(p) => checklist::move_step(ctx, p),
        WorkCall::ChecklistRemove(p) => checklist::remove(ctx, p),
        WorkCall::ChecklistMakeTask(p) => checklist::make_task(ctx, p),
        WorkCall::ChecklistUnlinkTask(p) => checklist::unlink_task(ctx, p),
        WorkCall::BugReport(p) => bugs::report(ctx, p),
        WorkCall::BugUpdate(p) => bugs::update(ctx, p),
        WorkCall::BugTriage(p) => bugs::triage(ctx, p),
        WorkCall::BugAssign(p) => bugs::assign(ctx, p),
        WorkCall::BugMarkFixed(p) => bugs::mark_fixed(ctx, p),
        WorkCall::BugVerify(p) => bugs::verify(ctx, p),
        WorkCall::BugClose(p) => bugs::close(ctx, p),
        WorkCall::BugReopen(p) => bugs::reopen(ctx, p),
    }?;
    plan.validate_number_scope()?;
    Ok(plan)
}

/// The items as the wire shows them. A bug's summary lists its fix tasks (the live tasks with a
/// `fixes` link to it), which only the link index knows.
pub(crate) fn summaries<'a>(
    txn: &mut dyn WorkTxn,
    items: impl Iterator<Item = &'a StoredItem>,
) -> Result<Vec<WorkItemSummary>, StoreError> {
    items
        .map(|item| {
            if item.kind != ItemKind::Bug {
                return Ok(item.summary());
            }
            let fixed_by = txn
                .linked_to(&item.id, LinkKind::Fixes)?
                .into_iter()
                .filter(|t| !t.deleted)
                .map(|t| t.id)
                .collect();
            Ok(item.summary_with_fixed_by(fixed_by))
        })
        .collect()
}

pub(crate) fn write(
    ctx: &mut Ctx,
    plan: Plan,
    change_id: Option<&str>,
) -> Result<(WorkAccepted, Vec<WorkChangedData>), StoreError> {
    let mut drafts: Vec<Draft> = plan
        .drafts
        .into_iter()
        .filter(|d| !d.events.is_empty())
        .collect();
    if drafts.is_empty() {
        // Nothing changed (a tick to the state the step has, a watch already held): no event,
        // no feed row, no position.
        let accepted = WorkAccepted {
            status: WorkStatus::Committed,
            position: None,
            items: summaries(ctx.txn, plan.unchanged.iter())?,
            features: vec![],
            docs: vec![],
        };
        return Ok((accepted, vec![]));
    }
    for d in &mut drafts {
        if let Some(scope) = &d.number_scope {
            d.item.number = Some(ctx.txn.allocate_number(scope)?);
        }
        ctx.txn.put_item(&d.item, &d.events)?;
    }
    let refs: Vec<EventRef> = drafts
        .iter()
        .flat_map(|d| d.events.iter())
        .map(|e| EventRef {
            target_id: e.target_id.clone(),
            version: e.version,
        })
        .collect();
    let position = ctx.txn.record_change(ctx.org_position, &refs, change_id)?;
    // After the writes, so a bug's fix tasks are those the call left.
    let items = summaries(ctx.txn, drafts.iter().map(|d| &d.item))?;
    let accepted = WorkAccepted {
        status: WorkStatus::Committed,
        position: Some(position),
        items: items.clone(),
        features: vec![],
        docs: vec![],
    };
    let changed = feed::parts(position, change_id, &drafts, &items);
    Ok((accepted, changed))
}
