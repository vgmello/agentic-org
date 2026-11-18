//! The organization family's records, where each lives under `durable/state/`, and the
//! codec between a record and its file bytes (data contract documents 01, 04 and 06).
//!
//! A file holds every field of its record, null when unknown, so a record has one field
//! set. Team and employee files do not hold the organization ID: an office store holds one
//! organization.

mod yaml;

use org_store::{DurablePath, Id, StoreError, Timestamp};
use serde_json::{json, Value};

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

    fn write(&self, fields: &mut Fields) {
        fields.insert("schema_version".into(), json!(SCHEMA_VERSION));
        fields.insert("revision".into(), json!(self.revision));
        fields.insert("created_at".into(), json!(self.created_at));
        self.created_by.write(fields, "created");
        fields.insert("updated_at".into(), json!(self.updated_at));
        self.updated_by.write(fields, "updated");
    }

    fn read(reader: &mut Reader) -> Result<Self, RecordError> {
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
        && !RESERVED_HANDLES.contains(&text)
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

// ---- organization.yaml ----

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Organization {
    pub id: Id,
    pub slug: String,
    pub name: String,
    pub organization_repository_id: Id,
    pub state_branch: String,
    pub meta: Meta,
}

pub fn organization_path() -> DurablePath {
    path("state/organization.yaml")
}

pub fn encode_organization(record: &Organization) -> Vec<u8> {
    let mut fields = Fields::new();
    record.meta.write(&mut fields);
    fields.insert("id".into(), json!(record.id.as_str()));
    fields.insert("slug".into(), json!(record.slug));
    fields.insert("name".into(), json!(record.name));
    let repository = record.organization_repository_id.as_str();
    fields.insert("organization_repository_id".into(), json!(repository));
    fields.insert("state_branch".into(), json!(record.state_branch));
    yaml::encode(&fields)
}

pub fn decode_organization(bytes: &[u8]) -> Result<Organization, RecordError> {
    let mut reader = yaml::decode(bytes)?;
    let record = Organization {
        meta: Meta::read(&mut reader)?,
        id: id(&mut reader, "id")?,
        slug: checked(&mut reader, "slug", is_slug, "not a slug")?,
        name: line(&mut reader, "name")?,
        organization_repository_id: id(&mut reader, "organization_repository_id")?,
        state_branch: checked(&mut reader, "state_branch", is_branch, "not a branch name")?,
    };
    reader.finish()?;
    Ok(record)
}

// ---- repositories.yaml ----

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Repository {
    pub id: Id,
    pub key: Option<String>,
    pub kind: RepositoryKind,
    /// Present exactly when the kind is remote.
    pub remote_url: Option<String>,
    pub meta: Meta,
}

pub fn repositories_path() -> DurablePath {
    path("state/repositories.yaml")
}

pub fn encode_repositories(records: &[Repository]) -> Vec<u8> {
    let entry = |record: &Repository| {
        let mut fields = Fields::new();
        record.meta.write(&mut fields);
        fields.insert("id".into(), json!(record.id.as_str()));
        fields.insert("key".into(), json!(record.key));
        fields.insert("kind".into(), json!(record.kind.as_str()));
        fields.insert("remote_url".into(), json!(record.remote_url));
        Value::Object(fields)
    };
    let mut fields = Fields::new();
    fields.insert("schema_version".into(), json!(SCHEMA_VERSION));
    let entries = records.iter().map(entry).collect();
    fields.insert("repositories".into(), Value::Array(entries));
    yaml::encode(&fields)
}

pub fn decode_repositories(bytes: &[u8]) -> Result<Vec<Repository>, RecordError> {
    let mut reader = yaml::decode(bytes)?;
    if reader.number("schema_version")? != SCHEMA_VERSION {
        return Err(reader.error("schema_version", "not 1"));
    }
    let mut records: Vec<Repository> = Vec::new();
    for mut entry in reader.list("repositories")? {
        let record = Repository {
            meta: Meta::read(&mut entry)?,
            id: id(&mut entry, "id")?,
            key: optional_line(&mut entry, "key")?,
            kind: RepositoryKind::read(&mut entry, "kind")?,
            remote_url: entry.optional_text("remote_url")?,
        };
        let url_ok = match (record.kind, &record.remote_url) {
            (RepositoryKind::Remote, Some(url)) => is_remote_url(url),
            (RepositoryKind::Local, None) => true,
            _ => false,
        };
        if !url_ok {
            return Err(entry.error("remote_url", "a remote has a URL and a local has none"));
        }
        if records.iter().any(|earlier| earlier.id == record.id) {
            return Err(entry.error("id", "listed twice"));
        }
        let key_taken = |earlier: &Repository| earlier.key.is_some() && earlier.key == record.key;
        if records.iter().any(key_taken) {
            return Err(entry.error("key", "listed twice"));
        }
        entry.finish()?;
        records.push(record);
    }
    reader.finish()?;
    Ok(records)
}

// ---- offices/<office-id>.yaml ----

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Office {
    pub id: Id,
    pub slug: String,
    pub name: String,
    pub role: OfficeRole,
    pub meta: Meta,
}

pub fn office_path(office: &Id) -> DurablePath {
    path(&format!("state/offices/{office}.yaml"))
}

pub fn encode_office(record: &Office) -> Vec<u8> {
    let mut fields = Fields::new();
    record.meta.write(&mut fields);
    fields.insert("id".into(), json!(record.id.as_str()));
    fields.insert("slug".into(), json!(record.slug));
    fields.insert("name".into(), json!(record.name));
    fields.insert("role".into(), json!(record.role.as_str()));
    yaml::encode(&fields)
}

pub fn decode_office(bytes: &[u8]) -> Result<Office, RecordError> {
    let mut reader = yaml::decode(bytes)?;
    let record = Office {
        meta: Meta::read(&mut reader)?,
        id: id(&mut reader, "id")?,
        slug: checked(&mut reader, "slug", is_slug, "not a slug")?,
        name: line(&mut reader, "name")?,
        role: OfficeRole::read(&mut reader, "role")?,
    };
    reader.finish()?;
    Ok(record)
}

// ---- identity/office-codes.yaml ----

/// One code HQ has issued, current or retired. A code is never changed or issued twice.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OfficeCode {
    pub code: String,
    pub office_id: Id,
    pub issued_at: String,
    pub retired_at: Option<String>,
}

pub fn office_codes_path() -> DurablePath {
    path("state/identity/office-codes.yaml")
}

pub fn encode_office_codes(records: &[OfficeCode]) -> Vec<u8> {
    let entry = |record: &OfficeCode| {
        json!({
            "code": record.code,
            "office_id": record.office_id.as_str(),
            "issued_at": record.issued_at,
            "retired_at": record.retired_at,
        })
    };
    let mut fields = Fields::new();
    fields.insert("schema_version".into(), json!(SCHEMA_VERSION));
    let entries = records.iter().map(entry).collect();
    fields.insert("codes".into(), Value::Array(entries));
    yaml::encode(&fields)
}

pub fn decode_office_codes(bytes: &[u8]) -> Result<Vec<OfficeCode>, RecordError> {
    let mut reader = yaml::decode(bytes)?;
    if reader.number("schema_version")? != SCHEMA_VERSION {
        return Err(reader.error("schema_version", "not 1"));
    }
    let mut records: Vec<OfficeCode> = Vec::new();
    for mut entry in reader.list("codes")? {
        let record = OfficeCode {
            code: checked(&mut entry, "code", is_office_code, "not an office code")?,
            office_id: id(&mut entry, "office_id")?,
            issued_at: time(&mut entry, "issued_at")?,
            retired_at: optional_time(&mut entry, "retired_at")?,
        };
        if records.iter().any(|earlier| earlier.code == record.code) {
            return Err(entry.error("code", "issued twice"));
        }
        let current = |earlier: &OfficeCode| {
            earlier.retired_at.is_none() && earlier.office_id == record.office_id
        };
        if record.retired_at.is_none() && records.iter().any(current) {
            return Err(entry.error("office_id", "has two current codes"));
        }
        entry.finish()?;
        records.push(record);
    }
    reader.finish()?;
    Ok(records)
}

// ---- teams/<id>/teams/<id>/.../team.yaml ----

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Team {
    pub id: Id,
    pub name: String,
    /// The organization-wide `TEAM-<number>`.
    pub number: u64,
    pub parent_team_id: Option<Id>,
    pub lead_employee_id: Option<Id>,
    pub status: TeamStatus,
    pub meta: Meta,
}

impl Team {
    pub fn is_active(&self) -> bool {
        self.status == TeamStatus::Active
    }
}

/// The readable reference of the team numbered `number`.
pub fn team_key(number: u64) -> String {
    format!("TEAM-{number}")
}

/// The folder of the team at the end of `chain`: a top-level team first, then each child.
fn team_folder(chain: &[Id]) -> String {
    let mut folder = String::from("state");
    for team in chain {
        folder.push_str("/teams/");
        folder.push_str(team.as_str());
    }
    folder
}

/// The file of the team at the end of `chain`. Refused when the tree is too deep for a
/// path.
pub fn team_path(chain: &[Id]) -> Result<DurablePath, StoreError> {
    if chain.is_empty() {
        return Err(StoreError::invalid("a team path names a team"));
    }
    DurablePath::parse(&format!("{}/team.yaml", team_folder(chain)))
}

pub fn encode_team(record: &Team) -> Vec<u8> {
    let id = |id: &Option<Id>| json!(id.as_ref().map(Id::as_str));
    let mut fields = Fields::new();
    record.meta.write(&mut fields);
    fields.insert("id".into(), json!(record.id.as_str()));
    fields.insert("name".into(), json!(record.name));
    fields.insert("number".into(), json!(record.number));
    fields.insert("parent_team_id".into(), id(&record.parent_team_id));
    fields.insert("lead_employee_id".into(), id(&record.lead_employee_id));
    fields.insert("status".into(), json!(record.status.as_str()));
    yaml::encode(&fields)
}

pub fn decode_team(bytes: &[u8]) -> Result<Team, RecordError> {
    let mut reader = yaml::decode(bytes)?;
    let record = Team {
        meta: Meta::read(&mut reader)?,
        id: id(&mut reader, "id")?,
        name: line(&mut reader, "name")?,
        number: reader.number("number")?,
        parent_team_id: optional_id(&mut reader, "parent_team_id")?,
        lead_employee_id: optional_id(&mut reader, "lead_employee_id")?,
        status: TeamStatus::read(&mut reader, "status")?,
    };
    if record.number == 0 {
        return Err(reader.error("number", "below 1"));
    }
    // The key is derived from the number. A file may repeat it, and then it must agree.
    if reader.has("key") && reader.text("key")? != team_key(record.number) {
        return Err(reader.error("key", "not TEAM-<number>"));
    }
    if record.parent_team_id.as_ref() == Some(&record.id) {
        return Err(reader.error("parent_team_id", "a team is not its own parent"));
    }
    reader.finish()?;
    Ok(record)
}

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
        body,
    };
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

// ---- identity/counters/organization/<org-id>/team.yaml ----

/// The organization's team counter: HQ's high-water of issued team numbers.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TeamCounter {
    pub organization_id: Id,
    pub last_number: u64,
}

impl TeamCounter {
    /// The counter after issuing the next number.
    pub fn advance(&self) -> Result<Self, StoreError> {
        if self.last_number >= i64::MAX as u64 {
            return Err(StoreError::Exhausted("team numbers"));
        }
        Ok(Self {
            organization_id: self.organization_id.clone(),
            last_number: self.last_number + 1,
        })
    }
}

pub fn team_counter_path(organization: &Id) -> DurablePath {
    path(&format!(
        "state/identity/counters/organization/{organization}/team.yaml"
    ))
}

pub fn encode_team_counter(record: &TeamCounter) -> Vec<u8> {
    let fields = json!({
        "scope_kind": "organization",
        "scope_id": record.organization_id.as_str(),
        "entity_kind": "team",
        "last_number": record.last_number,
    });
    yaml::encode(fields.as_object().expect("an object"))
}

/// The scope every team counter and allocation has: this organization, teams.
fn team_scope(reader: &mut Reader) -> Result<Id, RecordError> {
    if reader.text("scope_kind")? != "organization" {
        return Err(reader.error("scope_kind", "not organization"));
    }
    let organization = id(reader, "scope_id")?;
    if reader.text("entity_kind")? != "team" {
        return Err(reader.error("entity_kind", "not team"));
    }
    Ok(organization)
}

pub fn decode_team_counter(bytes: &[u8]) -> Result<TeamCounter, RecordError> {
    let mut reader = yaml::decode(bytes)?;
    let record = TeamCounter {
        organization_id: team_scope(&mut reader)?,
        last_number: reader.number("last_number")?,
    };
    reader.finish()?;
    Ok(record)
}

// ---- identity/allocations/organization/<org-id>/team/<number>.yaml ----

/// The immutable record that a team number was issued to a team.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TeamAllocation {
    pub organization_id: Id,
    pub number: u64,
    pub team_id: Id,
    pub allocated_at: String,
}

pub fn team_allocation_path(organization: &Id, number: u64) -> DurablePath {
    path(&format!(
        "state/identity/allocations/organization/{organization}/team/{number}.yaml"
    ))
}

pub fn encode_team_allocation(record: &TeamAllocation) -> Vec<u8> {
    let fields = json!({
        "scope_kind": "organization",
        "scope_id": record.organization_id.as_str(),
        "entity_kind": "team",
        "number": record.number,
        "entity_id": record.team_id.as_str(),
        "allocated_at": record.allocated_at,
    });
    yaml::encode(fields.as_object().expect("an object"))
}

pub fn decode_team_allocation(bytes: &[u8]) -> Result<TeamAllocation, RecordError> {
    let mut reader = yaml::decode(bytes)?;
    let record = TeamAllocation {
        organization_id: team_scope(&mut reader)?,
        number: reader.number("number")?,
        team_id: id(&mut reader, "entity_id")?,
        allocated_at: time(&mut reader, "allocated_at")?,
    };
    if record.number == 0 {
        return Err(reader.error("number", "below 1"));
    }
    reader.finish()?;
    Ok(record)
}

// ---- which record a path holds ----

/// The record a path of this family holds, with what the path itself says.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    Organization,
    Repositories,
    Office(Id),
    OfficeCodes,
    /// The teams from the top-level one down to this one.
    Team(Vec<Id>),
    /// The home team's chain, the folder's kind and the employee.
    Employee(Vec<Id>, EmployeeKind, Id),
    /// The scope the path names.
    TeamCounter(Id),
    TeamAllocation(Id, u64),
}

/// A number as a path writes it: decimal, no sign, no leading zero, at least 1.
fn path_number(text: &str) -> Option<u64> {
    let canonical = !text.starts_with('0') && text.bytes().all(|b| b.is_ascii_digit());
    text.parse().ok().filter(|_| canonical)
}

/// Which record `path` holds; `None` when the path is not this family's. Decided by the
/// shape of the path alone, so every path has at most one owner.
pub fn kind_of(path: &DurablePath) -> Option<Kind> {
    let parts: Vec<&str> = path.components().collect();
    let id = |text: &str| stable_id(text);
    let file = |name: &str| name.strip_suffix(".yaml").and_then(stable_id);
    match parts.as_slice() {
        ["state", "organization.yaml"] => Some(Kind::Organization),
        ["state", "repositories.yaml"] => Some(Kind::Repositories),
        ["state", "offices", name] => file(name).map(Kind::Office),
        ["state", "identity", "office-codes.yaml"] => Some(Kind::OfficeCodes),
        ["state", "identity", "counters", "organization", scope, "team.yaml"] => {
            id(scope).map(Kind::TeamCounter)
        }
        ["state", "identity", "allocations", "organization", scope, "team", name] => {
            let number = name.strip_suffix(".yaml").and_then(path_number)?;
            Some(Kind::TeamAllocation(id(scope)?, number))
        }
        ["state", teams @ ..] => {
            let mut chain = Vec::new();
            let mut rest = teams;
            while let ["teams", team, below @ ..] = rest {
                chain.push(id(team)?);
                rest = below;
            }
            let kind = |folder: &str| {
                let kinds = [EmployeeKind::Person, EmployeeKind::Agent];
                kinds.into_iter().find(|kind| kind.folder() == folder)
            };
            match rest {
                _ if chain.is_empty() => None,
                ["team.yaml"] => Some(Kind::Team(chain)),
                [folder, employee, "profile.md"] => {
                    Some(Kind::Employee(chain, kind(folder)?, id(employee)?))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
