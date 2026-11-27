//! Closing a local ask from an inbound relayed answer and resuming its asker.

use org_store::Id;

use super::types::{RelayedAskAnswer, RelayedRecipientCtx};

pub(super) fn attach_relayed_ask_answer(
    view: &impl crate::messaging::ports::MessagingRead,
    rctx: &RelayedRecipientCtx<'_>,
    incoming_answer: Option<&(Id, String, crate::messaging::records::ParticipantAnswerFile)>,
    mut change: org_store::Change,
) -> Result<(org_store::Change, Option<RelayedAskAnswer>, Option<String>), org_store::Refusal> {
    let Some((emp_id, ask_id, answer_file)) = incoming_answer else {
        return Ok((change, None, None));
    };
    let Some(ask_msg) = view
        .message(ask_id)
        .map_err(|_| org_store::Refusal::invalid("read ask message"))?
    else {
        return Ok((change, None, None));
    };
    if ask_msg.conversation_id != rctx.conv.id || ask_msg.kind != "request" {
        return Err(org_store::Refusal::invalid(
            "relayed answer does not target a request in this conversation",
        ));
    }
    let Some(card) = crate::messaging::asks::parse_ask_card(&ask_msg) else {
        return Err(org_store::Refusal::invalid(
            "relayed answer targets message without ask card",
        ));
    };
    let Some(responder) = rctx.employees.iter().find(|e| e.id == *emp_id) else {
        return Err(org_store::Refusal::invalid("relayed ask responder missing"));
    };
    // Reuse native approval, self-answer, addressee, and typed answer-set rules (`TASK-52` P6-#3).
    if !rctx.active_members.contains(emp_id) && emp_id != rctx.owner {
        return Err(org_store::Refusal::invalid(
            "relayed ask responder is not a conversation member",
        ));
    }
    if card.approval && responder.kind != crate::organization::records::EmployeeKind::Person {
        return Err(org_store::Refusal::invalid(
            "only a person may answer an approval",
        ));
    }
    if ask_msg.sender_employee_id.as_ref() == Some(emp_id) {
        return Err(org_store::Refusal::invalid(
            "the asker cannot answer their own ask",
        ));
    }
    if card
        .addressed_to
        .as_deref()
        .is_some_and(|t| t != emp_id.as_str())
    {
        return Err(org_store::Refusal::invalid(
            "only the addressed recipient may answer this ask",
        ));
    }
    let (Some(decoded_ans), _) = crate::messaging::asks::decode_ask_answer_file(answer_file) else {
        return Err(org_store::Refusal::invalid("malformed relayed ask answer"));
    };
    let validated =
        crate::messaging::asks::validate_answer_set(&card, decoded_ans.answers.clone())?;
    let existing_canonical = view
        .answer_for_message(ask_id)
        .map_err(|_| org_store::Refusal::invalid("read existing ask answer"))?;
    let is_ask_origin = ask_msg.origin_office_id == rctx.here.office_id;
    let (skip_write, emit_resume) = evaluate_relayed_ask_close(
        is_ask_origin,
        existing_canonical.as_ref(),
        emp_id,
        answer_file,
    );
    let resume_info = emit_resume
        .then(|| build_local_ask_answer_resume(rctx, &ask_msg, &card, &validated, answer_file))
        .flatten();
    let closed_ask_id = emit_resume.then(|| ask_id.clone());
    if skip_write {
        return Ok((change, resume_info, closed_ask_id));
    }
    let mut stored_answer = answer_file.clone();
    if is_ask_origin {
        stored_answer.reason = Some("origin-close".to_owned());
    }
    let encoded_answer = crate::messaging::records::encode_json(&stored_answer);
    let path = crate::messaging::records::participant_answer_path(&rctx.conv.id, emp_id, ask_id);
    change = change.put(path, encoded_answer);
    let (next_change, _) = crate::messaging::asks::mark_ask_recipients_done(
        view,
        rctx.here,
        &ask_msg,
        ask_msg.org_sequence,
        &ask_msg.created_at,
        change,
    )?;
    Ok((next_change, resume_info, closed_ask_id))
}

/// Evaluates `(skip_write, emit_resume)` for a relayed ask answer:
/// - On the ask's origin office (`is_ask_origin`), enforces strict first-close consistency so
///   display and resume never diverge (`TASK-52` P9-#6).
/// - On non-origin offices, preserves every valid responder's submission and converges onto the
///   deterministic `(accepted_at, employee_id)` canonical winner (`TASK-52` P7-#5 / P8-#9).
fn evaluate_relayed_ask_close(
    is_ask_origin: bool,
    existing_canonical: Option<&crate::messaging::records::ParticipantAnswerFile>,
    emp_id: &Id,
    answer_file: &crate::messaging::records::ParticipantAnswerFile,
) -> (bool, bool) {
    let Some(existing) = existing_canonical else {
        return (false, true);
    };
    let exact_replay = existing.employee_id == *emp_id && existing.answer == answer_file.answer;
    if is_ask_origin {
        return (true, exact_replay);
    }
    let responder_stored = existing.employee_id == *emp_id;
    let emit_resume = is_canonical_relayed_ask_winner(Some(existing), emp_id, answer_file);
    (responder_stored, emit_resume)
}

/// Deterministic `(accepted_at, employee_id)` canonical winner comparison so concurrent offline
/// answers converge to the identical canonical answer across all offices (`TASK-52` P7-#5 / P8-#9).
pub(super) fn is_canonical_relayed_ask_winner(
    existing_canonical: Option<&crate::messaging::records::ParticipantAnswerFile>,
    emp_id: &Id,
    answer_file: &crate::messaging::records::ParticipantAnswerFile,
) -> bool {
    let Some(existing) = existing_canonical else {
        return true;
    };
    let incoming_key = (
        answer_file.accepted_at.as_deref().unwrap_or(""),
        emp_id.as_str(),
    );
    let existing_key = (
        existing.accepted_at.as_deref().unwrap_or(""),
        existing.employee_id.as_str(),
    );
    incoming_key < existing_key
        || (existing.employee_id == *emp_id && existing.answer == answer_file.answer)
}

fn build_local_ask_answer_resume(
    rctx: &RelayedRecipientCtx<'_>,
    ask_msg: &crate::messaging::records::MessageRecord,
    card: &crate::messaging::asks::AskCard,
    validated: &[org_proto::ask::QuestionAnswer],
    answer_file: &crate::messaging::records::ParticipantAnswerFile,
) -> Option<RelayedAskAnswer> {
    let asker_id = ask_msg.sender_employee_id.as_ref()?;
    let owner_office = rctx
        .placement
        .employee_offices
        .get(asker_id.as_str())
        .map(String::as_str)
        .unwrap_or(rctx.placement.hq_office_id.as_str());
    if owner_office != rctx.here.office_id.as_str() {
        return None;
    }
    let is_agent = rctx
        .employees
        .iter()
        .any(|e| e.id == *asker_id && e.kind == crate::organization::records::EmployeeKind::Agent);
    if !is_agent {
        return None;
    }
    let question_summary = card
        .questions
        .iter()
        .map(|q| q.prompt.as_str())
        .collect::<Vec<_>>()
        .join(" / ");
    let answer_summary = crate::messaging::asks::render_answer_summary(card, validated);
    let attachment_ids = answer_file
        .attachments
        .iter()
        .map(|a| a.id.clone())
        .collect();
    Some(RelayedAskAnswer {
        ask_id: ask_msg.id.clone(),
        asker_id: asker_id.as_str().to_owned(),
        question_summary,
        answer_summary,
        attachment_ids,
    })
}
