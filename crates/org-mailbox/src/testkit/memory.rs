use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use org_proto::msg::{
    HeaderPage, MessageHeader, MessageLifecycleState as State, NotifyReason, KIND_DELIVERY_FAILED,
    OFFICE_SENDER,
};
use org_proto::{Member, MemberKind, Position, OWNER_HANDLE};

use crate::{
    parse_utc, past_deadline, CommitMessageOutcome, HeaderQuery, HeaderScope, LocalStateTransition,
    MemberDirectory, MessageStore, NewStoredMessage, NotifyCandidate, PeekUnreadOutcome,
    SnoozeDeadline, StoreError, StoredMessage, TriageStore, MAX_EXPIRED_IDS,
};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

/// An in-memory org.db directory. Clones share the same members.
#[derive(Clone)]
pub struct InMemoryDirectory(Arc<Mutex<(Vec<Member>, Position)>>);

impl InMemoryDirectory {
    pub fn new(members: Vec<Member>, position: Position) -> Self {
        Self(Arc::new(Mutex::new((members, position))))
    }

    pub fn upsert(&self, member: Member, position: Position) {
        let mut state = lock(&self.0);
        state.0.retain(|existing| existing.id != member.id);
        state.0.push(member);
        state.1 = position;
    }

    pub fn remove(&self, id: &str, position: Position) {
        let mut state = lock(&self.0);
        state.0.retain(|member| member.id != id);
        state.1 = position;
    }
}

impl MemberDirectory for InMemoryDirectory {
    /// Resolves agents and people as org-office's directory does (members of an unknown kind
    /// never resolve by handle here): the reference is trimmed of ASCII whitespace,
    /// `owner` and `office` compare ignoring ASCII case, an id compares exactly, and handles
    /// compare by Unicode lower case (`str::to_lowercase`; a person's name is trimmed first,
    /// as its stored key is). Unlike org-office, two owners is a storage error here.
    fn resolve_member(&self, reference: &str) -> Result<Option<(Member, Position)>, StoreError> {
        let state = lock(&self.0);
        let reference = reference.trim_ascii();
        if reference.eq_ignore_ascii_case(OFFICE_SENDER) {
            return Ok(None);
        }
        let selected: Vec<&Member> = if reference.eq_ignore_ascii_case(OWNER_HANDLE) {
            state.0.iter().filter(|member| member.owner).collect()
        } else if let Some(member) = state.0.iter().find(|member| member.id == reference) {
            vec![member]
        } else {
            let key = reference.to_lowercase();
            state
                .0
                .iter()
                .filter(|member| match member.kind {
                    MemberKind::Agent => member
                        .agent
                        .as_ref()
                        .is_some_and(|profile| profile.messaging_name.to_lowercase() == key),
                    MemberKind::Person => !member.owner && member.name.trim().to_lowercase() == key,
                    _ => false,
                })
                .collect()
        };
        if selected.len() > 1 {
            return Err(StoreError::new("directory address is not unique"));
        }
        Ok(selected.first().map(|member| ((*member).clone(), state.1)))
    }
}

#[derive(Default)]
struct Data {
    sequence: u64,
    rows: BTreeMap<String, StoredMessage>,
    /// Ids ever snoozed (`triaged_at` set): never swept again.
    triaged: BTreeSet<String>,
}

/// An in-memory office.db message store: the delivery and triage ports. Clones share
/// the same rows. One counter allocates both `local_seq` and a local `origin_seq`. It keeps
/// no `msg_state.reason` (no port reads it back) and no work log.
#[derive(Clone, Default)]
pub struct InMemoryStore(Arc<Mutex<Data>>);

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl MessageStore for InMemoryStore {
    fn commit_message(
        &self,
        mut new: NewStoredMessage,
    ) -> Result<CommitMessageOutcome, StoreError> {
        let mut data = lock(&self.0);
        if let Some(existing) = data.rows.get(&new.message.header.id) {
            if existing.content_hash != new.content_hash
                || existing.message.header.from_member_id != new.message.header.from_member_id
            {
                return Err(StoreError::AlreadyExists);
            }
            return Ok(CommitMessageOutcome {
                message: existing.clone(),
                inserted: false,
            });
        }
        if !matches!(new.initial_state, State::Unread | State::Read) {
            return Err(StoreError::InvalidTransition);
        }
        if let Some(origin_seq) = new.origin_seq {
            if origin_seq == 0 {
                return Err(StoreError::InvalidTransition);
            }
            if let Some(existing) = data.rows.values().find(|stored| {
                stored.origin_office_id == new.origin_office_id
                    && stored.origin_incarnation == new.origin_incarnation
                    && stored.origin_seq == origin_seq
            }) {
                return Ok(CommitMessageOutcome {
                    message: existing.clone(),
                    inserted: false,
                });
            }
        }
        let sequence = data
            .sequence
            .checked_add(1)
            .ok_or_else(|| StoreError::new("message sequence exhausted"))?;
        data.sequence = sequence;
        let header = &mut new.message.header;
        header.local_seq = sequence;
        header.state = new.initial_state;
        header.triage_gen = 0;
        header.snoozed_until = None;
        header.snooze_after_task = false;
        header.snooze_note = None;
        header.read_at = (new.initial_state == State::Read).then(|| header.created_at.clone());
        let stored = StoredMessage {
            message: new.message,
            origin_office_id: new.origin_office_id,
            origin_incarnation: new.origin_incarnation,
            origin_seq: new.origin_seq.unwrap_or(sequence),
            org_position: new.org_position,
            content_hash: new.content_hash,
        };
        data.rows
            .insert(stored.message.header.id.clone(), stored.clone());
        Ok(CommitMessageOutcome {
            message: stored,
            inserted: true,
        })
    }

    fn claim_read(
        &self,
        member: &str,
        id: Option<&str>,
        now: &str,
    ) -> Result<Option<StoredMessage>, StoreError> {
        let mut data = lock(&self.0);
        let id = match id {
            Some(id) => data
                .rows
                .get(id)
                .filter(|stored| stored.message.header.to_member_id == member)
                .map(|_| id.to_owned()),
            None => data
                .rows
                .values()
                .filter(|stored| {
                    let header = &stored.message.header;
                    header.to_member_id == member && open(header) && !expired(header, now)
                })
                .min_by_key(|stored| stored.message.header.local_seq)
                .map(|stored| stored.message.header.id.clone()),
        };
        let Some(id) = id else { return Ok(None) };
        let stored = data.rows.get_mut(&id).ok_or(StoreError::NotFound)?;
        let header = &mut stored.message.header;
        if matches!(header.state, State::Unread | State::Snoozed) {
            header.state = State::Read;
            header.snoozed_until = None;
            header.snooze_after_task = false;
            header.snooze_note = None;
            header.read_at = Some(now.to_owned());
        }
        Ok(Some(stored.clone()))
    }

    fn peek_next_unread(
        &self,
        member: &str,
        from: Option<&str>,
        urgent_only: bool,
        now: &str,
    ) -> Result<PeekUnreadOutcome, StoreError> {
        let data = lock(&self.0);
        let mut matching: Vec<&StoredMessage> = data
            .rows
            .values()
            .filter(|stored| {
                let header = &stored.message.header;
                header.to_member_id == member
                    && open(header)
                    && from.is_none_or(|from| header.from_member_id == from)
                    && (!urgent_only || header.urgent)
            })
            .collect();
        matching.sort_by_key(|stored| stored.message.header.local_seq);
        let expired_ids = matching
            .iter()
            .filter(|stored| expired(&stored.message.header, now))
            .map(|stored| stored.message.header.id.clone())
            .take(MAX_EXPIRED_IDS)
            .collect();
        let message = matching
            .into_iter()
            .find(|stored| !expired(&stored.message.header, now))
            .cloned();
        Ok(PeekUnreadOutcome {
            message,
            expired_ids,
        })
    }

    fn get_message(&self, id: &str) -> Result<Option<StoredMessage>, StoreError> {
        Ok(lock(&self.0).rows.get(id).cloned())
    }

    fn list_headers(&self, query: HeaderQuery) -> Result<HeaderPage, StoreError> {
        let data = lock(&self.0);
        let mut headers: Vec<MessageHeader> = data
            .rows
            .values()
            .map(|stored| &stored.message.header)
            .filter(|header| {
                let visible = match &query.scope {
                    HeaderScope::Inbox(member) => &header.to_member_id == member,
                    HeaderScope::Participant(member) => {
                        &header.to_member_id == member || &header.from_member_id == member
                    }
                    HeaderScope::All => true,
                };
                visible
                    && query
                        .from_member_id
                        .as_ref()
                        .is_none_or(|from| &header.from_member_id == from)
                    && query
                        .thread_id
                        .as_ref()
                        .is_none_or(|thread| &header.thread_id == thread)
                    && query.cursor.is_none_or(|cursor| header.local_seq < cursor)
                    && (query.include_snoozed || !snoozed(header))
                    && (!query.unread_only
                        || header.state == State::Unread
                        || (query.include_snoozed && header.state == State::Snoozed))
            })
            .cloned()
            .collect();
        headers.sort_by_key(|header| std::cmp::Reverse(header.local_seq));
        let more = headers.len() > query.limit as usize;
        headers.truncate(query.limit as usize);
        let next_cursor = if more {
            headers.last().map(|header| header.local_seq)
        } else {
            None
        };
        Ok(HeaderPage {
            headers,
            next_cursor,
        })
    }

    fn find_reply_for_ask(
        &self,
        ask: &str,
        responder: &str,
    ) -> Result<Option<StoredMessage>, StoreError> {
        let data = lock(&self.0);
        let Some(request) = data.rows.get(ask) else {
            return Ok(None);
        };
        let asker = &request.message.header.from_member_id;
        Ok(data
            .rows
            .values()
            .filter(|stored| {
                let header = &stored.message.header;
                header.reply_to.as_deref() == Some(ask)
                    && &header.to_member_id == asker
                    && (header.from_member_id == responder
                        || (header.from_member_id == OFFICE_SENDER
                            && header.kind == KIND_DELIVERY_FAILED))
            })
            .min_by_key(|stored| stored.message.header.local_seq)
            .cloned())
    }
}

impl TriageStore for InMemoryStore {
    fn update_state_local(
        &self,
        change: LocalStateTransition,
    ) -> Result<MessageHeader, StoreError> {
        let mut data = lock(&self.0);
        let header = &mut data
            .rows
            .get_mut(&change.id)
            .filter(|stored| stored.message.header.to_member_id == change.to_member_id)
            .ok_or(StoreError::NotFound)?
            .message
            .header;
        let open_states = matches!(header.state, State::Unread | State::Read | State::Snoozed);
        match (header.state, change.state) {
            (_, State::Snoozed) if open_states => {
                if change.snoozed_until.is_some() == change.snooze_after_task {
                    return Err(StoreError::InvalidTransition);
                }
                header.state = State::Snoozed;
                header.snoozed_until = change.snoozed_until;
                header.snooze_after_task = change.snooze_after_task;
                header.snooze_note = change.reason;
                header.triage_gen += 1;
            }
            (State::Done, State::Done)
            | (State::Read, State::Read)
            | (State::Expired, State::Expired) => {}
            (State::Unread | State::Read | State::Snoozed, State::Done)
            | (State::Unread | State::Snoozed, State::Read | State::Expired) => {
                if change.state == State::Read {
                    header.read_at = Some(change.now_utc);
                }
                header.state = change.state;
                clear_snooze(header);
            }
            // The flag is set only on an unread message: never reopen a read one for it.
            (State::Read, State::Unread) if change.expired_waiting_person => {
                return Err(StoreError::InvalidTransition)
            }
            (State::Read | State::Unread, State::Unread) => {
                header.state = State::Unread;
                header.read_at = None;
                header.expired_waiting_person |= change.expired_waiting_person;
            }
            _ => return Err(StoreError::InvalidTransition),
        }
        let header = header.clone();
        if change.state == State::Snoozed {
            data.triaged.insert(change.id);
        }
        Ok(header)
    }

    fn check_notify(
        &self,
        member: &str,
        after_task: bool,
        urgent_only: bool,
        sweep_before: &str,
        now: &str,
    ) -> Result<Vec<NotifyCandidate>, StoreError> {
        let mut data = lock(&self.0);
        let Data { rows, triaged, .. } = &mut *data;
        let now = parse_utc(now);
        let sweep_before = parse_utc(sweep_before);
        let mut candidates = Vec::new();
        for stored in rows.values_mut() {
            let header = &mut stored.message.header;
            if header.to_member_id != member {
                continue;
            }
            let reason = if header.state == State::Snoozed {
                // This store's own choice, not the port's contract: an unreadable time is
                // released rather than stranded.
                let due = header.snoozed_until.as_deref().is_some_and(|until| {
                    match (parse_utc(until), now) {
                        (Some(until), Some(now)) => until <= now,
                        _ => true,
                    }
                });
                let reason = if due {
                    NotifyReason::Due
                } else if after_task && header.snooze_after_task {
                    NotifyReason::AfterTask
                } else {
                    continue;
                };
                header.state = State::Unread;
                header.triage_gen += 1;
                clear_snooze(header);
                reason
            } else if open(header) {
                if after_task || header.urgent {
                    NotifyReason::New
                } else if !triaged.contains(&header.id)
                    && matches!(
                        (parse_utc(&header.created_at), sweep_before),
                        (Some(created), Some(before)) if created < before
                    )
                {
                    NotifyReason::Sweep
                } else {
                    continue;
                }
            } else {
                continue;
            };
            if !urgent_only || header.urgent {
                candidates.push(NotifyCandidate {
                    header: header.clone(),
                    reason,
                });
            }
        }
        Ok(candidates)
    }

    fn list_active_snooze_deadlines(&self) -> Result<Vec<SnoozeDeadline>, StoreError> {
        Ok(lock(&self.0)
            .rows
            .values()
            .map(|stored| &stored.message.header)
            .filter(|header| header.state == State::Snoozed)
            .filter_map(|header| {
                Some(SnoozeDeadline {
                    at_utc: header.snoozed_until.clone()?,
                    to_member_id: header.to_member_id.clone(),
                    id: header.id.clone(),
                })
            })
            .collect())
    }
}

fn clear_snooze(header: &mut MessageHeader) {
    header.snoozed_until = None;
    header.snooze_after_task = false;
    header.snooze_note = None;
}

fn snoozed(header: &MessageHeader) -> bool {
    header.state == State::Snoozed || header.snoozed_until.is_some() || header.snooze_after_task
}

fn open(header: &MessageHeader) -> bool {
    header.state == State::Unread && !snoozed(header)
}

/// Past its deadline plus the bounded skew grace ([`past_deadline`]), unless it's a person's
/// ask flagged `expired_waiting_person`. An unreadable `now` counts a deadline as passed.
fn expired(header: &MessageHeader, now: &str) -> bool {
    !header.expired_waiting_person
        && match parse_utc(now) {
            Some(now) => past_deadline(header, now),
            None => header.expires_at.is_some(),
        }
}

#[cfg(test)]
mod conformance {
    use super::{InMemoryDirectory, InMemoryStore};
    use org_proto::{Member, Position};

    fn open(members: &[Member], position: Position) -> (InMemoryStore, InMemoryDirectory) {
        (
            InMemoryStore::new(),
            InMemoryDirectory::new(members.to_vec(), position),
        )
    }

    crate::mailbox_conformance_tests!(open);

    mod triage {
        crate::triage_conformance_tests!(super::open);
    }

    /// A retry by exact recipient id still returns the first result after the recipient
    /// left the directory; by handle it must resolve again, so it is `unknown_recipient`.
    #[test]
    fn retry_by_exact_id_survives_recipient_removal() {
        use crate::testkit::fixture::*;
        use crate::{Mailbox, MailboxError, MemberDirectory, NoMessageEvents, OfficeIdentity};
        use org_proto::msg::{Meta, MsgSendParams, KIND_MSG};
        use std::sync::Arc;

        let directory = InMemoryDirectory::new(members(), POSITION);
        let mailbox = Mailbox::new(
            Arc::new(InMemoryStore::new()),
            Arc::new(directory.clone()),
            OfficeIdentity {
                office_id: OFFICE_ID.into(),
                incarnation: INCARNATION.into(),
                is_hq: true,
            },
            Arc::new(ManualClock::new(NOW_MS)),
            Arc::new(NoMessageEvents),
        );
        let params = |to: &str| MsgSendParams {
            id: msg_id(1),
            to: to.into(),
            kind: KIND_MSG.into(),
            priority: None,
            subject: None,
            body: "hello".into(),
            meta: Meta::new(),
            reply_to: None,
        };
        let first = mailbox.send(ADA_ID, params(PAT_ID)).unwrap();
        directory.remove(PAT_ID, POSITION);
        assert_eq!(mailbox.send(ADA_ID, params(PAT_ID)), Ok(first));
        assert_eq!(
            mailbox.send(ADA_ID, params(PAT_HANDLE)),
            Err(MailboxError::UnknownRecipient)
        );
        // PAT_ID still resolves (as the shadow agent's handle). Remy's id resolves to no
        // one once Remy is removed, and the exact-id retry must still return the first result.
        let remy = || MsgSendParams {
            id: msg_id(2),
            ..params(REMY_ID)
        };
        let second = mailbox.send(ADA_ID, remy()).unwrap();
        directory.remove(REMY_ID, POSITION);
        assert_eq!(directory.resolve_member(REMY_ID).unwrap(), None);
        assert_eq!(mailbox.send(ADA_ID, remy()), Ok(second));
    }
}
