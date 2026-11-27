//! Native message relay: checked inbound delivery, local wakes, outbox replies, and relay
//! target grouping and sync.

use std::collections::BTreeSet;
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_office::replication::StoredRelayItem;
use org_office::{Office, OrgDeltaPayload};
use org_sync::messages::{
    acknowledge, next as next_message, receive, Message, MessageOutput, Receipt,
};
use org_sync_proto::{Digest, OfficeId, OrgId, SyncFrame};
use sha2::{Digest as _, Sha256};

use super::{
    deterministic_uuid_v7, now_ms, InboundCallbacks, InboundMessage, RelayedWake, StaticPeer,
    WakeRelayHandler,
};

fn apply_native_relayed_message_if_applicable(
    office: &Office,
    org_id: &str,
    origin_office: &str,
    message_id: &str,
    payload: &[u8],
) -> Result<Option<org_office::RelayedMessageOutcome>, ()> {
    let Ok(delta) = serde_json::from_slice::<OrgDeltaPayload>(payload) else {
        return Ok(None);
    };
    if delta.v != 1 || delta.files.is_empty() {
        return Ok(None);
    }
    let Ok(files) = delta.decode_files() else {
        return Err(());
    };
    let op_id = deterministic_uuid_v7(&format!("msg:{org_id}:{origin_office}:{message_id}"));
    office
        .apply_relayed_messaging_files(origin_office, message_id, &op_id, files)
        .map(Some)
        .map_err(|_| ())
}

/// Derives and enqueues deduplicated local agent wakes at the destination office immediately
/// after an inbound relayed native message is durably committed (`TASK-52` P4-#8 / P5-#10).
/// Returns `true` iff every required local agent wake durably succeeded.
fn emit_local_wakes_for_delivered_message(
    wake_handler: Option<&WakeRelayHandler>,
    origin_office: &str,
    destination_office: &str,
    outcome: &org_office::RelayedMessageOutcome,
) -> bool {
    let Some(handler) = wake_handler else {
        return true;
    };
    if let Some(w) = &outcome.withdrawn_ask {
        if handler(RelayedWake {
            origin_office: origin_office.to_owned(),
            destination_office: destination_office.to_owned(),
            employee_id: w.asker_id.clone(),
            kind: "ask_withdrawal".to_owned(),
            source_id: format!("withdraw:{}:{}", w.ask_id, w.asker_id),
            sender_id: outcome.sender_id.clone(),
            sender_name: outcome.sender_name.clone(),
            sender_is_person: outcome.sender_is_person,
            conversation_id: Some(outcome.conversation_id.clone()),
            summary: String::new(),
            attachment_ids: Vec::new(),
            ask_answer: None,
            ask_withdrawal: Some(w.clone()),
        })
        .is_err()
        {
            return false;
        }
    }
    if let Some(ans) = &outcome.answered_ask {
        if handler(RelayedWake {
            origin_office: origin_office.to_owned(),
            destination_office: destination_office.to_owned(),
            employee_id: ans.asker_id.clone(),
            kind: "ask_answer".to_owned(),
            source_id: format!("ans:{}:{}", ans.ask_id, ans.asker_id),
            sender_id: outcome.sender_id.clone(),
            sender_name: outcome.sender_name.clone(),
            sender_is_person: outcome.sender_is_person,
            conversation_id: Some(outcome.conversation_id.clone()),
            summary: ans.answer_summary.clone(),
            attachment_ids: ans.attachment_ids.clone(),
            ask_answer: Some(ans.clone()),
            ask_withdrawal: None,
        })
        .is_err()
        {
            return false;
        }
    }
    if outcome.message_id.is_empty() || outcome.local_recipient_wakes.is_empty() {
        return true;
    }
    let msg_attachment_ids: Vec<String> = outcome
        .delivered_message
        .as_ref()
        .map(|m| m.attachments.iter().map(|a| a.id.clone()).collect())
        .unwrap_or_default();
    let answered_asker = outcome.answered_ask.as_ref().map(|a| a.asker_id.as_str());
    for wake in &outcome.local_recipient_wakes {
        if answered_asker == Some(wake.employee_id.as_str()) {
            continue;
        }
        if handler(RelayedWake {
            origin_office: origin_office.to_owned(),
            destination_office: destination_office.to_owned(),
            employee_id: wake.employee_id.clone(),
            kind: wake.kind.clone(),
            // Carry the clean replyable message UUIDv7 here (`TASK-157`); per-recipient
            // deduplication already appends `wake.employee_id` to `PendingWakeItem::item_id`.
            source_id: outcome.message_id.clone(),
            sender_id: outcome.sender_id.clone(),
            sender_name: outcome.sender_name.clone(),
            sender_is_person: outcome.sender_is_person,
            conversation_id: Some(outcome.conversation_id.clone()),
            summary: outcome.summary.clone(),
            attachment_ids: msg_attachment_ids.clone(),
            ask_answer: None,
            ask_withdrawal: None,
        })
        .is_err()
        {
            return false;
        }
    }
    true
}

/// Expires stale non-`close-` head entries in the satellite message outbox while preserving
/// authoritative ask closes/withdrawals (`close-*`) indefinitely (`TASK-52` P12-#5).
pub(super) fn expire_stale_non_close_head_messages(office: &Office) -> Result<(), String> {
    loop {
        let head_is_close = office
            .replication()
            .message_queue_snapshot()
            .ok()
            .and_then(|q| q.pending.front().map(|m| m.id.starts_with("close-")))
            .unwrap_or(false);
        if head_is_close {
            break;
        }
        match next_message(office.replication().as_ref(), now_ms())
            .map_err(|e| format!("message outbox error: {e}"))?
        {
            MessageOutput::Receipt(_) => continue,
            MessageOutput::Idle | MessageOutput::Deliver(_) => break,
            other => return Err(format!("unexpected message outbox state: {other:?}")),
        }
    }
    Ok(())
}

pub(super) fn apply_message_outbox_reply(
    office: &Office,
    peer: &StaticPeer,
    msg: Message,
    reply: Option<SyncFrame>,
) -> Result<usize, String> {
    match reply {
        Some(SyncFrame::MessageReceipt {
            org_id,
            origin_office,
            destination_office,
            message_id,
            fingerprint_hex,
            delivered,
            relayed_by_hq,
            ..
        }) => {
            let matches_sent = org_id == msg.org_id.as_str()
                && origin_office == msg.origin.as_str()
                && destination_office == msg.destination.as_str()
                && message_id == msg.id
                && fingerprint_hex == msg.fingerprint.to_hex();
            if !matches_sent {
                return Err("MessageReceipt does not match sent message".into());
            }
            if delivered {
                acknowledge(
                    office.replication().as_ref(),
                    peer,
                    Receipt {
                        message: msg,
                        delivered: true,
                    },
                )
                .map_err(|e| format!("failed applying message receipt: {e}"))?;
                Ok(1)
            } else if relayed_by_hq {
                Ok(0)
            } else {
                Err("HQ returned non-delivered message receipt".into())
            }
        }
        Some(SyncFrame::Error { code, .. }) if code == "destination_busy" => Ok(0),
        Some(SyncFrame::Error { code, .. }) if code == "destination_removed" => {
            // Destination office was permanently removed at HQ; retire the undeliverable outbox
            // entry durably so non-expiring closes never retry forever (`TASK-52` P15-#4).
            let _ = office
                .replication()
                .retire_undeliverable_message(&msg.id, msg.destination.as_str());
            Ok(0)
        }
        other => Err(format!("unexpected message receipt: {other:?}")),
    }
}

/// Validates and delivers an inbound cross-office message safely on both HQ and satellites:
/// short-circuits exact replays, refuses native digest mismatches WITHOUT recording `delivered: true`,
/// persists local wakes before recording terminal receipt, and notifies local waiters/live streams
/// (`TASK-52` P5-#10 / P6-#11 / P7-#10, `TASK-117`).
pub(super) fn deliver_inbound_message_checked(
    office: &Office,
    peer: &StaticPeer,
    callbacks: InboundCallbacks<'_>,
    msg: &InboundMessage,
    mut sync_msg: Message,
    payload: &[u8],
    fp: Digest,
) -> bool {
    let now = now_ms();
    if office
        .replication()
        .has_received_message_receipt(&msg.origin_office, &msg.message_id, &msg.fingerprint_hex)
        .unwrap_or(false)
    {
        return true;
    }
    let clock_high = office
        .replication()
        .message_queue_snapshot()
        .map_or(0, |q| q.clock_high_ms)
        .max(now);
    // Authoritative ask closes/recoveries (`close-*`) retain identity independently of transport
    // deadlines so a deadline crossing or refresh never wedges delivery on expired-envelope
    // divergence (`TASK-52` P14-#1).
    if msg.message_id.starts_with("close-") {
        sync_msg.deadline_ms = sync_msg
            .deadline_ms
            .max(clock_high.saturating_add(7 * 24 * 60 * 60 * 1000));
    } else if clock_high >= sync_msg.deadline_ms {
        let _ = receive(office.replication().as_ref(), peer, sync_msg, now);
        return false;
    }
    if !validate_inbound_message_before_native_apply(office, &sync_msg, payload, fp, now) {
        // Never invoke `receive` on a corrupt/digest-mismatched native payload, or `org-sync`
        // could persist `delivered: true` for an empty inbox (`TASK-52` P7-#10).
        return false;
    }
    let Ok(outcome) = apply_native_relayed_message_if_applicable(
        office,
        &msg.org_id,
        &msg.origin_office,
        &msg.message_id,
        payload,
    ) else {
        return false;
    };
    let wakes_ok = outcome.as_ref().is_none_or(|info| {
        emit_local_wakes_for_delivered_message(
            callbacks.wake,
            &msg.origin_office,
            &office.info().office_id,
            info,
        )
    });
    let delivered = wakes_ok
        && matches!(
            receive(office.replication().as_ref(), peer, sync_msg, now),
            Ok(MessageOutput::Receipt(Receipt {
                delivered: true,
                ..
            }))
        );
    if delivered {
        if let (Some(notify), Some(info)) = (callbacks.notify, outcome.as_ref()) {
            notify(info);
        }
    }
    delivered
}

fn validate_inbound_message_before_native_apply(
    office: &Office,
    sync_msg: &Message,
    payload: &[u8],
    fp: Digest,
    now_ms: u64,
) -> bool {
    if sync_msg.validate().is_err() {
        return false;
    }
    let clock_high = office
        .replication()
        .message_queue_snapshot()
        .map_or(0, |q| q.clock_high_ms)
        .max(now_ms);
    if clock_high >= sync_msg.deadline_ms {
        return false;
    }
    if is_native_messaging_delta(payload)
        && Digest::from_bytes(Sha256::digest(payload).into()) != fp
    {
        return false;
    }
    true
}

fn is_native_messaging_delta(payload: &[u8]) -> bool {
    serde_json::from_slice::<OrgDeltaPayload>(payload).is_ok_and(|d| {
        !d.files.is_empty()
            && d.files
                .iter()
                .all(|(p, _)| p.starts_with("messaging/conversations/"))
    })
}

fn build_native_relay_targets(
    office: &Office,
    org_id: &OrgId,
    local_id: &OfficeId,
    message_id: &str,
    destinations: Vec<String>,
    payload_bytes: &[u8],
    deadline_ms: u64,
) -> (Vec<Message>, Vec<StoredRelayItem>) {
    let fp = Digest::from_bytes(Sha256::digest(payload_bytes).into());
    let mut messages = Vec::new();
    let mut relays = Vec::new();
    if office.info().role == org_office::OfficeRole::Satellite {
        let multi_dest = destinations.len() > 1;
        for target_id in destinations {
            let Ok(dest_office) = OfficeId::new(&target_id) else {
                continue;
            };
            let wire_id = if multi_dest {
                format!("{message_id}_{target_id}")
            } else {
                message_id.to_owned()
            };
            messages.push(Message {
                org_id: org_id.clone(),
                origin: local_id.clone(),
                destination: dest_office,
                id: wire_id,
                fingerprint: fp,
                payload: Arc::from(payload_bytes),
                deadline_ms,
            });
        }
    } else {
        let payload_b64 = STANDARD.encode(payload_bytes);
        for target_id in destinations {
            let frame = SyncFrame::MessageDeliver {
                org_id: office.info().org_id.clone(),
                origin_office: office.info().office_id.clone(),
                destination_office: target_id.clone(),
                message_id: message_id.to_owned(),
                fingerprint_hex: fp.to_hex(),
                payload_b64: payload_b64.clone(),
                deadline_ms,
            };
            if let Ok(payload_json) = serde_json::to_string(&frame) {
                relays.push(StoredRelayItem {
                    id: format!("{message_id}:{target_id}"),
                    origin_office: office.info().office_id.clone(),
                    destination_office: target_id,
                    fingerprint_hex: fp.to_hex(),
                    kind: "message".into(),
                    payload_json,
                    expires_at_ms: if message_id.starts_with("close-") {
                        0
                    } else {
                        deadline_ms
                    },
                });
            }
        }
    }
    (messages, relays)
}

type DestRelayBuckets = std::collections::BTreeMap<String, (Vec<Message>, Vec<StoredRelayItem>)>;

fn group_ordinal_relays_by_dest(
    office: &Office,
    org_id: &OrgId,
    local_id: &OfficeId,
    items: Vec<org_office::PendingMessageJournalEntry>,
    now: u64,
) -> DestRelayBuckets {
    let default_deadline_ms = now.saturating_add(7 * 24 * 60 * 60 * 1000);
    let max_deadline_ms = now.saturating_add(org_sync::messages::MAX_MESSAGE_TTL_MS);
    let mut by_dest: DestRelayBuckets = std::collections::BTreeMap::new();
    for item in items {
        let deadline_ms = if item.message_id.starts_with("close-") {
            max_deadline_ms
        } else {
            item.expires_at_ms
                .unwrap_or(default_deadline_ms)
                .min(max_deadline_ms)
        };
        if item.files.is_empty() || item.destination_offices.is_empty() || deadline_ms <= now {
            continue;
        }
        let Ok(payload_bytes) = serde_json::to_vec(&OrgDeltaPayload {
            v: 1,
            files: item.files,
        }) else {
            continue;
        };
        if payload_bytes.len() > org_sync::messages::MAX_PAYLOAD_BYTES {
            continue;
        }
        let (msgs, rels) = build_native_relay_targets(
            office,
            org_id,
            local_id,
            &item.message_id,
            item.destination_offices,
            &payload_bytes,
            deadline_ms,
        );
        for m in msgs {
            by_dest
                .entry(m.destination.as_str().to_owned())
                .or_default()
                .0
                .push(m);
        }
        for r in rels {
            by_dest
                .entry(r.destination_office.clone())
                .or_default()
                .1
                .push(r);
        }
    }
    by_dest
}

/// Crash-safe, retry-safe `ARCH-03` journal publisher for native cross-office chat/ask/answer
/// messages (`TASK-52` #9, #10 & P3-#1 / P7-#1): groups all relay parts belonging to the same
/// journal ordinal (such as split `conversation.json` + `message.json`) into ONE atomic publish.
pub fn sync_native_message_relays(office: &Office) {
    if !office.is_multi_office().unwrap_or(false) {
        return;
    }
    let _ = org_office::promote_pending_removed_origin_withdrawals(office);
    let Ok(batch) = office.collect_pending_message_journal_entries() else {
        return;
    };
    let incarnation = batch.incarnation;
    let last_ord = batch.last_ordinal;
    let Ok(org_id) = OrgId::new(&office.info().org_id) else {
        return;
    };
    let Ok(local_id) = OfficeId::new(&office.info().office_id) else {
        return;
    };
    let now = now_ms();
    let mut by_ordinal: std::collections::BTreeMap<u64, Vec<_>> = std::collections::BTreeMap::new();
    for item in batch.entries {
        by_ordinal.entry(item.ordinal).or_default().push(item);
    }
    let mut completed_ord = 0u64;
    let mut blocked_dests: BTreeSet<String> = BTreeSet::new();
    for (ord, items) in by_ordinal {
        let by_dest = group_ordinal_relays_by_dest(office, &org_id, &local_id, items, now);
        // Publish per destination; once a destination fails an ordinal, stop advancing THAT
        // destination at later ordinals while continuing to publish for healthy destinations
        // (`TASK-52` P8-#7 / P9-#1).
        let mut ord_all_ok = true;
        for (dest, (dest_msgs, dest_rels)) in by_dest {
            if blocked_dests.contains(&dest) {
                ord_all_ok = false;
                continue;
            }
            if org_office::publish_message_journal_relay_for_dest_checked(
                office,
                &dest,
                dest_msgs,
                dest_rels,
                &incarnation,
                ord,
                now,
            )
            .is_err()
            {
                blocked_dests.insert(dest);
                ord_all_ok = false;
            }
        }
        if ord_all_ok && blocked_dests.is_empty() {
            completed_ord = completed_ord.max(ord);
        }
    }
    let advance_to = if !blocked_dests.is_empty() {
        completed_ord
    } else {
        completed_ord.max(last_ord)
    };
    if advance_to > 0 {
        let _ = office.advance_message_relay_consumer(advance_to);
    }
}
