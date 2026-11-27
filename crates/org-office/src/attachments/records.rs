//! The attachment record and its canonical YAML file (`TASK-98.1`, ADR-0037, `doc-7` #1).
//!
//! A standalone shared file is one Git-tracked file under `durable/state/`, written through
//! the journal, from which the projection's `attachments` table is rebuilt losslessly:
//!
//! - project-scoped: `state/projects/<project-id>/files/<attachment-id>.yaml`
//! - organization-wide: `state/files/<attachment-id>.yaml`
//!
//! The bytes are not in the file: it names them by `content_sha256` in the blob store. A
//! person's private files never get such a file; they live in HQ's `private/user-space.db`.

use org_proto::attachments::{
    ArtifactStatus, AttachmentMeta, AttachmentScope, AttachmentTargetKind, AttachmentView,
    ATTACHMENT_DESCRIPTION_MAX_CHARS, ATTACHMENT_FILENAME_MAX_BYTES, ATTACHMENT_TITLE_MAX_CHARS,
};
use org_store::{DurablePath, Id};
use serde_json::json;

use crate::organization::records::{
    is_stable_id,
    yaml::{self, Fields, Reader},
    RecordError,
};

use super::blobs::ContentHash;
use super::media::MediaType;

/// The version of the record's file format.
pub const SCHEMA_VERSION: u64 = 1;

/// One bound attachment: shared (projected from its file) or private (in `user-space.db`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AttachmentRecord {
    pub id: Id,
    pub target_kind: AttachmentTargetKind,
    /// `None` for an organization-wide file; the owner for a private one.
    pub target_id: Option<Id>,
    pub content_sha256: ContentHash,
    pub mime_type: String,
    pub byte_count: u64,
    pub filename: String,
    pub title: String,
    pub description: Option<String>,
    pub artifact_status: ArtifactStatus,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub page_count: Option<u32>,
    pub uploaded_by_employee_id: Id,
    pub created_at: String,
    pub updated_at: String,
}

/// A partial edit of a standalone file's metadata (`attachment.update_meta`). It is applied
/// where the write is decided (the journal's single writer, or the private space's
/// transaction) onto the record as it is then, so two edits of different fields never undo
/// each other.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MetaPatch {
    pub title: Option<String>,
    /// `Some("")` clears the description.
    pub description: Option<String>,
    pub artifact_status: Option<ArtifactStatus>,
    pub updated_at: String,
}

impl MetaPatch {
    /// `record` with the named fields replaced and `updated_at` set.
    pub fn apply(&self, record: &mut AttachmentRecord) {
        if let Some(title) = &self.title {
            record.title.clone_from(title);
        }
        if let Some(description) = &self.description {
            record.description = (!description.is_empty()).then(|| description.clone());
        }
        if let Some(status) = self.artifact_status {
            record.artifact_status = status;
        }
        record.updated_at.clone_from(&self.updated_at);
    }
}

impl AttachmentRecord {
    pub fn scope(&self) -> AttachmentScope {
        if self.target_kind == AttachmentTargetKind::UserPrivate {
            AttachmentScope::UserPrivate
        } else {
            AttachmentScope::Shared
        }
    }

    /// What a caller who passed the read check sees.
    pub fn to_view(&self) -> AttachmentView {
        AttachmentView {
            id: self.id.to_string(),
            scope: self.scope(),
            target_kind: self.target_kind,
            target_id: self.target_id.as_ref().map(Id::to_string),
            filename: self.filename.clone(),
            mime_type: self.mime_type.clone(),
            byte_count: self.byte_count,
            title: self.title.clone(),
            description: self.description.clone(),
            artifact_status: self.artifact_status,
            width: self.width,
            height: self.height,
            page_count: self.page_count,
            uploaded_by_employee_id: self.uploaded_by_employee_id.to_string(),
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
            content_sha256: self.content_sha256.to_string(),
        }
    }

    /// Lightweight metadata embedded in task, comment, message and ask responses.
    pub fn to_meta(&self) -> AttachmentMeta {
        AttachmentMeta {
            id: self.id.to_string(),
            filename: self.filename.clone(),
            title: self.title.clone(),
            description: self.description.clone(),
            artifact_status: self.artifact_status,
            mime_type: self.mime_type.clone(),
            byte_count: self.byte_count,
            width: self.width,
            height: self.height,
            page_count: self.page_count,
            uploaded_by_employee_id: self.uploaded_by_employee_id.to_string(),
            created_at: self.created_at.clone(),
        }
    }
}

/// Which file of the family a durable path is.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FileKind {
    Project { project: Id, attachment: Id },
    Organization { attachment: Id },
}

impl FileKind {
    pub fn attachment(&self) -> &Id {
        match self {
            Self::Project { attachment, .. } | Self::Organization { attachment } => attachment,
        }
    }
}

fn stable_id(text: &str) -> Option<Id> {
    is_stable_id(text).then(|| Id::parse(text).ok()).flatten()
}

/// The file kind of `path`, if the attachments family owns it.
pub fn kind_of(path: &DurablePath) -> Option<FileKind> {
    let parts: Vec<&str> = path.components().collect();
    match parts.as_slice() {
        ["state", "projects", project, "files", name] => Some(FileKind::Project {
            project: stable_id(project)?,
            attachment: stable_id(name.strip_suffix(".yaml")?)?,
        }),
        ["state", "files", name] => Some(FileKind::Organization {
            attachment: stable_id(name.strip_suffix(".yaml")?)?,
        }),
        _ => None,
    }
}

/// The canonical file of a shared attachment; `None` for a kind that has no standalone file.
pub fn record_path(record: &AttachmentRecord) -> Option<DurablePath> {
    let text = match (record.target_kind, &record.target_id) {
        (AttachmentTargetKind::Project, Some(project)) => {
            format!("state/projects/{project}/files/{}.yaml", record.id)
        }
        (AttachmentTargetKind::Organization, None) => format!("state/files/{}.yaml", record.id),
        _ => return None,
    };
    Some(DurablePath::parse(&text).expect("an attachment path is canonical"))
}

/// One line of visible text, 1 to 160 characters.
pub fn is_title(text: &str) -> bool {
    !text.is_empty()
        && text.trim() == text
        && text.chars().count() <= ATTACHMENT_TITLE_MAX_CHARS
        && !text.chars().any(char::is_control)
}

/// At most 280 characters; line breaks allowed, no other control character.
pub fn is_description(text: &str) -> bool {
    text.chars().count() <= ATTACHMENT_DESCRIPTION_MAX_CHARS
        && !text.chars().any(|c| c.is_control() && c != '\n')
}

pub fn is_filename(text: &str) -> bool {
    !text.is_empty()
        && text.trim() == text
        && text.len() <= ATTACHMENT_FILENAME_MAX_BYTES
        && !text.starts_with('.')
        && !text
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
}

/// The record's canonical YAML.
pub fn encode(record: &AttachmentRecord) -> Vec<u8> {
    let mut fields = Fields::new();
    fields.insert("schema_version".into(), json!(SCHEMA_VERSION));
    fields.insert("id".into(), json!(record.id.as_str()));
    fields.insert("target_kind".into(), json!(record.target_kind.as_str()));
    fields.insert(
        "target_id".into(),
        json!(record.target_id.as_ref().map(Id::as_str)),
    );
    fields.insert(
        "content_sha256".into(),
        json!(record.content_sha256.as_str()),
    );
    fields.insert("mime_type".into(), json!(record.mime_type));
    fields.insert("byte_count".into(), json!(record.byte_count));
    fields.insert("filename".into(), json!(record.filename));
    fields.insert("title".into(), json!(record.title));
    fields.insert("description".into(), json!(record.description));
    fields.insert(
        "artifact_status".into(),
        json!(record.artifact_status.as_str()),
    );
    for (key, value) in [
        ("width", record.width),
        ("height", record.height),
        ("page_count", record.page_count),
    ] {
        if let Some(value) = value {
            fields.insert(key.into(), json!(value));
        }
    }
    fields.insert(
        "uploaded_by_employee_id".into(),
        json!(record.uploaded_by_employee_id.as_str()),
    );
    fields.insert("created_at".into(), json!(record.created_at));
    fields.insert("updated_at".into(), json!(record.updated_at));
    yaml::encode(&fields)
}

fn read_id(reader: &mut Reader, key: &str) -> Result<Id, RecordError> {
    let text = reader.text(key)?;
    stable_id(&text).ok_or_else(|| reader.error(key, "not a lowercase UUIDv7"))
}

fn read_time(reader: &mut Reader, key: &str) -> Result<String, RecordError> {
    let text = reader.text(key)?;
    crate::time::parse_rfc3339_ms(&text)
        .ok_or_else(|| reader.error(key, "not an RFC 3339 timestamp"))?;
    Ok(text)
}

fn read_dimension(reader: &mut Reader, key: &str) -> Result<Option<u32>, RecordError> {
    if !reader.has(key) {
        return Ok(None);
    }
    let value = reader.number(key)?;
    match u32::try_from(value) {
        Ok(value) if value > 0 => Ok(Some(value)),
        _ => Err(reader.error(key, "not a positive 32-bit number")),
    }
}

/// Reads a record back from its YAML, refusing anything the encoder would not write.
pub fn decode(bytes: &[u8]) -> Result<AttachmentRecord, RecordError> {
    let mut reader = yaml::decode(bytes)?;
    if reader.number("schema_version")? != SCHEMA_VERSION {
        return Err(reader.error("schema_version", "not version 1"));
    }
    let id = read_id(&mut reader, "id")?;
    let kind_text = reader.text("target_kind")?;
    let target_kind = AttachmentTargetKind::parse(&kind_text)
        .ok_or_else(|| reader.error("target_kind", "not a known target kind"))?;
    let target_id = match reader.optional_text("target_id")? {
        Some(text) => Some(
            stable_id(&text).ok_or_else(|| reader.error("target_id", "not a lowercase UUIDv7"))?,
        ),
        None => None,
    };
    let sha_text = reader.text("content_sha256")?;
    let content_sha256 = ContentHash::parse(&sha_text)
        .ok_or_else(|| reader.error("content_sha256", "not 64 lowercase hex digits"))?;
    let mime_type = reader.text("mime_type")?;
    if MediaType::from_mime(&mime_type).is_none() {
        return Err(reader.error("mime_type", "not an accepted attachment type"));
    }
    let byte_count = reader.number("byte_count")?;
    let filename = reader.text("filename")?;
    if !is_filename(&filename) {
        return Err(reader.error("filename", "not one plain file name"));
    }
    let title = reader.text("title")?;
    if !is_title(&title) {
        return Err(reader.error("title", "not 1 to 160 characters of one line"));
    }
    let description = reader.optional_text("description")?;
    if description.as_deref().is_some_and(|d| !is_description(d)) {
        return Err(reader.error("description", "longer than 280 characters"));
    }
    let status_text = reader.text("artifact_status")?;
    let artifact_status = ArtifactStatus::parse(&status_text)
        .ok_or_else(|| reader.error("artifact_status", "not a known status"))?;
    let width = read_dimension(&mut reader, "width")?;
    let height = read_dimension(&mut reader, "height")?;
    let page_count = read_dimension(&mut reader, "page_count")?;
    let uploaded_by_employee_id = read_id(&mut reader, "uploaded_by_employee_id")?;
    let created_at = read_time(&mut reader, "created_at")?;
    let updated_at = read_time(&mut reader, "updated_at")?;
    reader.finish()?;
    Ok(AttachmentRecord {
        id,
        target_kind,
        target_id,
        content_sha256,
        mime_type,
        byte_count,
        filename,
        title,
        description,
        artifact_status,
        width,
        height,
        page_count,
        uploaded_by_employee_id,
        created_at,
        updated_at,
    })
}
