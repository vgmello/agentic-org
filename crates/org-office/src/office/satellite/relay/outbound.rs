//! Outbound relay: newly committed native messaging journal entries become bounded
//! per-message relay files routed to the offices of their recipients.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_store::{DurablePath, Files, Id};

use crate::error::OfficeError;
use crate::office::satellite::filters::is_replicated_journal_entry;
use crate::office::satellite::placement::{load_placement_context_locked, PlacementContext};
use crate::office::satellite::replication::OrgDeltaPayload;
use crate::office::{storage, Office};

use super::outbound_close::{append_hq_channel_only_relays, append_origin_close_answer_relays};

/// One locally committed native messaging journal entry ready for cross-office relay publication.
#[derive(Clone, Debug)]
pub struct PendingMessageJournalEntry {
    pub ordinal: u64,
    pub conversation_id: String,
    pub message_id: String,
    pub files: Vec<(String, Option<String>)>,
    /// Target offices determined by recipient placement (for DMs/GroupDMs) or channel membership
    /// (`TASK-52` P3-#1 / P4-#2).
    pub destination_offices: Vec<String>,
    /// Native message expiry timestamp in Unix ms (e.g. non-required ask expiry), preserved in
    /// transport deadlines (`TASK-52` P4-#9).
    pub expires_at_ms: Option<u64>,
}

/// Batch returned by [`Office::collect_pending_message_journal_entries`].
#[derive(Clone, Debug)]
pub struct PendingMessageJournalBatch {
    pub incarnation: String,
    pub last_ordinal: u64,
    pub entries: Vec<PendingMessageJournalEntry>,
}

impl Office {
    /// Collects ONLY `conversation.json` and the single `messages/<message_id>.json` file for
    /// bounded cross-office message relay (never copying historical messages, `TASK-52` #9/#10).
    pub fn collect_single_message_relay_files(
        &self,
        conversation_id: &str,
        message_id: &str,
    ) -> Result<Vec<(String, Option<String>)>, OfficeError> {
        self.store.sync().map_err(storage)?;
        let files = Files::open(&self.durable).map_err(OfficeError::storage)?;
        let conv_path = DurablePath::parse(&format!(
            "messaging/conversations/{conversation_id}/conversation.json"
        ))
        .map_err(storage)?;
        let msg_path = DurablePath::parse(&format!(
            "messaging/conversations/{conversation_id}/messages/{message_id}.json"
        ))
        .map_err(storage)?;
        let mut out = Vec::with_capacity(2);
        if let Some(bytes) = files.read(&conv_path).map_err(OfficeError::storage)? {
            out.push((conv_path.as_str().to_owned(), Some(STANDARD.encode(bytes))));
        }
        if let Some(bytes) = files.read(&msg_path).map_err(OfficeError::storage)? {
            out.push((msg_path.as_str().to_owned(), Some(STANDARD.encode(bytes))));
        }
        Ok(out)
    }

    /// Returns `true` while `org-store` still has queued, unconfirmed, or unmaterialized journal
    /// entries in flight (e.g. a freshly committed message whose projection is still catching up).
    pub fn has_pending_journal_pipeline(&self) -> bool {
        let st = self.store.status();
        st.materialized != st.confirmed || st.flushed != st.confirmed || st.queued > 0
    }

    /// Scans newly committed native messaging journal entries via the `ARCH-03` consumer
    /// `"org-sync-message-relay"`, returning bounded per-message relay files and recipient-routed
    /// destination offices for each locally committed message (`TASK-52` #10 / P3-#1 / P3-#4).
    pub fn collect_pending_message_journal_entries(
        &self,
    ) -> Result<PendingMessageJournalBatch, OfficeError> {
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let wm = self.register_replication_consumer("org-sync-message-relay")?;
        let confirmed = self.store.status().confirmed;
        let incarnation = self.identity.journal_incarnation.as_str().to_owned();
        let entries = self.store.entries_after_contiguous(wm).map_err(storage)?;
        let ctx = load_placement_context_locked(self)?;
        let mut out = Vec::new();
        let mut last_ord = wm;
        for entry in entries {
            let ord = entry.ordinal();
            if ord > confirmed {
                break;
            }
            let from_repl = is_replicated_journal_entry(&entry, self.replication.as_ref());
            if from_repl {
                // Even when a journal entry came from `replication.apply_message` or
                // `replication.apply_origin_close` (e.g. HQ owner withdrawal accepted at the
                // ask's origin), if this office is the ask's origin and just committed an
                // authoritative `"origin-close"`, publish that close durably to participants
                // (`TASK-52` P11-#3 / P13-#3).
                if entry.header.operation.as_ref().is_some_and(|op| {
                    op.kind == "replication.apply_message"
                        || op.kind == "replication.apply_origin_close"
                }) && !append_origin_close_answer_relays(
                    self, &ctx, ord, &entry, true, &mut out,
                )? {
                    break;
                }
            } else if !append_message_relay_entries(self, &ctx, ord, &entry, &mut out)? {
                // Conversation record not yet readable from committed generation; keep watermark
                // strictly before this entry so it is retried safely (`TASK-52` P3-#4).
                break;
            }
            last_ord = last_ord.max(ord);
        }
        Ok(PendingMessageJournalBatch {
            incarnation,
            last_ordinal: last_ord,
            entries: out,
        })
    }

    /// Advances the `"org-sync-message-relay"` journal consumer watermark after durable publication.
    pub fn advance_message_relay_consumer(&self, ordinal: u64) -> Result<(), OfficeError> {
        self.advance_replication_consumer("org-sync-message-relay", ordinal)
    }

    /// Collects the native durable `messaging/conversations/<conversation_id>/...` files for
    /// cross-office message relay so the recipient office commits the real native conversation and
    /// message into its `org-store` inbox (`TASK-52` H-13).
    pub fn collect_conversation_messaging_files(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<(String, Option<String>)>, OfficeError> {
        self.store.sync().map_err(storage)?;
        let files = Files::open(&self.durable).map_err(OfficeError::storage)?;
        let prefix = DurablePath::parse(&format!("messaging/conversations/{conversation_id}"))
            .map_err(storage)?;
        let mut out = Vec::new();
        for path in files.walk(&prefix).map_err(OfficeError::storage)? {
            if crate::messaging::records::kind_of(&path).is_some() {
                if let Some(bytes) = files.read(&path).map_err(OfficeError::storage)? {
                    out.push((path.as_str().to_owned(), Some(STANDARD.encode(bytes))));
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }
}

pub(super) struct ReadConversationOp {
    pub(super) conversation_id: Id,
}

impl crate::store::open::StoreOp for ReadConversationOp {
    type Out = Result<Option<crate::messaging::records::ConversationRecord>, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        let (conv, _) = store.read(|snap| {
            crate::messaging::ports::MessagingRead::conversation(snap, &self.conversation_id)
                .map_err(org_store::StoreError::Engine)
        })?;
        Ok(conv)
    }
}

pub(super) struct ReadConversationMembersOp {
    pub(super) conv: crate::messaging::records::ConversationRecord,
    pub(super) owner: Id,
}

impl crate::store::open::StoreOp for ReadConversationMembersOp {
    type Out = Result<Vec<Id>, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        let (members, _) = store.read(|snap| {
            crate::messaging::features::effective_auto_members(snap, &self.conv, &self.owner)
                .map_err(org_store::StoreError::Engine)
        })?;
        Ok(members)
    }
}

pub(super) fn message_expires_at_ms(msg: &crate::messaging::records::MessageRecord) -> Option<u64> {
    if let Some(exp) = msg.expires_at.as_deref() {
        return crate::parse_rfc3339_ms(exp);
    }
    msg.ttl_ms.and_then(|ttl| {
        crate::parse_rfc3339_ms(&msg.created_at).map(|created| created.saturating_add(ttl))
    })
}

fn route_message_destinations(
    office: &Office,
    ctx: &PlacementContext,
    conv: &crate::messaging::records::ConversationRecord,
    msg: &crate::messaging::records::MessageRecord,
) -> Result<Vec<String>, OfficeError> {
    let members = office
        .store
        .run(ReadConversationMembersOp {
            conv: conv.clone(),
            owner: office.owner.clone(),
        })
        .map_err(storage)?;
    let local_office_id = office.info.office_id.as_str();
    let sender = msg.sender_employee_id.as_ref();
    let targets: std::collections::BTreeSet<String> = members
        .into_iter()
        .filter(|emp| Some(emp) != sender)
        .map(|emp| {
            ctx.employee_offices
                .get(emp.as_str())
                .cloned()
                .unwrap_or_else(|| ctx.hq_office_id.clone())
        })
        .filter(|dest| {
            dest != local_office_id && (ctx.enrolled.contains(dest) || dest == &ctx.hq_office_id)
        })
        .collect();
    Ok(targets.into_iter().collect())
}

/// Extracts bounded message relay entries from one journal entry, reading the conversation from
/// the same committed journal entry or committed `Store` projection (never an unflushed file),
/// including any structured ask answer committed in the same transaction (`TASK-52` P3-#4 / P4-#4/#9).
fn append_message_relay_entries(
    office: &Office,
    ctx: &PlacementContext,
    ordinal: u64,
    entry: &org_store::Entry,
    out: &mut Vec<PendingMessageJournalEntry>,
) -> Result<bool, OfficeError> {
    let mut entry_convs: std::collections::BTreeMap<Id, Vec<u8>> =
        std::collections::BTreeMap::new();
    let mut entry_answers: std::collections::BTreeMap<(Id, String), (String, Vec<u8>)> =
        std::collections::BTreeMap::new();
    for fv in entry.versions() {
        let Some(bytes) = fv.bytes else {
            continue;
        };
        match crate::messaging::records::kind_of(fv.path) {
            Some(crate::messaging::records::Kind::Conversation(cid)) => {
                entry_convs.insert(cid, bytes.to_vec());
            }
            Some(crate::messaging::records::Kind::ParticipantAnswer(cid, _, ask_id)) => {
                entry_answers.insert((cid, ask_id), (fv.path.as_str().to_owned(), bytes.to_vec()));
            }
            _ => {}
        }
    }
    // Publish origin-authoritative ask closes and recoveries FIRST so any dependent reply message
    // in the same journal entry never fails foreign-key checks on a newly added/rejoined office
    // before the recovered ask is committed (`TASK-52` P14-#3 / P15-#2).
    if !append_origin_close_answer_relays(office, ctx, ordinal, entry, false, out)? {
        return Ok(false);
    }
    let mut relayed_conv_ids: std::collections::BTreeSet<Id> = std::collections::BTreeSet::new();
    for fv in entry.versions() {
        let Some(crate::messaging::records::Kind::Message(cid, mid)) =
            crate::messaging::records::kind_of(fv.path)
        else {
            continue;
        };
        let Some(msg_bytes) = fv.bytes else {
            continue;
        };
        relayed_conv_ids.insert(cid.clone());
        let conv_bytes = if let Some(bytes) = entry_convs.get(&cid) {
            bytes.clone()
        } else if let Some(conv) = office
            .store
            .run(ReadConversationOp {
                conversation_id: cid.clone(),
            })
            .map_err(storage)?
        {
            crate::messaging::records::encode_json(&conv)
        } else {
            return Ok(false);
        };
        let conv: crate::messaging::records::ConversationRecord =
            serde_json::from_slice(&conv_bytes)
                .map_err(|_| OfficeError::Invalid("malformed conversation in journal"))?;
        let msg: crate::messaging::records::MessageRecord = serde_json::from_slice(msg_bytes)
            .map_err(|_| OfficeError::Invalid("malformed message in journal"))?;
        let destination_offices = route_message_destinations(office, ctx, &conv, &msg)?;
        let conv_path = format!("messaging/conversations/{cid}/conversation.json");
        let conv_entry = (conv_path, Some(STANDARD.encode(&conv_bytes)));
        let msg_entry = (
            fv.path.as_str().to_owned(),
            Some(STANDARD.encode(msg_bytes)),
        );
        let ans_entry = msg.reply_to_message_id.as_ref().and_then(|reply_to| {
            entry_answers
                .get(&(cid.clone(), reply_to.clone()))
                .map(|(ans_path, ans_bytes)| (ans_path.clone(), Some(STANDARD.encode(ans_bytes))))
        });
        let mut combined = vec![conv_entry.clone(), msg_entry.clone()];
        if let Some(ref ans) = ans_entry {
            combined.push(ans.clone());
        }
        let encoded_len = serde_json::to_vec(&OrgDeltaPayload {
            v: 1,
            files: combined.clone(),
        })
        .map_or(usize::MAX, |b| b.len());
        if encoded_len > org_sync::messages::MAX_PAYLOAD_BYTES {
            // Deliver `conversation.json` separately FIRST when bundling with `message.json`
            // exceeds `MAX_PAYLOAD_BYTES` (e.g. a 1,000-member channel) so a satellite seeing
            // the channel for the first time still receives the conversation (`TASK-52` P6-#5).
            out.push(PendingMessageJournalEntry {
                ordinal,
                conversation_id: cid.as_str().to_owned(),
                message_id: format!("conv-{cid}-r{}", conv.revision),
                files: vec![conv_entry],
                destination_offices: destination_offices.clone(),
                expires_at_ms: message_expires_at_ms(&msg),
            });
            let mut msg_only = vec![msg_entry];
            if let Some(ans) = ans_entry {
                msg_only.push(ans);
            }
            out.push(PendingMessageJournalEntry {
                ordinal,
                conversation_id: cid.as_str().to_owned(),
                message_id: mid,
                files: msg_only,
                destination_offices,
                expires_at_ms: message_expires_at_ms(&msg),
            });
        } else {
            out.push(PendingMessageJournalEntry {
                ordinal,
                conversation_id: cid.as_str().to_owned(),
                message_id: mid,
                files: combined,
                destination_offices,
                expires_at_ms: message_expires_at_ms(&msg),
            });
        }
    }
    // Replicate HQ-authored channel creations and membership revisions immediately even before
    // any message is sent in the channel (`TASK-52` P9-#8).
    if office.info.role == crate::model::OfficeRole::Hq {
        append_hq_channel_only_relays(office, ctx, ordinal, &entry_convs, &relayed_conv_ids, out)?;
    }
    Ok(true)
}
