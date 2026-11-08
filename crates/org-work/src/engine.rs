//! One work write, start to finish, inside one [`crate::WorkStore::transact`]: the app-only
//! check, the change-id lookup, the age checks, then **decide** (read, check every rule and
//! compute the whole outcome, automatic changes included) and only then **write** (§3.2). A
//! refusal has nothing to undo: it writes only its change-result row. Running it again on the
//! same state gives the same answer, so the writer may re-run it.

use std::collections::HashMap;

use org_proto::org::{AgentStatus, MemberKind, Position};
use org_proto::work::{
    EventRef, WorkAccepted, WorkChangedData, WorkEvent, WorkEventKind, WorkStatus,
    WORK_PARAMS_MAX_BYTES,
};
use org_proto::{ApiError, ErrorCode};

use crate::aspects::{aspects_of, Aspect};
use crate::call::{Caller, WorkCall};
use crate::fail::{conflict, invalid, unknown_recipient, Fail};
use crate::hash::{canonical_json, params_hash};
use crate::ids::ulid_time_ms;
use crate::model::{StoredItem, StoredResult};
use crate::ports::{Clock, NumberScope, OrgView, StoreError, WorkTxn};
use crate::time::format_ms;
use crate::{checklist, feed, items, tasks, undo};

const MINUTE_MS: u64 = 60_000;
const DAY_MS: u64 = 24 * 60 * MINUTE_MS;
/// A change id dated more than this long ago is `expired` (unless its answer is stored).
pub const CHANGE_ID_MAX_AGE_MS: u64 = 29 * DAY_MS;
/// A change id dated more than this far ahead of the office's clock is `clock_skew`.
pub const CHANGE_ID_MAX_AHEAD_MS: u64 = 5 * MINUTE_MS;
/// How long an answer is kept, after its change id's time.
pub const CHANGE_RESULT_KEEP_MS: u64 = 30 * DAY_MS;

/// What one write request came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TxnOutput {
    /// Committed now. Answer the caller with `accepted`, then send each `changed` part to
    /// subscribers as `work.changed` (none when the call changed nothing).
    Committed {
        accepted: WorkAccepted,
        changed: Vec<WorkChangedData>,
    },
    /// Refused now. Answer the caller with it; nothing to send.
    Refused(ApiError),
    /// A retry of an earlier call: its stored answer, unchanged. Nothing to send.
    Replayed(Result<WorkAccepted, ApiError>),
}

impl TxnOutput {
    /// The answer to give the caller.
    pub fn answer(&self) -> Result<&WorkAccepted, &ApiError> {
        match self {
            Self::Committed { accepted, .. } => Ok(accepted),
            Self::Refused(e) => Err(e),
            Self::Replayed(r) => r.as_ref(),
        }
    }
}

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

/// Runs one work write as the body of [`crate::WorkStore::transact`]: decide, then write.
/// `office_id` is the office running it (the owner of what it creates, phase 1).
///
/// The order (§2.3): `not_authorized` (app-only calls on another connection); the change-id
/// lookup (the same id and params hash get the stored answer without re-validating; the same id
/// with other params is `invalid_request`); `expired`; `clock_skew`; then the call's own rules
/// in the design's error order, with `too_large` right after `not_found`.
pub fn execute(
    org: &dyn OrgView,
    txn: &mut dyn WorkTxn,
    clock: &dyn Clock,
    office_id: &str,
    caller: &Caller,
    call: &WorkCall,
) -> Result<TxnOutput, StoreError> {
    if call.app_only() && !caller.via_app {
        return Ok(TxnOutput::Refused(ApiError::new(
            ErrorCode::NotAuthorized,
            format!("`{}` can only be made from the app", call.method()),
        )));
    }
    // An agent's connection acts as an agent that isn't offboarded, and only the app's acts as
    // the owner or a person (§2.2). The office binds connections; this keeps the rule true
    // inside the library too.
    let member = org.member(&caller.member_id)?;
    let may_act = match &member {
        Some(m) if caller.via_app => m.owner || m.kind == MemberKind::Person,
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
        return Ok(TxnOutput::Refused(ApiError::new(
            ErrorCode::NotAuthorized,
            "this connection can't act as that member",
        )));
    }
    let params = call.params_json();
    let hash = params_hash(call.method(), &caller.member_id, &params);
    let change_id = call.change_id();
    if let Some(stored) = txn.change_result(change_id)? {
        if stored.params_hash == hash {
            return Ok(TxnOutput::Replayed(stored.answer));
        }
        return Ok(TxnOutput::Refused(ApiError::new(
            ErrorCode::InvalidRequest,
            "change id reused with other params",
        )));
    }
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
            let (accepted, changed) = write(&mut ctx, plan, change_id)?;
            ctx.txn
                .put_change_result(change_id, &stored(Ok(accepted.clone())))?;
            Ok(TxnOutput::Committed { accepted, changed })
        }
    }
}

fn decide(ctx: &mut Ctx, call: &WorkCall) -> Result<Plan, Fail> {
    match call {
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
    }
}

fn write(
    ctx: &mut Ctx,
    plan: Plan,
    change_id: &str,
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
            items: plan.unchanged.iter().map(StoredItem::summary).collect(),
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
    let position = ctx
        .txn
        .record_change(ctx.org_position, &refs, Some(change_id))?;
    let accepted = WorkAccepted {
        status: WorkStatus::Committed,
        position: Some(position),
        items: drafts.iter().map(|d| d.item.summary()).collect(),
        features: vec![],
        docs: vec![],
    };
    let changed = feed::parts(position, Some(change_id), &drafts);
    Ok((accepted, changed))
}

/// What a decision reads and who it acts for.
pub(crate) struct Ctx<'a> {
    pub org: &'a dyn OrgView,
    pub txn: &'a mut dyn WorkTxn,
    pub now_ms: u64,
    pub office_id: &'a str,
    pub caller: &'a Caller,
    pub org_position: Position,
}

/// The decided outcome of a call: the items it changes, each with its new state and the events
/// it records, in order; and the items it answers with although it changed nothing.
#[derive(Default)]
pub(crate) struct Plan {
    pub drafts: Vec<Draft>,
    pub unchanged: Vec<StoredItem>,
}

impl Plan {
    pub fn of(drafts: Vec<Draft>) -> Self {
        Self {
            drafts,
            unchanged: vec![],
        }
    }

    /// A call that changes nothing; it answers with `item` as it is.
    pub fn unchanged(item: StoredItem) -> Self {
        Self {
            drafts: vec![],
            unchanged: vec![item],
        }
    }
}

/// Who made an event.
pub(crate) enum Origin {
    /// The caller (the bound member).
    Caller,
    /// An automatic change (§2.4), caused by this event; undoable for an hour.
    Auto(EventRef),
    /// The office's bookkeeping (waiting spans): no actor, not automatic, never undone.
    System(EventRef),
    /// The caller undoing this automatic event (`cause` names it).
    Undo(EventRef),
}

/// One item's new state and the events that got it there, built one event at a time.
pub(crate) struct Draft {
    pub item: StoredItem,
    pub events: Vec<WorkEvent>,
    /// Allocate the item's number at write time (a create).
    pub number_scope: Option<NumberScope>,
    at_ms: u64,
    actor_id: String,
    org_position: Position,
}

impl Ctx<'_> {
    /// A draft of a change to an existing item. Its events are dated now, but never before the
    /// item's previous event (§2.7).
    pub fn draft(&self, item: StoredItem) -> Draft {
        Draft {
            at_ms: self.now_ms.max(item.updated_at_ms),
            item,
            events: vec![],
            number_scope: None,
            actor_id: self.caller.member_id.clone(),
            org_position: self.org_position,
        }
    }

    /// A draft of a new item (version 0 until its `Created` event), numbered at write time.
    pub fn draft_new(&self, item: StoredItem) -> Draft {
        let number_scope = NumberScope::of(item.kind, item.project_id.as_deref());
        Draft {
            number_scope,
            ..self.draft(item)
        }
    }

    pub fn caller_id(&self) -> &str {
        &self.caller.member_id
    }

    /// Refuses a stale `expected_version` (§2.3): above the item's version is
    /// `invalid_request`; an event after it that changed an aspect `wants` is `conflict`.
    pub fn check_expected(
        &mut self,
        item: &StoredItem,
        expected: u64,
        wants: impl Fn(&Aspect) -> bool,
    ) -> Result<(), Fail> {
        if expected > item.version {
            return Err(invalid("`expected_version` is ahead of the item's version"));
        }
        if expected == item.version {
            return Ok(());
        }
        let events = self.txn.events_since(&item.id, expected)?;
        for aspects in self.aspects(&item.id, &events)? {
            if aspects.iter().any(|a| *a == Aspect::Any || wants(a)) {
                return Err(conflict("someone changed this item since you last saw it"));
            }
        }
        Ok(())
    }

    /// The aspects of each of `events` (an item's), looking up the events `Undone` ones undid.
    pub fn aspects(
        &mut self,
        item_id: &str,
        events: &[WorkEvent],
    ) -> Result<Vec<Vec<Aspect>>, StoreError> {
        let mut known: HashMap<u64, WorkEvent> =
            events.iter().map(|e| (e.version, e.clone())).collect();
        for e in events {
            if let WorkEventKind::Undone { event_version } = e.change {
                if !known.contains_key(&event_version) && event_version > 0 {
                    if let Some(first) = self
                        .txn
                        .events_since(item_id, event_version - 1)?
                        .into_iter()
                        .next()
                    {
                        known.insert(event_version, first);
                    }
                }
            }
        }
        Ok(events
            .iter()
            .map(|e| aspects_of(e, &mut |v| known.get(&v).cloned()))
            .collect())
    }

    /// Checks that `member_id` may be assigned work (§2.1): an agent that isn't offboarded
    /// (active or paused), a person of the organization, or the owner.
    pub fn check_recipient(&self, member_id: &str) -> Result<(), Fail> {
        if member_id == self.org.owner_id()? {
            return Ok(());
        }
        let ok = match self.org.member(member_id)? {
            Some(m) if m.owner => true,
            Some(m) => match m.kind {
                MemberKind::Person => true,
                MemberKind::Agent => matches!(
                    m.agent.map(|a| a.status),
                    Some(AgentStatus::Active | AgentStatus::Paused)
                ),
                _ => false,
            },
            None => false,
        };
        if ok {
            Ok(())
        } else {
            Err(unknown_recipient(format!(
                "`{member_id}` isn't an agent or person of the organization who can get work"
            )))
        }
    }
}

impl Draft {
    /// Records one event on the item: the next version, dated by the draft.
    pub fn record(&mut self, change: WorkEventKind, origin: Origin) -> EventRef {
        self.item.version += 1;
        self.item.updated_at_ms = self.at_ms;
        if self.item.version == 1 {
            self.item.created_at_ms = self.at_ms;
        }
        let (actor_id, auto, cause) = match origin {
            Origin::Caller => (Some(self.actor_id.clone()), false, None),
            Origin::Auto(cause) => (None, true, Some(cause)),
            Origin::System(cause) => (None, false, Some(cause)),
            Origin::Undo(cause) => (Some(self.actor_id.clone()), false, Some(cause)),
        };
        self.events.push(WorkEvent {
            target_id: self.item.id.clone(),
            version: self.item.version,
            at: format_ms(self.at_ms),
            actor_id,
            auto,
            cause,
            org_position: self.org_position,
            change,
        });
        EventRef {
            target_id: self.item.id.clone(),
            version: self.item.version,
        }
    }

    /// The time the draft's events are dated.
    pub fn at_ms(&self) -> u64 {
        self.at_ms
    }
}
