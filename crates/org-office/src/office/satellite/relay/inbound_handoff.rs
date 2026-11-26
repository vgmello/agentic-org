//! Applying an inbound relayed handoff accept.

use org_store::{Actor, Id, Outcome};

use crate::error::OfficeError;
use crate::office::satellite::placement::PlacementContext;
use crate::office::{storage, Office};

use super::inbound::{ParsedHandoffAccept, RelayedMessageOutcome};
use super::inbound_ask::is_canonical_relayed_ask_winner;

struct ApplyRelayedHandoffAcceptOp {
    operation_id: Id,
    owner: Id,
    ctx: PlacementContext,
    origin_office: String,
    cid: Id,
    emp_id: Id,
    message_id: String,
    answer: crate::messaging::records::ParticipantAnswerFile,
}

impl crate::store::open::StoreOp for ApplyRelayedHandoffAcceptOp {
    type Out = Result<org_store::Reply, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        let op = org_store::Operation {
            id: self.operation_id.clone(),
            kind: "replication.apply_handoff_accept",
            actor: Actor::System,
            fingerprint: org_store::Sha256::of_parts(&[
                self.cid.as_str().as_bytes(),
                b"\0",
                self.message_id.as_bytes(),
                b"\0",
                self.emp_id.as_str().as_bytes(),
            ]),
        };
        store.submit(Some(op), false, move |uow| {
            decide_apply_relayed_handoff_accept(uow.view(), &self)
        })
    }
}

fn validate_relayed_handoff_accept_target(
    view: &impl crate::messaging::ports::MessagingRead,
    req: &ApplyRelayedHandoffAcceptOp,
    employees: &[crate::organization::records::Employee],
    conv: &crate::messaging::records::ConversationRecord,
    active_members: &[Id],
) -> Result<(), org_store::Refusal> {
    use crate::messaging::features::{build_mention_candidates, collect_recipient_reasons};
    use crate::messaging::mentions::extract_mentions;
    use org_store::Refusal;

    if !req
        .answer
        .accepted_at
        .as_deref()
        .is_some_and(|ts| !ts.trim().is_empty())
    {
        return Err(Refusal::invalid(
            "relayed handoff accept missing accepted_at",
        ));
    }
    if let Some(reason) = req.answer.reason.as_deref() {
        if org_proto::msg::json_string_len(reason) > org_proto::msg::MAX_NOTE_BYTES {
            return Err(Refusal::invalid(
                "relayed handoff accept reason exceeds size limit",
            ));
        }
    }
    let Some(msg) = view
        .message(&req.message_id)
        .map_err(|_| Refusal::invalid("read handoff message"))?
    else {
        return Err(Refusal::invalid(
            "handoff message not present on this office",
        ));
    };
    if msg.conversation_id != req.cid || msg.kind != "handoff" {
        return Err(Refusal::invalid("relayed handoff accept target mismatch"));
    }
    if msg.sender_employee_id.as_ref() == Some(&req.emp_id) {
        return Err(Refusal::invalid("handoff sender cannot accept own handoff"));
    }
    let sender_id = msg.sender_employee_id.as_ref().unwrap_or(&req.owner);
    let candidates = build_mention_candidates(employees, active_members, &req.owner);
    let scan = extract_mentions(&msg.body_markdown, &candidates);
    let reasons = collect_recipient_reasons(
        view,
        conv,
        sender_id,
        active_members,
        &scan.resolved,
        msg.thread_root_id.as_deref(),
        &msg.kind,
        &msg.metadata,
    )?;
    if !reasons.contains_key(&req.emp_id) {
        return Err(Refusal::invalid(
            "relayed handoff accepter is not an addressed recipient",
        ));
    }
    Ok(())
}

fn decide_apply_relayed_handoff_accept(
    view: &impl crate::messaging::ports::MessagingRead,
    req: &ApplyRelayedHandoffAcceptOp,
) -> Result<org_store::Change, org_store::Refusal> {
    use crate::messaging::records::{encode_json, participant_answer_path};
    use crate::organization::records::EmployeeStatus;
    use org_store::{Ack, Change, Refusal};

    let employees = view
        .employees()
        .map_err(|_| Refusal::invalid("read employees"))?;
    let Some(emp) = employees
        .iter()
        .find(|e| e.id == req.emp_id && e.status == EmployeeStatus::Active)
    else {
        return Err(Refusal::invalid("relayed handoff accepter is not active"));
    };
    // Enforce origin-office placement for ALL employees (Agents at their placed office; Persons
    // strictly at HQ per `doc-90`) so a satellite can never forge a Person/owner acceptance.
    let placed = req
        .ctx
        .employee_offices
        .get(emp.id.as_str())
        .map(String::as_str)
        .unwrap_or(req.ctx.hq_office_id.as_str());
    if placed != req.origin_office.as_str() {
        return Err(Refusal::invalid(
            "relayed handoff accepter is not placed at authenticated origin office",
        ));
    }
    let Some(conv) = view
        .conversation(&req.cid)
        .map_err(|_| Refusal::invalid("read conversation"))?
    else {
        return Err(Refusal::invalid("conversation missing for handoff accept"));
    };
    let active_members =
        crate::messaging::features::effective_auto_members(view, &conv, &req.owner)
            .map_err(|_| Refusal::invalid("resolve conversation members"))?;
    if !active_members.contains(&req.emp_id) && req.emp_id != req.owner {
        return Err(Refusal::invalid(
            "relayed handoff accepter is not an active member",
        ));
    }
    validate_relayed_handoff_accept_target(view, req, &employees, &conv, &active_members)?;
    let outcome = RelayedMessageOutcome {
        conversation_id: req.cid.as_str().to_owned(),
        sender_id: req.emp_id.as_str().to_owned(),
        handoff_accepted: true,
        ..Default::default()
    };
    let reply = serde_json::to_vec(&outcome).unwrap_or_default();
    if let Some(existing) = view
        .answer_for_message(&req.message_id)
        .map_err(|_| Refusal::invalid("read existing handoff accept"))?
    {
        if existing.accepted_by.as_ref() == Some(&req.emp_id)
            || !is_canonical_relayed_ask_winner(Some(&existing), &req.emp_id, &req.answer)
        {
            return Ok(Change::new(Ack::Durable).reply(reply));
        }
    }
    let path = participant_answer_path(&req.cid, &req.emp_id, &req.message_id);
    Ok(Change::new(Ack::Durable)
        .put(path, encode_json(&req.answer))
        .reply(reply))
}

pub(super) fn apply_relayed_handoff_accept(
    office: &Office,
    origin_office: &str,
    op_id: Id,
    ctx: PlacementContext,
    parsed: ParsedHandoffAccept,
) -> Result<RelayedMessageOutcome, OfficeError> {
    let reply = office
        .store
        .run(ApplyRelayedHandoffAcceptOp {
            operation_id: op_id,
            owner: office.owner.clone(),
            ctx,
            origin_office: origin_office.to_owned(),
            cid: parsed.cid,
            emp_id: parsed.emp_id,
            message_id: parsed.message_id,
            answer: parsed.answer,
        })
        .map_err(storage)?;
    match reply.outcome {
        Outcome::Completed => {
            if reply.bytes.is_empty() {
                Ok(RelayedMessageOutcome::default())
            } else {
                serde_json::from_slice(&reply.bytes)
                    .map_err(|_| OfficeError::Invalid("corrupt relayed handoff accept reply"))
            }
        }
        Outcome::Rejected => Err(OfficeError::Invalid(
            "relayed handoff accept rejected by validation",
        )),
    }
}
