//! Direct RPC translators for `doc new|edit|move|delete` apply commands (`TASK-55`).

use org_proto::docs::{
    DocStatusWire, DocsCreateParams, DocsDeleteParams, DocsMoveParams, DocsUpdateParams,
};
use org_proto::ApiError;
use serde_json::{Map, Value};

use super::{invalid, opt_str, opt_string, require_rev, require_str, OutputShape, PreparedRpc};

fn parse_status(raw: Option<&str>) -> Result<Option<DocStatusWire>, ApiError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    match raw {
        "draft" => Ok(Some(DocStatusWire::Draft)),
        "current" => Ok(Some(DocStatusWire::Current)),
        "deprecated" => Ok(Some(DocStatusWire::Deprecated)),
        _ => Err(invalid(format!("unknown doc status `{raw}`"))),
    }
}

pub(super) fn try_prepare(
    next_id: &mut impl FnMut() -> String,
    resource: &str,
    verb: &str,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Option<Result<PreparedRpc, ApiError>> {
    if resource != "doc" {
        return None;
    }
    match verb {
        "new" => Some(prep_doc_new(next_id, input)),
        "edit" => Some(prep_doc_edit(next_id, input, expected_revision)),
        "move" => Some(prep_doc_move(next_id, input, expected_revision)),
        "delete" => Some(prep_doc_delete(next_id, input, expected_revision)),
        _ => None,
    }
}

fn prep_doc_new(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
) -> Result<PreparedRpc, ApiError> {
    let params = DocsCreateParams {
        command_id: next_id(),
        project: require_str(input, "project")?.to_owned(),
        path: opt_string(input, "path"),
        section: opt_string(input, "section"),
        kind: require_str(input, "kind")?.to_owned(),
        title: require_str(input, "title")?.to_owned(),
        summary: opt_string(input, "summary"),
        owner_employee_id: opt_string(input, "owner"),
        status: parse_status(opt_str(input, "status"))?,
        body_markdown: opt_string(input, "body").unwrap_or_default(),
    };
    let val = serde_json::to_value(params).map_err(|e| invalid(e.to_string()))?;
    Ok(PreparedRpc {
        method: "docs.create",
        params: val,
        shape: OutputShape::Passthrough,
    })
}

fn prep_doc_edit(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let params = DocsUpdateParams {
        command_id: next_id(),
        project: require_str(input, "project")?.to_owned(),
        document_id: require_str(input, "document")?.to_owned(),
        expected_revision: require_rev(expected_revision)?,
        title: opt_string(input, "title"),
        kind: opt_string(input, "kind"),
        status: parse_status(opt_str(input, "status"))?,
        summary: opt_string(input, "summary"),
        owner_employee_id: opt_string(input, "owner"),
        body_markdown: opt_string(input, "body"),
    };
    let val = serde_json::to_value(params).map_err(|e| invalid(e.to_string()))?;
    Ok(PreparedRpc {
        method: "docs.update",
        params: val,
        shape: OutputShape::Passthrough,
    })
}

fn prep_doc_move(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let params = DocsMoveParams {
        command_id: next_id(),
        project: require_str(input, "project")?.to_owned(),
        document_id: require_str(input, "document")?.to_owned(),
        expected_revision: require_rev(expected_revision)?,
        new_path: opt_string(input, "new_path"),
        new_section: opt_string(input, "new_section"),
    };
    let val = serde_json::to_value(params).map_err(|e| invalid(e.to_string()))?;
    Ok(PreparedRpc {
        method: "docs.move",
        params: val,
        shape: OutputShape::Passthrough,
    })
}

fn prep_doc_delete(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreparedRpc, ApiError> {
    let params = DocsDeleteParams {
        command_id: next_id(),
        project: require_str(input, "project")?.to_owned(),
        document_id: require_str(input, "document")?.to_owned(),
        expected_revision: require_rev(expected_revision)?,
    };
    let val = serde_json::to_value(params).map_err(|e| invalid(e.to_string()))?;
    Ok(PreparedRpc {
        method: "docs.delete",
        params: val,
        shape: OutputShape::Passthrough,
    })
}
