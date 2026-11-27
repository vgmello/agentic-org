//! The organization, repository, office and office code records, and the Org CA escrow file.

use org_store::{DurablePath, Id};
use serde_json::{json, Value};

use super::yaml::RecordError;
use super::yaml::{Fields, Reader};
use super::{
    checked, id, is_branch, is_office_code, is_remote_url, is_slug, line, optional_line,
    optional_time, path, time, yaml, Meta, SCHEMA_VERSION,
};
use super::{OfficeRole, RepositoryKind};

// ---- organization.yaml ----

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Organization {
    pub id: Id,
    pub slug: String,
    pub name: String,
    pub organization_repository_id: Id,
    pub state_branch: String,
    /// The default definition of done a new task copies when neither it nor its project names
    /// one (`TASK-88`). Written only when set.
    pub default_dod: Option<Vec<String>>,
    pub meta: Meta,
}

/// `default_dod`, a list of step texts, when the file has it (`TASK-88`).
pub(crate) fn read_default_dod(reader: &mut Reader) -> Result<Option<Vec<String>>, RecordError> {
    if !reader.has("default_dod") {
        return Ok(None);
    }
    let items = reader.strings("default_dod")?;
    match org_proto::tasks::normalize_dod_items(&items) {
        Ok(normalized) if normalized == items => Ok(Some(items)),
        _ => Err(reader.error("default_dod", "not a list of definition-of-done items")),
    }
}

/// Writes `default_dod` when set (`TASK-88`), so a record without one keeps its bytes.
pub(crate) fn write_default_dod(default_dod: Option<&Vec<String>>, fields: &mut Fields) {
    if let Some(items) = default_dod {
        fields.insert("default_dod".into(), json!(items));
    }
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
    write_default_dod(record.default_dod.as_ref(), &mut fields);
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
        default_dod: read_default_dod(&mut reader)?,
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

// ---- security/ca-escrow.v1.bin ----

/// The largest escrow package ADR-0007 allows, in bytes.
pub const MAX_CA_ESCROW_BYTES: usize = 4475;

/// The magic and version an escrow package starts with (ADR-0007).
const CA_ESCROW_MAGIC: &[u8; 8] = b"AGORGCA1";

/// The one suite ADR-0007 defines.
const CA_ESCROW_SUITE: u8 = 1;

/// The ciphertext lengths ADR-0007 allows: up to 4096 PKCS#8 bytes plus the 16-byte tag.
const CA_ESCROW_CIPHERTEXT: std::ops::RangeInclusive<usize> = 17..=4112;

/// The organization's public CA escrow package (data contract document 01, ADR-0007): the
/// exact encrypted envelope, published to Git verbatim. Its bytes are the record.
pub fn ca_escrow_path() -> DurablePath {
    path("state/security/ca-escrow.v1.bin")
}

/// The CA's public key in the escrow package `package` of `organization`, once its framing
/// is checked: magic and version, the organization ID, the suite, an uncompressed SEC1
/// public key, and a ciphertext length that ends the package exactly. The ciphertext is not
/// opened: that takes the recovery code and is the PKI adapter's (`org-pki`).
pub fn ca_escrow_public_key(package: &[u8], organization: &Id) -> Result<[u8; 65], RecordError> {
    let refused = |reason: &str| RecordError::new("", reason);
    let short = || refused("cut short");
    if package.len() > MAX_CA_ESCROW_BYTES {
        return Err(refused("larger than an escrow package can be"));
    }
    let Some(rest) = package.strip_prefix(CA_ESCROW_MAGIC.as_slice()) else {
        return Err(refused("not an escrow package of version 1"));
    };
    let (length, rest) = rest.split_first_chunk::<2>().ok_or_else(short)?;
    let length = usize::from(u16::from_be_bytes(*length));
    if !(1..=255).contains(&length) {
        return Err(refused("the organization ID is 1 to 255 bytes"));
    }
    let (recorded, rest) = rest.split_at_checked(length).ok_or_else(short)?;
    if recorded != organization.as_str().as_bytes() {
        return Err(refused("sealed for another organization"));
    }
    let (&suite, rest) = rest.split_first().ok_or_else(short)?;
    if suite != CA_ESCROW_SUITE {
        return Err(refused("not suite 1"));
    }
    // The salt (16 bytes) and the nonce (12) are the PKI adapter's.
    let (_, rest) = rest.split_at_checked(16 + 12).ok_or_else(short)?;
    let (public, rest) = rest.split_first_chunk::<65>().ok_or_else(short)?;
    if public[0] != 4 {
        return Err(refused("the public key is not an uncompressed SEC1 key"));
    }
    let (ciphertext, rest) = rest.split_first_chunk::<4>().ok_or_else(short)?;
    let ciphertext = u32::from_be_bytes(*ciphertext) as usize;
    if !CA_ESCROW_CIPHERTEXT.contains(&ciphertext) || rest.len() != ciphertext {
        return Err(refused("the ciphertext is not the length the header gives"));
    }
    Ok(*public)
}
