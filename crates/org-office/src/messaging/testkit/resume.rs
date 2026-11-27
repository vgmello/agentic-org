//! In-memory [`MessagingRead`] resume queries for [`super::MessagingTables`] (`TASK-220`).

use std::cmp::Reverse;

use org_proto::msg::{is_urgent, MessageLifecycleState, KIND_REQUEST, OFFICE_SENDER};
use org_proto::resume::{ResumeInboxItem, ResumeInboxSummary};
use org_store::Id;

use super::super::ask_card::parse_ask_card;
use super::super::records::MessageRecord;
use super::MessagingTables;

impl MessagingTables {
    pub(super) fn unread_inbox_summary(
        &self,
        employee_id: &Id,
        limit: usize,
    ) -> ResumeInboxSummary {
        let mut unread_count: u64 = 0;
        let mut urgent_count: u64 = 0;
        let mut rows: Vec<(u64, String, ResumeInboxItem)> = Vec::new();
        for state in self.recipient_states.values() {
            if state.employee_id != *employee_id || state.state != MessageLifecycleState::Unread {
                continue;
            }
            let Some((local_seq, msg)) = self.messages.get(&state.message_id) else {
                continue;
            };
            let urgent = is_urgent(&msg.kind, msg.priority);
            unread_count = unread_count.saturating_add(1);
            if urgent {
                urgent_count = urgent_count.saturating_add(1);
            }
            let sort_seq = if *local_seq > 0 {
                *local_seq
            } else {
                msg.origin_sequence
            };
            let from_member_id = msg
                .sender_employee_id
                .as_ref()
                .map(|id| id.as_str().to_owned())
                .unwrap_or_else(|| OFFICE_SENDER.to_owned());
            rows.push((
                sort_seq,
                msg.id.clone(),
                ResumeInboxItem {
                    id: msg.id.clone(),
                    conversation_id: msg.conversation_id.as_str().to_owned(),
                    from_member_id,
                    from_display_name: msg.sender_display_name.clone(),
                    kind: msg.kind.clone(),
                    priority: msg.priority,
                    urgent,
                    subject: msg.subject.clone(),
                    delivered_because: state.delivered_because,
                    created_at: msg.created_at.clone(),
                },
            ));
        }
        rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
        rows.truncate(limit);
        let items: Vec<ResumeInboxItem> = rows.into_iter().map(|(_, _, row)| row).collect();
        let truncated = unread_count > items.len() as u64;
        ResumeInboxSummary {
            unread_count,
            urgent_count,
            items,
            truncated,
        }
    }

    fn is_answered(&self, message_id: &str) -> bool {
        self.answers
            .iter()
            .any(|((mid, _), ans)| mid == message_id && ans.answer.is_some())
    }

    fn is_active_recipient(&self, message_id: &str, employee_id: &Id) -> bool {
        self.recipient_states
            .get(&(message_id.to_owned(), employee_id.clone()))
            .is_some_and(|st| {
                matches!(
                    st.state,
                    MessageLifecycleState::Unread
                        | MessageLifecycleState::Read
                        | MessageLifecycleState::Snoozed
                )
            })
    }

    fn ask_is_unexpired(card: &super::super::ask_card::AskCard, now_ms: Option<u64>) -> bool {
        card.required
            || now_ms
                .zip(crate::time::parse_rfc3339_ms(&card.wait_until))
                .is_some_and(|(now, until)| until > now)
    }

    fn ask_targets_caller(
        card: &super::super::ask_card::AskCard,
        employee_id: &Id,
        caller_is_recipient: bool,
        caller_is_person: bool,
    ) -> bool {
        let addressed = card
            .addressed_to
            .as_deref()
            .is_none_or(|target| target == employee_id.as_str());
        let approval_ok = !card.approval || caller_is_person;
        caller_is_recipient && addressed && approval_ok
    }

    fn ask_candidate_for_employee(
        &self,
        msg: &MessageRecord,
        employee_id: &Id,
        now_ms: Option<u64>,
        caller_is_person: bool,
    ) -> Option<(MessageRecord, bool)> {
        if msg.kind != KIND_REQUEST || self.is_answered(&msg.id) {
            return None;
        }
        let card = parse_ask_card(msg)?;
        if !Self::ask_is_unexpired(&card, now_ms) {
            return None;
        }
        let is_sender = msg.sender_employee_id.as_ref() == Some(employee_id);
        let is_recipient = self.is_active_recipient(&msg.id, employee_id);
        let awaiting = Self::ask_targets_caller(&card, employee_id, is_recipient, caller_is_person);
        (is_sender || awaiting).then(|| (msg.clone(), is_recipient))
    }

    pub(super) fn open_asks_for_employee(
        &self,
        employee_id: &Id,
        now_utc: &str,
        caller_is_person: bool,
        limit: usize,
    ) -> Vec<(MessageRecord, bool)> {
        if limit == 0 {
            return Vec::new();
        }
        let now_ms = crate::time::parse_rfc3339_ms(now_utc);
        let mut candidates: Vec<(MessageRecord, bool)> = self
            .messages
            .values()
            .filter_map(|(_, msg)| {
                self.ask_candidate_for_employee(msg, employee_id, now_ms, caller_is_person)
            })
            .collect();
        candidates.sort_by_key(|(m, _)| (Reverse(m.created_at.clone()), Reverse(m.id.clone())));
        candidates.truncate(limit);
        candidates
    }
}
