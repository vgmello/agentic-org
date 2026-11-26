//! What a decision reads and what it decides: the context a call is decided in ([`Ctx`]), the
//! outcome it comes to ([`Plan`]) and each changed item's new state with the events that got it
//! there ([`Draft`]). Every rule module decides into these; [`crate::engine`] runs the decision
//! and then writes the plan.

use std::collections::HashMap;

use org_proto::org::{AgentStatus, MemberKind, Position};
use org_proto::work::{EventRef, ItemKind, WorkEvent, WorkEventKind};
use org_proto::{ApiError, ErrorCode};

use crate::aspects::{aspects_of, Aspect};
use crate::call::Caller;
use crate::fail::{conflict, invalid, unknown_recipient, Fail};
use crate::model::StoredItem;
use crate::ports::{NumberScope, OrgView, StoreError, WorkTxn};
use crate::time::format_ms;

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
    /// Refuse numbered Task/Bug drafts without a project as InvalidRequest.
    /// NumberScope owns the scopes; callers validate before allocating keys or writing drafts.
    pub fn validate_number_scope(&self) -> Result<(), ApiError> {
        for draft in &self.drafts {
            let item = &draft.item;
            if matches!(item.kind, ItemKind::Task | ItemKind::Bug)
                && item.number.is_some()
                && NumberScope::of(item.kind, item.project_id.as_deref()).is_none()
            {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "a numbered task or bug must have a project",
                ));
            }
        }
        Ok(())
    }

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
#[derive(Clone)]
pub(crate) enum Origin {
    /// The caller (the bound member).
    Caller,
    /// An automatic change (§2.4), caused by this event; undoable for an hour.
    Auto(EventRef),
    /// The office's bookkeeping (waiting spans): no actor, not automatic, never undone.
    System(EventRef),
    /// The caller undoing this automatic event (`cause` names it).
    Undo(EventRef),
    /// A cascade from an organization change (§2.5): no actor, not automatic, no cause.
    Cascade,
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
            Origin::Cascade => (None, false, None),
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
