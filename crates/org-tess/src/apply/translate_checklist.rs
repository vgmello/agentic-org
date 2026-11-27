//! Direct RPC translators and receipt shaping for `checklist add|check|edit|remove|move`.

use std::collections::BTreeSet;

use org_proto::tasks::{
    ChecklistAddItem, ChecklistSection, ChecklistStepWire, TasksChecklistAdd,
    TasksChecklistAddParams, TasksChecklistCheck, TasksChecklistCheckParams, TasksChecklistEdit,
    TasksChecklistEditParams, TasksChecklistMove, TasksChecklistMoveParams, TasksChecklistRemove,
    TasksChecklistRemoveParams, TasksChecklistResult,
};
use org_proto::{ApiError, Call};
use serde_json::{json, Map, Value};

use super::{
    invalid, is_set, opt_bool, opt_str_list, opt_string, opt_u64, require_rev, require_str,
    unreadable, OutputShape, PreparedRpc,
};

pub(super) fn try_prepare(
    next_id: &mut impl FnMut() -> String,
    resource: &str,
    verb: &str,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Option<Result<PreparedRpc, ApiError>> {
    if resource != "checklist" {
        return None;
    }
    match verb {
        "add" => Some(prep_add(next_id, input)),
        "check" => Some(prep_check(next_id, input, expected_revision)),
        "edit" => Some(prep_edit(next_id, input, expected_revision)),
        "remove" => Some(prep_remove(next_id, input, expected_revision)),
        "move" => Some(prep_move(next_id, input, expected_revision)),
        _ => None,
    }
}

pub(super) fn shape_receipt(
    method: &str,
    raw: Value,
    fallback_ids: &[String],
    removed: bool,
) -> Result<Value, ApiError> {
    let res: TasksChecklistResult = serde_json::from_value(raw).map_err(|_| unreadable(method))?;
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
    Ok(Value::Object(data))
}

fn section_progress(steps: &[ChecklistStepWire], section: Option<ChecklistSection>) -> Value {
    let (mut total, mut done, mut req_total, mut req_done) = (0u64, 0u64, 0u64, 0u64);
    for step in steps {
        if section.is_some_and(|sec| step.section != sec) {
            continue;
        }
        total += 1;
        if step.done {
            done += 1;
        }
        if step.required {
            req_total += 1;
            if step.done {
                req_done += 1;
            }
        }
    }
    json!({
        "total": total,
        "done": done,
        "required_total": req_total,
        "required_done": req_done,
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

fn parse_section(raw: &str) -> Result<ChecklistSection, ApiError> {
    ChecklistSection::parse(raw)
        .ok_or_else(|| invalid(format!("invalid checklist section `{raw}`")))
}

fn opt_section(
    input: &Map<String, Value>,
    field: &str,
) -> Result<Option<ChecklistSection>, ApiError> {
    opt_string(input, field)
        .map(|s| parse_section(&s))
        .transpose()
}

fn opt_u64_list(input: &Map<String, Value>, field: &str) -> Result<Vec<u64>, ApiError> {
    let Some(items) = input.get(field).and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    items
        .iter()
        .map(|v| {
            v.as_u64()
                .ok_or_else(|| invalid(format!("`{field}` entries must be positive integers")))
        })
        .collect()
}

fn parse_required_override(input: &Map<String, Value>) -> Result<Option<bool>, ApiError> {
    let req = opt_bool(input, "required");
    let opt = is_set(input, "optional");
    if opt && req == Some(true) {
        return Err(invalid("cannot pass both `required` and `optional`"));
    }
    Ok(if opt { Some(false) } else { req })
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

fn parse_single_target(
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<SingleTarget, ApiError> {
    let step_id = opt_string(input, "step_id").filter(|s| !s.trim().is_empty());
    let section = opt_section(input, "section")?;
    let index = opt_u64(input, "index")?;
    if step_id.is_some() && (section.is_some() || index.is_some()) {
        return Err(invalid("cannot combine `step_id` with `section`/`index`"));
    }
    if let Some(step_id) = step_id {
        return Ok(SingleTarget::StepId {
            step_id,
            expected_revision,
        });
    }
    let (Some(section), Some(index)) = (section, index) else {
        return Err(invalid(
            "pass `step_id` or both `section` and 1-based `index`",
        ));
    };
    if index == 0 {
        return Err(invalid("`index` is 1-based; use 1 or greater"));
    }
    Ok(SingleTarget::Ordinal {
        section,
        index,
        expected_revision: require_rev(expected_revision)?,
    })
}

fn prep_add(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
) -> Result<PreparedRpc, ApiError> {
    let task = require_str(input, "task")?.to_owned();
    let section = opt_section(input, "section")?;
    let required = parse_required_override(input)?;
    let texts = opt_str_list(input, "text").unwrap_or_default();
    if texts.is_empty() {
        return Err(invalid("at least one `text` step is required"));
    }
    let (params, fallback_ids) = if texts.len() == 1 {
        let step_id = next_id();
        let params = TasksChecklistAddParams {
            command_id: next_id(),
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
        for text in texts {
            let step_id = next_id();
            ids.push(step_id.clone());
            steps.push(ChecklistAddItem {
                step_id,
                text,
                required,
                section,
            });
        }
        let params = TasksChecklistAddParams {
            command_id: next_id(),
            task,
            step_id: String::new(),
            text: String::new(),
            required: None,
            section,
            steps,
        };
        (params, ids)
    };
    Ok(PreparedRpc {
        method: TasksChecklistAdd::METHOD,
        params: json!(params),
        shape: OutputShape::Checklist {
            fallback_ids,
            removed: false,
        },
    })
}

fn prep_check(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let done_flag = opt_bool(input, "done");
    let undone = is_set(input, "undone");
    if undone && done_flag == Some(true) {
        return Err(invalid("cannot pass both `done` and `undone`"));
    }
    let done = if undone {
        false
    } else {
        done_flag.unwrap_or(true)
    };
    let task = require_str(input, "task")?.to_owned();
    let step_id = opt_string(input, "step_id").filter(|s| !s.trim().is_empty());
    let section = opt_section(input, "section")?;
    let indices = opt_u64_list(input, "index")?;
    if step_id.is_some() && (section.is_some() || !indices.is_empty()) {
        return Err(invalid("cannot combine `step_id` with `section`/`index`"));
    }
    let (params, fallback_ids) = if let Some(step_id) = step_id {
        (
            TasksChecklistCheckParams {
                command_id: next_id(),
                task,
                step_id: step_id.clone(),
                section: None,
                indices: Vec::new(),
                expected_revision,
                done,
            },
            vec![step_id],
        )
    } else {
        let (Some(section), false) = (section, indices.is_empty()) else {
            return Err(invalid(
                "pass `step_id` or both `section` and one or more 1-based `index` values",
            ));
        };
        let mut seen = BTreeSet::new();
        for &ord in &indices {
            if ord == 0 {
                return Err(invalid("`index` is 1-based; use 1 or greater"));
            }
            if !seen.insert(ord) {
                return Err(invalid(format!("duplicate `index` {ord}")));
            }
        }
        (
            TasksChecklistCheckParams {
                command_id: next_id(),
                task,
                step_id: String::new(),
                section: Some(section),
                indices,
                expected_revision: Some(require_rev(expected_revision)?),
                done,
            },
            Vec::new(),
        )
    };
    Ok(PreparedRpc {
        method: TasksChecklistCheck::METHOD,
        params: json!(params),
        shape: OutputShape::Checklist {
            fallback_ids,
            removed: false,
        },
    })
}

fn prep_edit(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let task = require_str(input, "task")?.to_owned();
    let text = opt_string(input, "text");
    let required = parse_required_override(input)?;
    if text.is_none() && required.is_none() {
        return Err(invalid(
            "`checklist edit` requires `text`, `required`, or `optional`",
        ));
    }
    let (params, fallback_ids) = match parse_single_target(input, expected_revision)? {
        SingleTarget::StepId {
            step_id,
            expected_revision,
        } => (
            TasksChecklistEditParams {
                command_id: next_id(),
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
                command_id: next_id(),
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
    Ok(PreparedRpc {
        method: TasksChecklistEdit::METHOD,
        params: json!(params),
        shape: OutputShape::Checklist {
            fallback_ids,
            removed: false,
        },
    })
}

fn prep_remove(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let task = require_str(input, "task")?.to_owned();
    let (params, fallback_ids) = match parse_single_target(input, expected_revision)? {
        SingleTarget::StepId {
            step_id,
            expected_revision,
        } => (
            TasksChecklistRemoveParams {
                command_id: next_id(),
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
                command_id: next_id(),
                task,
                step_id: String::new(),
                section: Some(section),
                index: Some(index),
                expected_revision: Some(expected_revision),
            },
            Vec::new(),
        ),
    };
    Ok(PreparedRpc {
        method: TasksChecklistRemove::METHOD,
        params: json!(params),
        shape: OutputShape::Checklist {
            fallback_ids,
            removed: true,
        },
    })
}

fn prep_move(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let task = require_str(input, "task")?.to_owned();
    let to_section = opt_section(input, "to_section")?;
    let position = opt_u64(input, "position")?.ok_or_else(|| invalid("`position` is required"))?;
    let (params, fallback_ids) = match parse_single_target(input, expected_revision)? {
        SingleTarget::StepId {
            step_id,
            expected_revision,
        } => (
            TasksChecklistMoveParams {
                command_id: next_id(),
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
                command_id: next_id(),
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
    Ok(PreparedRpc {
        method: TasksChecklistMove::METHOD,
        params: json!(params),
        shape: OutputShape::Checklist {
            fallback_ids,
            removed: false,
        },
    })
}
