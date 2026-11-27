//! `skill list|read`: operational `agent-os` guides, read on demand.
//!
//! Bound calls ask the live office (`skill.list` / `skill.read`), returning both built-in product
//! guides and any dynamic organization/project skills. Passing `--offline` answers locally from
//! `org_process` (single source of truth, zero office RPCs) and declares partial scope
//! (`mode: "offline_builtin_only"`, `org_skills_included: false`, and an explicit `caveat`).

use org_command::{Answer, Refusal, Request};
use org_process::{find_builtin_skill, list_builtin_skills_by_category, SkillCategory};
use org_proto::processes::{
    SkillCategoryWire, SkillList, SkillListFilterParams, SkillListResult, SkillRead,
    SkillReadParams, SkillReadResult,
};
use org_proto::{Call, ErrorCode};
use serde_json::{json, Value};

use super::unreadable;
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{encode, is_set, opt_str, require_str};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

/// Explicit partial-scope mode reported by `agent-os skill list|read --offline` (`TASK-223`).
pub(crate) const OFFLINE_SKILL_MODE: &str = "offline_builtin_only";

/// Non-ambiguous caveat warning that offline responses omit dynamic organization/project skills.
pub(crate) const OFFLINE_SKILL_CAVEAT: &str = "Built-in product skills only (offline mode). Dynamic organization and project skills are only visible when bound to an office (`agent-os login --as @<handle>`).";

/// `Use when:` guidance of each `skill` command.
pub(super) const CUES: &[CommandCue] = &[
    (
        "skill",
        "list",
        "discovering available `agent-os` operational guides on demand (pass `--offline` for built-in guides only without an office binding; dynamic org/project skills require a bound session).",
    ),
    (
        "skill",
        "read",
        "reading the full Markdown instructions of a specific skill by `--name` (pass `--offline` for built-in guides only without an office binding).",
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
    let category = parse_category(request)?;
    if is_set(request, "offline") {
        return offline_skill_list(request, category);
    }
    if !transport.is_bound() {
        return Err(unbound_skill_refusal("list"));
    }
    let raw = transport
        .call_method(
            SkillList::METHOD,
            encode(&SkillListFilterParams { category }, SkillList::METHOD)?,
        )
        .map_err(|refusal| enrich_auth_refusal(refusal, "list"))?;
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
    if is_set(request, "offline") {
        return offline_skill_read(&name);
    }
    if !transport.is_bound() {
        return Err(unbound_skill_refusal("read"));
    }
    let raw = transport
        .call_method(
            SkillRead::METHOD,
            encode(&SkillReadParams { name }, SkillRead::METHOD)?,
        )
        .map_err(|refusal| enrich_auth_refusal(refusal, "read"))?;
    let res: SkillReadResult =
        serde_json::from_value(raw).map_err(|_| unreadable("skill.read", "skill read"))?;
    Ok(Answer::completed(encode(&res, SkillRead::METHOD)?))
}

fn parse_category(request: &Request) -> Result<Option<SkillCategoryWire>, Refusal> {
    match opt_str(request, "category") {
        None => Ok(None),
        Some(raw) => SkillCategoryWire::parse(raw).map(Some).ok_or_else(|| {
            Refusal::new(
                ErrorCode::InvalidRequest,
                format!("unknown skill category `{raw}`"),
                "Pass --category workflow, product_guide, or chief_of_staff.",
            )
        }),
    }
}

fn wire_to_process_category(category: SkillCategoryWire) -> Result<SkillCategory, Refusal> {
    match category {
        SkillCategoryWire::Workflow => Ok(SkillCategory::Workflow),
        SkillCategoryWire::ProductGuide => Ok(SkillCategory::ProductGuide),
        SkillCategoryWire::ChiefOfStaff => Ok(SkillCategory::ChiefOfStaff),
        _ => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "unknown skill category",
            "Pass --category workflow, product_guide, or chief_of_staff.",
        )),
    }
}

fn offline_skill_list(
    request: &Request,
    category: Option<SkillCategoryWire>,
) -> Result<Answer, Refusal> {
    let process_category = category.map(wire_to_process_category).transpose()?;
    let items: Vec<Value> = list_builtin_skills_by_category(process_category)
        .into_iter()
        .map(|skill| {
            json!({
                "key": skill.key,
                "title": skill.title,
                "description": skill.description,
                "source": skill.source,
                "category": skill.category.as_str(),
            })
        })
        .collect();
    paginate(request, items)
}

fn offline_skill_read(name: &str) -> Result<Answer, Refusal> {
    let Some(skill) = find_builtin_skill(name) else {
        return Err(Refusal::new(
            ErrorCode::NotFound,
            format!("built-in skill `{name}` not found in offline mode"),
            "Run `agent-os skill list --offline` to see built-in guides, or sign in with `agent-os login --as @<handle>` and omit `--offline` for dynamic organization/project skills.",
        ));
    };
    Ok(Answer::completed(json!({
        "key": skill.key,
        "title": skill.title,
        "description": skill.description,
        "source": skill.source,
        "category": skill.category.as_str(),
        "body_markdown": skill.body_markdown,
        "mode": OFFLINE_SKILL_MODE,
        "org_skills_included": false,
        "caveat": OFFLINE_SKILL_CAVEAT,
    })))
}

fn unbound_skill_refusal(verb: &str) -> Refusal {
    let offline_example = if verb == "read" {
        "agent-os skill read --offline --name <key>"
    } else {
        "agent-os skill list --offline"
    };
    Refusal::new(
        ErrorCode::NotAuthorized,
        format!("agent-os skill {verb} requires an office binding to return live organization and built-in skills."),
        format!(
            "Pass `--offline` (`{offline_example}`) for built-in guides without an office, or sign in with `agent-os login --as @<handle>` for organization skills."
        ),
    )
}

fn enrich_auth_refusal(mut refusal: Refusal, verb: &str) -> Refusal {
    if matches!(refusal.code, ErrorCode::NotAuthorized | ErrorCode::NotBound)
        && !refusal.hint.contains("--offline")
    {
        let offline_example = if verb == "read" {
            "agent-os skill read --offline --name <key>"
        } else {
            "agent-os skill list --offline"
        };
        if !refusal.hint.is_empty() && !refusal.hint.ends_with(' ') {
            refusal.hint.push(' ');
        }
        refusal.hint.push_str(&format!(
            "Pass `--offline` (`{offline_example}`) for built-in guides without an office, or sign in with `agent-os login --as @<handle>` for organization skills."
        ));
    }
    refusal
}
