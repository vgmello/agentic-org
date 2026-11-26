//! Withdrawing an ask.

use std::collections::BTreeSet;

use org_proto::ask::{AskWithdrawalRecord, MsgWithdrawParams, MsgWithdrawResult};
use org_proto::msg::{json_string_len, KIND_REQUEST, MAX_NOTE_BYTES};
use org_proto::ErrorCode;
use org_store::{Ack, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};

use crate::messaging::features::{is_active_employee, put_or_create_file, refusal};
use crate::messaging::ports::{MessagingRead, MessagingWrite};
use crate::messaging::records::{self, MessageRecord, ParticipantAnswerFile, SCHEMA_VERSION};
use crate::organization::ports::OrganizationRead;

use super::answer::mark_ask_recipients_done;
use super::{decode_ask_answer_file, parse_ask_card, AskOutcomeEnvelope, WithdrawCommitted};

fn authorize_withdraw_office(
    view: &impl MessagingRead,
    here: &OfficeIdentity,
    owner: &Id,
    caller: &Id,
    ask: &MessageRecord,
    removed_offices: &BTreeSet<String>,
) -> Result<bool, Refusal> {
    let is_origin = ask.origin_office_id == here.office_id;
    let is_hq = view
        .office(&here.office_id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read office"))?
        .is_some_and(|o| o.role == crate::organization::records::OfficeRole::Hq);
    if !is_origin && !(caller == owner && is_hq) {
        return Err(refusal(
            ErrorCode::NotAuthorized,
            "only the ask's origin office or the organization owner at HQ may withdraw this ask",
        ));
    }
    let hq_finalizes_removed =
        caller == owner && is_hq && removed_offices.contains(ask.origin_office_id.as_str());
    Ok(is_origin || hq_finalizes_removed)
}

#[expect(
    clippy::too_many_arguments,
    reason = "withdraw decision takes explicit transaction, identity, owner, caller, params, and clock inputs"
)]
pub fn decide_msg_withdraw<E>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    caller: &Id,
    params: &MsgWithdrawParams,
    org_seq: u64,
    now_utc: &str,
    removed_offices: &BTreeSet<String>,
) -> Result<(Change, WithdrawCommitted), Refusal>
where
    E: Engine,
    E::Tx: MessagingWrite,
{
    if params
        .reason
        .as_deref()
        .is_some_and(|r| json_string_len(r) > MAX_NOTE_BYTES)
    {
        return Err(refusal(ErrorCode::TooLarge, "withdrawal reason too long"));
    }
    let employees = uow
        .view()
        .employees()
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read employees"))?;
    if !employees
        .iter()
        .any(|e| e.id == *caller && is_active_employee(e))
    {
        return Err(refusal(
            ErrorCode::NotBound,
            "caller is not an active employee",
        ));
    }
    let ask = uow
        .view()
        .message(&params.ask_id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read ask"))?
        .filter(|m| m.kind == KIND_REQUEST)
        .ok_or_else(|| refusal(ErrorCode::NotFound, "ask not found"))?;
    if parse_ask_card(&ask).is_none() {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "message is not a structured ask",
        ));
    }
    let is_asker = ask.sender_employee_id.as_ref() == Some(caller);
    if !is_asker && caller != owner {
        return Err(refusal(
            ErrorCode::NotAuthorized,
            "only the asker or organization owner may withdraw this ask",
        ));
    }
    let authoritative_close =
        authorize_withdraw_office(uow.view(), here, owner, caller, &ask, removed_offices)?;
    let mut existing_revision: u64 = 0;
    if let Some(existing) = uow
        .view()
        .answer_for_message(&ask.id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read existing answer"))?
    {
        let (a, w) = decode_ask_answer_file(&existing);
        let promotable_provisional = authoritative_close
            && w.is_some()
            && existing.reason.as_deref() != Some("origin-close");
        if (a.is_some() || w.is_some()) && !promotable_provisional {
            return Err(refusal(ErrorCode::Conflict, "ask is already closed"));
        }
        existing_revision = existing.revision;
    }
    let withdrawal = AskWithdrawalRecord {
        by: caller.as_str().to_owned(),
        at: now_utc.to_owned(),
        reason: params
            .reason
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
    };
    let envelope = AskOutcomeEnvelope::Withdrawal(withdrawal.clone());
    let file = ParticipantAnswerFile {
        schema_version: SCHEMA_VERSION,
        revision: existing_revision.saturating_add(1),
        organization_id: here.organization_id.clone(),
        conversation_id: ask.conversation_id.clone(),
        employee_id: caller.clone(),
        message_id: ask.id.clone(),
        accepted_by: None,
        accepted_at: None,
        reason: authoritative_close.then(|| "origin-close".to_owned()),
        answer: Some(serde_json::to_value(&envelope).expect("envelope serializes")),
    };
    let path = records::participant_answer_path(&ask.conversation_id, caller, &ask.id);
    let change = put_or_create_file(
        uow,
        Change::new(Ack::Committed),
        path,
        records::encode_json(&file),
    )?;
    let (change, affected) =
        mark_ask_recipients_done(uow.view(), here, &ask, org_seq, now_utc, change)?;
    Ok((
        change,
        WithdrawCommitted {
            result: MsgWithdrawResult {
                ask_id: ask.id,
                conversation_id: ask.conversation_id.as_str().to_owned(),
                withdrawal,
            },
            asker_id: ask
                .sender_employee_id
                .as_ref()
                .map(|id| id.as_str().to_owned()),
            affected_members: affected,
        },
    ))
}
