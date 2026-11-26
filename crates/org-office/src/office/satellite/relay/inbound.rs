//! Inbound relay entry point: the parse, validation and assembly of a relayed delta before it is
//! applied.

use org_store::{DurablePath, Id, Outcome};

use crate::error::OfficeError;
use crate::office::satellite::placement::load_placement_context;
use crate::office::{storage, Office};

use super::inbound_close::{
    apply_relayed_origin_close, stage_close_recovery, CheckAskRecoveryEntitlementOp,
    StagedAskRecovery,
};
use super::inbound_handoff::apply_relayed_handoff_accept;
use super::inbound_message::{ApplyRelayedConversationOnlyOp, ApplyRelayedMessageOp};
use super::types::{
    message_expires_at_ms, ParsedHandoffAccept, ParsedOriginAskClose, RelayedMessageOutcome,
};

impl Office {
    /// Validates and atomically applies an inbound relayed native message (with optional new
    /// conversation and structured ask answer) to this office's messaging store, synthesizing
    /// recipient-state rows ONLY for recipients owned by this office (`TASK-52` P3-#2/#3, P4-#1..#4/#8/#9).
    pub fn apply_relayed_messaging_files(
        &self,
        origin_office: &str,
        message_id: &str,
        operation_id: &str,
        files: Vec<(String, Option<Vec<u8>>)>,
    ) -> Result<RelayedMessageOutcome, OfficeError> {
        apply_validated_relayed_message(self, origin_office, message_id, operation_id, files)
    }
}

struct ParsedRelayedMessage {
    conv: Option<crate::messaging::records::ConversationRecord>,
    answer: Option<(Id, String, crate::messaging::records::ParticipantAnswerFile)>,
    msg: crate::messaging::records::MessageRecord,
}

struct ParsedOriginAskRecovery {
    conv: Option<crate::messaging::records::ConversationRecord>,
    cid: Id,
    ask_msg: crate::messaging::records::MessageRecord,
}

enum ParsedRelayedDelta {
    ConversationOnly(Box<crate::messaging::records::ConversationRecord>),
    OriginAskRecovery(Box<ParsedOriginAskRecovery>),
    OriginAskClose(Box<ParsedOriginAskClose>),
    HandoffAccept(Box<ParsedHandoffAccept>),
    Message(Box<ParsedRelayedMessage>),
}

fn validate_relayed_answer_binding(
    msg_cid: &Id,
    msg: &crate::messaging::records::MessageRecord,
    parsed_answer: Option<(
        Id,
        Id,
        String,
        crate::messaging::records::ParticipantAnswerFile,
    )>,
) -> Result<Option<(Id, String, crate::messaging::records::ParticipantAnswerFile)>, OfficeError> {
    let Some((ans_cid, emp, ask_id, ans)) = parsed_answer else {
        return Ok(None);
    };
    let (decoded_ans, _) = crate::messaging::asks::decode_ask_answer_file(&ans);
    let responder_matches = msg.sender_employee_id.as_ref() == Some(&emp)
        && decoded_ans.as_ref().is_some_and(|a| a.by == emp.as_str());
    if ans_cid != *msg_cid
        || msg.reply_to_message_id.as_deref() != Some(ask_id.as_str())
        || !responder_matches
    {
        return Err(OfficeError::Invalid(
            "relayed answer does not match authenticated reply message",
        ));
    }
    Ok(Some((emp, ask_id, ans)))
}

fn parse_validated_relayed_delta(
    org_id: &Id,
    origin_office: &str,
    hq_office_id: &str,
    is_ask_recovery: bool,
    files: Vec<(String, Option<Vec<u8>>)>,
) -> Result<ParsedRelayedDelta, OfficeError> {
    use crate::messaging::records::{
        kind_of, ConversationRecord, Kind, MessageRecord, ParticipantAnswerFile,
    };

    let mut parsed_conv: Option<(Id, ConversationRecord)> = None;
    let mut parsed_msg: Option<(Id, String, MessageRecord)> = None;
    let mut parsed_answer: Option<(Id, Id, String, ParticipantAnswerFile)> = None;
    for (path, opt_bytes) in files {
        let Some(bytes) = opt_bytes else {
            return Err(OfficeError::Invalid(
                "relayed message cannot delete messaging files",
            ));
        };
        let dp = DurablePath::parse(&path)
            .map_err(|_| OfficeError::Invalid("invalid relayed messaging path"))?;
        match kind_of(&dp) {
            Some(Kind::Conversation(cid)) => {
                let conv: ConversationRecord = serde_json::from_slice(&bytes)
                    .map_err(|_| OfficeError::Invalid("malformed ConversationRecord"))?;
                if parsed_conv.is_some() || conv.organization_id != *org_id || conv.id != cid {
                    return Err(OfficeError::Invalid("ConversationRecord validation failed"));
                }
                parsed_conv = Some((cid, conv));
            }
            Some(Kind::Message(cid, mid)) => {
                let msg: MessageRecord = serde_json::from_slice(&bytes)
                    .map_err(|_| OfficeError::Invalid("malformed MessageRecord"))?;
                if parsed_msg.is_some()
                    || msg.organization_id != *org_id
                    || msg.conversation_id != cid
                    || msg.id != mid
                    || msg.content_sha256 != msg.compute_content_sha256()
                {
                    return Err(OfficeError::Invalid("MessageRecord validation failed"));
                }
                parsed_msg = Some((cid, mid, msg));
            }
            Some(Kind::ParticipantAnswer(cid, emp, ask_id)) => {
                let ans: ParticipantAnswerFile = serde_json::from_slice(&bytes)
                    .map_err(|_| OfficeError::Invalid("malformed ParticipantAnswerFile"))?;
                if parsed_answer.is_some()
                    || ans.organization_id != *org_id
                    || ans.conversation_id != cid
                    || ans.employee_id != emp
                    || ans.message_id != ask_id
                {
                    return Err(OfficeError::Invalid(
                        "ParticipantAnswerFile validation failed",
                    ));
                }
                parsed_answer = Some((cid, emp, ask_id, ans));
            }
            _ => {
                return Err(OfficeError::Invalid(
                    "relayed delta may only contain conversation, message, and optional ask answer",
                ));
            }
        }
    }
    assemble_parsed_relayed_delta(
        origin_office,
        hq_office_id,
        is_ask_recovery,
        parsed_conv,
        parsed_msg,
        parsed_answer,
    )
}

fn assemble_message_less_delta(
    parsed_conv: Option<(Id, crate::messaging::records::ConversationRecord)>,
    parsed_answer: Option<(
        Id,
        Id,
        String,
        crate::messaging::records::ParticipantAnswerFile,
    )>,
) -> Result<ParsedRelayedDelta, OfficeError> {
    match (parsed_conv, parsed_answer) {
        (Some((_, conv)), None) => Ok(ParsedRelayedDelta::ConversationOnly(Box::new(conv))),
        (None, Some((cid, emp, msg_id, ans)))
            if ans.answer.is_none() && ans.accepted_by.as_ref() == Some(&emp) =>
        {
            Ok(ParsedRelayedDelta::HandoffAccept(Box::new(
                ParsedHandoffAccept {
                    cid,
                    emp_id: emp,
                    message_id: msg_id,
                    answer: ans,
                },
            )))
        }
        (None, Some((cid, emp, ask_id, mut ans)))
            if ans.answer.is_some()
                && (ans.reason.as_deref() == Some("origin-close")
                    || crate::messaging::asks::decode_ask_answer_file(&ans)
                        .1
                        .is_some()) =>
        {
            ans.reason = Some("origin-close".to_owned());
            Ok(ParsedRelayedDelta::OriginAskClose(Box::new(
                ParsedOriginAskClose {
                    conv: None,
                    ask_msg: None,
                    cid,
                    emp_id: emp,
                    ask_id,
                    answer: ans,
                },
            )))
        }
        _ => Err(OfficeError::Invalid("relayed delta missing MessageRecord")),
    }
}

fn assemble_parsed_relayed_delta(
    origin_office: &str,
    hq_office_id: &str,
    is_ask_recovery: bool,
    parsed_conv: Option<(Id, crate::messaging::records::ConversationRecord)>,
    parsed_msg: Option<(Id, String, crate::messaging::records::MessageRecord)>,
    parsed_answer: Option<(
        Id,
        Id,
        String,
        crate::messaging::records::ParticipantAnswerFile,
    )>,
) -> Result<ParsedRelayedDelta, OfficeError> {
    let Some((msg_cid, msg_id, msg)) = parsed_msg else {
        return assemble_message_less_delta(parsed_conv, parsed_answer);
    };
    let conv = match parsed_conv {
        Some((conv_cid, c)) if conv_cid == msg_cid => Some(c),
        Some(_) => return Err(OfficeError::Invalid("conversation_id mismatch")),
        None => None,
    };
    let close_origin_ok =
        msg.origin_office_id.as_str() == origin_office || origin_office == hq_office_id;
    let Some((ans_cid, emp, ask_id, mut ans)) = parsed_answer else {
        if is_ask_recovery && msg.kind == "request" && close_origin_ok {
            return Ok(ParsedRelayedDelta::OriginAskRecovery(Box::new(
                ParsedOriginAskRecovery {
                    conv,
                    cid: msg_cid,
                    ask_msg: msg,
                },
            )));
        }
        if msg.origin_office_id.as_str() != origin_office {
            return Err(OfficeError::Invalid(
                "MessageRecord origin_office_id mismatch",
            ));
        }
        return Ok(ParsedRelayedDelta::Message(Box::new(
            ParsedRelayedMessage {
                conv,
                answer: None,
                msg,
            },
        )));
    };
    let is_origin_close_bundle = ans_cid == msg_cid
        && msg_id == ask_id
        && msg.kind == "request"
        && close_origin_ok
        && (ans.reason.as_deref() == Some("origin-close")
            || crate::messaging::asks::decode_ask_answer_file(&ans)
                .1
                .is_some());
    if is_origin_close_bundle {
        ans.reason = Some("origin-close".to_owned());
        return Ok(ParsedRelayedDelta::OriginAskClose(Box::new(
            ParsedOriginAskClose {
                conv,
                ask_msg: Some(msg),
                cid: ans_cid,
                emp_id: emp,
                ask_id,
                answer: ans,
            },
        )));
    }
    if msg.origin_office_id.as_str() != origin_office {
        return Err(OfficeError::Invalid(
            "MessageRecord origin_office_id mismatch",
        ));
    }
    let answer =
        validate_relayed_answer_binding(&msg_cid, &msg, Some((ans_cid, emp, ask_id, ans)))?;
    Ok(ParsedRelayedDelta::Message(Box::new(
        ParsedRelayedMessage { conv, answer, msg },
    )))
}

fn apply_validated_relayed_message(
    office: &Office,
    origin_office: &str,
    message_id: &str,
    operation_id: &str,
    files: Vec<(String, Option<Vec<u8>>)>,
) -> Result<RelayedMessageOutcome, OfficeError> {
    let ctx = load_placement_context(office)?;
    let parsed = parse_validated_relayed_delta(
        &office.identity.organization_id,
        origin_office,
        &ctx.hq_office_id,
        message_id.starts_with("close-ask-"),
        files,
    )?;
    let op_id = Id::parse(operation_id)
        .map_err(|_| OfficeError::Invalid("invalid replication operation_id"))?;
    office.replication.record_replicated_op_id(operation_id)?;
    let ParsedRelayedMessage { conv, answer, msg } = match parsed {
        ParsedRelayedDelta::ConversationOnly(conv) => {
            let reply = office
                .store
                .run(ApplyRelayedConversationOnlyOp {
                    operation_id: op_id,
                    origin_office: origin_office.to_owned(),
                    ctx,
                    conv: *conv,
                })
                .map_err(storage)?;
            return match reply.outcome {
                Outcome::Completed => Ok(RelayedMessageOutcome::default()),
                Outcome::Rejected => Err(OfficeError::Invalid(
                    "relayed conversation rejected by validation",
                )),
            };
        }
        ParsedRelayedDelta::OriginAskRecovery(boxed) => {
            let ParsedOriginAskRecovery { conv, cid, ask_msg } = *boxed;
            let already_present = office
                .store
                .run(CheckAskRecoveryEntitlementOp {
                    here: office.identity.clone(),
                    owner: office.owner.clone(),
                    ctx,
                    origin_office: origin_office.to_owned(),
                    incoming_conv: conv.clone(),
                    cid,
                    ask_msg: ask_msg.clone(),
                })
                .map_err(storage)?
                .map_err(|_| OfficeError::Invalid("relayed ask recovery rejected by validation"))?;
            if !already_present {
                let staged = StagedAskRecovery { conv, ask_msg };
                stage_close_recovery(office, origin_office, &staged.ask_msg.id, &staged)?;
            }
            return Ok(RelayedMessageOutcome::default());
        }
        ParsedRelayedDelta::OriginAskClose(boxed) => {
            return apply_relayed_origin_close(office, origin_office, op_id, ctx, *boxed);
        }
        ParsedRelayedDelta::HandoffAccept(boxed) => {
            return apply_relayed_handoff_accept(office, origin_office, op_id, ctx, *boxed);
        }
        ParsedRelayedDelta::Message(boxed) => *boxed,
    };
    let now_ms = office.monotonic_now_ms();
    if message_expires_at_ms(&msg).is_some_and(|exp| exp <= now_ms) {
        return Err(OfficeError::Invalid("relayed message already expired"));
    }
    let delivered_msg_for_outcome = msg.clone();
    let reply = office
        .store
        .run(ApplyRelayedMessageOp {
            operation_id: op_id,
            here: office.identity.clone(),
            owner: office.owner.clone(),
            origin_office: origin_office.to_owned(),
            now_ms,
            ctx,
            incoming_conv: conv,
            incoming_answer: answer,
            msg,
        })
        .map_err(storage)?;
    match reply.outcome {
        Outcome::Completed => {
            let mut outcome: RelayedMessageOutcome = serde_json::from_slice(&reply.bytes)
                .map_err(|_| OfficeError::Invalid("corrupt relayed message commit reply"))?;
            if outcome.inserted {
                outcome.delivered_message = Some(delivered_msg_for_outcome);
            }
            Ok(outcome)
        }
        Outcome::Rejected => Err(OfficeError::Invalid(
            "relayed message rejected by native messaging validation",
        )),
    }
}
