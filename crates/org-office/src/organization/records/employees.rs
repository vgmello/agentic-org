//! Employee and role records.

use org_store::{DurablePath, Id, StoreError};
use serde_json::json;

use super::teams::team_folder;
use super::yaml::Fields;
use super::yaml::RecordError;
use super::{
    checked, id, is_handle, line, optional_id, optional_line, optional_time, path, yaml,
    BuiltinKey, Meta,
};
use super::{EmployeeKind, EmployeeStatus};

// ---- teams/.../people/<id>/profile.md and teams/.../agents/<id>/profile.md ----

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Employee {
    pub id: Id,
    pub handle: String,
    pub kind: EmployeeKind,
    pub name: String,
    pub home_team_id: Id,
    pub role_id: Option<Id>,
    pub reports_to_employee_id: Option<Id>,
    pub status: EmployeeStatus,
    pub contact: Option<String>,
    pub title: Option<String>,
    pub focus: Option<String>,
    pub role_text: Option<String>,
    pub title_in_team: Option<String>,
    pub offboarded_at: Option<String>,
    /// Hiring power: on for a person's new record, off for an agent's.
    pub may_hire: bool,
    /// Set only on a built-in employee: the HR lead or the Chief of Staff.
    pub builtin_key: Option<BuiltinKey>,
    pub meta: Meta,
    /// The Markdown after the front matter.
    pub body: String,
}

/// The profile of an employee whose home team is at the end of `chain`.
pub fn employee_path(
    chain: &[Id],
    kind: EmployeeKind,
    employee: &Id,
) -> Result<DurablePath, StoreError> {
    if chain.is_empty() {
        return Err(StoreError::invalid("an employee has a home team"));
    }
    let (folder, kind) = (team_folder(chain), kind.folder());
    DurablePath::parse(&format!("{folder}/{kind}/{employee}/profile.md"))
}

pub fn encode_employee(record: &Employee) -> Vec<u8> {
    let id = |id: &Option<Id>| json!(id.as_ref().map(Id::as_str));
    let mut fields = Fields::new();
    record.meta.write(&mut fields);
    fields.insert("id".into(), json!(record.id.as_str()));
    fields.insert("handle".into(), json!(record.handle));
    fields.insert("kind".into(), json!(record.kind.as_str()));
    fields.insert("name".into(), json!(record.name));
    fields.insert("home_team_id".into(), json!(record.home_team_id.as_str()));
    fields.insert("role_id".into(), id(&record.role_id));
    let reports_to = id(&record.reports_to_employee_id);
    fields.insert("reports_to_employee_id".into(), reports_to);
    fields.insert("status".into(), json!(record.status.as_str()));
    fields.insert("contact".into(), json!(record.contact));
    fields.insert("title".into(), json!(record.title));
    fields.insert("focus".into(), json!(record.focus));
    fields.insert("role_text".into(), json!(record.role_text));
    fields.insert("title_in_team".into(), json!(record.title_in_team));
    fields.insert("offboarded_at".into(), json!(record.offboarded_at));
    fields.insert("may_hire".into(), json!(record.may_hire));
    let builtin = record.builtin_key.map(BuiltinKey::as_str);
    fields.insert("builtin_key".into(), json!(builtin));
    yaml::encode_document(&fields, &record.body)
}

pub fn decode_employee(bytes: &[u8]) -> Result<Employee, RecordError> {
    let (mut reader, body) = yaml::decode_document(bytes)?;
    let record = Employee {
        meta: Meta::read(&mut reader)?,
        id: id(&mut reader, "id")?,
        handle: checked(&mut reader, "handle", is_handle, "not a handle")?,
        kind: EmployeeKind::read(&mut reader, "kind")?,
        name: line(&mut reader, "name")?,
        home_team_id: id(&mut reader, "home_team_id")?,
        role_id: optional_id(&mut reader, "role_id")?,
        reports_to_employee_id: optional_id(&mut reader, "reports_to_employee_id")?,
        status: EmployeeStatus::read(&mut reader, "status")?,
        contact: optional_line(&mut reader, "contact")?,
        title: optional_line(&mut reader, "title")?,
        focus: optional_line(&mut reader, "focus")?,
        role_text: optional_line(&mut reader, "role_text")?,
        title_in_team: optional_line(&mut reader, "title_in_team")?,
        offboarded_at: optional_time(&mut reader, "offboarded_at")?,
        may_hire: reader.flag("may_hire")?,
        builtin_key: BuiltinKey::read_optional(&mut reader, "builtin_key")?,
        body,
    };
    let builtin_ok = record.builtin_key.is_none_or(|key| {
        record.kind == EmployeeKind::Agent && (key.may_hold_hiring() || !record.may_hire)
    });
    if !builtin_ok {
        let reason = "only an agent, and the HR lead without may_hire";
        return Err(reader.error("builtin_key", reason));
    }
    if record.status == EmployeeStatus::Paused && record.kind != EmployeeKind::Agent {
        return Err(reader.error("status", "only an agent is paused"));
    }
    if record.offboarded_at.is_some() && record.status != EmployeeStatus::Inactive {
        return Err(reader.error("offboarded_at", "set only when inactive"));
    }
    if record.reports_to_employee_id.as_ref() == Some(&record.id) {
        return Err(reader.error("reports_to_employee_id", "nobody reports to themselves"));
    }
    reader.finish()?;
    Ok(record)
}

// ---- roles/<role-id>.md ----

/// A role of the organization (document 01). Read-only in v1: no command writes one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Role {
    pub id: Id,
    pub key: Option<String>,
    pub name: String,
    pub meta: Meta,
    /// The Markdown after the front matter.
    pub body: String,
}

pub fn role_path(role: &Id) -> DurablePath {
    path(&format!("state/roles/{role}.md"))
}

pub fn encode_role(record: &Role) -> Vec<u8> {
    let mut fields = Fields::new();
    record.meta.write(&mut fields);
    fields.insert("id".into(), json!(record.id.as_str()));
    fields.insert("key".into(), json!(record.key));
    fields.insert("name".into(), json!(record.name));
    yaml::encode_document(&fields, &record.body)
}

pub fn decode_role(bytes: &[u8]) -> Result<Role, RecordError> {
    let (mut reader, body) = yaml::decode_document(bytes)?;
    let record = Role {
        meta: Meta::read(&mut reader)?,
        id: id(&mut reader, "id")?,
        key: optional_line(&mut reader, "key")?,
        name: line(&mut reader, "name")?,
        body,
    };
    reader.finish()?;
    Ok(record)
}
