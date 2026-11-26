//! `org-mailbox` storage: local state transitions, deferral, unsnooze and notification checks.

use std::collections::BTreeMap;

use org_proto::msg::{MessageHeader, MessageLifecycleState, NotifyReason};
use org_proto::org::Position;
use org_proto::{ApiError, ErrorCode};
use org_store::{Ack, Change, Id, OfficeIdentity, Refusal, Store, StoreError};

use crate::messaging::{
    features::to_wire_header,
    ports::MessagingRead,
    records::{MessageRecord, RecipientStateRecord},
};
use crate::store::open::{OfficeEngine, StoreOp};

use super::mailbox_commit::{resolve_local_seq, upsert_state_change, upsert_states_batch};
use super::{storage_refusal, unwrap_reply};

#[derive(Debug, Clone)]
pub struct LocalTransitionInput {
    pub id: String,
    pub to_member_id: Id,
    pub state: MessageLifecycleState,
    pub snoozed_until: Option<String>,
    pub snooze_after_task: bool,
    pub reason: Option<String>,
    pub now_utc: String,
    pub owner_office_id: Id,
    pub origin_incarnation: String,
    pub org_position: Position,
    pub expired_waiting_person: bool,
}

pub struct MailboxUpdateStateOp {
    pub here: OfficeIdentity,
    pub input: LocalTransitionInput,
}

fn apply_local_transition(
    st: &mut RecipientStateRecord,
    inp: &LocalTransitionInput,
) -> Result<bool, Refusal> {
    let invalid = || {
        Refusal::rejected(
            serde_json::to_vec(&ApiError::new(
                ErrorCode::Conflict,
                "invalid state transition",
            ))
            .unwrap(),
        )
    };
    let mut mutated = true;
    match (st.state, inp.state) {
        (s, t)
            if s == t
                && matches!(
                    s,
                    MessageLifecycleState::Read
                        | MessageLifecycleState::Done
                        | MessageLifecycleState::Expired
                ) =>
        {
            mutated = false;
        }
        (
            MessageLifecycleState::Unread
            | MessageLifecycleState::Read
            | MessageLifecycleState::Snoozed,
            MessageLifecycleState::Snoozed,
        ) => {
            if inp.snoozed_until.is_some() == inp.snooze_after_task {
                return Err(invalid());
            }
            st.state = MessageLifecycleState::Snoozed;
            st.triage_generation = st.triage_generation.saturating_add(1);
            st.snoozed_until = inp.snoozed_until.clone();
            st.snooze_after_task = inp.snooze_after_task;
            st.snooze_note = inp.reason.clone();
            st.triaged_at = Some(inp.now_utc.clone());
            st.read_at = None;
        }
        (
            MessageLifecycleState::Unread
            | MessageLifecycleState::Read
            | MessageLifecycleState::Snoozed,
            MessageLifecycleState::Done,
        ) => {
            st.state = MessageLifecycleState::Done;
            st.snoozed_until = None;
            st.snooze_after_task = false;
            st.snooze_note = None;
            st.reason = inp.reason.clone();
        }
        (
            MessageLifecycleState::Unread | MessageLifecycleState::Snoozed,
            MessageLifecycleState::Read,
        ) => {
            st.state = MessageLifecycleState::Read;
            st.snoozed_until = None;
            st.snooze_after_task = false;
            st.snooze_note = None;
            st.read_at = Some(inp.now_utc.clone());
        }
        (MessageLifecycleState::Read, MessageLifecycleState::Unread) => {
            if inp.expired_waiting_person {
                return Err(invalid());
            }
            st.state = MessageLifecycleState::Unread;
            st.read_at = None;
        }
        (MessageLifecycleState::Unread, MessageLifecycleState::Unread) => {
            if inp.expired_waiting_person {
                st.expired_waiting_person = true;
            } else {
                mutated = false;
            }
        }
        (
            MessageLifecycleState::Unread | MessageLifecycleState::Snoozed,
            MessageLifecycleState::Expired,
        ) => {
            st.state = MessageLifecycleState::Expired;
            st.snoozed_until = None;
            st.snooze_after_task = false;
            st.snooze_note = None;
        }
        _ => return Err(invalid()),
    }
    Ok(mutated)
}

fn check_snooze_expiry_guard(
    msg: &MessageRecord,
    inp: &LocalTransitionInput,
) -> Result<(), Refusal> {
    if inp.state != MessageLifecycleState::Snoozed {
        return Ok(());
    }
    let Some(exp) = msg.expires_at.as_deref() else {
        return Ok(());
    };
    let (already_expired, snooze_past_expiry) = match (
        crate::time::parse_rfc3339_ms(exp),
        crate::time::parse_rfc3339_ms(&inp.now_utc),
    ) {
        (Some(e), Some(n)) => {
            let past = inp
                .snoozed_until
                .as_deref()
                .is_some_and(|at| crate::time::parse_rfc3339_ms(at).is_none_or(|a| a >= e));
            (e <= n, past)
        }
        // Fail closed on corrupted expires_at / now_utc.
        _ => (true, true),
    };
    if already_expired || snooze_past_expiry {
        return Err(Refusal::rejected(
            serde_json::to_vec(&ApiError::new(
                ErrorCode::InvalidRequest,
                "snoozed_until must be earlier than the ask expires_at deadline",
            ))
            .unwrap(),
        ));
    }
    Ok(())
}

fn stamp_local_sync(st: &mut RecipientStateRecord, inp: &LocalTransitionInput) {
    st.updated_at = inp.now_utc.clone();
    st.owner_office_id = inp.owner_office_id.clone();
    st.origin_incarnation = inp.origin_incarnation.clone();
    st.org_epoch = inp.org_position.epoch.max(1);
    st.org_sequence = inp.org_position.seq;
}

fn not_found_refusal() -> Refusal {
    Refusal::rejected(
        serde_json::to_vec(&ApiError::new(ErrorCode::NotFound, "message not found")).unwrap(),
    )
}

fn run_local_state_op<E: OfficeEngine>(
    store: &Store<E>,
    here: OfficeIdentity,
    input: LocalTransitionInput,
    enforce_expiry_guard: bool,
) -> Result<Result<MessageHeader, ApiError>, StoreError> {
    let out = std::sync::Arc::new(std::sync::Mutex::new(None));
    let slot = std::sync::Arc::clone(&out);
    let reply = store.submit(None, false, move |uow| {
        let inp = &input;
        let mut st = uow
            .view()
            .recipient_state(&inp.id, &inp.to_member_id)
            .map_err(|_| storage_refusal("read state"))?
            .ok_or_else(not_found_refusal)?;
        let msg = uow
            .view()
            .message(&inp.id)
            .map_err(|_| storage_refusal("read message"))?
            .ok_or_else(not_found_refusal)?;
        if enforce_expiry_guard {
            check_snooze_expiry_guard(&msg, inp)?;
        }
        let mutated = apply_local_transition(&mut st, inp)?;
        let mut change = Change::new(Ack::Committed);
        if mutated {
            stamp_local_sync(&mut st, inp);
            change = upsert_state_change(
                uow.view(),
                &here.organization_id,
                &st.conversation_id,
                &inp.to_member_id,
                st.clone(),
                change,
            )?;
        }
        let seq = resolve_local_seq(uow.view(), &msg).map_err(|_| storage_refusal("local_seq"))?;
        *slot.lock().unwrap_or_else(|p| p.into_inner()) =
            Some(to_wire_header(&msg, Some(&st), seq));
        Ok(change)
    });
    unwrap_reply(reply, out)
}

impl StoreOp for MailboxUpdateStateOp {
    type Out = Result<Result<MessageHeader, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        run_local_state_op(store, self.here, self.input, false)
    }
}

pub struct MailboxDeferOp {
    pub here: OfficeIdentity,
    pub input: LocalTransitionInput,
}

impl StoreOp for MailboxDeferOp {
    type Out = Result<Result<MessageHeader, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        run_local_state_op(store, self.here, self.input, true)
    }
}

pub struct MailboxUnsnoozeOp {
    pub here: OfficeIdentity,
    pub input: LocalTransitionInput,
}

fn apply_unsnooze_record(
    st: &mut RecipientStateRecord,
    inp: &LocalTransitionInput,
) -> Result<(), Refusal> {
    if st.state != MessageLifecycleState::Snoozed || inp.expired_waiting_person {
        return Err(Refusal::rejected(
            serde_json::to_vec(&ApiError::new(
                ErrorCode::Conflict,
                "message is not snoozed",
            ))
            .unwrap(),
        ));
    }
    st.state = MessageLifecycleState::Unread;
    st.triage_generation = st.triage_generation.saturating_add(1);
    st.snoozed_until = None;
    st.snooze_after_task = false;
    st.snooze_note = None;
    st.triaged_at = Some(inp.now_utc.clone());
    st.read_at = None;
    stamp_local_sync(st, inp);
    Ok(())
}

impl StoreOp for MailboxUnsnoozeOp {
    type Out = Result<Result<MessageHeader, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let inp = &self.input;
            let mut st = uow
                .view()
                .recipient_state(&inp.id, &inp.to_member_id)
                .map_err(|_| storage_refusal("read state"))?
                .ok_or_else(not_found_refusal)?;
            let msg = uow
                .view()
                .message(&inp.id)
                .map_err(|_| storage_refusal("read message"))?
                .ok_or_else(not_found_refusal)?;
            apply_unsnooze_record(&mut st, inp)?;
            let change = upsert_state_change(
                uow.view(),
                &self.here.organization_id,
                &st.conversation_id,
                &inp.to_member_id,
                st.clone(),
                Change::new(Ack::Committed),
            )?;
            let seq =
                resolve_local_seq(uow.view(), &msg).map_err(|_| storage_refusal("local_seq"))?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) =
                Some(to_wire_header(&msg, Some(&st), seq));
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

#[derive(Debug, Clone)]
pub struct NotifyCandidateOut {
    pub header: MessageHeader,
    pub reason: NotifyReason,
}

pub struct MailboxCheckNotifyOp {
    pub here: OfficeIdentity,
    pub to_member_id: Id,
    pub after_task: bool,
    pub urgent_only: bool,
    pub sweep_before_utc: String,
    pub now_utc: String,
}

fn evaluate_notify_state(
    st: &mut RecipientStateRecord,
    msg: &MessageRecord,
    after_task: bool,
    sweep_before_utc: &str,
    now_utc: &str,
) -> (Option<NotifyReason>, bool) {
    let urgent = org_proto::msg::is_urgent(&msg.kind, msg.priority);
    if st.state == MessageLifecycleState::Snoozed {
        // Per TriageStore::check_notify contract, an unreadable snoozed_until releases as due.
        let time_due = st.snoozed_until.as_deref().is_some_and(|at| {
            match (
                crate::time::parse_rfc3339_ms(at),
                crate::time::parse_rfc3339_ms(now_utc),
            ) {
                (Some(a), Some(n)) => a <= n,
                _ => true,
            }
        });
        let task_due = after_task && st.snooze_after_task;
        if time_due || task_due {
            st.state = MessageLifecycleState::Unread;
            st.triage_generation = st.triage_generation.saturating_add(1);
            st.snoozed_until = None;
            st.snooze_after_task = false;
            st.snooze_note = None;
            st.updated_at = now_utc.to_owned();
            let reason = if time_due {
                NotifyReason::Due
            } else {
                NotifyReason::AfterTask
            };
            return (Some(reason), true);
        }
        return (None, false);
    }
    if st.state == MessageLifecycleState::Unread {
        if after_task || urgent {
            return (Some(NotifyReason::New), false);
        }
        if st.triaged_at.is_none() && msg.created_at.as_str() < sweep_before_utc {
            return (Some(NotifyReason::Sweep), false);
        }
    }
    (None, false)
}

impl StoreOp for MailboxCheckNotifyOp {
    type Out = Result<Result<Vec<NotifyCandidateOut>, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let states = uow
                .view()
                .recipient_states_for_employee(&self.to_member_id)
                .map_err(|_| storage_refusal("list states"))?;
            let mut change = Change::new(Ack::Committed);
            let mut candidates = Vec::new();
            let mut woken_by_conv: BTreeMap<Id, Vec<RecipientStateRecord>> = BTreeMap::new();
            for mut st in states {
                let Some(msg) = uow
                    .view()
                    .message(&st.message_id)
                    .map_err(|_| storage_refusal("read msg"))?
                else {
                    continue;
                };
                let (reason, woke) = evaluate_notify_state(
                    &mut st,
                    &msg,
                    self.after_task,
                    &self.sweep_before_utc,
                    &self.now_utc,
                );
                if woke {
                    woken_by_conv
                        .entry(st.conversation_id.clone())
                        .or_default()
                        .push(st.clone());
                }
                let urgent = org_proto::msg::is_urgent(&msg.kind, msg.priority);
                if let Some(r) = reason.filter(|_| !self.urgent_only || urgent) {
                    let seq = resolve_local_seq(uow.view(), &msg)
                        .map_err(|_| storage_refusal("local_seq"))?;
                    candidates.push(NotifyCandidateOut {
                        header: to_wire_header(&msg, Some(&st), seq),
                        reason: r,
                    });
                }
            }
            for (conv_id, woken) in woken_by_conv {
                change = upsert_states_batch(
                    uow.view(),
                    &self.here.organization_id,
                    &conv_id,
                    &self.to_member_id,
                    woken,
                    change,
                )?;
            }
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(candidates);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}
