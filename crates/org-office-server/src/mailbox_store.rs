//! Bridges `org_office::Office` (`org-store` messaging family on isolated `protected/messaging.db`)
//! to `org_mailbox::{MessageStore, TriageStore}` and `inbox::Source` (`B6`).

use std::sync::Arc;

use org_mailbox::{
    CommitMessageOutcome, HeaderQuery, HeaderScope, LocalStateTransition, MessageStore,
    NewStoredMessage, NotifyCandidate, PeekUnreadOutcome, SnoozeDeadline, StoreError,
    StoredMessage, TriageStore,
};
use org_office::{LocalTransitionInput, MailboxCommitInput, MailboxStored, Office};
use org_proto::inbox::{
    Action, InboxClears, InboxCounts, InboxItem, InboxKind, InboxState, Open, OpenTarget,
};
use org_proto::msg::{HeaderPage, MessageHeader};
use org_proto::ErrorCode;
use org_store::Id;

use crate::inbox::{Query, Source, SourceFailure, SourceRead};

/// Production `org-mailbox` storage adapter backed by `org_office::Office` (`org-store`).
pub(crate) struct OfficeMailboxStore {
    office: Arc<Office>,
}

impl OfficeMailboxStore {
    pub(crate) fn new(office: Arc<Office>) -> Self {
        Self { office }
    }
}

fn bytes_to_hex(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

fn hex_to_bytes(hex: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    if hex.len() == 64 {
        for (i, slot) in out.iter_mut().enumerate() {
            if let Ok(byte) = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16) {
                *slot = byte;
            }
        }
    }
    out
}

fn to_stored(m: MailboxStored) -> StoredMessage {
    StoredMessage {
        message: m.message,
        origin_office_id: m.origin_office_id,
        origin_incarnation: m.origin_incarnation,
        origin_seq: m.origin_seq,
        org_position: m.org_position,
        content_hash: hex_to_bytes(&m.content_hash),
    }
}

fn map_api_err(err: org_proto::ApiError) -> StoreError {
    match err.code {
        ErrorCode::AlreadyExists => StoreError::AlreadyExists,
        ErrorCode::NotFound => StoreError::NotFound,
        ErrorCode::Conflict | ErrorCode::InvalidRequest => StoreError::InvalidTransition,
        _ => StoreError::new(err.message),
    }
}

impl MessageStore for OfficeMailboxStore {
    fn commit_message(&self, msg: NewStoredMessage) -> Result<CommitMessageOutcome, StoreError> {
        let input = MailboxCommitInput {
            message: msg.message,
            origin_office_id: msg.origin_office_id,
            origin_incarnation: msg.origin_incarnation,
            origin_seq: msg.origin_seq,
            org_position: msg.org_position,
            content_hash: bytes_to_hex(&msg.content_hash),
            initial_state: msg.initial_state,
        };
        let res = self
            .office
            .mailbox_commit(input)
            .map_err(StoreError::new)?
            .map_err(map_api_err)?;
        Ok(CommitMessageOutcome {
            inserted: res.inserted,
            message: to_stored(res.stored),
        })
    }

    fn claim_read(
        &self,
        to_member_id: &str,
        id: Option<&str>,
        now_utc: &str,
    ) -> Result<Option<StoredMessage>, StoreError> {
        let found = self
            .office
            .mailbox_claim_read(to_member_id, id, now_utc)
            .map_err(StoreError::new)?
            .map_err(map_api_err)?;
        Ok(found.map(to_stored))
    }

    fn peek_next_unread(
        &self,
        to_member_id: &str,
        from_member_id: Option<&str>,
        urgent_only: bool,
        now_utc: &str,
    ) -> Result<PeekUnreadOutcome, StoreError> {
        let out = self
            .office
            .mailbox_peek_unread(to_member_id, from_member_id, urgent_only, now_utc)
            .map_err(StoreError::new)?;
        Ok(PeekUnreadOutcome {
            message: out.message.map(to_stored),
            expired_ids: out.expired_ids,
        })
    }

    fn get_message(&self, id: &str) -> Result<Option<StoredMessage>, StoreError> {
        let found = self.office.mailbox_get(id).map_err(StoreError::new)?;
        Ok(found.map(to_stored))
    }

    fn list_headers(&self, query: HeaderQuery) -> Result<HeaderPage, StoreError> {
        let (scope_inbox, scope_participant) = match &query.scope {
            HeaderScope::Inbox(id) => (Some(id.as_str()), None),
            HeaderScope::Participant(id) => (None, Some(id.as_str())),
            HeaderScope::All => (None, None),
        };
        self.office
            .mailbox_list_headers(
                scope_inbox,
                scope_participant,
                query.from_member_id.as_deref(),
                query.thread_id.as_deref(),
                query.unread_only,
                query.include_snoozed,
                query.cursor,
                query.limit,
            )
            .map_err(StoreError::new)
    }

    fn find_reply_for_ask(
        &self,
        ask_id: &str,
        responder_id: &str,
    ) -> Result<Option<StoredMessage>, StoreError> {
        let found = self
            .office
            .mailbox_find_reply_for_ask(ask_id, responder_id)
            .map_err(StoreError::new)?;
        Ok(found.map(to_stored))
    }
}

impl TriageStore for OfficeMailboxStore {
    fn update_state_local(
        &self,
        change: LocalStateTransition,
    ) -> Result<MessageHeader, StoreError> {
        let to_member_id = Id::parse(&change.to_member_id).map_err(|_| StoreError::NotFound)?;
        let owner_office_id = Id::parse(&change.owner_office_id)
            .unwrap_or_else(|_| Id::parse(&self.office.info().office_id).expect("valid office id"));
        let input = LocalTransitionInput {
            id: change.id,
            to_member_id,
            state: change.state,
            snoozed_until: change.snoozed_until,
            snooze_after_task: change.snooze_after_task,
            reason: change.reason,
            now_utc: change.now_utc,
            owner_office_id,
            origin_incarnation: change.origin_incarnation,
            org_position: change.org_position,
            expired_waiting_person: change.expired_waiting_person,
        };
        self.office
            .mailbox_update_state_local(input)
            .map_err(StoreError::new)?
            .map_err(map_api_err)
    }

    fn check_notify(
        &self,
        to_member_id: &str,
        after_task: bool,
        urgent_only: bool,
        sweep_before_utc: &str,
        now_utc: &str,
    ) -> Result<Vec<NotifyCandidate>, StoreError> {
        let list = self
            .office
            .mailbox_check_notify(
                to_member_id,
                after_task,
                urgent_only,
                sweep_before_utc,
                now_utc,
            )
            .map_err(StoreError::new)?
            .map_err(map_api_err)?;
        Ok(list
            .into_iter()
            .map(|c| NotifyCandidate {
                header: c.header,
                reason: c.reason,
            })
            .collect())
    }

    fn list_active_snooze_deadlines(&self) -> Result<Vec<SnoozeDeadline>, StoreError> {
        let rows = self
            .office
            .mailbox_active_snooze_deadlines()
            .map_err(StoreError::new)?;
        Ok(rows
            .into_iter()
            .map(|(id, to_member_id, at_utc)| SnoozeDeadline {
                id,
                to_member_id,
                at_utc,
            })
            .collect())
    }
}

/// Owner's unread message source for `inbox.list` and `inbox.changed` hints (`doc-5` B6).
pub(crate) struct MailboxInboxSource {
    office: Arc<Office>,
    office_id: String,
}

impl MailboxInboxSource {
    pub(crate) fn new(office: Arc<Office>) -> Self {
        let office_id = office.info().office_id.clone();
        Self { office, office_id }
    }
}

impl Source for MailboxInboxSource {
    fn name(&self) -> &'static str {
        "mailbox"
    }

    fn read(&self, query: &Query<'_>) -> Result<SourceRead, SourceFailure> {
        let entries = self.office.open_owner_inbox_entries(100).map_err(|err| {
            crate::logged("inbox.mailbox", &err);
            SourceFailure
        })?;
        let mut counts = InboxCounts::default();
        let mut items = Vec::new();
        for entry in entries {
            let item = build_inbox_item(&self.office_id, entry);
            match item.kind {
                InboxKind::Approval => counts.approvals = counts.approvals.saturating_add(1),
                InboxKind::Decision => counts.decisions = counts.decisions.saturating_add(1),
                InboxKind::Problem => counts.problems = counts.problems.saturating_add(1),
                _ => counts.questions = counts.questions.saturating_add(1),
            }
            counts.total = counts.total.saturating_add(1);
            let include = query.state == InboxState::Open
                && query.kinds.is_none_or(|kinds| kinds.contains(&item.kind))
                && query.project.is_none()
                && query.team.is_none()
                && query.after.is_none_or(|after| after.admits(&item));
            if include {
                items.push(item);
            }
        }
        items.truncate(query.limit.min(500));
        Ok(SourceRead { items, counts })
    }
}

fn build_inbox_item(office_id: &str, entry: org_office::OwnerInboxEntry) -> InboxItem {
    let header = entry.header;
    let open_action = Action::Open {
        label: "Open".into(),
        open: Open {
            target: OpenTarget::Message,
            id: header.id.clone(),
        },
    };
    let created_at_ms = org_office::parse_rfc3339_ms(&header.created_at);
    let Some(card) = entry.card else {
        let summary = header
            .subject
            .clone()
            .unwrap_or_else(|| format!("Message from {}", header.from_display_name));
        return InboxItem {
            reference: format!("{office_id}:msg:{}", header.id),
            source: "mailbox".into(),
            kind: InboxKind::Question,
            member: Some(header.from_member_id),
            project: None,
            team: None,
            summary,
            context: (!entry.body.is_empty()).then_some(entry.body),
            recommendation: None,
            options: None,
            created_at_ms,
            expires_at_ms: header
                .expires_at
                .as_deref()
                .and_then(org_office::parse_rfc3339_ms),
            state: InboxState::Open,
            handled_at_ms: None,
            outcome: None,
            clears: InboxClears::Answered,
            dismissable: true,
            actions: vec![open_action],
        };
    };
    let kind = if card.approval {
        InboxKind::Approval
    } else if card.questions.len() == 1 && !card.options.is_empty() {
        InboxKind::Decision
    } else {
        InboxKind::Question
    };
    let options =
        (!card.options.is_empty()).then(|| card.options.iter().map(|o| o.label.clone()).collect());
    let summary = header
        .subject
        .clone()
        .or_else(|| card.questions.first().map(|q| q.prompt.clone()))
        .unwrap_or_else(|| format!("Ask from {}", header.from_display_name));
    let expires_at_ms = (!card.required)
        .then(|| org_office::parse_rfc3339_ms(&card.wait_until))
        .flatten();
    InboxItem {
        reference: format!("{office_id}:msg:{}", header.id),
        source: if card.approval {
            "approval".into()
        } else {
            "ask".into()
        },
        kind,
        member: Some(header.from_member_id),
        project: None,
        team: None,
        summary,
        context: (!entry.body.is_empty()).then_some(entry.body),
        recommendation: card.default_option,
        options,
        created_at_ms,
        expires_at_ms,
        state: InboxState::Open,
        handled_at_ms: None,
        outcome: None,
        clears: InboxClears::Answered,
        dismissable: !card.required,
        actions: vec![open_action],
    }
}
