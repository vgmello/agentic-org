//! Validating the typed questions of an ask (`choice`, `yes_no`, `free_text`).

use std::collections::BTreeSet;

use org_proto::ask::{AskOption, AskQuestion, AskQuestionType, ASK_OPTIONS_MAX, ASK_QUESTIONS_MAX};
use org_proto::ErrorCode;
use org_store::Refusal;

use crate::messaging::features::refusal;

fn validate_choice_options(q: &AskQuestion) -> Result<Vec<AskOption>, Refusal> {
    if q.options.is_empty() || q.options.len() > ASK_OPTIONS_MAX {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            format!(
                "question `{}` choice requires 1..={ASK_OPTIONS_MAX} options",
                q.id
            ),
        ));
    }
    let mut seen = BTreeSet::new();
    let mut cleaned = Vec::with_capacity(q.options.len());
    for opt in &q.options {
        let oid = opt.id.trim();
        let label = opt.label.trim();
        if oid.is_empty() || oid.len() > 64 || label.is_empty() || !seen.insert(oid.to_owned()) {
            return Err(refusal(
                ErrorCode::InvalidRequest,
                format!(
                    "question `{}` options need unique non-empty ids and labels",
                    q.id
                ),
            ));
        }
        cleaned.push(AskOption {
            id: oid.to_owned(),
            label: label.to_owned(),
            description: opt
                .description
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
        });
    }
    Ok(cleaned)
}

fn validate_one_question(q: &AskQuestion) -> Result<AskQuestion, Refusal> {
    let qid = q.id.trim();
    let prompt = q.prompt.trim();
    if qid.is_empty() || qid.len() > 64 || prompt.is_empty() {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "each question needs a non-empty id and prompt",
        ));
    }
    let options = match q.question_type {
        AskQuestionType::Choice => validate_choice_options(q)?,
        AskQuestionType::YesNo | AskQuestionType::FreeText => {
            if !q.options.is_empty() || q.allow_multiple {
                return Err(refusal(
                    ErrorCode::InvalidRequest,
                    format!("question `{qid}` takes no options or allow_multiple"),
                ));
            }
            Vec::new()
        }
        _ => {
            return Err(refusal(
                ErrorCode::InvalidRequest,
                format!("question `{qid}` type must be choice, yes_no, or free_text"),
            ));
        }
    };
    let default_value = match q
        .default_value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None => None,
        Some(def) => {
            let valid = match q.question_type {
                AskQuestionType::Choice => options.iter().any(|o| o.id == def),
                AskQuestionType::YesNo => matches!(def, "yes" | "no"),
                _ => false,
            };
            if !valid {
                return Err(refusal(
                    ErrorCode::InvalidRequest,
                    format!("question `{qid}` has an invalid default_value"),
                ));
            }
            Some(def.to_owned())
        }
    };
    Ok(AskQuestion {
        id: qid.to_owned(),
        question_type: q.question_type,
        prompt: prompt.to_owned(),
        options,
        allow_multiple: q.question_type == AskQuestionType::Choice && q.allow_multiple,
        default_value,
        optional: q.optional,
    })
}

pub fn validate_ask_questions(questions: &[AskQuestion]) -> Result<Vec<AskQuestion>, Refusal> {
    if questions.is_empty() || questions.len() > ASK_QUESTIONS_MAX {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            format!("an ask requires 1..={ASK_QUESTIONS_MAX} questions"),
        ));
    }
    let mut ids = BTreeSet::new();
    let mut out = Vec::with_capacity(questions.len());
    for q in questions {
        let cleaned = validate_one_question(q)?;
        if !ids.insert(cleaned.id.clone()) {
            return Err(refusal(
                ErrorCode::InvalidRequest,
                format!("duplicate question id `{}`", cleaned.id),
            ));
        }
        out.push(cleaned);
    }
    Ok(out)
}
