//! `session context` (shorthand `context`): everything an agent needs before its first command
//! or after context compaction, in one envelope (`doc-28`, `doc-96` AX, `TASK-248`).
//!
//! - **Offline** (`--offline`, or a shell with no office binding): the standing rules, the grouped
//!   catalogue of the live table and the index of the built-in guides, from this build alone: no
//!   office is asked, and the envelope stays within 6 KiB (its tests hold it there).
//! - **Bound** (`mode: managed` or `mode: external`, when a session token or workspace binding is
//!   set and `--offline` is not given): the same, with mode-truthful standing rules (`CORE_RULES`
//!   for managed sessions, `EXTERNAL_RULES` for external workspaces), who the caller is and the
//!   must-follow (non-overridable) processes and standards that apply, whole, within
//!   [`MANDATORY_LIMIT`]. A binding that does not work fails the command: it never falls back to
//!   offline.

use org_command::{Answer, Command, Example, Field, Kind, Refusal, Request, Type};
use org_proto::directory::{DirectoryCard, WhoResult};
use org_proto::external_auth::{AuthWhoami, AuthWhoamiResult, SessionMode};
use org_proto::org::MemberKind;
use org_proto::processes::{
    DocumentKindWire, EffectiveProcessEntryWire, ProcessEffective, ProcessEffectiveParams,
    ProcessEffectiveResult, ProcessForWire,
};
use org_proto::sessions::SessionInspectResult;
use org_proto::{Call, ErrorCode};
use serde_json::{json, Map, Value};

use super::skills::SKILL_INDEX;
use super::{READ_ERRORS, STRING_TYPE};
use crate::runtime::input::opt_str;
use crate::runtime::transport::OfficeTransport;

const CONTEXT_INPUT: &[Field] = &[
    Field::optional(
        "project",
        Type::String,
        "Also the must-follow processes of this project (UUIDv7, P-<number> key or slug)",
    ),
    Field::optional(
        "offline",
        Type::Boolean,
        "Answer from this build alone, without the office: rules, catalogue and skills",
    ),
];

const IDENTITY_TEAM_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Team ID"),
    Field::required("name", Type::String, "Team name"),
    Field::optional("title_in_team", Type::String, "Title in the team"),
];

const IDENTITY_TEAM_TYPE: Type = Type::Object(IDENTITY_TEAM_FIELDS);

const IDENTITY_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Your employee ID"),
    Field::required("name", Type::String, "Your display name"),
    Field::optional("handle", Type::String, "Your handle (@handle)"),
    Field::required(
        "kind",
        Type::Enum(&["person", "agent", "unknown"]),
        "Employee kind",
    ),
    Field::optional("role_title", Type::String, "Your role title"),
    Field::required(
        "teams",
        Type::List(&IDENTITY_TEAM_TYPE),
        "Your teams, home team first",
    ),
];

const SKILL_INDEX_FIELDS: &[Field] = &[
    Field::required("key", Type::String, "Pass to `skill read --name`"),
    Field::required("description", Type::String, "What the guide covers"),
];

const SKILL_INDEX_TYPE: Type = Type::Object(SKILL_INDEX_FIELDS);

const MANDATORY_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Document ID"),
    Field::optional("key", Type::String, "Document key"),
    Field::required(
        "kind",
        Type::Enum(&["process", "standard"]),
        "Process or standard",
    ),
    Field::required(
        "level",
        Type::String,
        "Where it is set: organization, team or project",
    ),
    Field::optional(
        "scope_name",
        Type::String,
        "The team or project it is set for",
    ),
    Field::required("title", Type::String, "Title"),
    Field::required("revision", Type::Integer, "Document revision"),
    Field::required("body", Type::String, "The whole Markdown text, never cut"),
];

const MANDATORY_TYPE: Type = Type::Object(MANDATORY_FIELDS);

const CONTEXT_OUTPUT: &[Field] = &[
    Field::required(
        "mode",
        Type::Enum(&["offline", "managed", "external"]),
        "offline: this build alone; managed: office-started session; external: bound external workspace",
    ),
    Field::optional(
        "identity",
        Type::Object(IDENTITY_FIELDS),
        "Who you are (bound only)",
    ),
    Field::required(
        "rules",
        Type::String,
        "The standing rules for your session mode",
    ),
    Field::required(
        "catalogue",
        Type::String,
        "Every command, grouped by resource",
    ),
    Field::required(
        "skills",
        Type::List(&SKILL_INDEX_TYPE),
        "Built-in guides: read one with `skill read --name <key>`",
    ),
    Field::optional(
        "mandatory",
        Type::List(&MANDATORY_TYPE),
        "Must-follow (non-overridable) processes and standards that apply to you, whole (bound only)",
    ),
    Field::optional(
        "mandatory_truncated",
        Type::Boolean,
        "Whether must-follow documents were left out to stay within 8 KiB (bound only)",
    ),
    Field::optional(
        "mandatory_omitted",
        Type::List(&STRING_TYPE),
        "The key (or ID) of each must-follow document left out; read each with `process inspect`",
    ),
    Field::required("next_action", Type::String, "What to do next"),
];

/// The `session context` row.
pub(super) const SESSION_CONTEXT: Command = Command {
    resource: "session",
    verb: "context",
    summary: "Load rules, catalogue, skills and must-follow processes",
    call: "session.context",
    kind: Kind::Read,
    input: CONTEXT_INPUT,
    content_field: None,
    output: CONTEXT_OUTPUT,
    errors: READ_ERRORS,
    example: Example {
        input: r#"{"offline":true}"#,
        output: r###"{"mode":"offline","rules":"# How you work here","catalogue":"## Commands","skills":[{"key":"agent-os-basics","description":"Command grammar."}],"next_action":"Run `agent-os context` without --offline."}"###,
    },
};

/// The standing rules of every managed agent: the same file `org-process` builds into a managed
/// session's standing instructions.
pub const CORE_RULES: &str = include_str!("../../../../org-process/prompts/core-rules.md");

/// The standing rules of an external workspace agent (`mode: external`): omits managed-only wake
/// and process-reaping rules.
pub const EXTERNAL_RULES: &str = include_str!("../../../../org-process/prompts/external-rules.md");

/// The most bytes of must-follow text a bound context carries. Documents are whole or left out,
/// never cut: those that do not fit are named in `mandatory_omitted`.
const MANDATORY_LIMIT: usize = 8 * 1024;

fn unreadable(method: &str) -> Refusal {
    Refusal::new(
        ErrorCode::Unavailable,
        format!("The office returned an unreadable `{method}` answer."),
        "Retry `agent-os context`; if it persists, the office and agent-os versions differ.",
    )
}

/// Answers `session context`; `catalogue` is the live table's launch catalogue.
pub(super) fn read_session_context<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    catalogue: &str,
) -> Result<Answer, Refusal> {
    let offline = request
        .input
        .get("offline")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let project = opt_str(request, "project");
    let bound = transport.is_bound();
    if bound && !offline {
        return bound_context(transport, project, catalogue).map(Answer::completed);
    }
    if project.is_some() {
        let why = if offline {
            "--project needs the office, and --offline answers without it."
        } else {
            "--project needs the office, and this shell has no office binding."
        };
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            why,
            "Drop --project, or run `agent-os context --project <project>` in a bound session.",
        ));
    }
    let next_action = if bound {
        "Run `agent-os context` without --offline for who you are and your organization's must-follow processes."
    } else {
        "This shell has no office binding: these are the built-in rules only. Sign in with `agent-os login --as @<handle>` (which returns your context automatically), or work inside an office-started session."
    };
    let rules = rules_for_mode(transport.session_mode());
    let mut data = common("offline", rules, catalogue);
    data.insert("next_action".into(), json!(next_action));
    Ok(Answer::completed(Value::Object(data)))
}

fn rules_for_mode(mode: Option<SessionMode>) -> &'static str {
    match mode {
        Some(SessionMode::External) => EXTERNAL_RULES,
        _ => CORE_RULES,
    }
}

fn mode_name(mode: SessionMode) -> &'static str {
    match mode {
        SessionMode::External => "external",
        _ => "managed",
    }
}

/// What every context answers: its mode, the mode-appropriate rules, the catalogue and the guides.
fn common(mode: &str, rules: &str, catalogue: &str) -> Map<String, Value> {
    let skills: Vec<Value> = SKILL_INDEX
        .iter()
        .map(|(key, description)| json!({"key": key, "description": description}))
        .collect();
    let mut data = Map::new();
    data.insert("mode".into(), json!(mode));
    data.insert("rules".into(), json!(rules.trim_end()));
    data.insert("catalogue".into(), json!(catalogue.trim_end()));
    data.insert("skills".into(), Value::Array(skills));
    data
}

/// The bound context briefing (`data` of `agent-os context`, including `catalogue`) over
/// `transport`: what `agent-os login` returns verbatim under `data.context` (`TASK-248`).
pub(crate) fn bound_briefing<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    catalogue: &str,
) -> Result<Value, Refusal> {
    bound_context(transport, None, catalogue)
}

fn bound_context<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    project: Option<&str>,
    catalogue: &str,
) -> Result<Value, Refusal> {
    let (card, mode) = caller_card_and_mode(transport)?;
    let params = ProcessEffectiveParams {
        employee_id: None,
        r#for: project.map(|project_id| ProcessForWire::Project {
            project_id: project_id.to_owned(),
        }),
        kind: None,
    };
    let params = serde_json::to_value(params).map_err(|_| unreadable(ProcessEffective::METHOD))?;
    let raw = transport.call_method(ProcessEffective::METHOD, params)?;
    let effective: ProcessEffectiveResult =
        serde_json::from_value(raw).map_err(|_| unreadable(ProcessEffective::METHOD))?;
    let must_follow = effective
        .entries
        .iter()
        .filter(|entry| entry.applies && entry.mandatory);
    let (mandatory, omitted) = pack_mandatory(must_follow);
    let mut data = common(mode_name(mode), rules_for_mode(Some(mode)), catalogue);
    data.insert("identity".into(), identity(&card));
    data.insert("mandatory".into(), Value::Array(mandatory));
    data.insert("mandatory_truncated".into(), json!(!omitted.is_empty()));
    // The same scope `process inspect` resolves the left-out documents in.
    let scope = project.map_or_else(String::new, |project| format!(" --project {project}"));
    let next_action = match omitted.first() {
        Some((flag, name)) => format!(
            "Must-follow documents over the 8 KiB limit were left out whole, never cut: read each in mandatory_omitted before you act, starting with `agent-os process inspect {flag} {name}{scope}`."
        ),
        None => "Follow the must-follow processes above. Before a kind of work you have not done here, read its guide with `agent-os skill read --name <key>`.".to_owned(),
    };
    if !omitted.is_empty() {
        let names: Vec<&str> = omitted.iter().map(|(_, name)| name.as_str()).collect();
        data.insert("mandatory_omitted".into(), json!(names));
    }
    data.insert("next_action".into(), json!(next_action));
    Ok(Value::Object(data))
}

/// The caller's directory card and session mode (`managed` or `external`).
fn caller_card_and_mode<T: OfficeTransport + ?Sized>(
    transport: &mut T,
) -> Result<(DirectoryCard, SessionMode), Refusal> {
    let (member, mode) = resolve_caller(transport)?;
    let raw = transport.call_method("who", json!({ "member": member }))?;
    let who: WhoResult = serde_json::from_value(raw).map_err(|_| unreadable("who"))?;
    let card = who.members.into_iter().next().ok_or_else(|| {
        Refusal::new(
            ErrorCode::NotFound,
            format!("The office has no directory card for `{member}`."),
            "Check that your session is still active with `agent-os session inspect`.",
        )
    })?;
    Ok((card, mode))
}

/// Resolves the caller's member ID and session mode. A workspace binding already knows both
/// locally; otherwise `auth.whoami` tells both (falling back to `member.session_inspect` only on
/// an older office that refuses `auth.whoami` with `Unsupported` or `NotBound`).
fn resolve_caller<T: OfficeTransport + ?Sized>(
    transport: &mut T,
) -> Result<(String, SessionMode), Refusal> {
    if let (Some(member), Some(mode)) = (transport.caller_member_id(), transport.session_mode()) {
        return Ok((member, mode));
    }
    match transport.call_method(AuthWhoami::METHOD, json!({})) {
        Ok(raw) => {
            let who: AuthWhoamiResult =
                serde_json::from_value(raw).map_err(|_| unreadable(AuthWhoami::METHOD))?;
            let member = transport.caller_member_id().unwrap_or(who.member_id);
            Ok((member, who.mode))
        }
        Err(refusal) if matches!(refusal.code, ErrorCode::Unsupported | ErrorCode::NotBound) => {
            let member = match transport.caller_member_id() {
                Some(member) => member,
                None => session_inspect_member(transport)?,
            };
            let mode = transport.session_mode().unwrap_or(SessionMode::Managed);
            Ok((member, mode))
        }
        Err(refusal) => Err(refusal),
    }
}

fn session_inspect_member<T: OfficeTransport + ?Sized>(
    transport: &mut T,
) -> Result<String, Refusal> {
    let raw = transport.call_method("member.session_inspect", json!({}))?;
    let inspected: SessionInspectResult =
        serde_json::from_value(raw).map_err(|_| unreadable("member.session_inspect"))?;
    Ok(inspected.session.employee_id)
}

fn identity(card: &DirectoryCard) -> Value {
    let kind = match card.kind {
        MemberKind::Person => "person",
        MemberKind::Agent => "agent",
        _ => "unknown",
    };
    let teams: Vec<Value> = card
        .teams
        .iter()
        .map(|team| {
            let mut out = Map::new();
            out.insert("id".into(), json!(team.id));
            out.insert("name".into(), json!(team.name));
            if let Some(title) = &team.title_in_team {
                out.insert("title_in_team".into(), json!(title));
            }
            Value::Object(out)
        })
        .collect();
    let mut out = Map::new();
    out.insert("id".into(), json!(card.id));
    out.insert("name".into(), json!(card.name));
    if let Some(handle) = &card.handle {
        out.insert("handle".into(), json!(handle));
    }
    out.insert("kind".into(), json!(kind));
    if let Some(role) = &card.role_title {
        out.insert("role_title".into(), json!(role));
    }
    out.insert("teams".into(), Value::Array(teams));
    Value::Object(out)
}

/// The must-follow documents, in the office's order (organization, then team, then project),
/// each whole, while their text stays within [`MANDATORY_LIMIT`]. The first that does not fit
/// and every one after it are left out, so the order of authority is never broken; each left
/// out is answered as the `process inspect` flag and value that read it (`--key`, or `--id`
/// for a document without a key).
fn pack_mandatory<'a>(
    entries: impl Iterator<Item = &'a EffectiveProcessEntryWire>,
) -> (Vec<Value>, Vec<(&'static str, String)>) {
    let mut used = 0usize;
    let mut kept = Vec::new();
    let mut omitted = Vec::new();
    for entry in entries {
        let doc = &entry.document;
        let size = doc.title.len().saturating_add(doc.body_markdown.len());
        let total = used.saturating_add(size);
        if omitted.is_empty() && total <= MANDATORY_LIMIT {
            used = total;
            kept.push(document(entry));
        } else {
            omitted.push(match &doc.key {
                Some(key) => ("--key", key.clone()),
                None => ("--id", doc.id.clone()),
            });
        }
    }
    (kept, omitted)
}

fn document(entry: &EffectiveProcessEntryWire) -> Value {
    let doc = &entry.document;
    let kind = match doc.kind {
        DocumentKindWire::Standard => "standard",
        _ => "process",
    };
    let mut out = Map::new();
    out.insert("id".into(), json!(doc.id));
    if let Some(key) = &doc.key {
        out.insert("key".into(), json!(key));
    }
    out.insert("kind".into(), json!(kind));
    out.insert("level".into(), json!(doc.level));
    if let Some(scope) = &doc.scope_name {
        out.insert("scope_name".into(), json!(scope));
    }
    out.insert("title".into(), json!(doc.title));
    out.insert("revision".into(), json!(doc.revision));
    out.insert("body".into(), json!(doc.body_markdown));
    Value::Object(out)
}

#[cfg(test)]
mod tests;
