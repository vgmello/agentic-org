//! `skill list|read`: the built-in `agent-os` guides, read on demand.

use org_command::{Answer, Refusal, Request};
use org_proto::processes::{
    SkillCategoryWire, SkillList, SkillListFilterParams, SkillListResult, SkillRead,
    SkillReadParams, SkillReadResult,
};
use org_proto::{Call, ErrorCode};
use serde_json::Value;

use super::unreadable;
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{encode, opt_str, require_str};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

/// `Use when:` guidance of each `skill` command.
pub(super) const CUES: &[CommandCue] = &[
    (
        "skill",
        "list",
        "discovering built-in `agent-os` operational guides available to load on demand.",
    ),
    (
        "skill",
        "read",
        "reading the full Markdown instructions of a specific built-in skill by `--name`.",
    ),
];

/// Runs a `skill` command.
pub(super) fn dispatch<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    Some(match request.command.verb {
        "list" => read_skill_list(transport, request),
        "read" => read_skill_read(transport, request),
        _ => return None,
    })
}

fn read_skill_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let category = match opt_str(request, "category") {
        None => None,
        Some(raw) => match SkillCategoryWire::parse(raw) {
            Some(parsed) => Some(parsed),
            None => {
                return Err(Refusal::new(
                    ErrorCode::InvalidRequest,
                    format!("unknown skill category `{raw}`"),
                    "Pass --category workflow, product_guide, or chief_of_staff.",
                ));
            }
        },
    };
    let raw = transport.call_method(
        SkillList::METHOD,
        encode(&SkillListFilterParams { category }, SkillList::METHOD)?,
    )?;
    let res: SkillListResult =
        serde_json::from_value(raw).map_err(|_| unreadable("skill.list", "skill list"))?;
    let items = match serde_json::to_value(res.skills) {
        Ok(Value::Array(items)) => items,
        _ => Vec::new(),
    };
    paginate(request, items)
}

fn read_skill_read<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let name = require_str(&request.input, "name")?.to_owned();
    let raw = transport.call_method(
        SkillRead::METHOD,
        encode(&SkillReadParams { name }, SkillRead::METHOD)?,
    )?;
    let res: SkillReadResult =
        serde_json::from_value(raw).map_err(|_| unreadable("skill.read", "skill read"))?;
    Ok(Answer::completed(encode(&res, SkillRead::METHOD)?))
}
