//! `session boot` (shorthands `boot`, `instructions overview`): everything an agent needs before
//! its first command, in one envelope (`doc-28`, `doc-96` AX).
//!
//! - **Offline** (`--offline`, or a shell with no office binding): the standing rules
//!   (`org-process`'s `core-rules.md`), the grouped catalogue of the live table and the index of
//!   the built-in guides, from this build alone: no office is asked, and the envelope stays
//!   within 6 KiB (its tests hold it there).
//! - **Bound** (a session token or binding file is set and `--offline` is not given): the same,
//!   with who the caller is and the must-follow (non-overridable) processes and standards that
//!   apply, whole, within [`MANDATORY_LIMIT`]. A binding that does not work fails the command:
//!   it never falls back to offline.

use org_command::{Answer, Command, Example, Field, Kind, Refusal, Request, Type};
use org_proto::directory::{DirectoryCard, WhoResult};
use org_proto::external_auth::{AuthWhoami, AuthWhoamiResult};
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

const BOOT_INPUT: &[Field] = &[
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

const BOOT_OUTPUT: &[Field] = &[
    Field::required(
        "mode",
        Type::Enum(&["offline", "bound"]),
        "offline: this build alone; bound: also asked the office",
    ),
    Field::optional(
        "identity",
        Type::Object(IDENTITY_FIELDS),
        "Who you are (bound only)",
    ),
    Field::required("rules", Type::String, "The standing rules of every agent"),
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

/// The `session boot` row.
pub(super) const SESSION_BOOT: Command = Command {
    resource: "session",
    verb: "boot",
    summary: "Load rules, catalogue, skills and must-follow processes",
    call: "session.boot",
    kind: Kind::Read,
    input: BOOT_INPUT,
    content_field: None,
    output: BOOT_OUTPUT,
    errors: READ_ERRORS,
    example: Example {
        input: r#"{"offline":true}"#,
        output: r###"{"mode":"offline","rules":"# How you work here","catalogue":"## Commands","skills":[{"key":"agent-os-basics","description":"Command grammar."}],"next_action":"Run `agent-os boot` without --offline."}"###,
    },
};

/// The standing rules of every agent: the same file `org-process` builds into a managed
/// session's standing instructions.
pub const CORE_RULES: &str = include_str!("../../../../org-process/prompts/core-rules.md");

/// The most bytes of must-follow text a bound boot carries. Documents are whole or left out,
/// never cut: those that do not fit are named in `mandatory_omitted`.
const MANDATORY_LIMIT: usize = 8 * 1024;

fn unreadable(method: &str) -> Refusal {
    Refusal::new(
        ErrorCode::Unavailable,
        format!("The office returned an unreadable `{method}` answer."),
        "Retry `agent-os boot`; if it persists, the office and agent-os versions differ.",
    )
}

/// Answers `session boot`; `catalogue` is the live table's launch catalogue.
pub(super) fn read_session_boot<T: OfficeTransport + ?Sized>(
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
        return bound_boot(transport, project, catalogue).map(Answer::completed);
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
            "Drop --project, or run `agent-os boot --project <project>` in a bound session.",
        ));
    }
    let next_action = if bound {
        "Run `agent-os boot` without --offline for who you are and your organization's must-follow processes."
    } else {
        "This shell has no office binding: these are the built-in rules only. Sign in with `agent-os login --as @<handle>`, or work inside an office-started session, then run `agent-os boot`."
    };
    let mut data = common("offline", catalogue);
    data.insert("next_action".into(), json!(next_action));
    Ok(Answer::completed(Value::Object(data)))
}

/// What every boot answers: its mode, the rules, the catalogue and the guides.
fn common(mode: &str, catalogue: &str) -> Map<String, Value> {
    let skills: Vec<Value> = SKILL_INDEX
        .iter()
        .map(|(key, description)| json!({"key": key, "description": description}))
        .collect();
    let mut data = Map::new();
    data.insert("mode".into(), json!(mode));
    data.insert("rules".into(), json!(CORE_RULES.trim_end()));
    data.insert("catalogue".into(), json!(catalogue.trim_end()));
    data.insert("skills".into(), Value::Array(skills));
    data
}

/// The bound boot briefing (`data` of `agent-os boot`) over `transport`, for every project scope:
/// what `agent-os login` prints once a workspace is signed in (`TASK-137`).
pub(crate) fn bound_briefing<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    catalogue: &str,
) -> Result<Value, Refusal> {
    bound_boot(transport, None, catalogue)
}

fn bound_boot<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    project: Option<&str>,
    catalogue: &str,
) -> Result<Value, Refusal> {
    let card = caller_card(transport)?;
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
    let mut data = common("bound", catalogue);
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

/// The caller's directory card: by the employee the binding names, else the member the office
/// bound the connection to (`auth.whoami`: an agent, or a person through a grant), else the
/// employee whose session the connection runs (`member.session_inspect`, agents only).
fn caller_card<T: OfficeTransport + ?Sized>(transport: &mut T) -> Result<DirectoryCard, Refusal> {
    let member = match transport.caller_member_id() {
        Some(member) => member,
        None => bound_member(transport)?,
    };
    let raw = transport.call_method("who", json!({ "member": member }))?;
    let who: WhoResult = serde_json::from_value(raw).map_err(|_| unreadable("who"))?;
    who.members.into_iter().next().ok_or_else(|| {
        Refusal::new(
            ErrorCode::NotFound,
            format!("The office has no directory card for `{member}`."),
            "Check that your session is still active with `agent-os session inspect`.",
        )
    })
}

fn bound_member<T: OfficeTransport + ?Sized>(transport: &mut T) -> Result<String, Refusal> {
    match transport.call_method(AuthWhoami::METHOD, json!({})) {
        Ok(raw) => serde_json::from_value::<AuthWhoamiResult>(raw)
            .map(|who| who.member_id)
            .map_err(|_| unreadable(AuthWhoami::METHOD)),
        // An office without external agents, or a connection the office has not bound to a
        // member (a session-token-only run): ask which session's employee this is.
        Err(refusal) if matches!(refusal.code, ErrorCode::Unsupported | ErrorCode::NotBound) => {
            let raw = transport.call_method("member.session_inspect", json!({}))?;
            let inspected: SessionInspectResult =
                serde_json::from_value(raw).map_err(|_| unreadable("member.session_inspect"))?;
            Ok(inspected.session.employee_id)
        }
        Err(refusal) => Err(refusal),
    }
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
