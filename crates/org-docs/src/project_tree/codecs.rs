//! Deterministic Markdown/YAML codecs for project documentation tree documents and media sidecars.

use std::fmt;

use org_proto::docs::DOC_MEDIA_MAX_BYTES;
use org_proto::is_uuid_v7;
use serde_json::{json, Map, Value};

use super::{
    is_body, is_doc_kind, is_sha256_hex, is_summary, is_title_line, media_type_of, yaml_encode,
    yaml_encode_document, DocPath, DocStatus, MediaFile, MediaKind,
};
use crate::{parse, parse_mapping};

/// Maximum media asset size in bytes (`documentation_media.bytes`).
pub const MEDIA_RECORD_MAX_BYTES: u64 = DOC_MEDIA_MAX_BYTES as u64;

/// Pure actor attribution stored on project document / media metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocActor {
    /// Active or historical employee UUIDv7.
    Employee(String),
    /// System actor.
    System,
    /// Unknown actor.
    Unknown,
}

impl DocActor {
    /// Wire/SQL actor kind string.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Employee(_) => "employee",
            Self::System => "system",
            Self::Unknown => "unknown",
        }
    }

    /// Employee UUIDv7 if `Self::Employee`, else `None`.
    pub fn employee_id(&self) -> Option<&str> {
        match self {
            Self::Employee(id) => Some(id.as_str()),
            Self::System | Self::Unknown => None,
        }
    }
}

/// Shared schema/revision/audit metadata on project documents and media sidecars.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocMeta {
    /// Revision (`>= 1`).
    pub revision: u64,
    /// Optional RFC 3339 creation timestamp.
    pub created_at: Option<String>,
    /// Creator actor.
    pub created_by: DocActor,
    /// Optional RFC 3339 update timestamp.
    pub updated_at: Option<String>,
    /// Last updater actor.
    pub updated_by: DocActor,
}

/// Pure project document record (without storage crate dependencies).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawProjectDoc {
    /// Document UUIDv7.
    pub id: String,
    /// Owning project UUIDv7.
    pub project_id: String,
    /// Validated relative path under `docs/`.
    pub path: DocPath,
    /// Kind slug outside `brief`/`changelog`.
    pub kind: String,
    /// Single-line title.
    pub title: String,
    /// Document lifecycle status.
    pub status: DocStatus,
    /// Optional summary.
    pub summary: Option<String>,
    /// Optional owner employee UUIDv7.
    pub owner_employee_id: Option<String>,
    /// Markdown body.
    pub body_markdown: String,
    /// Metadata.
    pub meta: DocMeta,
}

/// Pure project document metadata entry (without body).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDocEntry {
    /// Document UUIDv7.
    pub id: String,
    /// Owning project UUIDv7.
    pub project_id: String,
    /// Validated relative path under `docs/`.
    pub path: DocPath,
    /// Kind slug.
    pub kind: String,
    /// Title.
    pub title: String,
    /// Lifecycle status.
    pub status: DocStatus,
    /// Optional summary.
    pub summary: Option<String>,
    /// Optional owner employee UUIDv7.
    pub owner_employee_id: Option<String>,
    /// Metadata.
    pub meta: DocMeta,
}

impl RawProjectDoc {
    /// Projects this document into its body-less [`RawDocEntry`].
    pub fn entry(&self) -> RawDocEntry {
        RawDocEntry {
            id: self.id.clone(),
            project_id: self.project_id.clone(),
            path: self.path.clone(),
            kind: self.kind.clone(),
            title: self.title.clone(),
            status: self.status,
            summary: self.summary.clone(),
            owner_employee_id: self.owner_employee_id.clone(),
            meta: self.meta.clone(),
        }
    }
}

/// Pure project media metadata record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDocMedia {
    /// Media UUIDv7.
    pub id: String,
    /// Owning project UUIDv7.
    pub project_id: String,
    /// Validated media file name.
    pub file: MediaFile,
    /// Image or diagram.
    pub kind: MediaKind,
    /// MIME type.
    pub media_type: String,
    /// Byte length.
    pub bytes: u64,
    /// Lowercase hex SHA-256 digest.
    pub sha256: String,
    /// Title.
    pub title: String,
    /// Summary (may be empty).
    pub summary: String,
    /// Metadata.
    pub meta: DocMeta,
}

/// Error decoding a project documentation file or media sidecar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecError {
    /// Offending field name (empty for whole-file errors).
    pub field: String,
    /// Human-readable reason.
    pub reason: String,
}

impl CodecError {
    /// Constructs a new codec error.
    pub fn new(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.field.is_empty() {
            f.write_str(&self.reason)
        } else {
            write!(f, "{}: {}", self.field, self.reason)
        }
    }
}

impl std::error::Error for CodecError {}

fn text(m: &mut Map<String, Value>, k: &str) -> Result<String, CodecError> {
    match m.remove(k) {
        Some(Value::String(s)) => Ok(s),
        Some(_) => Err(CodecError::new(k, "expected a string")),
        None => Err(CodecError::new(k, "missing required field")),
    }
}

fn opt_text(m: &mut Map<String, Value>, k: &str) -> Result<Option<String>, CodecError> {
    match m.remove(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(CodecError::new(k, "expected a string or null")),
    }
}

fn num(m: &mut Map<String, Value>, k: &str) -> Result<u64, CodecError> {
    match m.remove(k) {
        Some(Value::Number(n)) => n
            .as_u64()
            .ok_or_else(|| CodecError::new(k, "expected an unsigned integer")),
        Some(_) => Err(CodecError::new(k, "expected an integer")),
        None => Err(CodecError::new(k, "missing required field")),
    }
}

fn uuid(m: &mut Map<String, Value>, k: &str) -> Result<String, CodecError> {
    let v = text(m, k)?;
    if !is_uuid_v7(&v) {
        return Err(CodecError::new(k, "not a lowercase UUIDv7"));
    }
    Ok(v)
}

fn opt_uuid(m: &mut Map<String, Value>, k: &str) -> Result<Option<String>, CodecError> {
    match opt_text(m, k)? {
        None => Ok(None),
        Some(v) if is_uuid_v7(&v) => Ok(Some(v)),
        Some(_) => Err(CodecError::new(k, "not a lowercase UUIDv7")),
    }
}

fn finish(m: &Map<String, Value>) -> Result<(), CodecError> {
    if let Some(extra) = m.keys().next() {
        return Err(CodecError::new(extra, "unknown field"));
    }
    Ok(())
}

fn write_meta(fields: &mut Map<String, Value>, meta: &DocMeta) {
    fields.insert("schema_version".into(), json!(1u64));
    fields.insert("revision".into(), json!(meta.revision));
    fields.insert("created_at".into(), json!(meta.created_at));
    fields.insert("created_by_kind".into(), json!(meta.created_by.kind()));
    fields.insert("created_by_id".into(), json!(meta.created_by.employee_id()));
    fields.insert("updated_at".into(), json!(meta.updated_at));
    fields.insert("updated_by_kind".into(), json!(meta.updated_by.kind()));
    fields.insert("updated_by_id".into(), json!(meta.updated_by.employee_id()));
}

fn read_actor(
    m: &mut Map<String, Value>,
    kind_key: &str,
    id_key: &str,
) -> Result<DocActor, CodecError> {
    let kind = text(m, kind_key)?;
    let id = opt_uuid(m, id_key)?;
    match (kind.as_str(), id) {
        ("employee", Some(emp)) => Ok(DocActor::Employee(emp)),
        ("system", None) => Ok(DocActor::System),
        ("unknown", None) => Ok(DocActor::Unknown),
        _ => Err(CodecError::new(kind_key, "invalid actor kind/id pair")),
    }
}

fn read_meta(m: &mut Map<String, Value>) -> Result<DocMeta, CodecError> {
    if num(m, "schema_version")? != 1 {
        return Err(CodecError::new("schema_version", "must be 1"));
    }
    let revision = num(m, "revision")?;
    if revision < 1 {
        return Err(CodecError::new("revision", "must be at least 1"));
    }
    let created_at = opt_text(m, "created_at")?;
    let created_by = read_actor(m, "created_by_kind", "created_by_id")?;
    let updated_at = opt_text(m, "updated_at")?;
    let updated_by = read_actor(m, "updated_by_kind", "updated_by_id")?;
    Ok(DocMeta {
        revision,
        created_at,
        created_by,
        updated_at,
        updated_by,
    })
}

/// Encodes a project documentation file as Markdown with YAML front matter.
pub fn encode_project_doc(doc: &RawProjectDoc) -> Vec<u8> {
    let mut fields = Map::new();
    write_meta(&mut fields, &doc.meta);
    fields.insert("id".into(), json!(doc.id));
    fields.insert("project_id".into(), json!(doc.project_id));
    fields.insert("kind".into(), json!(doc.kind));
    fields.insert("title".into(), json!(doc.title));
    fields.insert("status".into(), json!(doc.status.as_str()));
    fields.insert("summary".into(), json!(doc.summary));
    fields.insert("owner_employee_id".into(), json!(doc.owner_employee_id));
    yaml_encode_document(&fields, &doc.body_markdown)
}

/// Decodes and validates a project documentation file at `path` of `expected_project_id`.
pub fn decode_project_doc(
    bytes: &[u8],
    expected_project_id: &str,
    path: DocPath,
) -> Result<RawProjectDoc, CodecError> {
    let raw = std::str::from_utf8(bytes).map_err(|_| CodecError::new("", "not UTF-8"))?;
    let parsed = parse(raw).map_err(|e| CodecError::new(e.field, format!("{:?}", e.reason)))?;
    let mut m = parsed
        .front_matter
        .ok_or_else(|| CodecError::new("", "missing YAML front matter"))?;
    let body_markdown = parsed.body.to_owned();
    let meta = read_meta(&mut m)?;
    let id = uuid(&mut m, "id")?;
    let project_id = uuid(&mut m, "project_id")?;
    if project_id != expected_project_id {
        return Err(CodecError::new(
            "project_id",
            "not the project the file is in",
        ));
    }
    let kind = text(&mut m, "kind")?;
    if !is_doc_kind(&kind) {
        return Err(CodecError::new(
            "kind",
            "not a lowercase kind slug outside brief/changelog",
        ));
    }
    let title = text(&mut m, "title")?;
    if !is_title_line(&title) {
        return Err(CodecError::new("title", "not one line of visible text"));
    }
    let status = DocStatus::parse(&text(&mut m, "status")?)
        .ok_or_else(|| CodecError::new("status", "not one of: draft current deprecated"))?;
    let summary = opt_text(&mut m, "summary")?;
    if summary
        .as_deref()
        .is_some_and(|s| s.is_empty() || !is_summary(s))
    {
        return Err(CodecError::new("summary", "not a summary of at most 2 KiB"));
    }
    let owner_employee_id = opt_uuid(&mut m, "owner_employee_id")?;
    if status == DocStatus::Current && owner_employee_id.is_none() {
        return Err(CodecError::new(
            "owner_employee_id",
            "a current document has an owner",
        ));
    }
    if !is_body(&body_markdown) {
        return Err(CodecError::new("body", "larger than 256 KiB or holds NUL"));
    }
    finish(&m)?;
    Ok(RawProjectDoc {
        id,
        project_id,
        path,
        kind,
        title,
        status,
        summary,
        owner_employee_id,
        body_markdown,
        meta,
    })
}

/// Encodes a project media sidecar to deterministic YAML.
pub fn encode_doc_media(media: &RawDocMedia) -> Vec<u8> {
    let mut fields = Map::new();
    write_meta(&mut fields, &media.meta);
    fields.insert("id".into(), json!(media.id));
    fields.insert("project_id".into(), json!(media.project_id));
    fields.insert("path".into(), json!(media.file.rel_path()));
    fields.insert("type".into(), json!(media.kind.as_str()));
    fields.insert("media_type".into(), json!(media.media_type));
    fields.insert("bytes".into(), json!(media.bytes));
    fields.insert("sha256".into(), json!(media.sha256));
    fields.insert("title".into(), json!(media.title));
    fields.insert("summary".into(), json!(media.summary));
    yaml_encode(&fields)
}

/// Decodes and validates a project media metadata sidecar beside `file` in `expected_project_id`.
pub fn decode_doc_media(
    bytes: &[u8],
    expected_project_id: &str,
    file: MediaFile,
) -> Result<RawDocMedia, CodecError> {
    let raw = std::str::from_utf8(bytes).map_err(|_| CodecError::new("", "not UTF-8"))?;
    let mut m =
        parse_mapping(raw).map_err(|e| CodecError::new(e.field, format!("{:?}", e.reason)))?;
    let meta = read_meta(&mut m)?;
    let id = uuid(&mut m, "id")?;
    let project_id = uuid(&mut m, "project_id")?;
    if project_id != expected_project_id {
        return Err(CodecError::new(
            "project_id",
            "not the project the file is in",
        ));
    }
    if text(&mut m, "path")? != file.rel_path() {
        return Err(CodecError::new("path", "not the asset beside this file"));
    }
    let kind = MediaKind::parse(&text(&mut m, "type")?)
        .ok_or_else(|| CodecError::new("type", "not one of: image diagram"))?;
    let media_type = text(&mut m, "media_type")?;
    if media_type != media_type_of(&file) {
        return Err(CodecError::new(
            "media_type",
            "not the type the file name says",
        ));
    }
    let bytes_len = num(&mut m, "bytes")?;
    if bytes_len == 0 || bytes_len > MEDIA_RECORD_MAX_BYTES {
        return Err(CodecError::new("bytes", "not 1 byte to 5 MiB"));
    }
    let sha256 = text(&mut m, "sha256")?;
    if !is_sha256_hex(&sha256) {
        return Err(CodecError::new("sha256", "not lowercase hex SHA-256"));
    }
    let title = text(&mut m, "title")?;
    if !is_title_line(&title) {
        return Err(CodecError::new("title", "not one line of visible text"));
    }
    let summary = text(&mut m, "summary")?;
    if !is_summary(&summary) {
        return Err(CodecError::new("summary", "not a summary of at most 2 KiB"));
    }
    finish(&m)?;
    Ok(RawDocMedia {
        id,
        project_id,
        file,
        kind,
        media_type,
        bytes: bytes_len,
        sha256,
        title,
        summary,
        meta,
    })
}
