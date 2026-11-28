//! Durable records of the documentation tree and thin `Id`/`Meta` adapters over
//! `org_docs::project_tree::codecs`.

pub use org_docs::project_tree::codecs::MEDIA_RECORD_MAX_BYTES;
use org_docs::project_tree::codecs::{
    decode_doc_media, decode_project_doc, encode_doc_media, encode_project_doc, DocActor, DocMeta,
    RawDocEntry, RawDocMedia, RawProjectDoc,
};
pub use org_docs::project_tree::{
    is_body, is_doc_kind, is_sha256_hex, is_summary, media_type_of, DocStatus, MediaKind,
};
use org_store::Id;

use super::paths::{DocPath, MediaFile};
use crate::organization::records::{By, Meta, RecordError};

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

/// A document without its body.
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
    pub sha256: String,
    pub title: String,
    pub summary: String,
    pub meta: Meta,
}

fn by_to_actor(by: &By) -> DocActor {
    match by {
        By::Employee(id) => DocActor::Employee(id.to_string()),
        By::System => DocActor::System,
        By::Unknown => DocActor::Unknown,
    }
}

fn actor_to_by(actor: DocActor) -> Result<By, RecordError> {
    match actor {
        DocActor::Employee(id) => Id::parse(&id)
            .map(By::Employee)
            .map_err(|_| RecordError::new("actor_id", "not a lowercase UUIDv7")),
        DocActor::System => Ok(By::System),
        DocActor::Unknown => Ok(By::Unknown),
    }
}

pub(crate) fn meta_to_doc_meta(meta: &Meta) -> DocMeta {
    DocMeta {
        revision: meta.revision,
        created_at: meta.created_at.clone(),
        created_by: by_to_actor(&meta.created_by),
        updated_at: meta.updated_at.clone(),
        updated_by: by_to_actor(&meta.updated_by),
    }
}

fn doc_meta_to_meta(meta: DocMeta) -> Result<Meta, RecordError> {
    Ok(Meta {
        revision: meta.revision,
        created_at: meta.created_at,
        created_by: actor_to_by(meta.created_by)?,
        updated_at: meta.updated_at,
        updated_by: actor_to_by(meta.updated_by)?,
    })
}

pub(crate) fn entry_to_raw(e: &DocEntry) -> RawDocEntry {
    RawDocEntry {
        id: e.id.to_string(),
        project_id: e.project_id.to_string(),
        path: e.path.clone(),
        kind: e.kind.clone(),
        title: e.title.clone(),
        status: e.status,
        summary: e.summary.clone(),
        owner_employee_id: e.owner_employee_id.as_ref().map(Id::to_string),
        meta: meta_to_doc_meta(&e.meta),
    }
}

pub(crate) fn doc_to_raw(d: &ProjectDoc) -> RawProjectDoc {
    let e = entry_to_raw(&d.entry());
    RawProjectDoc {
        id: e.id,
        project_id: e.project_id,
        path: e.path,
        kind: e.kind,
        title: e.title,
        status: e.status,
        summary: e.summary,
        owner_employee_id: e.owner_employee_id,
        body_markdown: d.body_markdown.clone(),
        meta: e.meta,
    }
}

pub(crate) fn media_to_raw(m: &DocMedia) -> RawDocMedia {
    RawDocMedia {
        id: m.id.to_string(),
        project_id: m.project_id.to_string(),
        file: m.file.clone(),
        kind: m.kind,
        media_type: m.media_type.clone(),
        bytes: m.bytes,
        sha256: m.sha256.clone(),
        title: m.title.clone(),
        summary: m.summary.clone(),
        meta: meta_to_doc_meta(&m.meta),
    }
}

fn parse_id(field: &str, raw: &str) -> Result<Id, RecordError> {
    Id::parse(raw).map_err(|_| RecordError::new(field, "not a lowercase UUIDv7"))
}

/// The document's file: front matter, then its body.
pub fn encode_doc(doc: &ProjectDoc) -> Vec<u8> {
    encode_project_doc(&doc_to_raw(doc))
}

/// Reads the document file at `path` of `project`.
pub fn decode_doc(bytes: &[u8], project: &Id, path: DocPath) -> Result<ProjectDoc, RecordError> {
    let raw = decode_project_doc(bytes, project.as_str(), path)
        .map_err(|e| RecordError::new(e.field, e.reason))?;
    Ok(ProjectDoc {
        id: parse_id("id", &raw.id)?,
        project_id: parse_id("project_id", &raw.project_id)?,
        path: raw.path,
        kind: raw.kind,
        title: raw.title,
        status: raw.status,
        summary: raw.summary,
        owner_employee_id: raw
            .owner_employee_id
            .as_deref()
            .map(|id| parse_id("owner_employee_id", id))
            .transpose()?,
        body_markdown: raw.body_markdown,
        meta: doc_meta_to_meta(raw.meta)?,
    })
}

/// The media asset's metadata sidecar.
pub fn encode_media(media: &DocMedia) -> Vec<u8> {
    encode_doc_media(&media_to_raw(media))
}

/// Reads the metadata sidecar of `file` in `project`.
pub fn decode_media(bytes: &[u8], project: &Id, file: MediaFile) -> Result<DocMedia, RecordError> {
    let raw = decode_doc_media(bytes, project.as_str(), file)
        .map_err(|e| RecordError::new(e.field, e.reason))?;
    Ok(DocMedia {
        id: parse_id("id", &raw.id)?,
        project_id: parse_id("project_id", &raw.project_id)?,
        file: raw.file,
        kind: raw.kind,
        media_type: raw.media_type,
        bytes: raw.bytes,
        sha256: raw.sha256,
        title: raw.title,
        summary: raw.summary,
        meta: doc_meta_to_meta(raw.meta)?,
    })
}
