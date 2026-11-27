//! The organization family's records, where each lives under `durable/state/`, and the
//! codec between a record and its file bytes (data contract documents 01, 04 and 06).
//!
//! A file holds every field of its record, null when unknown, so a record has one field
//! set. Team and employee files do not hold the organization ID: an office store holds one
//! organization.

mod employees;
mod kind;
mod org;
mod teams;

pub use employees::{
    decode_employee, decode_role, employee_path, encode_employee, encode_role, role_path, Employee,
    Role,
};
pub(crate) use kind::path_number;
pub use kind::{kind_of, Kind};
pub use org::{
    ca_escrow_path, ca_escrow_public_key, decode_office, decode_office_codes, decode_organization,
    decode_repositories, encode_office, encode_office_codes, encode_organization,
    encode_repositories, office_codes_path, office_path, organization_path, repositories_path,
    Office, OfficeCode, Organization, Repository, MAX_CA_ESCROW_BYTES,
};
pub(crate) use teams::team_folder;
pub use teams::{
    decode_team, decode_team_allocation, decode_team_counter, encode_team, encode_team_allocation,
    encode_team_counter, team_allocation_path, team_counter_path, team_key, team_path, Team,
    TeamAllocation, TeamCounter, DEFAULT_MAX_MEMBERS,
};

mod hiring;
pub(crate) mod yaml;

use org_store::{DurablePath, Id, Timestamp};
use serde_json::{json, Value};

pub use hiring::{
    agent_execution_path, decode_agent_execution, decode_hire_request, decode_model_profile,
    decode_provider_connection, decode_watching_model, encode_agent_execution, encode_hire_request,
    encode_model_profile, encode_provider_connection, encode_watching_model, hire_request_path,
    model_profile_path, provider_connection_path, watching_model_path, AgentExecutionConfig,
    HireProposal, HireRequest, HireState, ModelProfile, PermissionPreset, ProviderAuthMode,
    ProviderConnection, ReasoningEffort, WatchingModelSetting,
};
pub use yaml::RecordError;
use yaml::{Fields, Reader};

/// The only schema version there is.
const SCHEMA_VERSION: u64 = 1;
/// The longest one-line text field, in characters.
pub const MAX_TEXT_CHARS: usize = 200;
/// The longest remote URL, in bytes.
pub const MAX_URL_BYTES: usize = 2048;
/// The longest handle, in bytes: the limit the office has always held an agent's handle to.
pub const MAX_HANDLE_BYTES: usize = org_proto::org::AGENT_MESSAGING_NAME_MAX_CHARS;
/// Words that address something else, so nobody has them as a handle: `here` is a
/// channel's members (ADR-0024), the others are the owner and the office in messaging.
const RESERVED_HANDLES: [&str; 3] = ["here", org_proto::OWNER_HANDLE, org_proto::OFFICE_HANDLE];

/// Whether `key`, a handle in lowercase, is a word nobody may have. The one list for the
/// records and for the policy that admits a name.
pub fn is_reserved_handle(key: &str) -> bool {
    RESERVED_HANDLES.contains(&key)
}

macro_rules! text_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }

            fn read(reader: &mut Reader, key: &str) -> Result<Self, RecordError> {
                match reader.text(key)?.as_str() {
                    $($text => Ok(Self::$variant),)+
                    _ => Err(reader.error(key, concat!("not one of: ", $($text, " "),+))),
                }
            }
        }
    };
}

text_enum!(RepositoryKind { Remote => "remote", Local => "local" });
text_enum!(OfficeRole { Hq => "hq", Satellite => "satellite" });
text_enum!(TeamStatus { Active => "active", Archived => "archived" });
text_enum!(EmployeeKind { Person => "person", Agent => "agent" });
text_enum!(
    /// One field of state: `paused` is for agents only.
    EmployeeStatus { Active => "active", Paused => "paused", Inactive => "inactive" }
);
/// What makes an employee part of the product (document 04), one agent per organization
/// for each key:
///
/// - `hr`: the built-in HR lead, never with hiring power.
/// - `chief_of_staff`: the built-in Chief of Staff (Tess, `TASK-80.1`), the owner's operator
///   agent. She may hold hiring power, which the owner turns off and on; with it she hires
///   into any team (`ADR-0029`), and she is never offboarded while the organization exists.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum BuiltinKey {
    Hr,
    ChiefOfStaff,
}

impl BuiltinKey {
    /// Every key, in the order the data contract lists them.
    pub const ALL: [Self; 2] = [Self::Hr, Self::ChiefOfStaff];

    pub const fn as_str(self) -> &'static str {
        match self {
            BuiltinKey::Hr => "hr",
            BuiltinKey::ChiefOfStaff => "chief_of_staff",
        }
    }

    /// The key `text` names exactly, or `None`.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.as_str() == text)
    }

    /// Whether an employee with this key may hold `may_hire` (the HR lead never does).
    pub const fn may_hold_hiring(self) -> bool {
        matches!(self, BuiltinKey::ChiefOfStaff)
    }

    /// How a refusal names the built-in employee with this key.
    pub const fn described(self) -> &'static str {
        match self {
            BuiltinKey::Hr => "the built-in HR lead",
            BuiltinKey::ChiefOfStaff => "the built-in Chief of Staff",
        }
    }

    fn read_optional(reader: &mut Reader, key: &str) -> Result<Option<Self>, RecordError> {
        match reader.optional_text(key)?.as_deref() {
            None => Ok(None),
            Some(text) => Self::parse(text)
                .map(Some)
                .ok_or_else(|| reader.error(key, "not one of: hr chief_of_staff")),
        }
    }
}

impl EmployeeKind {
    /// The folder of a team that holds this kind's profiles.
    pub const fn folder(self) -> &'static str {
        match self {
            EmployeeKind::Person => "people",
            EmployeeKind::Agent => "agents",
        }
    }
}

/// Who created or last changed a record.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum By {
    Employee(Id),
    System,
    Unknown,
}

impl By {
    pub fn employee(&self) -> Option<&Id> {
        match self {
            By::Employee(id) => Some(id),
            By::System | By::Unknown => None,
        }
    }

    fn write(&self, fields: &mut Fields, prefix: &str) {
        let (kind, id) = match self {
            By::Employee(id) => ("employee", json!(id.as_str())),
            By::System => ("system", Value::Null),
            By::Unknown => ("unknown", Value::Null),
        };
        fields.insert(format!("{prefix}_by_kind"), json!(kind));
        fields.insert(format!("{prefix}_by_id"), id);
    }

    fn read(reader: &mut Reader, prefix: &str) -> Result<Self, RecordError> {
        let (kind_key, id_key) = (format!("{prefix}_by_kind"), format!("{prefix}_by_id"));
        let kind = reader.text(&kind_key)?;
        let id = optional_id(reader, &id_key)?;
        match (kind.as_str(), id) {
            ("employee", Some(id)) => Ok(By::Employee(id)),
            ("employee", None) => Err(reader.error(&id_key, "an employee has an ID")),
            ("system", None) => Ok(By::System),
            ("unknown", None) => Ok(By::Unknown),
            ("system" | "unknown", Some(_)) => {
                Err(reader.error(&id_key, "only an employee has an ID"))
            }
            _ => Err(reader.error(&kind_key, "not one of: employee system unknown")),
        }
    }
}

/// The fields every attributed record has (document 08).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Meta {
    pub revision: u64,
    pub created_at: Option<String>,
    pub created_by: By,
    pub updated_at: Option<String>,
    pub updated_by: By,
}

impl Meta {
    /// A record at its first revision.
    pub fn created(by: &By, at: Timestamp) -> Self {
        let at = Some(time_text(at));
        Self {
            revision: 1,
            created_at: at.clone(),
            created_by: by.clone(),
            updated_at: at,
            updated_by: by.clone(),
        }
    }

    pub(crate) fn write(&self, fields: &mut Fields) {
        fields.insert("schema_version".into(), json!(SCHEMA_VERSION));
        fields.insert("revision".into(), json!(self.revision));
        fields.insert("created_at".into(), json!(self.created_at));
        self.created_by.write(fields, "created");
        fields.insert("updated_at".into(), json!(self.updated_at));
        self.updated_by.write(fields, "updated");
    }

    pub(crate) fn read(reader: &mut Reader) -> Result<Self, RecordError> {
        if reader.number("schema_version")? != SCHEMA_VERSION {
            return Err(reader.error("schema_version", "not 1"));
        }
        let revision = reader.number("revision")?;
        if revision == 0 {
            return Err(reader.error("revision", "below 1"));
        }
        Ok(Self {
            revision,
            created_at: optional_time(reader, "created_at")?,
            created_by: By::read(reader, "created")?,
            updated_at: optional_time(reader, "updated_at")?,
            updated_by: By::read(reader, "updated")?,
        })
    }
}

/// The text form of an instant: RFC 3339, UTC, milliseconds.
pub fn time_text(at: Timestamp) -> String {
    crate::time::format_epoch_ms(at.0)
}

/// Whether `text` is a stable ID: a UUID version 7 in lowercase hyphenated text.
pub fn is_stable_id(text: &str) -> bool {
    org_proto::is_uuid_v7(text)
}

/// One line of visible text with no edge spaces, at most [`MAX_TEXT_CHARS`] long.
pub fn is_line(text: &str) -> bool {
    !text.is_empty()
        && text.trim() == text
        && text.chars().count() <= MAX_TEXT_CHARS
        && !text
            .chars()
            .any(|c| c.is_control() || org_docs::is_invisible_or_break(c))
}

/// Lowercase letters, digits and single inner hyphens, at most 64 bytes.
pub fn is_slug(text: &str) -> bool {
    let parts_ok = text.split('-').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    });
    text.len() <= 64 && parts_ok
}

/// The characters and length every handle has, whoever checks it: 1 to
/// [`MAX_HANDLE_BYTES`] ASCII letters, digits, `-`, `_` and `.`, starting with a letter or
/// digit. The one copy of that rule; [`is_handle`] and the office's check of an agent's
/// messaging name both stand on it.
pub fn has_handle_characters(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.len() <= MAX_HANDLE_BYTES
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// A handle as a record stores it, without `@` (data contract, README "handles"):
/// [`has_handle_characters`], in lowercase, with dots only between other characters and
/// never two together, not a reserved word and not shaped like an address.
pub fn is_handle(text: &str) -> bool {
    has_handle_characters(text)
        && !text.bytes().any(|b| b.is_ascii_uppercase())
        && !text.ends_with('.')
        && !text.contains("..")
        && !is_reserved_handle(text)
        && !org_proto::is_shaped_like_an_address(text)
}

/// One or two letters, without `I` and `O` (document 06).
pub fn is_office_code(text: &str) -> bool {
    matches!(text.len(), 1 | 2)
        && text
            .bytes()
            .all(|b| b.is_ascii_uppercase() && b != b'I' && b != b'O')
}

fn id(reader: &mut Reader, key: &str) -> Result<Id, RecordError> {
    let text = reader.text(key)?;
    stable_id(&text).ok_or_else(|| reader.error(key, "not a UUIDv7 in lowercase text"))
}

fn optional_id(reader: &mut Reader, key: &str) -> Result<Option<Id>, RecordError> {
    let Some(text) = reader.optional_text(key)? else {
        return Ok(None);
    };
    let id = stable_id(&text);
    id.map(Some)
        .ok_or_else(|| reader.error(key, "not a UUIDv7 in lowercase text"))
}

fn stable_id(text: &str) -> Option<Id> {
    Id::parse(text).ok().filter(|_| is_stable_id(text))
}

fn checked(
    reader: &mut Reader,
    key: &str,
    ok: fn(&str) -> bool,
    what: &str,
) -> Result<String, RecordError> {
    let text = reader.text(key)?;
    if !ok(&text) {
        return Err(reader.error(key, what));
    }
    Ok(text)
}

fn line(reader: &mut Reader, key: &str) -> Result<String, RecordError> {
    checked(reader, key, is_line, "not one line of text")
}

fn optional_line(reader: &mut Reader, key: &str) -> Result<Option<String>, RecordError> {
    let text = reader.optional_text(key)?;
    if text.as_deref().is_some_and(|text| !is_line(text)) {
        return Err(reader.error(key, "not one line of text"));
    }
    Ok(text)
}

fn is_time(text: &str) -> bool {
    crate::time::parse_rfc3339_ms(text).is_some()
}

fn time(reader: &mut Reader, key: &str) -> Result<String, RecordError> {
    checked(reader, key, is_time, "not an RFC 3339 time")
}

fn optional_time(reader: &mut Reader, key: &str) -> Result<Option<String>, RecordError> {
    let text = reader.optional_text(key)?;
    if text.as_deref().is_some_and(|text| !is_time(text)) {
        return Err(reader.error(key, "not an RFC 3339 time"));
    }
    Ok(text)
}

/// A Git branch name as far as a record cares: one visible word.
pub fn is_branch(text: &str) -> bool {
    is_line(text) && !text.chars().any(char::is_whitespace)
}

/// Whether `text` can be a remote URL: visible, unbroken and at most [`MAX_URL_BYTES`].
pub fn is_remote_url(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_URL_BYTES
        && !text.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn path(text: &str) -> DurablePath {
    DurablePath::parse(text).expect("fixed names and IDs are safe components")
}

#[cfg(test)]
mod tests;
