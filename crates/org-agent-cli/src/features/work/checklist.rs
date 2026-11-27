//! `checklist` handlers: listing progress/ordinals, batch adding, checking, editing, removing,
//! and moving steps (`TASK-49`, `TASK-90`).

use std::collections::BTreeSet;

use org_command::{Answer, Refusal, Request};
use org_proto::tasks::{
    ChecklistAddItem, ChecklistSection, ChecklistStepWire, TaskInspectResult, TasksChecklistAdd,
    TasksChecklistAddParams, TasksChecklistCheck, TasksChecklistCheckParams, TasksChecklistEdit,
    TasksChecklistEditParams, TasksChecklistMove, TasksChecklistMoveParams, TasksChecklistRemove,
    TasksChecklistRemoveParams, TasksChecklistResult, TasksInspect, TasksInspectParams,
};
use org_proto::{Call, ErrorCode};
use serde_json::{json, Map, Value};

use super::{cmd_id, unreadable};
use crate::runtime::input::{
    entity_id, is_set, opt_bool, opt_str_list, opt_string, opt_u64, require_if_match, require_str,
    write_answer,
};
use crate::runtime::transport::OfficeTransport;

pub(super) fn try_checklist<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    match request.command.verb {
        "list" => Some(read_checklist_list(transport, request)),
        "add" => Some(write_checklist_add(transport, request)),
        "check" => Some(write_checklist_check(transport, request)),
        "edit" => Some(write_checklist_edit(transport, request)),
        "remove" => Some(write_checklist_remove(transport, request)),
        "move" => Some(write_checklist_move(transport, request)),
        _ => None,
    }
}

fn invalid(message: impl Into<String>, hint: impl Into<String>) -> Refusal {
    Refusal::new(ErrorCode::InvalidRequest, message, hint)
}

fn parse_section(raw: &str) -> Result<ChecklistSection, Refusal> {
    ChecklistSection::parse(raw).ok_or_else(|| {
        invalid(
            format!("invalid checklist section `{raw}`"),
            "Use acceptance, dod, or work.",
        )
    })
}

fn opt_section(
    input: &Map<String, Value>,
    field: &str,
) -> Result<Option<ChecklistSection>, Refusal> {
    opt_string(input, field)
        .map(|s| parse_section(&s))
        .transpose()
}

fn opt_u64_list(input: &Map<String, Value>, field: &str) -> Result<Vec<u64>, Refusal> {
    let Some(items) = input.get(field).and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    items
        .iter()
        .map(|v| {
            v.as_u64().ok_or_else(|| {
                let flag = field.replace('_', "-");
                invalid(
                    format!("`--{flag}` must be a positive integer."),
                    format!("Pass 1-based step ordinals (1, 2, ...) for `--{flag}`."),
                )
            })
        })
        .collect()
}

fn section_progress(steps: &[ChecklistStepWire], section: Option<ChecklistSection>) -> Value {
    let mut total = 0u64;
    let mut done = 0u64;
    let mut required_total = 0u64;
    let mut required_done = 0u64;
    for step in steps {
        if section.is_some_and(|sec| step.section != sec) {
            continue;
        }
        total += 1;
        if step.done {
            done += 1;
        }
        if step.required {
            required_total += 1;
            if step.done {
                required_done += 1;
            }
        }
    }
    json!({
        "total": total,
        "done": done,
        "required_total": required_total,
        "required_done": required_done,
    })
}

fn build_progress(steps: &[ChecklistStepWire]) -> Value {
    let mut overall = section_progress(steps, None);
    if let Some(map) = overall.as_object_mut() {
        map.insert(
            "acceptance".into(),
            section_progress(steps, Some(ChecklistSection::Acceptance)),
        );
        map.insert(
            "dod".into(),
            section_progress(steps, Some(ChecklistSection::Dod)),
        );
        map.insert(
            "work".into(),
            section_progress(steps, Some(ChecklistSection::Work)),
        );
    }
    overall
}

fn step_to_ordinal_row(step: &ChecklistStepWire) -> Value {
    let mut m = Map::new();
    m.insert("index".into(), json!(step.position.saturating_add(1)));
    m.insert("section".into(), json!(step.section));
    m.insert("step_id".into(), json!(step.id));
    m.insert("text".into(), json!(step.text));
    m.insert("required".into(), json!(step.required));
    m.insert("done".into(), json!(step.done));
    if let Some(ref who) = step.checked_by_employee_id {
        m.insert("checked_by_employee_id".into(), json!(who));
    }
    if let Some(ref at) = step.checked_at {
        m.insert("checked_at".into(), json!(at));
    }
    m.insert("revision".into(), json!(step.revision));
    Value::Object(m)
}

/// Compact receipt for checklist writes: progress across the whole checklist plus only the
/// mutated step(s) (or `removed_step_id` for `remove`).
fn checklist_receipt(
    request: &Request,
    raw: Value,
    fallback_ids: &[String],
    removed: bool,
) -> Result<Answer, Refusal> {
    let res: TasksChecklistResult = serde_json::from_value(raw).map_err(unreadable)?;
    let mutated: &[String] = if !res.mutated_step_ids.is_empty() {
        &res.mutated_step_ids
    } else {
        fallback_ids
    };
    let mut data = Map::new();
    data.insert("task_id".into(), json!(res.task.id));
    data.insert("key".into(), json!(res.task.key));
    data.insert("revision".into(), json!(res.task.revision));
    data.insert("progress".into(), build_progress(&res.checklist));
    if removed {
        if let Some(id) = mutated.first() {
            data.insert("removed_step_id".into(), json!(id));
        }
    } else {
        let wanted: BTreeSet<&str> = mutated.iter().map(String::as_str).collect();
        let rows: Vec<Value> = res
            .checklist
            .iter()
            .filter(|step| wanted.contains(step.id.as_str()))
            .map(step_to_ordinal_row)
            .collect();
        data.insert("steps".into(), Value::Array(rows));
    }
    Ok(write_answer(
        request,
        Value::Object(data),
        Some(res.task.revision),
    ))
}

enum SingleTarget {
    StepId {
        step_id: String,
        expected_revision: Option<u64>,
    },
    Ordinal {
        section: ChecklistSection,
        index: u64,
        expected_revision: u64,
    },
}

fn parse_single_target(request: &Request) -> Result<SingleTarget, Refusal> {
    let i = &request.input;
    let step_id = opt_string(i, "step_id").filter(|s| !s.trim().is_empty());
    let section = opt_section(i, "section")?;
    let index = opt_u64(i, "index")?;
    let has_ordinal = section.is_some() || index.is_some();
    if step_id.is_some() && has_ordinal {
        return Err(invalid(
            "cannot combine `--step-id` with `--section`/`--index`",
            "Target the step either by `--step-id <uuid>` or by `--section <sec> --index <n> --if-match <rev>`.",
        ));
    }
    if let Some(step_id) = step_id {
        return Ok(SingleTarget::StepId {
            step_id,
            expected_revision: request.if_match,
        });
    }
    let (Some(section), Some(index)) = (section, index) else {
        return Err(invalid(
            "missing step target: pass `--step-id` or both `--section` and 1-based `--index`",
            "Run `checklist list --task <ref>` to see step IDs and 1-based ordinals.",
        ));
    };
    if index == 0 {
        return Err(invalid(
            "`--index` is 1-based; use 1 or greater",
            "Pass `--index 1` for the first step in the section.",
        ));
    }
    let expected_revision = require_if_match(request)?;
    Ok(SingleTarget::Ordinal {
        section,
        index,
        expected_revision,
    })
}

enum CheckTargets {
    StepId {
        step_id: String,
        expected_revision: Option<u64>,
    },
    Ordinals {
        section: ChecklistSection,
        indices: Vec<u64>,
        expected_revision: u64,
    },
}

fn parse_check_targets(request: &Request) -> Result<CheckTargets, Refusal> {
    let i = &request.input;
    let step_id = opt_string(i, "step_id").filter(|s| !s.trim().is_empty());
    let section = opt_section(i, "section")?;
    let indices = opt_u64_list(i, "index")?;
    let has_ordinal = section.is_some() || !indices.is_empty();
    if step_id.is_some() && has_ordinal {
        return Err(invalid(
            "cannot combine `--step-id` with `--section`/`--index`",
            "Target step(s) either by `--step-id <uuid>` or by `--section <sec> --index <n>... --if-match <rev>`.",
        ));
    }
    if let Some(step_id) = step_id {
        return Ok(CheckTargets::StepId {
            step_id,
            expected_revision: request.if_match,
        });
    }
    let (Some(section), false) = (section, indices.is_empty()) else {
        return Err(invalid(
            "missing step target: pass `--step-id` or both `--section` and one or more 1-based `--index` values",
            "Run `checklist list --task <ref>` to see step IDs and 1-based ordinals.",
        ));
    };
    let mut seen = BTreeSet::new();
    for &ord in &indices {
        if ord == 0 {
            return Err(invalid(
                "`--index` is 1-based; use 1 or greater",
                "Pass `--index 1` for the first step in the section.",
            ));
        }
        if !seen.insert(ord) {
            return Err(invalid(
                format!("duplicate `--index {ord}`"),
                "List each 1-based step ordinal at most once.",
            ));
        }
    }
    let expected_revision = require_if_match(request)?;
    Ok(CheckTargets::Ordinals {
        section,
        indices,
        expected_revision,
    })
}

pub(super) fn read_checklist_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let task = require_str(&request.input, "task")?.to_owned();
    let section_filter = match opt_string(&request.input, "section").as_deref() {
        None | Some("all") => None,
        Some(other) => Some(parse_section(other)?),
    };
    let unchecked_only = is_set(&request.input, "unchecked");
    let params = TasksInspectParams {
        task,
        fields: Some(vec!["checklist".to_owned()]),
    };
    let raw = transport.call_method(TasksInspect::METHOD, json!(params))?;
    let res: TaskInspectResult = serde_json::from_value(raw).map_err(unreadable)?;
    let progress = build_progress(&res.checklist);
    let steps: Vec<Value> = res
        .checklist
        .iter()
        .filter(|step| section_filter.is_none_or(|sec| step.section == sec))
        .filter(|step| !unchecked_only || !step.done)
        .map(step_to_ordinal_row)
        .collect();
    Ok(Answer::completed(json!({
        "task_id": res.task.id,
        "key": res.task.key,
        "revision": res.task.revision,
        "progress": progress,
        "steps": steps,
    })))
}

fn parse_required_override(input: &Map<String, Value>) -> Result<Option<bool>, Refusal> {
    let req = opt_bool(input, "required");
    let opt = is_set(input, "optional");
    if opt && req == Some(true) {
        return Err(invalid(
            "cannot pass both `--required` and `--optional`",
            "Use `--required` to require the step or `--optional` to make it optional.",
        ));
    }
    Ok(if opt { Some(false) } else { req })
}

pub(super) fn write_checklist_add<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let command_id = cmd_id(request)?;
    let task = require_str(&request.input, "task")?.to_owned();
    let section = opt_section(&request.input, "section")?;
    let required = parse_required_override(&request.input)?;
    let texts = opt_str_list(&request.input, "text").unwrap_or_default();
    if texts.is_empty() {
        return Err(invalid(
            "at least one `--text` step is required",
            "Pass `--text <step>` (repeat for multiple steps).",
        ));
    }
    let (params, fallback_ids) = if texts.len() == 1 {
        let step_id = entity_id(request, None, "checklist-step")?;
        let params = TasksChecklistAddParams {
            command_id,
            task,
            step_id: step_id.clone(),
            text: texts.into_iter().next().unwrap_or_default(),
            required,
            section,
            steps: Vec::new(),
        };
        (params, vec![step_id])
    } else {
        let mut steps = Vec::with_capacity(texts.len());
        let mut ids = Vec::with_capacity(texts.len());
        for (idx, text) in texts.into_iter().enumerate() {
            let step_id = entity_id(request, None, &format!("checklist-step-{idx}"))?;
            ids.push(step_id.clone());
            steps.push(ChecklistAddItem {
                step_id,
                text,
                required,
                section,
            });
        }
        let params = TasksChecklistAddParams {
            command_id,
            task,
            step_id: String::new(),
            text: String::new(),
            required: None,
            section,
            steps,
        };
        (params, ids)
    };
    let raw = transport.call_method(TasksChecklistAdd::METHOD, json!(params))?;
    checklist_receipt(request, raw, &fallback_ids, false)
}

pub(super) fn write_checklist_check<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let command_id = cmd_id(request)?;
    let done_flag = opt_bool(&request.input, "done");
    let undone = is_set(&request.input, "undone");
    if undone && done_flag == Some(true) {
        return Err(invalid(
            "cannot pass both `--done` and `--undone`",
            "Use `--done` (default) to check or `--undone` to uncheck.",
        ));
    }
    let done = if undone {
        false
    } else {
        done_flag.unwrap_or(true)
    };
    let task = require_str(&request.input, "task")?.to_owned();
    let (params, fallback_ids) = match parse_check_targets(request)? {
        CheckTargets::StepId {
            step_id,
            expected_revision,
        } => (
            TasksChecklistCheckParams {
                command_id,
                task,
                step_id: step_id.clone(),
                section: None,
                indices: Vec::new(),
                expected_revision,
                done,
            },
            vec![step_id],
        ),
        CheckTargets::Ordinals {
            section,
            indices,
            expected_revision,
        } => (
            TasksChecklistCheckParams {
                command_id,
                task,
                step_id: String::new(),
                section: Some(section),
                indices,
                expected_revision: Some(expected_revision),
                done,
            },
            Vec::new(),
        ),
    };
    let raw = transport.call_method(TasksChecklistCheck::METHOD, json!(params))?;
    checklist_receipt(request, raw, &fallback_ids, false)
}

pub(super) fn write_checklist_edit<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let command_id = cmd_id(request)?;
    let task = require_str(&request.input, "task")?.to_owned();
    let text = opt_string(&request.input, "text");
    let required = parse_required_override(&request.input)?;
    if text.is_none() && required.is_none() {
        return Err(invalid(
            "`checklist edit` requires `--text`, `--required`, or `--optional`",
            "Pass updated `--text <text>` and/or `--required` / `--optional`.",
        ));
    }
    let (params, fallback_ids) = match parse_single_target(request)? {
        SingleTarget::StepId {
            step_id,
            expected_revision,
        } => (
            TasksChecklistEditParams {
                command_id,
                task,
                step_id: step_id.clone(),
                section: None,
                index: None,
                expected_revision,
                text,
                required,
            },
            vec![step_id],
        ),
        SingleTarget::Ordinal {
            section,
            index,
            expected_revision,
        } => (
            TasksChecklistEditParams {
                command_id,
                task,
                step_id: String::new(),
                section: Some(section),
                index: Some(index),
                expected_revision: Some(expected_revision),
                text,
                required,
            },
            Vec::new(),
        ),
    };
    let raw = transport.call_method(TasksChecklistEdit::METHOD, json!(params))?;
    checklist_receipt(request, raw, &fallback_ids, false)
}

pub(super) fn write_checklist_remove<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let command_id = cmd_id(request)?;
    let task = require_str(&request.input, "task")?.to_owned();
    let (params, fallback_ids) = match parse_single_target(request)? {
        SingleTarget::StepId {
            step_id,
            expected_revision,
        } => (
            TasksChecklistRemoveParams {
                command_id,
                task,
                step_id: step_id.clone(),
                section: None,
                index: None,
                expected_revision,
            },
            vec![step_id],
        ),
        SingleTarget::Ordinal {
            section,
            index,
            expected_revision,
        } => (
            TasksChecklistRemoveParams {
                command_id,
                task,
                step_id: String::new(),
                section: Some(section),
                index: Some(index),
                expected_revision: Some(expected_revision),
            },
            Vec::new(),
        ),
    };
    let raw = transport.call_method(TasksChecklistRemove::METHOD, json!(params))?;
    checklist_receipt(request, raw, &fallback_ids, true)
}

pub(super) fn write_checklist_move<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let command_id = cmd_id(request)?;
    let task = require_str(&request.input, "task")?.to_owned();
    let to_section = opt_section(&request.input, "to_section")?;
    let position = opt_u64(&request.input, "position")?.ok_or_else(|| {
        invalid(
            "`position` is required",
            "Provide a zero-based target position.",
        )
    })?;
    let (params, fallback_ids) = match parse_single_target(request)? {
        SingleTarget::StepId {
            step_id,
            expected_revision,
        } => (
            TasksChecklistMoveParams {
                command_id,
                task,
                step_id: step_id.clone(),
                from_section: None,
                index: None,
                expected_revision,
                position,
                section: to_section,
            },
            vec![step_id],
        ),
        SingleTarget::Ordinal {
            section,
            index,
            expected_revision,
        } => (
            TasksChecklistMoveParams {
                command_id,
                task,
                step_id: String::new(),
                from_section: Some(section),
                index: Some(index),
                expected_revision: Some(expected_revision),
                position,
                section: to_section,
            },
            Vec::new(),
        ),
    };
    let raw = transport.call_method(TasksChecklistMove::METHOD, json!(params))?;
    checklist_receipt(request, raw, &fallback_ids, false)
}
