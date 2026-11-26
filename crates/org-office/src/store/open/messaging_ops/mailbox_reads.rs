//! `org-mailbox` storage reads: one message, unread peeks, headers, ask replies and snooze
//! deadlines.

use org_proto::msg::{
    HeaderPage, MessageHeader, MessageLifecycleState, KIND_DELIVERY_FAILED, OFFICE_SENDER,
};
use org_store::{Id, StoreError};

use crate::messaging::{features::to_wire_header, ports::MessagingRead};
use crate::store::open::ReadOp;
use crate::tasks::ports::TasksRead;

use super::mailbox_commit::{resolve_local_seq, stored_from_records, MailboxStored};

pub struct GetMailboxMessageRead(pub String);

impl ReadOp for GetMailboxMessageRead {
    type Out = Option<MailboxStored>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let Some(msg) = snapshot.message(&self.0)? else {
            return Ok(None);
        };
        let states = snapshot.all_recipient_states()?;
        let st = states.into_iter().find(|s| s.message_id == msg.id);
        let seq = resolve_local_seq(snapshot, &msg)?;
        Ok(Some(stored_from_records(&msg, st.as_ref(), seq)))
    }
}

#[derive(Debug, Clone)]
pub struct PeekUnreadOut {
    pub message: Option<MailboxStored>,
    pub expired_ids: Vec<String>,
}

pub struct PeekUnreadRead {
    pub to_member_id: Id,
    pub from_member_id: Option<String>,
    pub urgent_only: bool,
    pub now_utc: String,
}

impl ReadOp for PeekUnreadRead {
    type Out = PeekUnreadOut;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let mut states = snapshot.recipient_states_for_employee(&self.to_member_id)?;
        states.retain(|s| s.state == MessageLifecycleState::Unread);
        states.sort_by(|a, b| {
            a.origin_sequence
                .cmp(&b.origin_sequence)
                .then_with(|| a.message_id.cmp(&b.message_id))
        });
        let now_ms = crate::time::parse_rfc3339_ms(&self.now_utc);
        let mut expired_ids = Vec::new();
        let mut chosen = None;
        for st in states {
            let Some(msg) = snapshot.message(&st.message_id)? else {
                continue;
            };
            let from = msg
                .sender_employee_id
                .as_ref()
                .map(|i| i.as_str())
                .unwrap_or(OFFICE_SENDER);
            if let Some(filter) = &self.from_member_id {
                if from != filter {
                    continue;
                }
            }
            if self.urgent_only && !org_proto::msg::is_urgent(&msg.kind, msg.priority) {
                continue;
            }
            let is_expired = if st.expired_waiting_person {
                false
            } else if let Some(exp) = &msg.expires_at {
                let skew = (msg.ttl_ms.unwrap_or(0) / 4).min(2_000);
                match (crate::time::parse_rfc3339_ms(exp), now_ms) {
                    (Some(exp_ms), Some(now)) => now >= exp_ms.saturating_add(skew),
                    _ => true,
                }
            } else {
                false
            };
            if is_expired {
                if expired_ids.len() < 100 {
                    expired_ids.push(msg.id);
                }
                continue;
            }
            if chosen.is_none() {
                let seq = resolve_local_seq(snapshot, &msg)?;
                chosen = Some(stored_from_records(&msg, Some(&st), seq));
            }
        }
        Ok(PeekUnreadOut {
            message: chosen,
            expired_ids,
        })
    }
}

pub struct ListMailboxHeadersRead {
    pub scope_inbox: Option<Id>,
    pub scope_participant: Option<Id>,
    pub from_member_id: Option<String>,
    pub thread_id: Option<String>,
    pub unread_only: bool,
    pub include_snoozed: bool,
    pub cursor: Option<u64>,
    pub limit: u32,
}

impl ListMailboxHeadersRead {
    fn matches_common_filters(&self, hdr: &MessageHeader) -> bool {
        if self.unread_only && hdr.state != MessageLifecycleState::Unread {
            return false;
        }
        if !self.include_snoozed && hdr.state == MessageLifecycleState::Snoozed {
            return false;
        }
        if self
            .thread_id
            .as_deref()
            .is_some_and(|t| hdr.thread_id != t)
        {
            return false;
        }
        if self.cursor.is_some_and(|c| hdr.local_seq >= c) {
            return false;
        }
        true
    }

    fn collect_inbox<S: MessagingRead>(
        &self,
        snapshot: &S,
        inbox_emp: &Id,
    ) -> Result<Vec<MessageHeader>, StoreError> {
        let mut headers = Vec::new();
        for st in snapshot.recipient_states_for_employee(inbox_emp)? {
            let Some(msg) = snapshot.message(&st.message_id)? else {
                continue;
            };
            let seq = resolve_local_seq(snapshot, &msg)?;
            let hdr = to_wire_header(&msg, Some(&st), seq);
            if !self.matches_common_filters(&hdr) {
                continue;
            }
            if self
                .from_member_id
                .as_deref()
                .is_some_and(|f| hdr.from_member_id != f)
            {
                continue;
            }
            headers.push(hdr);
        }
        Ok(headers)
    }

    fn collect_all_or_participant<S: MessagingRead>(
        &self,
        snapshot: &S,
    ) -> Result<Vec<MessageHeader>, StoreError> {
        let mut headers = Vec::new();
        let states = snapshot.all_recipient_states()?;
        for msg in snapshot.all_messages()? {
            let st = states.iter().find(|s| s.message_id == msg.id);
            let seq = resolve_local_seq(snapshot, &msg)?;
            let hdr = to_wire_header(&msg, st, seq);
            if !self.matches_common_filters(&hdr) {
                continue;
            }
            if let Some(part) = &self.scope_participant {
                let p = part.as_str();
                if hdr.from_member_id != p && hdr.to_member_id != p {
                    continue;
                }
            }
            headers.push(hdr);
        }
        Ok(headers)
    }
}

impl ReadOp for ListMailboxHeadersRead {
    type Out = HeaderPage;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let mut headers = match &self.scope_inbox {
            Some(inbox_emp) => self.collect_inbox(snapshot, inbox_emp)?,
            None => self.collect_all_or_participant(snapshot)?,
        };
        headers.sort_by(|a, b| b.local_seq.cmp(&a.local_seq).then_with(|| b.id.cmp(&a.id)));
        let more = headers.len() > self.limit as usize;
        headers.truncate(self.limit as usize);
        let next_cursor = if more {
            headers.last().map(|h| h.local_seq)
        } else {
            None
        };
        Ok(HeaderPage {
            headers,
            next_cursor,
        })
    }
}

pub struct FindReplyForAskRead {
    pub ask_id: String,
    pub responder_id: String,
}

impl ReadOp for FindReplyForAskRead {
    type Out = Option<MailboxStored>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let Some(ask) = snapshot.message(&self.ask_id)? else {
            return Ok(None);
        };
        let asker = ask
            .sender_employee_id
            .as_ref()
            .map(|i| i.as_str())
            .unwrap_or(OFFICE_SENDER);
        let states = snapshot.all_recipient_states()?;
        for msg in snapshot.all_messages()? {
            if msg.reply_to_message_id.as_deref() != Some(self.ask_id.as_str()) {
                continue;
            }
            let from = msg
                .sender_employee_id
                .as_ref()
                .map(|i| i.as_str())
                .unwrap_or(OFFICE_SENDER);
            let st = states.iter().find(|s| s.message_id == msg.id);
            let to = st.map(|s| s.employee_id.as_str()).unwrap_or("");
            let is_match = to == asker
                && (from == self.responder_id
                    || (from == OFFICE_SENDER && msg.kind == KIND_DELIVERY_FAILED));
            if is_match {
                let seq = resolve_local_seq(snapshot, &msg)?;
                return Ok(Some(stored_from_records(&msg, st, seq)));
            }
        }
        Ok(None)
    }
}

pub struct ListActiveSnoozeDeadlinesRead;

impl ReadOp for ListActiveSnoozeDeadlinesRead {
    type Out = Vec<(String, String, String)>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let mut out = Vec::new();
        for st in snapshot.all_recipient_states()? {
            if st.state == MessageLifecycleState::Snoozed {
                if let Some(at) = st.snoozed_until {
                    out.push((st.message_id, st.employee_id.as_str().to_owned(), at));
                }
            }
        }
        Ok(out)
    }
}
