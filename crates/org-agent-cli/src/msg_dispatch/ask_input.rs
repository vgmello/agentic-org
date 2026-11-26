//! The questions of `msg ask`: the full `questions` array (given in `--json`), or one question
//! written with the scalar flags `--prompt`, `--question-type`, `--option` and
//! `--default-value` (`doc-28` §3.3). The two forms never mix, and neither is guessed: a line
//! that gives both, or neither, is refused before anything is sent.

use org_command::{Refusal, Request};
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

/// The input fields of the one-question form, as their flags are named.
const SCALAR_FIELDS: [&str; 4] = ["prompt", "question_type", "option", "default_value"];

/// The ID the one question of the scalar form gets.
const SCALAR_QUESTION_ID: &str = "q1";

fn invalid(message: impl Into<String>, hint: &str) -> Refusal {
    Refusal::new(ErrorCode::InvalidRequest, message, hint)
}

/// The `questions` array to send.
pub(super) fn questions(request: &Request) -> Result<Value, Refusal> {
    let given: Vec<String> = SCALAR_FIELDS
        .iter()
        .filter(|name| request.input.contains_key(**name))
        .map(|name| format!("--{}", name.replace('_', "-")))
        .collect();
    match (request.input.get("questions"), given.is_empty()) {
        (Some(_), false) => Err(invalid(
            format!(
                "{} cannot be combined with a `questions` array.",
                given.join(", ")
            ),
            "Ask one question with --prompt (and --question-type, --option, --default-value), or several with --json '{\"questions\":[...]}', not both.",
        )),
        (Some(questions), true) => Ok(questions.clone()),
        (None, true) => Err(invalid(
            "`msg ask` needs a question: --prompt, or a `questions` array in --json.",
            "Example: agent-os msg ask --to owner --prompt 'Deploy to prod?' --question-type yes_no",
        )),
        (None, false) => single_question(&request.input),
    }
}

fn single_question(input: &Map<String, Value>) -> Result<Value, Refusal> {
    let text = |name: &str| input.get(name).and_then(Value::as_str);
    let prompt = text("prompt").ok_or_else(|| {
        invalid(
            "--question-type, --option and --default-value need --prompt.",
            "Pass --prompt <question text>.",
        )
    })?;
    let options: Vec<&str> = input
        .get("option")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let question_type = match text("question_type") {
        Some(kind) => kind,
        None if options.is_empty() => "free_text",
        None => "choice",
    };
    match (question_type, options.is_empty()) {
        ("choice", true) => {
            return Err(invalid(
                "A choice question needs at least one --option.",
                "Repeat --option <id> or --option <id>=<label> for each choice.",
            ))
        }
        ("yes_no" | "free_text", false) => {
            return Err(invalid(
                format!("--option is only for a choice question, not `{question_type}`."),
                "Use --question-type choice with --option, or drop --option.",
            ))
        }
        _ => {}
    }
    let mut question = Map::new();
    question.insert("id".into(), json!(SCALAR_QUESTION_ID));
    question.insert("question_type".into(), json!(question_type));
    question.insert("prompt".into(), json!(prompt));
    if !options.is_empty() {
        let options = options
            .into_iter()
            .map(option)
            .collect::<Result<Vec<_>, _>>()?;
        question.insert("options".into(), Value::Array(options));
    }
    if let Some(default) = text("default_value") {
        question.insert("default_value".into(), json!(default));
    }
    Ok(Value::Array(vec![Value::Object(question)]))
}

/// One `--option`: `id=Label`, or a bare word that is both the ID and the label.
fn option(raw: &str) -> Result<Value, Refusal> {
    let (id, label) = raw.split_once('=').unwrap_or((raw, raw));
    let (id, label) = (id.trim(), label.trim());
    if id.is_empty() || label.is_empty() {
        return Err(invalid(
            format!("--option `{raw}` needs a non-empty ID and label."),
            "Write --option dev, or --option dev=Development.",
        ));
    }
    Ok(json!({"id": id, "label": label}))
}
