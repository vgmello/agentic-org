//! Applying an inbound relayed origin ask close, with staged recovery when the ask has not
//! arrived yet.

use std::path::PathBuf;

use org_store::{Actor, Id, Outcome};
use serde::{Deserialize, Serialize};

use crate::office::satellite::placement::PlacementContext;
use crate::office::{storage, Office};
use crate::{
    error::OfficeError,
    private_fs::{private_dir_synced, write_private_atomic},
};

use super::inbound_message::canonicalize_deterministic_auto_channel;
use super::types::{ParsedOriginAskClose, RelayedAskWithdrawal, RelayedMessageOutcome};

#[derive(Serialize, Deserialize)]
pub(super) struct StagedAskRecovery {
    pub(super) conv: Option<crate::messaging::records::ConversationRecord>,
    pub(super) ask_msg: crate::messaging::records::MessageRecord,
}

fn close_recovery_stage_path(office: &Office, origin_office: &str, ask_id: &str) -> PathBuf {
    let key =
        org_store::Sha256::of_parts(&[origin_office.as_bytes(), b"\0", ask_id.as_bytes()]).to_hex();
    // Store staged split-recovery asks in durable office-local storage (same durability scope as
    // `replication/state.v1.json`) so rebuilding `runtime/` never strands an acknowledged
    // `close-ask-*` delivery (`TASK-52` P16-#1).
    office
        .durable
        .join("local")
        .join("offices")
        .join(&office.info.office_id)
        .join("close-recovery")
        .join(format!("{key}.json"))
}

pub(super) fn stage_close_recovery(
    office: &Office,
    origin_office: &str,
    ask_id: &str,
    staged: &StagedAskRecovery,
) -> Result<(), OfficeError> {
    let _guard = office
        .federation_io_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let path = close_recovery_stage_path(office, origin_office, ask_id);
    if let Some(parent) = path.parent() {
        private_dir_synced(&office.durable, parent).map_err(OfficeError::storage)?;
    }
    let bytes = serde_json::to_vec(staged).map_err(OfficeError::storage)?;
    write_private_atomic(&path, &bytes).map_err(OfficeError::storage)
}

fn load_staged_close_recovery(
    office: &Office,
    origin_office: &str,
    ask_id: &str,
) -> Option<StagedAskRecovery> {
    let _guard = office
        .federation_io_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let path = close_recovery_stage_path(office, origin_office, ask_id);
    let raw = std::fs::read(&path).ok()?;
    serde_json::from_slice(&raw).ok()
}

fn remove_staged_close_recovery(office: &Office, origin_office: &str, ask_id: &str) {
    let _guard = office
        .federation_io_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let path = close_recovery_stage_path(office, origin_office, ask_id);
    let _ = std::fs::remove_file(path);
}

/// Read-only entitlement check for a split `close-ask-*` recovery delivery: returns `Ok(true)` if
/// the ask is already committed locally with matching digest, `Ok(false)` if the local office is
/// entitled and should stage the ask until `close-*` arrives, or `Err` if unentitled/invalid
/// (`TASK-52` P15-#1).
pub(super) struct CheckAskRecoveryEntitlementOp {
    pub(super) here: org_store::OfficeIdentity,
    pub(super) owner: Id,
    pub(super) ctx: PlacementContext,
    pub(super) origin_office: String,
    pub(super) incoming_conv: Option<crate::messaging::records::ConversationRecord>,
    pub(super) cid: Id,
    pub(super) ask_msg: crate::messaging::records::MessageRecord,
}

impl crate::store::open::StoreOp for CheckAskRecoveryEntitlementOp {
    type Out = Result<Result<bool, org_store::Refusal>, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        let (res, _) = store.read(|snap| Ok(check_ask_recovery_entitlement(snap, &self)))?;
        Ok(res)
    }
}

fn check_ask_recovery_entitlement(
    view: &impl crate::messaging::ports::MessagingRead,
    req: &CheckAskRecoveryEntitlementOp,
) -> Result<bool, org_store::Refusal> {
    use org_store::Refusal;
    if let Some(existing) = view
        .message(&req.ask_msg.id)
        .map_err(|_| Refusal::invalid("read ask message"))?
    {
        if existing.content_sha256 != req.ask_msg.content_sha256 {
            return Err(Refusal::invalid("recovered ask digest conflict"));
        }
        return Ok(true);
    }
    let valid_origin = req.ask_msg.origin_office_id.as_str() == req.origin_office.as_str()
        || req.origin_office == req.ctx.hq_office_id;
    if req.ask_msg.conversation_id != req.cid || req.ask_msg.kind != "request" || !valid_origin {
        return Err(Refusal::invalid("invalid ask recovery payload"));
    }
    let (conv, _) = resolve_conv_for_close_recovery(
        view,
        &req.cid,
        &req.origin_office,
        &req.ctx.hq_office_id,
        req.incoming_conv.as_ref(),
    )?;
    ensure_local_office_entitled(view, &conv, &req.owner, &req.ctx, &req.here.office_id)?;
    Ok(false)
}

fn ensure_local_office_entitled(
    view: &impl crate::messaging::ports::MessagingRead,
    conv: &crate::messaging::records::ConversationRecord,
    owner: &Id,
    ctx: &PlacementContext,
    local_office_id: &Id,
) -> Result<(), org_store::Refusal> {
    let members = crate::messaging::features::effective_auto_members(view, conv, owner)
        .map_err(|_| org_store::Refusal::invalid("resolve conversation members"))?;
    let local_entitled = members.iter().any(|emp| {
        ctx.employee_offices
            .get(emp.as_str())
            .map(String::as_str)
            .unwrap_or(ctx.hq_office_id.as_str())
            == local_office_id.as_str()
    });
    if local_entitled {
        Ok(())
    } else {
        Err(org_store::Refusal::invalid(
            "office is not entitled to this conversation",
        ))
    }
}

struct ApplyRelayedOriginCloseOp {
    operation_id: Id,
    here: org_store::OfficeIdentity,
    owner: Id,
    ctx: PlacementContext,
    origin_office: String,
    incoming_conv: Option<crate::messaging::records::ConversationRecord>,
    incoming_ask: Option<crate::messaging::records::MessageRecord>,
    cid: Id,
    emp_id: Id,
    ask_id: String,
    answer: crate::messaging::records::ParticipantAnswerFile,
}

impl crate::store::open::StoreOp for ApplyRelayedOriginCloseOp {
    type Out = Result<org_store::Reply, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        let op = org_store::Operation {
            id: self.operation_id.clone(),
            kind: "replication.apply_origin_close",
            actor: Actor::System,
            fingerprint: org_store::Sha256::of_parts(&[
                self.cid.as_str().as_bytes(),
                b"\0",
                self.ask_id.as_bytes(),
                b"\0",
                self.emp_id.as_str().as_bytes(),
            ]),
        };
        store.submit(Some(op), false, move |uow| {
            decide_apply_relayed_origin_close(uow.view(), &self)
        })
    }
}

fn resolve_origin_close_ask(
    view: &impl crate::messaging::ports::MessagingRead,
    req: &ApplyRelayedOriginCloseOp,
) -> Result<
    (
        crate::messaging::records::MessageRecord,
        Option<crate::messaging::records::ConversationRecord>,
        bool,
    ),
    org_store::Refusal,
> {
    use org_store::Refusal;
    if let Some(local_ask) = view
        .message(&req.ask_id)
        .map_err(|_| Refusal::invalid("read ask message"))?
    {
        if local_ask.conversation_id != req.cid || local_ask.kind != "request" {
            return Err(Refusal::invalid("origin-close ask record mismatch"));
        }
        if req
            .incoming_ask
            .as_ref()
            .is_some_and(|inc| inc.content_sha256 != local_ask.content_sha256)
        {
            return Err(Refusal::invalid("origin-close ask digest conflict"));
        }
        return Ok((local_ask, None, false));
    }
    let Some(inc_ask) = req.incoming_ask.as_ref() else {
        return Err(Refusal::invalid("ask not present on this office"));
    };
    let valid_origin = inc_ask.origin_office_id.as_str() == req.origin_office.as_str()
        || req.origin_office == req.ctx.hq_office_id;
    if inc_ask.id != req.ask_id
        || inc_ask.conversation_id != req.cid
        || inc_ask.kind != "request"
        || !valid_origin
    {
        return Err(Refusal::invalid("invalid bundled origin-close ask"));
    }
    let (conv, conv_to_create) = resolve_conv_for_close_recovery(
        view,
        &req.cid,
        &req.origin_office,
        &req.ctx.hq_office_id,
        req.incoming_conv.as_ref(),
    )?;
    ensure_local_office_entitled(view, &conv, &req.owner, &req.ctx, &req.here.office_id)?;
    Ok((inc_ask.clone(), conv_to_create, true))
}

fn resolve_conv_for_close_recovery(
    view: &impl crate::messaging::ports::MessagingRead,
    cid: &Id,
    origin_office: &str,
    hq_office_id: &str,
    incoming_conv: Option<&crate::messaging::records::ConversationRecord>,
) -> Result<
    (
        crate::messaging::records::ConversationRecord,
        Option<crate::messaging::records::ConversationRecord>,
    ),
    org_store::Refusal,
> {
    use org_store::Refusal;
    if let Some(local_conv) = view
        .conversation(cid)
        .map_err(|_| Refusal::invalid("read conversation"))?
    {
        return Ok((local_conv, None));
    }
    let Some(remote) = incoming_conv else {
        return Err(Refusal::invalid(
            "conversation missing for relayed ask close",
        ));
    };
    let is_channel = remote.kind == org_proto::chat::ConversationKind::Channel;
    if !is_channel || origin_office == hq_office_id {
        return Ok((remote.clone(), Some(remote.clone())));
    }
    let Some(canonical) = canonicalize_deterministic_auto_channel(view, remote) else {
        return Err(Refusal::invalid(
            "only HQ may create relayed custom channels",
        ));
    };
    Ok((canonical.clone(), Some(canonical)))
}

fn build_local_ask_withdrawal_reply(
    view: &impl crate::messaging::ports::MessagingRead,
    req: &ApplyRelayedOriginCloseOp,
    ask_msg: &crate::messaging::records::MessageRecord,
    is_withdrawal: bool,
) -> Vec<u8> {
    let withdrawn_ask = if is_withdrawal {
        ask_msg.sender_employee_id.as_ref().and_then(|asker_id| {
            let owner_office = req
                .ctx
                .employee_offices
                .get(asker_id.as_str())
                .map(String::as_str)
                .unwrap_or(req.ctx.hq_office_id.as_str());
            if owner_office != req.here.office_id.as_str() {
                return None;
            }
            let is_agent = view.employees().ok().is_some_and(|emps| {
                emps.iter().any(|e| {
                    e.id == *asker_id && e.kind == crate::organization::records::EmployeeKind::Agent
                })
            });
            is_agent.then(|| RelayedAskWithdrawal {
                ask_id: req.ask_id.clone(),
                asker_id: asker_id.as_str().to_owned(),
            })
        })
    } else {
        None
    };
    let outcome = RelayedMessageOutcome {
        conversation_id: req.cid.as_str().to_owned(),
        sender_id: req.emp_id.as_str().to_owned(),
        withdrawn_ask,
        closed_ask_id: Some(req.ask_id.clone()),
        closed_ask_reason: Some(
            if is_withdrawal {
                "withdrawn"
            } else {
                "answered"
            }
            .to_owned(),
        ),
        ..Default::default()
    };
    serde_json::to_vec(&outcome).unwrap_or_default()
}

fn decide_apply_relayed_origin_close(
    view: &impl crate::messaging::ports::MessagingRead,
    req: &ApplyRelayedOriginCloseOp,
) -> Result<org_store::Change, org_store::Refusal> {
    use crate::messaging::records::{
        conversation_path, encode_json, message_path, participant_answer_path,
    };
    use org_store::{Ack, Change, Refusal};
    let (ask_msg, conv_to_create, create_ask) = resolve_origin_close_ask(view, req)?;
    let is_withdrawal = crate::messaging::asks::decode_ask_answer_file(&req.answer)
        .1
        .is_some();
    let from_origin = ask_msg.origin_office_id.as_str() == req.origin_office.as_str();
    let from_hq_owner_withdrawal =
        is_withdrawal && req.origin_office == req.ctx.hq_office_id && req.emp_id == req.owner;
    if !from_origin && !from_hq_owner_withdrawal {
        return Err(Refusal::invalid(
            "origin-close answer must originate from the ask's origin office or HQ owner withdrawal",
        ));
    }
    // At the ask's origin office, preserve an already-committed local close if a provisional HQ
    // withdrawal races behind it; on non-origin participants, HQ owner withdrawals are only ever
    // fanned out when the origin office was removed and therefore override provisional answers
    // (`TASK-52` P13-#3 / P14-#5).
    if !from_origin && ask_msg.origin_office_id == req.here.office_id {
        if let Some(existing) = view
            .answer_for_message(&req.ask_id)
            .map_err(|_| Refusal::invalid("read existing ask answer"))?
        {
            let (a, w) = crate::messaging::asks::decode_ask_answer_file(&existing);
            if a.is_some() || w.is_some() {
                let same_withdrawal = w.is_some() && existing.employee_id == req.emp_id;
                let reply = build_local_ask_withdrawal_reply(view, req, &ask_msg, same_withdrawal);
                return Ok(Change::new(Ack::Durable).reply(reply));
            }
        }
    }
    let mut change = Change::new(Ack::Durable);
    if let Some(conv_rec) = conv_to_create {
        change = change.create(conversation_path(&conv_rec.id), encode_json(&conv_rec));
    }
    if create_ask {
        change = change.create(message_path(&req.cid, &ask_msg.id), encode_json(&ask_msg));
    }
    let path = participant_answer_path(&req.cid, &req.emp_id, &req.ask_id);
    change = change.put(path, encode_json(&req.answer));
    if !create_ask {
        let (next_change, _) = crate::messaging::asks::mark_ask_recipients_done(
            view,
            &req.here,
            &ask_msg,
            ask_msg.org_sequence,
            &ask_msg.created_at,
            change,
        )?;
        change = next_change;
    }
    let reply = build_local_ask_withdrawal_reply(view, req, &ask_msg, is_withdrawal);
    Ok(change.reply(reply))
}

pub(super) fn apply_relayed_origin_close(
    office: &Office,
    origin_office: &str,
    op_id: Id,
    ctx: PlacementContext,
    parsed: ParsedOriginAskClose,
) -> Result<RelayedMessageOutcome, OfficeError> {
    let ParsedOriginAskClose {
        mut conv,
        mut ask_msg,
        cid,
        emp_id,
        ask_id,
        answer,
    } = parsed;
    let mut used_staged = false;
    if ask_msg.is_none() {
        if let Some(staged) = load_staged_close_recovery(office, origin_office, &ask_id) {
            if conv.is_none() {
                conv = staged.conv;
            }
            ask_msg = Some(staged.ask_msg);
            used_staged = true;
        }
    }
    let reply = office
        .store
        .run(ApplyRelayedOriginCloseOp {
            operation_id: op_id,
            here: office.identity.clone(),
            owner: office.owner.clone(),
            ctx,
            origin_office: origin_office.to_owned(),
            incoming_conv: conv,
            incoming_ask: ask_msg,
            cid,
            emp_id,
            ask_id: ask_id.clone(),
            answer,
        })
        .map_err(storage)?;
    match reply.outcome {
        Outcome::Completed => {
            if used_staged {
                remove_staged_close_recovery(office, origin_office, &ask_id);
            }
            if reply.bytes.is_empty() {
                Ok(RelayedMessageOutcome::default())
            } else {
                serde_json::from_slice(&reply.bytes)
                    .map_err(|_| OfficeError::Invalid("corrupt relayed origin ask close reply"))
            }
        }
        Outcome::Rejected => Err(OfficeError::Invalid(
            "relayed origin ask close rejected by validation",
        )),
    }
}
