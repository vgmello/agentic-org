//! In-memory [`MessagingRead`] resume queries for [`super::MessagingTables`] (`TASK-220`).

use std::cmp::Reverse;

use org_proto::msg::{is_urgent, MessageLifecycleState, KIND_REQUEST, OFFICE_SENDER};
use org_proto::resume::{ResumeInboxItem, ResumeInboxSummary};
use org_store::Id;

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

    pub(super) fn open_asks_for_employee(
        &self,
        employee_id: &Id,
        limit: usize,
    ) -> Vec<(MessageRecord, bool)> {
        let mut candidates: Vec<(MessageRecord, bool)> = Vec::new();
        for (_, msg) in self.messages.values() {
            if msg.kind != KIND_REQUEST {
                continue;
            }
            let answered = self
                .answers
                .iter()
                .any(|((mid, _), ans)| mid == &msg.id && ans.answer.is_some());
            if answered {
                continue;
            }
            let is_sender = msg.sender_employee_id.as_ref() == Some(employee_id);
            let caller_is_recipient = self
                .recipient_states
                .get(&(msg.id.clone(), employee_id.clone()))
                .is_some_and(|st| {
                    matches!(
                        st.state,
                        MessageLifecycleState::Unread
                            | MessageLifecycleState::Read
                            | MessageLifecycleState::Snoozed
                    )
                });
            if is_sender || caller_is_recipient {
                candidates.push((msg.clone(), caller_is_recipient));
            }
        }
        candidates.sort_by_key(|(m, _)| (Reverse(m.created_at.clone()), Reverse(m.id.clone())));
        candidates.truncate(limit);
        candidates
    }
}
