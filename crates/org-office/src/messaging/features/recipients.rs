//! Send targets and recipients: who a message reaches and why, participant snapshots,
//! mention candidates and per-recipient state rows.

use std::collections::BTreeMap;

use org_proto::chat::{ConversationKind, DeliveredBecause, MentionedMember};
use org_proto::msg::{MessageLifecycleState, Meta};
use org_proto::ErrorCode;
use org_store::{Change, Id, OfficeIdentity, Refusal};

use crate::messaging::{
    mentions::MentionCandidate,
    ports::MessagingRead,
    records::{
        self, ConversationMembership, ConversationRecord, MessageRecord,
        ParticipantReceivedSnapshot, RecipientStateRecord, SCHEMA_VERSION,
    },
};
use crate::organization::records::Employee;

use super::conversations::{all_conversations_including_auto, find_channel};
use super::{is_active_employee, refusal, resolve_employee};

pub(in crate::messaging) fn resolve_send_target(
    read: &impl MessagingRead,
    here: &OfficeIdentity,
    owner: &Id,
    sender: &Employee,
    employees: &[Employee],
    to_raw: &str,
    now_utc: &str,
) -> Result<(ConversationRecord, bool), Refusal> {
    let all = all_conversations_including_auto(read, now_utc)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to load conversations"))?;
    let trimmed = to_raw.trim();
    if trimmed.is_empty() {
        return Err(refusal(ErrorCode::InvalidRequest, "target cannot be empty"));
    }
    // 1. Explicit conversation UUIDv7
    if let Ok(cid) = Id::parse(trimmed) {
        if let Some(conv) = all.iter().find(|c| c.id == cid) {
            let persisted = read
                .conversation(&conv.id)
                .map_err(|_| refusal(ErrorCode::StorageError, "failed to read conversation"))?
                .is_some();
            return Ok((conv.clone(), persisted));
        }
    }
    // 2. Channel by `#name`
    if trimmed.starts_with('#') {
        let conv = find_channel(&all, trimmed)
            .ok_or_else(|| refusal(ErrorCode::NotFound, format!("channel {trimmed} not found")))?;
        let persisted = read
            .conversation(&conv.id)
            .map_err(|_| refusal(ErrorCode::StorageError, "failed to read channel"))?
            .is_some();
        return Ok((conv, persisted));
    }
    // 3. Employee (DM) by owner / id / handle
    if let Ok(target_emp) = resolve_employee(employees, owner, trimmed) {
        if !is_active_employee(target_emp) {
            return Err(refusal(
                ErrorCode::UnknownRecipient,
                "recipient is not active",
            ));
        }
        let dm_id = records::dm_conversation_id(&here.organization_id, &sender.id, &target_emp.id);
        if let Some(conv) = read
            .conversation(&dm_id)
            .map_err(|_| refusal(ErrorCode::StorageError, "failed to read dm"))?
        {
            return Ok((conv, true));
        }
        let mut memberships = vec![ConversationMembership {
            employee_id: sender.id.clone(),
            joined_at: now_utc.to_owned(),
            left_at: None,
        }];
        if target_emp.id != sender.id {
            memberships.push(ConversationMembership {
                employee_id: target_emp.id.clone(),
                joined_at: now_utc.to_owned(),
                left_at: None,
            });
        }
        let conv = ConversationRecord {
            schema_version: SCHEMA_VERSION,
            revision: 1,
            organization_id: here.organization_id.clone(),
            id: dm_id,
            kind: ConversationKind::Dm,
            name: None,
            auto_scope: None,
            team_id: None,
            project_id: None,
            archived: false,
            created_at: now_utc.to_owned(),
            memberships,
            pins: Vec::new(),
        };
        return Ok((conv, false));
    }
    // 4. Bare channel name fallback (e.g., "general")
    if let Some(conv) = find_channel(&all, trimmed) {
        let persisted = read
            .conversation(&conv.id)
            .map_err(|_| refusal(ErrorCode::StorageError, "failed to read channel"))?
            .is_some();
        return Ok((conv, persisted));
    }
    Err(refusal(
        ErrorCode::UnknownRecipient,
        format!("unknown recipient or channel {trimmed:?}"),
    ))
}

fn insert_recipient_reason(
    reasons: &mut BTreeMap<Id, DeliveredBecause>,
    sender_id: &Id,
    emp_id: Id,
    because: DeliveredBecause,
) {
    if emp_id == *sender_id {
        return;
    }
    reasons
        .entry(emp_id)
        .and_modify(|existing| {
            if because < *existing {
                *existing = because;
            }
        })
        .or_insert(because);
}

fn collect_thread_reply_recipients(
    read: &impl MessagingRead,
    conv_id: &Id,
    root_id: &str,
    active_members: &[Id],
    sender_id: &Id,
    reasons: &mut BTreeMap<Id, DeliveredBecause>,
) -> Result<(), Refusal> {
    let conv_msgs = read
        .messages_in_conversation(conv_id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read thread"))?;
    for m in &conv_msgs {
        let in_thread = m.id == root_id || m.thread_root_id.as_deref() == Some(root_id);
        if !in_thread {
            continue;
        }
        if let Some(author) = m
            .sender_employee_id
            .as_ref()
            .filter(|a| active_members.contains(a))
        {
            insert_recipient_reason(
                reasons,
                sender_id,
                author.clone(),
                DeliveredBecause::ThreadReply,
            );
        }
    }
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "recipient reason collection evaluates conversation, sender, members, mentions, and reply/thread context"
)]
pub(crate) fn collect_recipient_reasons(
    read: &impl MessagingRead,
    conv: &ConversationRecord,
    sender_id: &Id,
    active_members: &[Id],
    mentions: &[MentionedMember],
    thread_root_id: Option<&str>,
    kind: &str,
    meta: &Meta,
) -> Result<BTreeMap<Id, DeliveredBecause>, Refusal> {
    let mut reasons: BTreeMap<Id, DeliveredBecause> = BTreeMap::new();
    if matches!(conv.kind, ConversationKind::Dm | ConversationKind::GroupDm) {
        for emp_id in active_members {
            insert_recipient_reason(
                &mut reasons,
                sender_id,
                emp_id.clone(),
                DeliveredBecause::Addressed,
            );
        }
        return Ok(reasons);
    }
    if kind == "handoff" || kind == "request" {
        if let Some(target_id) = meta
            .get("card")
            .and_then(|c| c.get("addressed_to"))
            .and_then(|v| v.as_str())
            .and_then(|s| Id::parse(s).ok())
            .filter(|id| active_members.contains(id))
        {
            insert_recipient_reason(
                &mut reasons,
                sender_id,
                target_id,
                DeliveredBecause::Addressed,
            );
        }
    }
    if let Some(root_id) = thread_root_id {
        collect_thread_reply_recipients(
            read,
            &conv.id,
            root_id,
            active_members,
            sender_id,
            &mut reasons,
        )?;
    }
    for eid in mentions
        .iter()
        .filter_map(|m| Id::parse(&m.employee_id).ok())
        .filter(|id| active_members.contains(id))
    {
        insert_recipient_reason(&mut reasons, sender_id, eid, DeliveredBecause::Mention);
    }
    Ok(reasons)
}

pub(crate) fn upsert_participant_snapshot(
    read: &impl MessagingRead,
    org_id: &Id,
    conv_id: &Id,
    emp_id: &Id,
    state: RecipientStateRecord,
    change: Change,
) -> Result<Change, Refusal> {
    let existing_states = read
        .recipient_states_for_employee(emp_id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read recipient states"))?;
    let mut map: BTreeMap<String, RecipientStateRecord> = existing_states
        .into_iter()
        .filter(|s| s.conversation_id == *conv_id)
        .map(|s| (s.message_id.clone(), s))
        .collect();
    map.insert(state.message_id.clone(), state);
    let snap = ParticipantReceivedSnapshot {
        schema_version: SCHEMA_VERSION,
        revision: map.len() as u64,
        organization_id: org_id.clone(),
        conversation_id: conv_id.clone(),
        employee_id: emp_id.clone(),
        states: map,
    };
    let path = records::participant_received_snapshot_path(conv_id, emp_id);
    let existed = read
        .source_hash(&path)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to check snapshot file"))?
        .is_some();
    let bytes = records::encode_json(&snap);
    Ok(if existed {
        change.put(path, bytes)
    } else {
        change.create(path, bytes)
    })
}

pub(crate) fn build_mention_candidates(
    employees: &[Employee],
    active_members: &[Id],
    owner: &Id,
) -> Vec<MentionCandidate> {
    let mut out = Vec::new();
    for e in employees {
        if !is_active_employee(e) || !active_members.contains(&e.id) {
            continue;
        }
        out.push(MentionCandidate {
            employee_id: e.id.clone(),
            handle: e.handle.clone(),
            display_name: e.name.clone(),
        });
        if e.id == *owner && !e.handle.eq_ignore_ascii_case("owner") {
            out.push(MentionCandidate {
                employee_id: e.id.clone(),
                handle: "owner".into(),
                display_name: e.name.clone(),
            });
        }
    }
    out
}

/// Resolves the single office owning durable inbox state for `emp_id` (an agent's placed office
/// when hired into an enrolled satellite, else HQ or `here_office_id` in single-office mode,
/// `TASK-52` P5-#3).
pub(in crate::messaging) fn recipient_owner_office_id(
    read: &impl MessagingRead,
    here_office_id: &Id,
    emp_id: &Id,
) -> Id {
    let Ok(offices) = read.offices() else {
        return here_office_id.clone();
    };
    if offices.len() <= 1 {
        return here_office_id.clone();
    }
    let hq_id = offices
        .iter()
        .find(|o| o.role == crate::organization::records::OfficeRole::Hq)
        .map(|o| o.id.clone())
        .unwrap_or_else(|| here_office_id.clone());
    let Ok(hires) = read.hire_requests() else {
        return hq_id;
    };
    for req in hires {
        if req.state == crate::organization::records::HireState::Hired
            && req.employee_id.as_ref() == Some(emp_id)
        {
            if let Some(oid) = req.proposal.office_id.as_deref() {
                if let Some(found) = offices.iter().find(|o| o.id.as_str() == oid) {
                    return found.id.clone();
                }
            }
        }
    }
    hq_id
}

#[expect(
    clippy::too_many_arguments,
    reason = "recipient state builder threads read view, message record, roster, reasons, and delivery metadata"
)]
pub(in crate::messaging) fn append_send_recipient_states(
    read: &impl MessagingRead,
    here: &OfficeIdentity,
    conv_id: &Id,
    record: &MessageRecord,
    employees: &[Employee],
    reasons: BTreeMap<Id, DeliveredBecause>,
    org_seq: u64,
    now_utc: &str,
    mut change: Change,
) -> Result<(Change, Vec<(Id, RecipientStateRecord)>), Refusal> {
    let origin_incarnation = here.journal_incarnation.as_str().to_owned();
    let mut state_seq = read
        .max_state_sequence(&here.office_id, &origin_incarnation)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to allocate state_sequence"))?;
    let mut recipients = Vec::new();
    for (emp_id, because) in reasons {
        let owner_office = recipient_owner_office_id(read, &here.office_id, &emp_id);
        let disp = employees
            .iter()
            .find(|e| e.id == emp_id)
            .map(|e| e.name.clone());
        let is_local_owner = owner_office == here.office_id;
        if is_local_owner {
            state_seq = state_seq.saturating_add(1);
        }
        let state = RecipientStateRecord {
            organization_id: here.organization_id.clone(),
            message_id: record.id.clone(),
            conversation_id: conv_id.clone(),
            employee_id: emp_id.clone(),
            recipient_display_name: disp,
            state: MessageLifecycleState::Unread,
            triage_generation: 0,
            snoozed_until: None,
            snooze_after_task: false,
            snooze_note: None,
            triaged_at: None,
            read_at: None,
            updated_at: now_utc.to_owned(),
            reason: None,
            delivered_because: because,
            related_message_ids: Vec::new(),
            expired_waiting_person: false,
            owner_office_id: owner_office,
            origin_incarnation: origin_incarnation.clone(),
            origin_sequence: state_seq,
            org_epoch: 1,
            org_sequence: org_seq,
        };
        // Persist durable inbox state ONLY at the recipient's single owning office while still
        // returning every recipient in `recipients` for delivery notifications (`TASK-52` P5-#3).
        if is_local_owner {
            change = upsert_participant_snapshot(
                read,
                &here.organization_id,
                conv_id,
                &emp_id,
                state.clone(),
                change,
            )?;
        }
        recipients.push((emp_id, state));
    }
    Ok((change, recipients))
}
