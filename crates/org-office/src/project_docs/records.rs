//! Durable records of the documentation tree and their codecs: a document is Markdown with YAML
//! front matter (read through `org_docs::SourceDoc`, bounded), a media asset's metadata is a
//! YAML sidecar beside its bytes. A document's path is where its file is, never a field: a move
//! changes the location only.

pub use org_docs::project_tree::{
    is_body, is_doc_kind, is_sha256_hex, is_summary, media_type_of, DocStatus, MediaKind,
};
use org_proto::docs::DOC_MEDIA_MAX_BYTES;
use org_store::Id;
use serde_json::{json, Value};

use super::paths::{DocPath, MediaFile};
use crate::organization::records::{
    is_line, is_stable_id,
    yaml::{self, Fields, Reader},
    Meta, RecordError,
};

/// The largest media asset the tree keeps (the data contract's `documentation_media.bytes`):
/// [`DOC_MEDIA_MAX_BYTES`], so every asset replicates to satellites as one `org-sync` payload.
pub const MEDIA_RECORD_MAX_BYTES: u64 = DOC_MEDIA_MAX_BYTES as u64;

/// One document of a project's tree (`documents` with a `path`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ProjectDoc {
    pub id: Id,
    pub project_id: Id,
    pub path: DocPath,
    pub kind: String,
    pub title: String,
    pub status: DocStatus,
    pub summary: Option<String>,
    pub owner_employee_id: Option<Id>,
    pub body_markdown: String,
    pub meta: Meta,
}

/// A document without its body: all the index, the lists and a write's view of the rest of the
/// tree need, so none of them loads every body of a project.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DocEntry {
    pub id: Id,
    pub project_id: Id,
    pub path: DocPath,
    pub kind: String,
    pub title: String,
    pub status: DocStatus,
    pub summary: Option<String>,
    pub owner_employee_id: Option<Id>,
    pub meta: Meta,
}

impl ProjectDoc {
    /// This document without its body.
    pub fn entry(&self) -> DocEntry {
        DocEntry {
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

/// One media asset of a project's tree (`documentation_media`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DocMedia {
    pub id: Id,
    pub project_id: Id,
    pub file: MediaFile,
    pub kind: MediaKind,
    pub media_type: String,
    pub bytes: u64,
    /// Lowercase hex SHA-256 of the asset's bytes.
    pub sha256: String,
    pub title: String,
    pub summary: String,
    pub meta: Meta,
}

fn read_id(reader: &mut Reader, key: &str) -> Result<Id, RecordError> {
    let text = reader.text(key)?;
    if !is_stable_id(&text) {
        return Err(reader.error(key, "not a lowercase UUIDv7"));
    }
    Id::parse(&text).map_err(|_| reader.error(key, "not a lowercase UUIDv7"))
}

fn read_optional_id(reader: &mut Reader, key: &str) -> Result<Option<Id>, RecordError> {
    match reader.optional_text(key)? {
        None => Ok(None),
        Some(text) if is_stable_id(&text) => Id::parse(&text)
            .map(Some)
            .map_err(|_| reader.error(key, "not a lowercase UUIDv7")),
        Some(_) => Err(reader.error(key, "not a lowercase UUIDv7")),
    }
}

fn read_title(reader: &mut Reader) -> Result<String, RecordError> {
    let title = reader.text("title")?;
    if !is_line(&title) {
        return Err(reader.error("title", "not one line of visible text"));
    }
    Ok(title)
}

fn optional_id_value(id: Option<&Id>) -> Value {
    id.map_or(Value::Null, |id| json!(id.as_str()))
}

/// The document's file: front matter, then its body.
pub fn encode_doc(doc: &ProjectDoc) -> Vec<u8> {
    let mut fields = Fields::new();
    doc.meta.write(&mut fields);
    fields.insert("id".into(), json!(doc.id.as_str()));
    fields.insert("project_id".into(), json!(doc.project_id.as_str()));
    fields.insert("kind".into(), json!(doc.kind));
    fields.insert("title".into(), json!(doc.title));
    fields.insert("status".into(), json!(doc.status.as_str()));
    fields.insert(
        "summary".into(),
        doc.summary.as_deref().map_or(Value::Null, |s| json!(s)),
    );
    fields.insert(
        "owner_employee_id".into(),
        optional_id_value(doc.owner_employee_id.as_ref()),
    );
    yaml::encode_document(&fields, &doc.body_markdown)
}

/// Reads the document file at `path` of `project`.
pub fn decode_doc(bytes: &[u8], project: &Id, path: DocPath) -> Result<ProjectDoc, RecordError> {
    let (mut reader, body_markdown) = yaml::decode_source_document(bytes)?;
    let meta = Meta::read(&mut reader)?;
    let id = read_id(&mut reader, "id")?;
    let project_id = read_id(&mut reader, "project_id")?;
    if project_id != *project {
        return Err(reader.error("project_id", "not the project the file is in"));
    }
    let kind = reader.text("kind")?;
    if !is_doc_kind(&kind) {
        return Err(reader.error("kind", "not a lowercase kind slug outside brief/changelog"));
    }
    let title = read_title(&mut reader)?;
    let status_text = reader.text("status")?;
    let status = DocStatus::parse(&status_text)
        .ok_or_else(|| reader.error("status", "not one of: draft current deprecated"))?;
    let summary = reader.optional_text("summary")?;
    if summary
        .as_deref()
        .is_some_and(|s| s.is_empty() || !is_summary(s))
    {
        return Err(reader.error("summary", "not a summary of at most 2 KiB"));
    }
    let owner_employee_id = read_optional_id(&mut reader, "owner_employee_id")?;
    if status == DocStatus::Current && owner_employee_id.is_none() {
        return Err(reader.error("owner_employee_id", "a current document has an owner"));
    }
    if !is_body(&body_markdown) {
        return Err(RecordError::new("body", "larger than 256 KiB or holds NUL"));
    }
    reader.finish()?;
    Ok(ProjectDoc {
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

/// The media asset's metadata sidecar.
pub fn encode_media(media: &DocMedia) -> Vec<u8> {
    let mut fields = Fields::new();
    media.meta.write(&mut fields);
    fields.insert("id".into(), json!(media.id.as_str()));
    fields.insert("project_id".into(), json!(media.project_id.as_str()));
    fields.insert("path".into(), json!(media.file.rel_path()));
    fields.insert("type".into(), json!(media.kind.as_str()));
    fields.insert("media_type".into(), json!(media.media_type));
    fields.insert("bytes".into(), json!(media.bytes));
    fields.insert("sha256".into(), json!(media.sha256));
    fields.insert("title".into(), json!(media.title));
    fields.insert("summary".into(), json!(media.summary));
    yaml::encode(&fields)
}

/// Reads the metadata sidecar of `file` in `project`.
pub fn decode_media(bytes: &[u8], project: &Id, file: MediaFile) -> Result<DocMedia, RecordError> {
    let mut reader = yaml::decode(bytes)?;
    let meta = Meta::read(&mut reader)?;
    let id = read_id(&mut reader, "id")?;
    let project_id = read_id(&mut reader, "project_id")?;
    if project_id != *project {
        return Err(reader.error("project_id", "not the project the file is in"));
    }
    if reader.text("path")? != file.rel_path() {
        return Err(reader.error("path", "not the asset beside this file"));
    }
    let kind_text = reader.text("type")?;
    let kind = MediaKind::parse(&kind_text)
        .ok_or_else(|| reader.error("type", "not one of: image diagram"))?;
    let media_type = reader.text("media_type")?;
    if media_type != media_type_of(&file) {
        return Err(reader.error("media_type", "not the type the file name says"));
    }
    let bytes_len = reader.number("bytes")?;
    if bytes_len == 0 || bytes_len > MEDIA_RECORD_MAX_BYTES {
        return Err(reader.error("bytes", "not 1 byte to 5 MiB"));
    }
    let sha256 = reader.text("sha256")?;
    if !is_sha256_hex(&sha256) {
        return Err(reader.error("sha256", "not lowercase hex SHA-256"));
    }
    let title = read_title(&mut reader)?;
    let summary = reader.text("summary")?;
    if !is_summary(&summary) {
        return Err(reader.error("summary", "not a summary of at most 2 KiB"));
    }
    reader.finish()?;
    Ok(DocMedia {
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
