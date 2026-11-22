//! Local triage and snooze resurfacing methods on [`Mailbox`] (`msg.defer`, `msg.unsnooze`,
//! `msg.done`, and scheduled/read-time due-snooze release, ADR-0003 & `TASK-81`).

use org_proto::msg::{
    MessageLifecycleState, MsgDeferParams, MsgDeferResult, MsgDoneParams, MsgDoneResult,
    MsgUnsnoozeParams, MsgUnsnoozeResult, NotifyReason, SnoozeTarget, MAX_ID_BYTES, MAX_NOTE_BYTES,
};

use crate::error::MailboxError;
use crate::mailbox::{epoch_ms, lock, optional_size, size, Mailbox};
use crate::model::LocalStateTransition;
use crate::time::{format_utc, parse_utc};

impl Mailbox {
    /// `msg.defer`: snooze a message addressed to the bound member until a time or its next
    /// break (ADR-0003 §2.3). It leaves the default inbox and every wait until it's due, and
    /// its `triage_gen` goes up. A time snooze gets a timer, so a held wait returns it when
    /// it comes due.
    pub fn defer(
        &self,
        bound_member_id: &str,
        params: MsgDeferParams,
    ) -> Result<MsgDeferResult, MailboxError> {
        let (_, position) = self.bound_member(bound_member_id)?;
        let triage = self.triage()?;
        size(&params.id, MAX_ID_BYTES)?;
        optional_size(params.note.as_deref(), MAX_NOTE_BYTES)?;
        let now_ms = self.clock.now_ms();
        let now = format_utc(now_ms).ok_or(MailboxError::StorageError)?;
        let (snoozed_until, snooze_after_task) = validate_defer_target(params.until, now_ms)?;
        let header = {
            let _ordered = lock(&self.triage_writes);
            let header = triage.defer_local(LocalStateTransition {
                snoozed_until,
                snooze_after_task,
                reason: params.note,
                ..self.transition(
                    &params.id,
                    bound_member_id,
                    MessageLifecycleState::Snoozed,
                    position,
                    &now,
                )
            })?;
            let mut timers = lock(&self.snoozes);
            match header.snoozed_until.as_deref() {
                Some(at) => timers.insert(
                    parse_utc(at).map_or(0, epoch_ms),
                    bound_member_id,
                    &header.id,
                ),
                None => timers.remove(bound_member_id, &header.id),
            }
            header
        };
        // A held wait recomputes its bound with the new timer.
        self.waiters.notify(bound_member_id);
        self.publish_state(&header, header.snooze_note.clone(), &now);
        Ok(MsgDeferResult {
            id: header.id,
            triage_gen: header.triage_gen,
            snoozed_until: header.snoozed_until,
            snooze_after_task: header.snooze_after_task,
            snooze_note: header.snooze_note,
        })
    }

    /// `msg.unsnooze`: release a snoozed message addressed to the bound member back to active
    /// `unread` early (`TASK-81`). Clears snooze fields, removes the recipient's timer, notifies
    /// waiters, and publishes `message.state_changed` for live SSE & badge updates.
    pub fn unsnooze(
        &self,
        bound_member_id: &str,
        params: MsgUnsnoozeParams,
    ) -> Result<MsgUnsnoozeResult, MailboxError> {
        let (_, position) = self.bound_member(bound_member_id)?;
        let triage = self.triage()?;
        size(&params.id, MAX_ID_BYTES)?;
        let now = self.now_utc()?;
        let header = {
            let _ordered = lock(&self.triage_writes);
            let header = triage.unsnooze_local(self.transition(
                &params.id,
                bound_member_id,
                MessageLifecycleState::Unread,
                position,
                &now,
            ))?;
            lock(&self.snoozes).remove(bound_member_id, &header.id);
            header
        };
        self.waiters.notify(bound_member_id);
        self.publish_state(&header, None, &now);
        Ok(MsgUnsnoozeResult {
            id: header.id,
            state: header.state,
            triage_gen: header.triage_gen,
        })
    }

    /// `msg.done`: close a message addressed to the bound member without replying. It never
    /// resurfaces. Done again is accepted (the store keeps the first close); `resolved_at`
    /// is this call's time, and its `state_changed` carries this call's note.
    pub fn done(
        &self,
        bound_member_id: &str,
        params: MsgDoneParams,
    ) -> Result<MsgDoneResult, MailboxError> {
        let (_, position) = self.bound_member(bound_member_id)?;
        let triage = self.triage()?;
        size(&params.id, MAX_ID_BYTES)?;
        optional_size(params.note.as_deref(), MAX_NOTE_BYTES)?;
        let now = self.now_utc()?;
        let header = {
            let _ordered = lock(&self.triage_writes);
            let header = triage.update_state_local(LocalStateTransition {
                reason: params.note.clone(),
                ..self.transition(
                    &params.id,
                    bound_member_id,
                    MessageLifecycleState::Done,
                    position,
                    &now,
                )
            })?;
            lock(&self.snoozes).remove(bound_member_id, &header.id);
            header
        };
        self.waiters.notify(bound_member_id);
        self.publish_state(&header, params.note, &now);
        Ok(MsgDoneResult {
            id: header.id,
            resolved_at: now,
        })
    }

    /// Releases any due time snoozes for `member` right now (before inbox/list reads, `TASK-81`).
    pub fn release_due_snoozes_for(&self, member: &str) -> Result<usize, MailboxError> {
        let now_ms = self.clock.now_ms();
        let now = format_utc(now_ms).ok_or(MailboxError::StorageError)?;
        self.release_due_snoozes(member, now_ms, &now)
    }

    /// Releases all due time snoozes across all members right now (`TASK-81` background resurfacer).
    pub fn release_all_due_snoozes(&self) -> Result<usize, MailboxError> {
        let Some(triage) = self.triage.as_deref() else {
            return Ok(0);
        };
        let now_ms = self.clock.now_ms();
        let now = format_utc(now_ms).ok_or(MailboxError::StorageError)?;
        let due_by_member = lock(&self.snoozes).take_all_due(epoch_ms(now_ms));
        if due_by_member.is_empty() {
            return Ok(0);
        }
        let sweep_before = format_utc(0).ok_or(MailboxError::StorageError)?;
        let mut total_released = 0usize;
        let mut first_err = None;
        for (member, due) in due_by_member {
            match triage.check_notify(&member, false, false, &sweep_before, &now) {
                Ok(candidates) => {
                    let released: Vec<_> = candidates
                        .into_iter()
                        .filter(|c| c.reason == NotifyReason::Due)
                        .collect();
                    if !released.is_empty() {
                        self.waiters.notify(&member);
                    }
                    for candidate in &released {
                        self.publish_state(&candidate.header, None, &now);
                    }
                    total_released += released.len();
                }
                Err(err) => {
                    let mut timers = lock(&self.snoozes);
                    for (at_ms, id) in due {
                        timers.restore(at_ms, &member, &id);
                    }
                    if first_err.is_none() {
                        first_err = Some(err);
                    }
                }
            }
        }
        if let Some(err) = first_err {
            return Err(err.into());
        }
        Ok(total_released)
    }

    /// Earliest active time-snooze deadline across all members in epoch milliseconds, if any.
    pub fn next_snooze_deadline_ms(&self) -> Option<i64> {
        lock(&self.snoozes).next_deadline()
    }

    /// Releases the member's due time snoozes (§2.2) through `check_notify`, before a wait
    /// peeks. Only when the timers say one is due, and only with triage.
    pub(crate) fn release_due_snoozes(
        &self,
        member: &str,
        now_ms: u64,
        now: &str,
    ) -> Result<usize, MailboxError> {
        let Some(triage) = self.triage.as_deref() else {
            return Ok(0);
        };
        let due = lock(&self.snoozes).take_due(member, epoch_ms(now_ms));
        if due.is_empty() {
            return Ok(0);
        }
        let sweep_before = format_utc(0).ok_or(MailboxError::StorageError)?;
        let candidates = match triage.check_notify(member, false, false, &sweep_before, now) {
            Ok(candidates) => candidates,
            Err(error) => {
                let mut timers = lock(&self.snoozes);
                for (at_ms, id) in due {
                    timers.restore(at_ms, member, &id);
                }
                return Err(error.into());
            }
        };
        let released: Vec<_> = candidates
            .into_iter()
            .filter(|candidate| candidate.reason == NotifyReason::Due)
            .collect();
        if !released.is_empty() {
            self.waiters.notify(member);
        }
        for candidate in &released {
            self.publish_state(&candidate.header, None, now);
        }
        Ok(released.len())
    }
}

fn validate_defer_target(
    until: SnoozeTarget,
    now_ms: u64,
) -> Result<(Option<String>, bool), MailboxError> {
    match until {
        SnoozeTarget::Until { at } => {
            let Some(at_ms) = parse_utc(&at).filter(|ms| *ms > now_ms) else {
                return Err(MailboxError::InvalidRequest);
            };
            let canonical = format_utc(at_ms).ok_or(MailboxError::StorageError)?;
            Ok((Some(canonical), false))
        }
        SnoozeTarget::AfterTask => Ok((None, true)),
        _ => Err(MailboxError::InvalidRequest),
    }
}
