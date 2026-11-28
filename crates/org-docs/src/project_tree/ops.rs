//! Pure engine-independent operations for the project documentation tree (`docs.*`).

use std::collections::BTreeMap;

use org_proto::docs::{
    DocMediaKindWire, DocMediaWire, DocSectionWire, DocStatusWire, DocSummaryWire,
    DocsCreateParams, DocsDeleteParams, DocsDeleteResult, DocsIndexResult, DocsInspectParams,
    DocsInspectResult, DocsListParams, DocsListResult, DocsMoveParams, DocsUpdateParams,
    DocsWriteResult, DOC_BODY_MAX_BYTES, DOC_LIST_DEFAULT_LIMIT, DOC_LIST_MAX_LIMIT,
    DOC_REF_MAX_BYTES,
};
use org_proto::{is_uuid_v7, ApiError, ErrorCode};
use serde::Serialize;

use super::codecs::{
    encode_doc_media, encode_project_doc, DocActor, DocMeta, RawDocEntry, RawDocMedia,
    RawProjectDoc,
};
use super::{
    file_stem_from_title, is_body, is_doc_kind, is_summary, is_title_line, render_project_index,
    DocPath, DocStatus, IndexDocRow, IndexMediaRow, MediaFile, MediaKind, PathError, Section,
    INDEX_FILE, INDEX_MAX_BYTES, MEDIA_FOLDER,
};

/// Maximum documents permitted in one project documentation tree.
pub const MAX_DOCS_PER_PROJECT: usize = 1_000;
/// Maximum media assets permitted in one project documentation tree.
pub const MAX_MEDIA_PER_PROJECT: usize = 500;

/// Failure from a pure documentation tree operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeOpError {
    /// Caller-facing API refusal.
    Refused(ApiError),
    /// Storage layer error text.
    Storage(String),
}

/// Durable file writes/removals and serialized JSON reply emitted by a tree write decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeMutation {
    /// Durable `(path, bytes)` files to write.
    pub puts: Vec<(String, Vec<u8>)>,
    /// Durable paths to remove.
    pub removes: Vec<String>,
    /// Serialized JSON reply bytes.
    pub reply: Vec<u8>,
}

/// Authorized caller & project context for a tree operation.
#[derive(Debug, Clone, Copy)]
pub struct TreeCaller<'a> {
    /// Active caller employee UUIDv7.
    pub caller_id: &'a str,
    /// Authorized project UUIDv7.
    pub project_id: &'a str,
    /// RFC 3339 timestamp for mutations.
    pub now_rfc3339: &'a str,
}

/// Storage read port consumed by pure documentation tree operations.
pub trait ProjectTreeView {
    /// Every document entry of `project_id`, in path order.
    fn project_doc_entries(&self, project_id: &str) -> Result<Vec<RawDocEntry>, String>;
    /// One full document by ID.
    fn project_doc(&self, doc_id: &str) -> Result<Option<RawProjectDoc>, String>;
    /// Every media asset of `project_id`, in path order.
    fn project_doc_media(&self, project_id: &str) -> Result<Vec<RawDocMedia>, String>;
    /// Whether employee `id` exists and is `active`.
    fn active_employee_exists(&self, emp_id: &str) -> Result<bool, String>;
    /// Sorted IDs of Why links referencing `(document, doc_id)` as source or target.
    fn document_link_ids(&self, doc_id: &str) -> Result<Vec<String>, String>;
}

fn refused(code: ErrorCode, msg: impl Into<String>) -> TreeOpError {
    TreeOpError::Refused(ApiError::new(code, msg.into()))
}

fn invalid(field: &str, rule: impl std::fmt::Display) -> TreeOpError {
    refused(ErrorCode::InvalidRequest, format!("{field}: {rule}"))
}

fn path_err(field: &str, err: &PathError) -> TreeOpError {
    invalid(field, err)
}

fn stor<T>(r: Result<T, String>) -> Result<T, TreeOpError> {
    r.map_err(TreeOpError::Storage)
}

fn reply<T: Serialize>(answer: &T) -> Vec<u8> {
    serde_json::to_vec(answer).expect("a documentation reply encodes")
}

/// Durable path of `docs/index.yaml` for `project_id`.
pub fn index_durable_path(project_id: &str) -> String {
    format!("state/projects/{project_id}/docs/{INDEX_FILE}")
}

/// Durable path of one project document.
pub fn doc_durable_path(project_id: &str, path: &DocPath) -> String {
    format!("state/projects/{project_id}/docs/{}", path.as_str())
}

/// Durable path of one project media blob.
pub fn media_blob_durable_path(project_id: &str, file: &MediaFile) -> String {
    format!(
        "state/projects/{project_id}/docs/{MEDIA_FOLDER}/{}",
        file.as_str()
    )
}

/// Durable path of one project media metadata sidecar.
pub fn media_meta_durable_path(project_id: &str, file: &MediaFile) -> String {
    format!(
        "state/projects/{project_id}/docs/{MEDIA_FOLDER}/{}.yaml",
        file.as_str()
    )
}

/// Converts a [`RawDocEntry`] to its wire summary.
pub fn wire_doc_summary(doc: &RawDocEntry) -> DocSummaryWire {
    DocSummaryWire {
        id: doc.id.clone(),
        project_id: doc.project_id.clone(),
        path: doc.path.as_str().to_owned(),
        section: doc.path.section().as_str().to_owned(),
        kind: doc.kind.clone(),
        title: doc.title.clone(),
        status: match doc.status {
            DocStatus::Draft => DocStatusWire::Draft,
            DocStatus::Current => DocStatusWire::Current,
            DocStatus::Deprecated => DocStatusWire::Deprecated,
        },
        summary: doc.summary.clone(),
        owner_employee_id: doc.owner_employee_id.clone(),
        revision: doc.meta.revision,
        created_at: doc.meta.created_at.clone(),
        updated_at: doc.meta.updated_at.clone(),
        updated_by_id: doc.meta.updated_by.employee_id().map(str::to_owned),
    }
}

/// Converts a [`RawDocMedia`] to its wire view.
pub fn wire_doc_media(media: &RawDocMedia) -> DocMediaWire {
    DocMediaWire {
        id: media.id.clone(),
        project_id: media.project_id.clone(),
        path: media.file.rel_path(),
        kind: match media.kind {
            MediaKind::Image => DocMediaKindWire::Image,
            MediaKind::Diagram => DocMediaKindWire::Diagram,
        },
        media_type: media.media_type.clone(),
        bytes: media.bytes,
        sha256: media.sha256.clone(),
        title: media.title.clone(),
        summary: media.summary.clone(),
        revision: media.meta.revision,
        created_at: media.meta.created_at.clone(),
        updated_at: media.meta.updated_at.clone(),
    }
}

/// Validates a document kind slug.
pub fn validate_kind(raw: &str) -> Result<String, TreeOpError> {
    if is_doc_kind(raw) {
        Ok(raw.to_owned())
    } else {
        Err(invalid(
            "kind",
            "a lowercase slug of letters, digits and `-` (at most 64 bytes), not brief or changelog",
        ))
    }
}

/// Validates and trims a document or media title.
pub fn validate_title(raw: &str) -> Result<String, TreeOpError> {
    let trimmed = raw.trim();
    if is_title_line(trimmed) {
        Ok(trimmed.to_owned())
    } else {
        Err(invalid(
            "title",
            "one line of visible text, 1 to 200 characters",
        ))
    }
}

/// Validates an optional summary (`None` or blank clears).
pub fn validate_summary(raw: Option<&str>) -> Result<Option<String>, TreeOpError> {
    let Some(text) = raw.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    if is_summary(text) {
        Ok(Some(text.to_owned()))
    } else {
        Err(invalid("summary", "visible text of at most 2 KiB"))
    }
}

/// Validates a Markdown body.
pub fn validate_body(raw: &str) -> Result<String, TreeOpError> {
    if raw.len() > DOC_BODY_MAX_BYTES {
        return Err(refused(
            ErrorCode::TooLarge,
            "body_markdown: at most 256 KiB",
        ));
    }
    if !is_body(raw) {
        return Err(invalid("body_markdown", "holds no NUL character"));
    }
    Ok(raw.to_owned())
}

/// Validates a wire document status.
pub fn validate_status(raw: DocStatusWire) -> Result<DocStatus, TreeOpError> {
    match raw {
        DocStatusWire::Draft => Ok(DocStatus::Draft),
        DocStatusWire::Current => Ok(DocStatus::Current),
        DocStatusWire::Deprecated => Ok(DocStatus::Deprecated),
        DocStatusWire::Unknown => Err(invalid("status", "one of: draft current deprecated")),
    }
}

fn validate_uuid(field: &str, raw: &str) -> Result<String, TreeOpError> {
    if is_uuid_v7(raw) {
        Ok(raw.to_owned())
    } else {
        Err(invalid(field, "a lowercase UUIDv7"))
    }
}

fn validate_owner<V: ProjectTreeView>(
    view: &V,
    raw: Option<&str>,
) -> Result<Option<String>, TreeOpError> {
    let Some(text) = raw.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    let id = validate_uuid("owner_employee_id", text)?;
    if stor(view.active_employee_exists(&id))? {
        Ok(Some(id))
    } else {
        Err(invalid(
            "owner_employee_id",
            "an active employee of the organization",
        ))
    }
}

fn resolve_new_path(
    path: Option<&str>,
    in_section: Option<&str>,
    title: &str,
) -> Result<DocPath, TreeOpError> {
    let wanted = in_section
        .map(|s| Section::parse(s).map_err(|e| path_err("section", &e)))
        .transpose()?;
    match path {
        Some(raw) => {
            let parsed = DocPath::parse(raw).map_err(|e| path_err("path", &e))?;
            if wanted.is_some_and(|s| s != parsed.section()) {
                return Err(invalid("path", "not in the given section"));
            }
            Ok(parsed)
        }
        None => {
            let file = format!("{}.md", file_stem_from_title(title));
            DocPath::join(&wanted.unwrap_or_default(), &file).map_err(|e| path_err("path", &e))
        }
    }
}

fn find_doc<V: ProjectTreeView>(
    view: &V,
    project_id: &str,
    raw: &str,
) -> Result<RawProjectDoc, TreeOpError> {
    let not_found = || refused(ErrorCode::NotFound, "document not found");
    if raw.is_empty() || raw.len() > DOC_REF_MAX_BYTES {
        return Err(invalid("document", "required"));
    }
    if is_uuid_v7(raw) {
        return match stor(view.project_doc(raw))? {
            Some(doc) if doc.project_id == project_id => Ok(doc),
            _ => Err(not_found()),
        };
    }
    let path = DocPath::parse(raw).map_err(|e| path_err("document", &e))?;
    let entry = stor(view.project_doc_entries(project_id))?
        .into_iter()
        .find(|d| d.path == path)
        .ok_or_else(not_found)?;
    stor(view.project_doc(&entry.id))?.ok_or_else(not_found)
}

fn current_doc<V: ProjectTreeView>(
    view: &V,
    project_id: &str,
    document_id: &str,
    expected_revision: u64,
) -> Result<RawProjectDoc, TreeOpError> {
    let id = validate_uuid("document_id", document_id.trim())?;
    let doc = find_doc(view, project_id, &id)?;
    if doc.meta.revision != expected_revision {
        return Err(refused(
            ErrorCode::Conflict,
            format!(
                "expected revision {expected_revision}, but the document is at revision {}",
                doc.meta.revision
            ),
        ));
    }
    Ok(doc)
}

/// Pure single-write tree accumulator that always regenerates `docs/index.yaml`.
pub struct RawTreeEdit {
    project_id: String,
    docs: Vec<RawDocEntry>,
    media: Vec<RawDocMedia>,
    puts: Vec<(String, Vec<u8>)>,
    removes: Vec<String>,
}

impl RawTreeEdit {
    /// Loads the current tree of `project_id`.
    pub fn load<V: ProjectTreeView>(view: &V, project_id: &str) -> Result<Self, TreeOpError> {
        Ok(Self {
            project_id: project_id.to_owned(),
            docs: stor(view.project_doc_entries(project_id))?,
            media: stor(view.project_doc_media(project_id))?,
            puts: Vec::new(),
            removes: Vec::new(),
        })
    }

    /// Finds an existing media asset by file name.
    pub fn media_at(&self, file: &MediaFile) -> Option<&RawDocMedia> {
        self.media.iter().find(|m| m.file == *file)
    }

    /// Writes `doc`, removing its previous file if moved, and refusing path collisions.
    pub fn put_doc(&mut self, doc: RawProjectDoc) -> Result<(), TreeOpError> {
        if self
            .docs
            .iter()
            .any(|d| d.path == doc.path && d.id != doc.id)
        {
            return Err(refused(
                ErrorCode::AlreadyExists,
                "path: another document is at this path",
            ));
        }
        match self.docs.iter().position(|d| d.id == doc.id) {
            Some(at) => {
                let old = std::mem::replace(&mut self.docs[at], doc.entry());
                if old.path != doc.path {
                    self.removes
                        .push(doc_durable_path(&old.project_id, &old.path));
                }
            }
            None if self.docs.len() >= MAX_DOCS_PER_PROJECT => {
                return Err(refused(
                    ErrorCode::TooLarge,
                    "the project's documentation holds the most documents it may",
                ));
            }
            None => self.docs.push(doc.entry()),
        }
        self.puts.push((
            doc_durable_path(&doc.project_id, &doc.path),
            encode_project_doc(&doc),
        ));
        Ok(())
    }

    /// Removes `doc` from the tree.
    pub fn remove_doc(&mut self, doc: &RawProjectDoc) -> Result<(), TreeOpError> {
        let Some(at) = self.docs.iter().position(|d| d.id == doc.id) else {
            return Err(refused(ErrorCode::NotFound, "document not found"));
        };
        self.docs.remove(at);
        self.removes
            .push(doc_durable_path(&doc.project_id, &doc.path));
        Ok(())
    }

    /// Writes a media blob and its metadata sidecar.
    pub fn put_media(&mut self, media: RawDocMedia, bytes: Vec<u8>) -> Result<(), TreeOpError> {
        match self.media.iter().position(|m| m.file == media.file) {
            Some(at) => self.media[at] = media.clone(),
            None if self.media.len() >= MAX_MEDIA_PER_PROJECT => {
                return Err(refused(
                    ErrorCode::TooLarge,
                    "the project's documentation holds the most media assets it may",
                ));
            }
            None => self.media.push(media.clone()),
        }
        self.puts.push((
            media_blob_durable_path(&media.project_id, &media.file),
            bytes,
        ));
        self.puts.push((
            media_meta_durable_path(&media.project_id, &media.file),
            encode_doc_media(&media),
        ));
        Ok(())
    }

    /// Renders `docs/index.yaml` and produces the final [`TreeMutation`].
    pub fn finish(mut self, reply_bytes: Vec<u8>) -> Result<TreeMutation, TreeOpError> {
        let doc_rows: Vec<IndexDocRow<'_>> = self
            .docs
            .iter()
            .map(|d| IndexDocRow {
                id: &d.id,
                path: &d.path,
                kind: &d.kind,
                title: &d.title,
                status: d.status,
                summary: d.summary.as_deref(),
                owner_employee_id: d.owner_employee_id.as_deref(),
                revision: d.meta.revision,
            })
            .collect();
        let media_rows: Vec<IndexMediaRow<'_>> = self
            .media
            .iter()
            .map(|m| IndexMediaRow {
                id: &m.id,
                file: &m.file,
                kind: m.kind,
                media_type: &m.media_type,
                bytes: m.bytes,
                sha256: &m.sha256,
                title: &m.title,
                revision: m.meta.revision,
            })
            .collect();
        let rendered = render_project_index(&self.project_id, &doc_rows, &media_rows);
        if rendered.len() > INDEX_MAX_BYTES {
            return Err(refused(
                ErrorCode::TooLarge,
                "the project's documentation index would be over 256 KiB",
            ));
        }
        self.puts
            .push((index_durable_path(&self.project_id), rendered));
        Ok(TreeMutation {
            puts: self.puts,
            removes: self.removes,
            reply: reply_bytes,
        })
    }
}

/// Pure `docs.index` read.
pub fn read_index<V: ProjectTreeView>(
    view: &V,
    project_id: &str,
) -> Result<DocsIndexResult, TreeOpError> {
    let docs = stor(view.project_doc_entries(project_id))?;
    let mut sections: BTreeMap<String, u64> = BTreeMap::new();
    for doc in &docs {
        *sections
            .entry(doc.path.section().as_str().to_owned())
            .or_default() += 1;
    }
    Ok(DocsIndexResult {
        project_id: project_id.to_owned(),
        index_path: index_durable_path(project_id),
        sections: sections
            .into_iter()
            .map(|(path, document_count)| DocSectionWire {
                path,
                document_count,
            })
            .collect(),
        documents: docs.iter().map(wire_doc_summary).collect(),
        media: stor(view.project_doc_media(project_id))?
            .iter()
            .map(wire_doc_media)
            .collect(),
    })
}

/// Pure `docs.list` read.
pub fn read_list<V: ProjectTreeView>(
    view: &V,
    project_id: &str,
    params: &DocsListParams,
) -> Result<DocsListResult, TreeOpError> {
    let limit = params.limit.unwrap_or(DOC_LIST_DEFAULT_LIMIT);
    if !(1..=DOC_LIST_MAX_LIMIT).contains(&limit) {
        return Err(invalid("limit", "1 to 200"));
    }
    let section = params
        .section
        .as_deref()
        .map(|s| Section::parse(s).map_err(|e| path_err("section", &e)))
        .transpose()?;
    let kind = params.kind.as_deref().map(validate_kind).transpose()?;
    let status = params.status.map(validate_status).transpose()?;
    let after = params
        .cursor
        .as_deref()
        .map(|c| DocPath::parse(c).map_err(|e| path_err("cursor", &e)))
        .transpose()?;
    let mut matching = stor(view.project_doc_entries(project_id))?
        .into_iter()
        .filter(|doc| {
            after.as_ref().is_none_or(|a| doc.path > *a)
                && section
                    .as_ref()
                    .is_none_or(|s| s.contains(&doc.path.section()))
                && kind.as_ref().is_none_or(|k| doc.kind == *k)
                && status.is_none_or(|s| doc.status == s)
        });
    let page: Vec<_> = matching.by_ref().take(limit as usize).collect();
    let next_cursor = matching
        .next()
        .and(page.last())
        .map(|last| last.path.as_str().to_owned());
    Ok(DocsListResult {
        documents: page.iter().map(wire_doc_summary).collect(),
        next_cursor,
    })
}

/// Pure `docs.inspect` read.
pub fn read_inspect<V: ProjectTreeView>(
    view: &V,
    project_id: &str,
    params: &DocsInspectParams,
) -> Result<DocsInspectResult, TreeOpError> {
    let doc = find_doc(view, project_id, params.document.trim())?;
    Ok(DocsInspectResult {
        document: wire_doc_summary(&doc.entry()),
        body_markdown: doc.body_markdown,
    })
}

fn write_doc<V: ProjectTreeView>(
    view: &V,
    doc: RawProjectDoc,
) -> Result<TreeMutation, TreeOpError> {
    let mut edit = RawTreeEdit::load(view, &doc.project_id)?;
    let answer = DocsWriteResult {
        document: wire_doc_summary(&doc.entry()),
    };
    edit.put_doc(doc)?;
    edit.finish(reply(&answer))
}

fn touch(doc: &mut RawProjectDoc, ctx: &TreeCaller<'_>) {
    doc.meta.revision += 1;
    doc.meta.updated_at = Some(ctx.now_rfc3339.to_owned());
    doc.meta.updated_by = DocActor::Employee(ctx.caller_id.to_owned());
}

/// Pure `docs.create` decision.
pub fn decide_create<V: ProjectTreeView>(
    view: &V,
    ctx: &TreeCaller<'_>,
    params: &DocsCreateParams,
    new_id: String,
) -> Result<TreeMutation, TreeOpError> {
    let kind = validate_kind(params.kind.trim())?;
    let title = validate_title(&params.title)?;
    let path = resolve_new_path(params.path.as_deref(), params.section.as_deref(), &title)?;
    let summary = validate_summary(params.summary.as_deref())?;
    let body_markdown = validate_body(&params.body_markdown)?;
    let status = params
        .status
        .map(validate_status)
        .transpose()?
        .unwrap_or(DocStatus::Draft);
    let mut owner_employee_id = validate_owner(view, params.owner_employee_id.as_deref())?;
    if status == DocStatus::Current && owner_employee_id.is_none() {
        owner_employee_id = Some(ctx.caller_id.to_owned());
    }
    let actor = DocActor::Employee(ctx.caller_id.to_owned());
    let ts = Some(ctx.now_rfc3339.to_owned());
    let doc = RawProjectDoc {
        id: new_id,
        project_id: ctx.project_id.to_owned(),
        path,
        kind,
        title,
        status,
        summary,
        owner_employee_id,
        body_markdown,
        meta: DocMeta {
            revision: 1,
            created_at: ts.clone(),
            created_by: actor.clone(),
            updated_at: ts,
            updated_by: actor,
        },
    };
    write_doc(view, doc)
}

/// Pure `docs.update` decision.
pub fn decide_update<V: ProjectTreeView>(
    view: &V,
    ctx: &TreeCaller<'_>,
    params: &DocsUpdateParams,
) -> Result<TreeMutation, TreeOpError> {
    let before = current_doc(
        view,
        ctx.project_id,
        &params.document_id,
        params.expected_revision,
    )?;
    let mut doc = before.clone();
    if let Some(title) = &params.title {
        doc.title = validate_title(title)?;
    }
    if let Some(kind) = &params.kind {
        doc.kind = validate_kind(kind.trim())?;
    }
    if let Some(status) = params.status {
        doc.status = validate_status(status)?;
    }
    if let Some(summary) = &params.summary {
        doc.summary = validate_summary(Some(summary))?;
    }
    if let Some(owner) = &params.owner_employee_id {
        doc.owner_employee_id = validate_owner(view, Some(owner))?;
        if doc.owner_employee_id.is_none() && doc.status == DocStatus::Current {
            return Err(invalid(
                "owner_employee_id",
                "a current document keeps an owner: name another, or change its status first",
            ));
        }
    }
    if let Some(body) = &params.body_markdown {
        doc.body_markdown = validate_body(body)?;
    }
    if doc.status == DocStatus::Current && doc.owner_employee_id.is_none() {
        doc.owner_employee_id = Some(ctx.caller_id.to_owned());
    }
    if doc == before {
        return Err(invalid("document", "nothing to change"));
    }
    touch(&mut doc, ctx);
    write_doc(view, doc)
}

/// Pure `docs.move` decision.
pub fn decide_move<V: ProjectTreeView>(
    view: &V,
    ctx: &TreeCaller<'_>,
    params: &DocsMoveParams,
) -> Result<TreeMutation, TreeOpError> {
    let mut doc = current_doc(
        view,
        ctx.project_id,
        &params.document_id,
        params.expected_revision,
    )?;
    let target = match (params.new_path.as_deref(), params.new_section.as_deref()) {
        (Some(path), None) => DocPath::parse(path).map_err(|e| path_err("new_path", &e))?,
        (None, Some(section)) => {
            let sec = Section::parse(section).map_err(|e| path_err("section", &e))?;
            DocPath::join(&sec, doc.path.file_name()).map_err(|e| path_err("new_section", &e))?
        }
        _ => {
            return Err(invalid(
                "new_path",
                "give exactly one of new_path and new_section",
            ))
        }
    };
    if target == doc.path {
        return Err(invalid("new_path", "the document is already there"));
    }
    doc.path = target;
    touch(&mut doc, ctx);
    write_doc(view, doc)
}

/// Pure `docs.delete` decision: refuses with `Conflict` if any Why link references the document.
pub fn decide_delete<V: ProjectTreeView>(
    view: &V,
    ctx: &TreeCaller<'_>,
    params: &DocsDeleteParams,
) -> Result<TreeMutation, TreeOpError> {
    let doc = current_doc(
        view,
        ctx.project_id,
        &params.document_id,
        params.expected_revision,
    )?;
    let blocking = stor(view.document_link_ids(&doc.id))?;
    if !blocking.is_empty() {
        return Err(refused(
            ErrorCode::Conflict,
            format!(
                "cannot delete document `{}` while referenced by link(s): {}; remove those links first",
                doc.id,
                blocking.join(", ")
            ),
        ));
    }
    let mut edit = RawTreeEdit::load(view, &doc.project_id)?;
    let answer = DocsDeleteResult {
        deleted_id: doc.id.clone(),
        deleted_path: doc.path.as_str().to_owned(),
        index_path: index_durable_path(&doc.project_id),
    };
    edit.remove_doc(&doc)?;
    edit.finish(reply(&answer))
}
