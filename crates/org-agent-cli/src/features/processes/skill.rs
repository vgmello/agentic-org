//! `skill list|read`: the built-in `agent-os` guides, read on demand.

use org_command::{Answer, Command, Example, Field, Kind, Refusal, Request, Type};
use org_proto::processes::{
    SkillCategoryWire, SkillList, SkillListFilterParams, SkillListResult, SkillRead,
    SkillReadParams, SkillReadResult,
};
use org_proto::{Call, ErrorCode};
use serde_json::Value;

use super::{unreadable, READ_ERRORS};
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{encode, opt_str, require_str};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

const SKILL_CATEGORY_TYPE: Type = Type::Enum(&["workflow", "product_guide", "chief_of_staff"]);

const SKILL_LIST_INPUT: &[Field] = &[Field::optional(
    "category",
    SKILL_CATEGORY_TYPE,
    "Filter by skill category (workflow, product_guide, or chief_of_staff)",
)];

const SKILL_SUMMARY_FIELDS: &[Field] = &[
    Field::required("key", Type::String, "Skill key"),
    Field::required("title", Type::String, "Skill title"),
    Field::required("description", Type::String, "One-line description"),
    Field::required("source", Type::String, "Origin (built_in)"),
    Field::optional("category", SKILL_CATEGORY_TYPE, "Skill category"),
];

const SKILL_READ_INPUT: &[Field] = &[Field::required(
    "name",
    Type::String,
    "Skill key from `skill list`",
)];

const SKILL_READ_OUTPUT: &[Field] = &[
    Field::required("key", Type::String, "Skill key"),
    Field::required("title", Type::String, "Skill title"),
    Field::required("description", Type::String, "One-line description"),
    Field::required("source", Type::String, "Origin (built_in)"),
    Field::optional("category", SKILL_CATEGORY_TYPE, "Skill category"),
    Field::required("body_markdown", Type::String, "Full Markdown guidance"),
];

/// The `skill` rows.
pub(super) const COMMANDS: &[Command] = &[
    Command {
        resource: "skill",
        verb: "list",
        summary: "List built-in skills",
        call: "skill.list",
        kind: Kind::List,
        input: SKILL_LIST_INPUT,
        content_field: None,
        output: SKILL_SUMMARY_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"key":"agent-os-basics","title":"Using agent-os","description":"Grammar, envelopes, and session attach.","source":"built_in","category":"workflow"}]"#,
        },
    },
    Command {
        resource: "skill",
        verb: "read",
        summary: "Read built-in skill",
        call: "skill.read",
        kind: Kind::Read,
        input: SKILL_READ_INPUT,
        content_field: None,
        output: SKILL_READ_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"name":"agent-os-basics"}"#,
            output: r##"{"key":"agent-os-basics","title":"Using agent-os","description":"Grammar, envelopes, and session attach.","source":"built_in","category":"workflow","body_markdown":"# Using agent-os"}"##,
        },
    },
];

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
