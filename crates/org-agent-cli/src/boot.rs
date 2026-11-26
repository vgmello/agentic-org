//! `session boot` (shorthands `boot`, `instructions overview`): everything an agent needs before
//! its first command, in one envelope (`doc-28`, `doc-96` AX).
//!
//! - **Offline** (`--offline`, or a shell with no office binding): the standing rules
//!   (`org-process`'s `core-rules.md`), the grouped catalogue of the live table and the index of
//!   the built-in guides, from this build alone: no office is asked, and the envelope stays
//!   within [`OFFLINE_LIMIT`].
//! - **Bound** (a session token or binding file is set and `--offline` is not given): the same,
//!   with who the caller is and the must-follow (non-overridable) processes and standards that
//!   apply, whole, within [`MANDATORY_LIMIT`]. A binding that does not work fails the command:
//!   it never falls back to offline.

use org_command::{Answer, Refusal, Request};
use org_proto::directory::{DirectoryCard, WhoResult};
use org_proto::org::MemberKind;
use org_proto::processes::{
    DocumentKindWire, EffectiveProcessEntryWire, ProcessEffective, ProcessEffectiveParams,
    ProcessEffectiveResult, ProcessForWire,
};
use org_proto::sessions::SessionInspectResult;
use org_proto::{Call, ErrorCode};
use serde_json::{json, Map, Value};

use crate::transport::OfficeTransport;

mod skills;

pub use skills::SKILL_INDEX;

/// The standing rules of every agent: the same file `org-process` builds into a managed
/// session's standing instructions.
pub const CORE_RULES: &str = include_str!("../../org-process/prompts/core-rules.md");

/// The most bytes of must-follow text a bound boot carries. Documents are whole or left out,
/// never cut: those that do not fit are named in `mandatory_omitted`.
pub const MANDATORY_LIMIT: usize = 8 * 1024;

/// The most bytes an offline boot envelope may take, in any format.
pub const OFFLINE_LIMIT: usize = 6 * 1024;

fn opt_str<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request.input.get(name).and_then(Value::as_str)
}

fn unreadable(method: &str) -> Refusal {
    Refusal::new(
        ErrorCode::Unavailable,
        format!("The office returned an unreadable `{method}` answer."),
        "Retry `agent-os boot`; if it persists, the office and agent-os versions differ.",
    )
}

/// Answers `session boot`.
pub(crate) fn read_session_boot<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let offline = request
        .input
        .get("offline")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let project = opt_str(request, "project");
    let bound = transport.is_bound();
    if bound && !offline {
        return bound_boot(transport, project).map(Answer::completed);
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
    let mut data = common("offline");
    data.insert("next_action".into(), json!(next_action));
    Ok(Answer::completed(Value::Object(data)))
}

/// What every boot answers: its mode, the rules, the catalogue and the guides.
fn common(mode: &str) -> Map<String, Value> {
    let skills: Vec<Value> = SKILL_INDEX
        .iter()
        .map(|(key, description)| json!({"key": key, "description": description}))
        .collect();
    let mut data = Map::new();
    data.insert("mode".into(), json!(mode));
    data.insert("rules".into(), json!(CORE_RULES.trim_end()));
    data.insert(
        "catalogue".into(),
        json!(crate::table::runtime_table().catalogue().trim_end()),
    );
    data.insert("skills".into(), Value::Array(skills));
    data
}

fn bound_boot<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    project: Option<&str>,
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
    let mut data = common("bound");
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

/// The caller's directory card: by the employee the binding names, or the one its session runs.
fn caller_card<T: OfficeTransport + ?Sized>(transport: &mut T) -> Result<DirectoryCard, Refusal> {
    let member = match transport.caller_member_id() {
        Some(member) => member,
        None => {
            let raw = transport.call_method("member.session_inspect", json!({}))?;
            let inspected: SessionInspectResult =
                serde_json::from_value(raw).map_err(|_| unreadable("member.session_inspect"))?;
            inspected.session.employee_id
        }
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
        let size = doc.title.len() + doc.body_markdown.len();
        if omitted.is_empty() && used + size <= MANDATORY_LIMIT {
            used += size;
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
