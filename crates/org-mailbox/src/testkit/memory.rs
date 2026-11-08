use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use org_proto::msg::{
    HeaderPage, MessageHeader, MessageLifecycleState as State, NotifyReason, ThreadPage,
    ThreadSummary,
};
use org_proto::{Member, MemberKind, Position};

use crate::{
    parse_utc, CommitMessageOutcome, DeliveredMessage, HeaderQuery, HeaderScope,
    LocalStateTransition, MemberDirectory, MessageStore, NewStoredMessage, NotifyCandidate,
    PeekUnreadOutcome, SnoozeDeadline, StoreError, StoredMessage, SyncedMessageState, ThreadQuery,
};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

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
    fn resolve_member(&self, reference: &str) -> Result<Option<(Member, Position)>, StoreError> {
        let state = lock(&self.0);
        if reference.eq_ignore_ascii_case("office") {
            return Ok(None);
        }
        let selected = if reference.eq_ignore_ascii_case("owner") {
            state
                .0
                .iter()
                .filter(|member| member.owner)
                .collect::<Vec<_>>()
        } else if let Some(member) = state.0.iter().find(|member| member.id == reference) {
            vec![member]
        } else {
            state
                .0
                .iter()
                .filter(|member| match member.kind {
                    MemberKind::Agent => member.agent.as_ref().is_some_and(|profile| {
                        profile.messaging_name.eq_ignore_ascii_case(reference)
                    }),
                    MemberKind::Person if !member.owner => {
                        member.name.eq_ignore_ascii_case(reference)
                    }
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

#[derive(Clone)]
struct Record {
    stored: StoredMessage,
    last_state: Option<SyncedMessageState>,
    triaged_at: Option<String>,
}

#[derive(Default)]
struct Data {
    sequence: u64,
    rows: BTreeMap<String, Record>,
}

impl Data {
    fn allocate(&mut self) -> Result<u64, StoreError> {
        let next = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| StoreError::new("message sequence exhausted"))?;
        self.sequence = next;
        Ok(next)
    }
}

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
            if existing.stored.content_hash != new.content_hash
                || existing.stored.message.header.from_member_id
                    != new.message.header.from_member_id
            {
                return Err(StoreError::AlreadyExists);
            }
            return Ok(CommitMessageOutcome {
                message: existing.stored.clone(),
                inserted: false,
            });
        }
        if !matches!(new.initial_state, State::Unread | State::Read) {
            return Err(StoreError::InvalidTransition);
        }
        if let Some(origin_seq) = new.origin_seq {
            if origin_seq == 0
                || data.rows.values().any(|record| {
                    record.stored.origin_office_id == new.origin_office_id
                        && record.stored.origin_incarnation == new.origin_incarnation
                        && record.stored.origin_seq == origin_seq
                })
            {
                return Err(StoreError::AlreadyExists);
            }
        }
        let sequence = data.allocate()?;
        new.message.header.local_seq = sequence;
        new.message.header.state = new.initial_state;
        new.message.header.triage_gen = 0;
        new.message.header.snoozed_until = None;
        new.message.header.snooze_after_task = false;
        new.message.header.snooze_note = None;
        new.message.header.read_at =
            (new.initial_state == State::Read).then(|| new.message.header.created_at.clone());
        let stored = StoredMessage {
            message: new.message,
            origin_office_id: new.origin_office_id,
            origin_incarnation: new.origin_incarnation,
            origin_seq: new.origin_seq.unwrap_or(sequence),
            org_position: new.org_position,
            content_hash: new.content_hash,
        };
        let last_state = if new.initial_state == State::Read {
            Some(state_stamp(
                &stored,
                data.allocate()?,
                State::Read,
                &stored.message.header.created_at,
            ))
        } else {
            None
        };
        data.rows.insert(
            stored.message.header.id.clone(),
            Record {
                stored: stored.clone(),
                last_state,
                triaged_at: None,
            },
        );
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
                .filter(|record| record.stored.message.header.to_member_id == member)
                .map(|_| id.to_owned()),
            None => data
                .rows
                .values()
                .filter(|record| {
                    let header = &record.stored.message.header;
                    header.to_member_id == member && open(header) && !expired(header, now)
                })
                .min_by_key(|record| record.stored.message.header.local_seq)
                .map(|record| record.stored.message.header.id.clone()),
        };
        let Some(id) = id else { return Ok(None) };
        let mut record = data.rows.get(&id).cloned().ok_or(StoreError::NotFound)?;
        if matches!(
            record.stored.message.header.state,
            State::Unread | State::Snoozed
        ) {
            let sequence = data.allocate()?;
            let header = &mut record.stored.message.header;
            header.state = State::Read;
            header.snoozed_until = None;
            header.snooze_after_task = false;
            header.snooze_note = None;
            header.read_at = Some(now.to_owned());
            record.last_state = Some(state_stamp(&record.stored, sequence, State::Read, now));
            record.triaged_at = Some(now.to_owned());
            data.rows.insert(id, record.clone());
        }
        Ok(Some(record.stored))
    }

    fn peek_next_unread(
        &self,
        member: &str,
        from: Option<&str>,
        urgent_only: bool,
        now: &str,
    ) -> Result<PeekUnreadOutcome, StoreError> {
        let data = lock(&self.0);
        let mut matching: Vec<_> = data
            .rows
            .values()
            .filter(|record| {
                let header = &record.stored.message.header;
                header.to_member_id == member
                    && open(header)
                    && from.is_none_or(|from| header.from_member_id == from)
                    && (!urgent_only || header.urgent)
            })
            .collect();
        matching.sort_by_key(|record| record.stored.message.header.local_seq);
        let expired_ids = matching
            .iter()
            .filter(|record| expired(&record.stored.message.header, now))
            .map(|record| record.stored.message.header.id.clone())
            .collect();
        let message = matching
            .into_iter()
            .find(|record| !expired(&record.stored.message.header, now))
            .map(|record| record.stored.clone());
        Ok(PeekUnreadOutcome {
            message,
            expired_ids,
        })
    }

    fn get_message(&self, id: &str) -> Result<Option<StoredMessage>, StoreError> {
        Ok(lock(&self.0)
            .rows
            .get(id)
            .map(|record| record.stored.clone()))
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
        let asker = &request.stored.message.header.from_member_id;
        Ok(data
            .rows
            .values()
            .filter(|record| {
                let header = &record.stored.message.header;
                header.reply_to.as_deref() == Some(ask)
                    && &header.to_member_id == asker
                    && (header.from_member_id == responder
                        || (header.from_member_id == "office" && header.kind == "delivery_failed"))
            })
            .min_by_key(|record| record.stored.message.header.local_seq)
            .map(|record| record.stored.clone()))
    }

    fn list_headers(&self, query: HeaderQuery) -> Result<HeaderPage, StoreError> {
        let data = lock(&self.0);
        let mut headers: Vec<_> = data
            .rows
            .values()
            .map(|record| &record.stored.message.header)
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
                    && (query.include_snoozed || header.state != State::Snoozed)
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

    fn update_state_local(
        &self,
        change: LocalStateTransition,
    ) -> Result<MessageHeader, StoreError> {
        transition(&mut lock(&self.0), change)
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
        let due: Vec<_> = data
            .rows
            .values()
            .filter_map(|record| {
                let header = &record.stored.message.header;
                if header.to_member_id != member || header.state != State::Snoozed {
                    return None;
                }
                let timed = header.snoozed_until.as_deref().is_some_and(|at| at <= now);
                if !timed && !(after_task && header.snooze_after_task) {
                    return None;
                }
                Some((
                    record.stored.clone(),
                    if timed {
                        NotifyReason::Due
                    } else {
                        NotifyReason::AfterTask
                    },
                ))
            })
            .collect();
        let mut released = BTreeMap::new();
        for (stored, reason) in due {
            let id = stored.message.header.id.clone();
            transition(
                &mut data,
                LocalStateTransition {
                    id: id.clone(),
                    to_member_id: member.to_owned(),
                    state: State::Unread,
                    snoozed_until: None,
                    snooze_after_task: false,
                    reason: None,
                    now_utc: now.to_owned(),
                    owner_office_id: stored.origin_office_id,
                    origin_incarnation: stored.origin_incarnation,
                    org_position: stored.org_position,
                },
            )?;
            released.insert(id, reason);
        }
        let mut candidates: Vec<_> = data
            .rows
            .values()
            .filter_map(|record| {
                let header = &record.stored.message.header;
                if header.to_member_id != member
                    || !open(header)
                    || expired(header, now)
                    || (urgent_only && !header.urgent)
                {
                    return None;
                }
                let reason = released.get(&header.id).copied().or_else(|| {
                    if header.urgent || after_task {
                        Some(NotifyReason::New)
                    } else if record.triaged_at.is_none()
                        && header.created_at.as_str() <= sweep_before
                    {
                        Some(NotifyReason::Sweep)
                    } else {
                        None
                    }
                })?;
                Some(NotifyCandidate {
                    header: header.clone(),
                    reason,
                })
            })
            .collect();
        candidates.sort_by_key(|candidate| (!candidate.header.urgent, candidate.header.local_seq));
        Ok(candidates)
    }

    fn list_active_snooze_deadlines(&self) -> Result<Vec<SnoozeDeadline>, StoreError> {
        Ok(lock(&self.0)
            .rows
            .values()
            .filter_map(|record| {
                let header = &record.stored.message.header;
                (header.state == State::Snoozed).then_some(())?;
                Some(SnoozeDeadline {
                    at_utc: header.snoozed_until.clone()?,
                    to_member_id: header.to_member_id.clone(),
                    id: header.id.clone(),
                })
            })
            .collect())
    }

    fn apply_synced_state(&self, synced: SyncedMessageState) -> Result<bool, StoreError> {
        let mut data = lock(&self.0);
        let record = data
            .rows
            .get_mut(&synced.msg_id)
            .ok_or(StoreError::NotFound)?;
        if record.stored.message.header.to_member_id != synced.to_member_id
            || synced.state == State::Unknown
        {
            return Err(StoreError::InvalidTransition);
        }
        if let Some(previous) = &record.last_state {
            let previous_position = (previous.org_position.epoch, previous.org_position.seq);
            let next_position = (synced.org_position.epoch, synced.org_position.seq);
            if synced.origin_seq == 0 || next_position < previous_position {
                return Ok(false);
            }
            if next_position == previous_position {
                if previous.origin_office_id != synced.origin_office_id
                    || previous.origin_incarnation != synced.origin_incarnation
                {
                    return Err(StoreError::InvalidTransition);
                }
                if synced.origin_seq <= previous.origin_seq {
                    return Ok(false);
                }
            }
        } else if synced.origin_seq == 0 {
            return Ok(false);
        }
        apply_header_state(&mut record.stored.message.header, &synced);
        record.triaged_at = Some(synced.updated_at.clone());
        record.last_state = Some(synced);
        Ok(true)
    }

    fn resolve_pending_references(
        &self,
        _position: Position,
        _now: &str,
    ) -> Result<Vec<DeliveredMessage>, StoreError> {
        // Slice-1 inputs have resolved local references; no unresolved rows can be created.
        Ok(Vec::new())
    }

    fn list_threads(&self, query: ThreadQuery) -> Result<ThreadPage, StoreError> {
        let data = lock(&self.0);
        let mut groups: BTreeMap<String, Vec<&MessageHeader>> = BTreeMap::new();
        for record in data.rows.values() {
            let header = &record.stored.message.header;
            groups
                .entry(header.thread_id.clone())
                .or_default()
                .push(header);
        }
        let mut threads: Vec<_> = groups
            .into_iter()
            .filter_map(|(thread_id, headers)| {
                let participants: BTreeSet<_> = headers
                    .iter()
                    .flat_map(|header| [header.from_member_id.clone(), header.to_member_id.clone()])
                    .collect();
                if query
                    .participant
                    .as_ref()
                    .is_some_and(|member| !participants.contains(member))
                {
                    return None;
                }
                let first = headers
                    .iter()
                    .min_by_key(|header| (&header.created_at, &header.id))?;
                let root = headers
                    .iter()
                    .find(|header| header.id == thread_id)
                    .unwrap_or(first);
                let last = headers
                    .iter()
                    .max_by_key(|header| (&header.created_at, &header.id))?;
                if query.cursor.as_ref().is_some_and(|cursor| {
                    (&last.created_at, &thread_id) >= (&cursor.last_msg_at, &cursor.thread_id)
                }) {
                    return None;
                }
                Some(ThreadSummary {
                    thread_id,
                    subject: root.subject.clone(),
                    created_at: first.created_at.clone(),
                    last_msg_id: last.id.clone(),
                    last_msg_at: last.created_at.clone(),
                    message_count: headers.len() as u64,
                    participants: participants.into_iter().collect(),
                })
            })
            .collect();
        threads.sort_by(|a, b| (&b.last_msg_at, &b.thread_id).cmp(&(&a.last_msg_at, &a.thread_id)));
        let more = threads.len() > query.limit as usize;
        threads.truncate(query.limit as usize);
        let next_cursor = if more {
            threads.last().map(|thread| org_proto::msg::ThreadCursor {
                last_msg_at: thread.last_msg_at.clone(),
                thread_id: thread.thread_id.clone(),
            })
        } else {
            None
        };
        Ok(ThreadPage {
            threads,
            next_cursor,
        })
    }
}

fn open(header: &MessageHeader) -> bool {
    header.state == State::Unread && header.snoozed_until.is_none() && !header.snooze_after_task
}

fn expired(header: &MessageHeader, now: &str) -> bool {
    if header.expired_waiting_person {
        return false;
    }
    let Some(expires_at) = header.expires_at.as_deref() else {
        return false;
    };
    let Some(deadline) = parse_utc(expires_at) else {
        return true;
    };
    let Some(now) = parse_utc(now) else {
        return true;
    };
    now >= deadline.saturating_add((header.ttl_ms.unwrap_or(0) / 4).min(2000))
}

fn state_stamp(
    stored: &StoredMessage,
    sequence: u64,
    state: State,
    now: &str,
) -> SyncedMessageState {
    SyncedMessageState {
        origin_office_id: stored.origin_office_id.clone(),
        origin_incarnation: stored.origin_incarnation.clone(),
        origin_seq: sequence,
        org_position: stored.org_position,
        msg_id: stored.message.header.id.clone(),
        to_member_id: stored.message.header.to_member_id.clone(),
        state,
        triage_gen: stored.message.header.triage_gen,
        snoozed_until: None,
        snooze_after_task: false,
        reason: None,
        updated_at: now.to_owned(),
    }
}

fn transition(data: &mut Data, change: LocalStateTransition) -> Result<MessageHeader, StoreError> {
    let mut record = data
        .rows
        .get(&change.id)
        .cloned()
        .ok_or(StoreError::NotFound)?;
    let header = &mut record.stored.message.header;
    if header.to_member_id != change.to_member_id {
        return Err(StoreError::NotFound);
    }
    if matches!(
        header.state,
        State::Done | State::Expired | State::DeadLetter
    ) {
        return if header.state == change.state {
            Ok(header.clone())
        } else {
            Err(StoreError::InvalidTransition)
        };
    }
    if change.state == State::Unknown
        || (change.state == State::Snoozed
            && (change.snoozed_until.is_some() == change.snooze_after_task))
    {
        return Err(StoreError::InvalidTransition);
    }
    if change.state == State::Snoozed
        || (header.state == State::Snoozed && change.state == State::Unread)
    {
        header.triage_gen = header
            .triage_gen
            .checked_add(1)
            .ok_or_else(|| StoreError::new("triage generation exhausted"))?;
    }
    let sequence = data.allocate()?;
    header.state = change.state;
    header.snoozed_until = change.snoozed_until.clone();
    header.snooze_after_task = change.snooze_after_task;
    if change.state == State::Snoozed {
        header.snooze_note = change.reason.clone();
    }
    if change.state == State::Read {
        header.read_at = Some(change.now_utc.clone());
    }
    let header = header.clone();
    record.last_state = Some(SyncedMessageState {
        origin_office_id: change.owner_office_id,
        origin_incarnation: change.origin_incarnation,
        origin_seq: sequence,
        org_position: change.org_position,
        msg_id: change.id.clone(),
        to_member_id: change.to_member_id,
        state: change.state,
        triage_gen: header.triage_gen,
        snoozed_until: change.snoozed_until,
        snooze_after_task: change.snooze_after_task,
        reason: change.reason,
        updated_at: change.now_utc.clone(),
    });
    record.triaged_at = Some(change.now_utc);
    data.rows.insert(change.id, record);
    Ok(header)
}

fn apply_header_state(header: &mut MessageHeader, state: &SyncedMessageState) {
    header.state = state.state;
    header.triage_gen = state.triage_gen;
    header.snoozed_until = state.snoozed_until.clone();
    header.snooze_after_task = state.snooze_after_task;
    header.snooze_note = state.reason.clone();
    if state.state == State::Read {
        header.read_at = Some(state.updated_at.clone());
    }
}
