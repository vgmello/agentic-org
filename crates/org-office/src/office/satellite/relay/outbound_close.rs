//! Outbound relay of origin ask closes, handoff accepts and HQ channel-only copies.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_store::Id;

use crate::error::OfficeError;
use crate::office::satellite::placement::PlacementContext;
use crate::office::satellite::replication::OrgDeltaPayload;
use crate::office::{storage, Office};

use super::types::{PendingMessageJournalEntry, ReadConversationMembersOp, ReadConversationOp};

struct ReadMessageOp {
    ask_id: String,
}

impl crate::store::open::StoreOp for ReadMessageOp {
    type Out = Result<Option<crate::messaging::records::MessageRecord>, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        let (msg, _) = store.read(|snap| {
            crate::messaging::ports::MessagingRead::message(snap, &self.ask_id)
                .map_err(org_store::StoreError::Engine)
        })?;
        Ok(msg)
    }
}

fn delta_fits_message_limit(files: &[(String, Option<String>)]) -> bool {
    serde_json::to_vec(&OrgDeltaPayload {
        v: 1,
        files: files.to_vec(),
    })
    .is_ok_and(|b| b.len() <= org_sync::messages::MAX_PAYLOAD_BYTES)
}

type EncodedRelayFiles = Vec<(String, Option<String>)>;

/// Builds bounded, ordered non-expiring `close-*` deliveries for an origin-authoritative ask
/// close: prefers one single bundle `[conv, ask, answer]`, falls back to `[ask, answer]`, and
/// splits oversized ask+answer combinations into ordered `close-conv-*` -> `close-ask-*` ->
/// `close-*` entries so a large valid ask can never wedge a newly added office (`TASK-52` P13-#2 / P14-#2).
fn build_origin_close_entries(
    cid: &Id,
    conv: &crate::messaging::records::ConversationRecord,
    ask_msg: &crate::messaging::records::MessageRecord,
    ans_path: &str,
    ans_bytes: &[u8],
    ask_id: &str,
) -> Vec<(String, EncodedRelayFiles)> {
    let ans_entry = (ans_path.to_owned(), Some(STANDARD.encode(ans_bytes)));
    let ask_entry = (
        format!("messaging/conversations/{cid}/messages/{}.json", ask_msg.id),
        Some(STANDARD.encode(crate::messaging::records::encode_json(ask_msg))),
    );
    let conv_entry = (
        format!("messaging/conversations/{cid}/conversation.json"),
        Some(STANDARD.encode(crate::messaging::records::encode_json(conv))),
    );
    let with_conv = vec![conv_entry.clone(), ask_entry.clone(), ans_entry.clone()];
    if delta_fits_message_limit(&with_conv) {
        return vec![(format!("close-{ask_id}"), with_conv)];
    }
    let with_ask = vec![ask_entry.clone(), ans_entry.clone()];
    if delta_fits_message_limit(&with_ask) {
        return vec![(format!("close-{ask_id}"), with_ask)];
    }
    let mut entries = Vec::new();
    let conv_and_ask = vec![conv_entry.clone(), ask_entry.clone()];
    if delta_fits_message_limit(&conv_and_ask) {
        entries.push((format!("close-ask-{ask_id}"), conv_and_ask));
    } else {
        entries.push((format!("close-conv-{ask_id}"), vec![conv_entry]));
        entries.push((format!("close-ask-{ask_id}"), vec![ask_entry]));
    }
    entries.push((format!("close-{ask_id}"), vec![ans_entry]));
    entries
}

struct OriginCloseCandidate<'a> {
    ordinal: u64,
    cid: &'a Id,
    ask_id: &'a str,
    ans_path: &'a str,
    bytes: &'a [u8],
    origin_only: bool,
}

/// Emits durable journal-backed `close-{ask_id}` relays for any origin-owned ask close
/// (`reason == "origin-close"`) or withdrawal (`TASK-52` P11-#3 / P11-#5 / P13-#2 / P13-#3).
pub(super) fn append_origin_close_answer_relays(
    office: &Office,
    ctx: &PlacementContext,
    ordinal: u64,
    entry: &org_store::Entry,
    origin_only: bool,
    out: &mut Vec<PendingMessageJournalEntry>,
) -> Result<bool, OfficeError> {
    for fv in entry.versions() {
        let Some(crate::messaging::records::Kind::ParticipantAnswer(cid, _, ask_id)) =
            crate::messaging::records::kind_of(fv.path)
        else {
            continue;
        };
        let Some(bytes) = fv.bytes else {
            continue;
        };
        let cand = OriginCloseCandidate {
            ordinal,
            cid: &cid,
            ask_id: &ask_id,
            ans_path: fv.path.as_str(),
            bytes,
            origin_only,
        };
        if !append_one_origin_close_relay(office, ctx, &cand, out)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn append_handoff_accept_relay(
    office: &Office,
    ctx: &PlacementContext,
    cand: &OriginCloseCandidate<'_>,
    conv: &crate::messaging::records::ConversationRecord,
    ask_msg: &crate::messaging::records::MessageRecord,
    ans: &crate::messaging::records::ParticipantAnswerFile,
    out: &mut Vec<PendingMessageJournalEntry>,
) -> Result<bool, OfficeError> {
    if cand.origin_only || ask_msg.kind != "handoff" {
        return Ok(true);
    }
    let dests = route_origin_close_destinations(office, ctx, conv, office.info.office_id.as_str())?;
    if !dests.is_empty() {
        out.push(PendingMessageJournalEntry {
            ordinal: cand.ordinal,
            conversation_id: cand.cid.as_str().to_owned(),
            message_id: format!("accept-{}-{}", cand.ask_id, ans.employee_id),
            files: vec![(cand.ans_path.to_owned(), Some(STANDARD.encode(cand.bytes)))],
            destination_offices: dests,
            expires_at_ms: None,
        });
    }
    Ok(true)
}

fn push_origin_close_entries(
    office: &Office,
    ctx: &PlacementContext,
    cand: &OriginCloseCandidate<'_>,
    conv: &crate::messaging::records::ConversationRecord,
    ask_msg: &crate::messaging::records::MessageRecord,
    out: &mut Vec<PendingMessageJournalEntry>,
) -> Result<(), OfficeError> {
    let dests = route_origin_close_destinations(office, ctx, conv, office.info.office_id.as_str())?;
    if dests.is_empty() {
        return Ok(());
    }
    for (message_id, files) in build_origin_close_entries(
        cand.cid,
        conv,
        ask_msg,
        cand.ans_path,
        cand.bytes,
        cand.ask_id,
    ) {
        out.push(PendingMessageJournalEntry {
            ordinal: cand.ordinal,
            conversation_id: cand.cid.as_str().to_owned(),
            message_id,
            files,
            destination_offices: dests.clone(),
            expires_at_ms: None,
        });
    }
    Ok(())
}

fn append_one_origin_close_relay(
    office: &Office,
    ctx: &PlacementContext,
    cand: &OriginCloseCandidate<'_>,
    out: &mut Vec<PendingMessageJournalEntry>,
) -> Result<bool, OfficeError> {
    let Ok(ans) =
        serde_json::from_slice::<crate::messaging::records::ParticipantAnswerFile>(cand.bytes)
    else {
        return Ok(true);
    };
    let is_committed_authoritative = ans.reason.as_deref() == Some("origin-close");
    let is_withdrawal = crate::messaging::asks::decode_ask_answer_file(&ans)
        .1
        .is_some();
    let is_handoff_accept = ans.answer.is_none() && ans.accepted_by.is_some();
    if !is_committed_authoritative && !is_withdrawal && !is_handoff_accept {
        return Ok(true);
    }
    let Some(conv) = office
        .store
        .run(ReadConversationOp {
            conversation_id: cand.cid.clone(),
        })
        .map_err(storage)?
    else {
        return Ok(false);
    };
    let Some(ask_msg) = office
        .store
        .run(ReadMessageOp {
            ask_id: cand.ask_id.to_owned(),
        })
        .map_err(storage)?
    else {
        return Ok(false);
    };
    if is_handoff_accept {
        return append_handoff_accept_relay(office, ctx, cand, &conv, &ask_msg, &ans, out);
    }
    let is_ask_origin = ask_msg.origin_office_id.as_str() == office.info.office_id.as_str();
    let hq_finalizes_removed = office.info.role == crate::model::OfficeRole::Hq
        && is_committed_authoritative
        && ctx
            .removed_offices
            .contains(ask_msg.origin_office_id.as_str());
    if cand.origin_only && !is_ask_origin {
        return Ok(true);
    }
    if is_ask_origin || hq_finalizes_removed {
        push_origin_close_entries(office, ctx, cand, &conv, &ask_msg, out)?;
        return Ok(true);
    }
    let origin = ask_msg.origin_office_id.as_str();
    if ctx.enrolled.contains(origin) || origin == ctx.hq_office_id {
        out.push(PendingMessageJournalEntry {
            ordinal: cand.ordinal,
            conversation_id: cand.cid.as_str().to_owned(),
            message_id: format!("close-{}", cand.ask_id),
            files: vec![(cand.ans_path.to_owned(), Some(STANDARD.encode(cand.bytes)))],
            destination_offices: vec![origin.to_owned()],
            expires_at_ms: None,
        });
    }
    Ok(true)
}

fn route_origin_close_destinations(
    office: &Office,
    ctx: &PlacementContext,
    conv: &crate::messaging::records::ConversationRecord,
    local_id: &str,
) -> Result<Vec<String>, OfficeError> {
    let members = office
        .store
        .run(ReadConversationMembersOp {
            conv: conv.clone(),
            owner: office.effective_owner_for_ops(),
        })
        .map_err(storage)?;
    let dests: std::collections::BTreeSet<String> = members
        .into_iter()
        .map(|emp| {
            ctx.employee_offices
                .get(emp.as_str())
                .cloned()
                .unwrap_or_else(|| ctx.hq_office_id.clone())
        })
        .filter(|oid| oid != local_id && (ctx.enrolled.contains(oid) || oid == &ctx.hq_office_id))
        .collect();
    Ok(dests.into_iter().collect())
}

pub(super) fn append_hq_channel_only_relays(
    office: &Office,
    ctx: &PlacementContext,
    ordinal: u64,
    entry_convs: &std::collections::BTreeMap<Id, Vec<u8>>,
    relayed_conv_ids: &std::collections::BTreeSet<Id>,
    out: &mut Vec<PendingMessageJournalEntry>,
) -> Result<(), OfficeError> {
    let local_id = office.info.office_id.as_str();
    let remote_offices: Vec<String> = ctx
        .enrolled
        .iter()
        .filter(|oid| oid.as_str() != local_id)
        .cloned()
        .collect();
    if remote_offices.is_empty() {
        return Ok(());
    }
    for (cid, conv_bytes) in entry_convs {
        if relayed_conv_ids.contains(cid) {
            continue;
        }
        let Ok(conv) =
            serde_json::from_slice::<crate::messaging::records::ConversationRecord>(conv_bytes)
        else {
            continue;
        };
        if conv.kind != org_proto::chat::ConversationKind::Channel {
            continue;
        }
        let conv_path = format!("messaging/conversations/{cid}/conversation.json");
        out.push(PendingMessageJournalEntry {
            ordinal,
            conversation_id: cid.as_str().to_owned(),
            message_id: format!("conv-{cid}-r{}", conv.revision),
            files: vec![(conv_path, Some(STANDARD.encode(conv_bytes)))],
            destination_offices: remote_offices.clone(),
            expires_at_ms: None,
        });
    }
    Ok(())
}
