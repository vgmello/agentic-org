//! Delivering answer candidates to participants and merging their answer state.

use crate::messaging::{
    ports::{MessagingRead, MessagingWrite},
    records::{self, ParticipantReceivedSnapshot, RecipientStateRecord, SCHEMA_VERSION},
};
use crate::organization::ports::OrganizationRead;
use crate::organization::records::Employee;
use org_proto::chat::DeliveredBecause;
use org_proto::msg::MessageLifecycleState;
use org_proto::ErrorCode;
use org_store::{Ack, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};
use std::collections::BTreeMap;

use super::channels::effective_auto_members;
use super::conversations::all_conversations_including_auto;
use super::refusal;

/// One validated `answer_candidate` inbox delivery produced by a channel watcher pass
/// (`ADR-0024` d11, `WATCH-1`, `WATCH-3`, `WATCH-7`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerCandidateDelivery {
    pub message_id: String,
    pub asker_id: Id,
    pub question_ids: Vec<String>,
    pub question_context: String,
}

/// Atomically delivers `answer_candidate` inbox entries for one channel watcher pass.
///
/// Idempotent across crash recovery (`WATCH-7`):
/// - Preserves higher-priority `DeliveredBecause` (`Addressed < ThreadReply < Mention < AnswerCandidate`)
///   so a message appears once in the asker's inbox (`WATCH-1`).
/// - Merges matched `question_ids` into `related_message_ids` without duplicates.
/// - Attaches `question_context` in `reason` so an asker with reset context sees the question text
///   beside the entry (`ADR-0024` d11).
/// - Returns only the `(message_id, asker_id)` pairs that received a genuinely new delivery or link.
pub fn deliver_answer_candidates<E: Engine<Tx: MessagingWrite>>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    conversation_id: &Id,
    deliveries: &[AnswerCandidateDelivery],
    org_seq: u64,
    now_utc: &str,
) -> Result<(Change, Vec<(String, Id)>), Refusal> {
    if deliveries.is_empty() {
        return Ok((Change::new(Ack::Committed), Vec::new()));
    }
    let view = uow.view();
    let Some(conv) = all_conversations_including_auto(view, now_utc)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to load conversations"))?
        .into_iter()
        .find(|c| c.id == *conversation_id)
    else {
        return Err(refusal(ErrorCode::NotFound, "conversation not found"));
    };
    let active_members = effective_auto_members(view, &conv, owner)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to resolve channel members"))?;
    let employees = view
        .employees()
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read employees"))?;
    let mut next_state_seq = view
        .max_state_sequence(&here.office_id, here.journal_incarnation.as_str())
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read state sequence"))?;
    let mut snapshots: BTreeMap<Id, ParticipantReceivedSnapshot> = BTreeMap::new();
    let mut newly_delivered = Vec::new();

    for d in deliveries {
        if !active_members.contains(&d.asker_id) {
            continue;
        }
        let Some(msg) = view
            .message(&d.message_id)
            .map_err(|_| refusal(ErrorCode::StorageError, "failed to read message"))?
        else {
            continue;
        };
        if msg.conversation_id != *conversation_id
            || msg.sender_employee_id.as_ref() == Some(&d.asker_id)
        {
            continue;
        }
        let existing = match snapshots
            .get(&d.asker_id)
            .and_then(|snap| snap.states.get(&d.message_id).cloned())
        {
            Some(st) => Some(st),
            None => view
                .recipient_state(&d.message_id, &d.asker_id)
                .map_err(|_| refusal(ErrorCode::StorageError, "failed to read recipient state"))?,
        };
        let Some(record) = merge_answer_candidate_state(
            existing,
            d,
            here,
            conversation_id,
            &employees,
            &mut next_state_seq,
            org_seq,
            now_utc,
        ) else {
            continue;
        };
        if !snapshots.contains_key(&d.asker_id) {
            let loaded = load_participant_snapshot(
                view,
                &here.organization_id,
                conversation_id,
                &d.asker_id,
            )?;
            snapshots.insert(d.asker_id.clone(), loaded);
        }
        if let Some(snap) = snapshots.get_mut(&d.asker_id) {
            snap.states.insert(d.message_id.clone(), record);
        }
        newly_delivered.push((d.message_id.clone(), d.asker_id.clone()));
    }

    let change = commit_participant_snapshots(view, conversation_id, snapshots);
    Ok((change, newly_delivered))
}

#[expect(
    clippy::too_many_arguments,
    reason = "answer candidate merge combines existing state, candidate delivery, office identity, and roster"
)]
fn merge_answer_candidate_state(
    existing: Option<RecipientStateRecord>,
    d: &AnswerCandidateDelivery,
    here: &OfficeIdentity,
    conversation_id: &Id,
    employees: &[Employee],
    next_state_seq: &mut u64,
    org_seq: u64,
    now_utc: &str,
) -> Option<RecipientStateRecord> {
    match existing {
        Some(mut st) => {
            let mut mutated = false;
            let best = st.delivered_because.min(DeliveredBecause::AnswerCandidate);
            if best != st.delivered_because {
                st.delivered_because = best;
                mutated = true;
            }
            for qid in &d.question_ids {
                if !st.related_message_ids.contains(qid) {
                    st.related_message_ids.push(qid.clone());
                    mutated = true;
                }
            }
            if st.reason.is_none() && !d.question_context.is_empty() {
                st.reason = Some(d.question_context.clone());
                mutated = true;
            }
            if mutated {
                st.updated_at = now_utc.to_owned();
                st.org_sequence = org_seq;
                Some(st)
            } else {
                None
            }
        }
        None => {
            *next_state_seq += 1;
            let recipient_name = employees
                .iter()
                .find(|e| e.id == d.asker_id)
                .map(|e| e.name.clone());
            Some(RecipientStateRecord {
                organization_id: here.organization_id.clone(),
                message_id: d.message_id.clone(),
                conversation_id: conversation_id.clone(),
                employee_id: d.asker_id.clone(),
                recipient_display_name: recipient_name,
                state: MessageLifecycleState::Unread,
                triage_generation: 0,
                snoozed_until: None,
                snooze_after_task: false,
                snooze_note: None,
                triaged_at: None,
                read_at: None,
                updated_at: now_utc.to_owned(),
                reason: (!d.question_context.is_empty()).then(|| d.question_context.clone()),
                delivered_because: DeliveredBecause::AnswerCandidate,
                related_message_ids: d.question_ids.clone(),
                expired_waiting_person: false,
                owner_office_id: here.office_id.clone(),
                origin_incarnation: here.journal_incarnation.as_str().to_owned(),
                origin_sequence: *next_state_seq,
                org_epoch: 0,
                org_sequence: org_seq,
            })
        }
    }
}

fn load_participant_snapshot(
    view: &impl MessagingRead,
    organization_id: &Id,
    conversation_id: &Id,
    employee_id: &Id,
) -> Result<ParticipantReceivedSnapshot, Refusal> {
    let states = view
        .recipient_states_for_employee(employee_id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read recipient states"))?
        .into_iter()
        .filter(|s| s.conversation_id == *conversation_id)
        .map(|s| (s.message_id.clone(), s))
        .collect();
    Ok(ParticipantReceivedSnapshot {
        schema_version: SCHEMA_VERSION,
        revision: 1,
        organization_id: organization_id.clone(),
        conversation_id: conversation_id.clone(),
        employee_id: employee_id.clone(),
        states,
    })
}

fn commit_participant_snapshots(
    _view: &impl MessagingRead,
    conversation_id: &Id,
    snapshots: BTreeMap<Id, ParticipantReceivedSnapshot>,
) -> Change {
    let mut change = Change::new(Ack::Committed);
    for (emp_id, mut snap) in snapshots {
        snap.revision = snap.revision.saturating_add(1);
        let path = records::participant_received_snapshot_path(conversation_id, &emp_id);
        change = change.put(path, records::encode_json(&snap));
    }
    change
}
