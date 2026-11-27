//! Project documentation tree wire contract (`TASK-55`, `L11`, `FC-180`): the Markdown documents
//! under `state/projects/<project-id>/docs/`, their media assets under `docs/media/`, and the
//! office-maintained `docs/index.yaml`.
//!
//! - Reads: `docs.index` (the whole tree), `docs.list` (filtered, paginated), `docs.inspect`
//!   (one document with its body).
//! - Writes: `docs.create`, `docs.update` (optimistic `expected_revision`), `docs.move` (an
//!   atomic path move) and `docs.put_media` (a validated image asset). Every write carries a
//!   UUIDv7 `command_id`: an exact retry answers the first reply, the same id with other params
//!   is `operation_reused`.
//!
//! Documents are addressed by their stable UUIDv7 `id` or by their `path` relative to `docs/`
//! (`guides/setup.md`). A project is named by UUIDv7, `P-<number>` or slug. An agent outside the
//! project's accountable and contributing teams is told `not_found`, exactly as for a project
//! that does not exist. Media bytes travel base64-encoded and `Debug` never prints them.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::Call;

/// Largest Markdown body of one document, in bytes.
pub const DOC_BODY_MAX_BYTES: usize = 256 * 1024;
/// Longest document or media title, in Unicode scalar values.
pub const DOC_TITLE_MAX_CHARS: usize = 200;
/// Longest document or media summary, in bytes.
pub const DOC_SUMMARY_MAX_BYTES: usize = 2 * 1024;
/// Longest document path relative to `docs/`, in bytes.
pub const DOC_PATH_MAX_BYTES: usize = 512;
/// Longest section (folder below `docs/`), in bytes.
pub const DOC_SECTION_MAX_BYTES: usize = 256;
/// Longest document kind, in bytes.
pub const DOC_KIND_MAX_BYTES: usize = 64;
/// Longest project, document or cursor reference a caller may send, in bytes.
pub const DOC_REF_MAX_BYTES: usize = 600;
/// `docs.list` page size when none is asked for.
pub const DOC_LIST_DEFAULT_LIMIT: u32 = 50;
/// Largest `docs.list` page.
pub const DOC_LIST_MAX_LIMIT: u32 = 200;
/// Largest decoded media asset `docs.put_media` takes (512 KiB): the request stays inside
/// [`crate::MAX_FRAME_BYTES`].
pub const DOC_MEDIA_MAX_BYTES: usize = 512 * 1024;
/// Largest base64 text of a media asset: `4 * ceil(512 KiB / 3)`.
pub const DOC_MEDIA_MAX_BASE64_CHARS: usize = 699_052;

/// Lifecycle state of a project document. `deprecated` is how a document is archived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocStatusWire {
    Draft,
    Current,
    Deprecated,
    #[serde(other)]
    Unknown,
}

/// What a media asset depicts (`documentation_media.type`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocMediaKindWire {
    #[default]
    Image,
    Diagram,
    #[serde(other)]
    Unknown,
}

/// One project document without its body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocSummaryWire {
    pub id: String,
    pub project_id: String,
    /// Relative to `docs/`, e.g. `guides/setup.md`.
    pub path: String,
    /// The folder of `path` (`guides`); empty at the top of the tree.
    pub section: String,
    pub kind: String,
    pub title: String,
    pub status: DocStatusWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_employee_id: Option<String>,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by_id: Option<String>,
}

/// One media asset of a project's documentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocMediaWire {
    pub id: String,
    pub project_id: String,
    /// Relative to `docs/`, e.g. `media/architecture.png`.
    pub path: String,
    #[serde(rename = "type")]
    pub kind: DocMediaKindWire,
    pub media_type: String,
    pub bytes: u64,
    pub sha256: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// One folder of the tree and how many documents sit directly in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocSectionWire {
    pub path: String,
    pub document_count: u64,
}

/// `docs.index`: the project's whole documentation tree, as `docs/index.yaml` records it.
pub struct DocsIndex;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocsIndexParams {
    pub project: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocsIndexResult {
    pub project_id: String,
    /// The office-maintained index file, relative to the durable state tree.
    pub index_path: String,
    /// Every folder holding a document, in path order (the top level is `""`).
    pub sections: Vec<DocSectionWire>,
    /// Every document, in path order.
    pub documents: Vec<DocSummaryWire>,
    /// Every media asset, in path order.
    pub media: Vec<DocMediaWire>,
}

impl Call for DocsIndex {
    const METHOD: &'static str = "docs.index";
    type Params = DocsIndexParams;
    type Result = DocsIndexResult;
}

/// `docs.list`: documents of one project, filtered, in path order, a page at a time.
pub struct DocsList;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocsListParams {
    pub project: String,
    /// Only documents in this folder or below it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<DocStatusWire>,
    /// Page size, 1 to [`DOC_LIST_MAX_LIMIT`]; [`DOC_LIST_DEFAULT_LIMIT`] when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// The `next_cursor` of the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocsListResult {
    pub documents: Vec<DocSummaryWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl Call for DocsList {
    const METHOD: &'static str = "docs.list";
    type Params = DocsListParams;
    type Result = DocsListResult;
}

/// `docs.inspect`: one document's front matter, body and revision.
pub struct DocsInspect;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocsInspectParams {
    pub project: String,
    /// The document's UUIDv7 `id` or its path relative to `docs/`.
    pub document: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocsInspectResult {
    pub document: DocSummaryWire,
    pub body_markdown: String,
}

impl Call for DocsInspect {
    const METHOD: &'static str = "docs.inspect";
    type Params = DocsInspectParams;
    type Result = DocsInspectResult;
}

/// What a document write answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocsWriteResult {
    pub document: DocSummaryWire,
}

/// `docs.create`: a new document. Its path is `path`, or `<section>/<title-slug>.md`.
pub struct DocsCreate;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocsCreateParams {
    pub command_id: String,
    /// Set from the route by the web gateway.
    #[serde(default)]
    pub project: String,
    pub kind: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_employee_id: Option<String>,
    /// `draft` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<DocStatusWire>,
    #[serde(default)]
    pub body_markdown: String,
}

impl Call for DocsCreate {
    const METHOD: &'static str = "docs.create";
    type Params = DocsCreateParams;
    type Result = DocsWriteResult;
}

/// `docs.update`: changes a document at `expected_revision`. An absent field is kept; an empty
/// `summary` or `owner_employee_id` clears it.
pub struct DocsUpdate;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocsUpdateParams {
    pub command_id: String,
    /// Set from the route by the web gateway.
    #[serde(default)]
    pub project: String,
    /// Set from the route by the web gateway.
    #[serde(default)]
    pub document_id: String,
    pub expected_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<DocStatusWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_employee_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_markdown: Option<String>,
}

impl Call for DocsUpdate {
    const METHOD: &'static str = "docs.update";
    type Params = DocsUpdateParams;
    type Result = DocsWriteResult;
}

/// `docs.move`: moves a document to `new_path`, or into `new_section` keeping its file name.
pub struct DocsMove;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocsMoveParams {
    pub command_id: String,
    /// Set from the route by the web gateway.
    #[serde(default)]
    pub project: String,
    /// Set from the route by the web gateway.
    #[serde(default)]
    pub document_id: String,
    pub expected_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_section: Option<String>,
}

impl Call for DocsMove {
    const METHOD: &'static str = "docs.move";
    type Params = DocsMoveParams;
    type Result = DocsWriteResult;
}

/// `docs.put_media`: stores or replaces one image asset at `docs/media/<file_name>`.
pub struct DocsPutMedia;

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocsPutMediaParams {
    pub command_id: String,
    #[serde(default)]
    pub project: String,
    /// One lowercase file name (`architecture.png`), its extension that of `media_type`.
    pub file_name: String,
    /// `image/png`, `image/jpeg`, `image/webp` or `image/gif`.
    pub media_type: String,
    #[serde(rename = "type")]
    pub kind: DocMediaKindWire,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub content_base64: String,
    /// Required to replace an existing asset; absent to create one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

impl fmt::Debug for DocsPutMediaParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DocsPutMediaParams")
            .field("command_id", &self.command_id)
            .field("project", &self.project)
            .field("file_name", &self.file_name)
            .field("media_type", &self.media_type)
            .field("kind", &self.kind)
            .field("title", &self.title)
            .field("summary", &self.summary)
            .field(
                "content_base64",
                &format_args!("<{} chars>", self.content_base64.len()),
            )
            .field("expected_revision", &self.expected_revision)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocsPutMediaResult {
    pub media: DocMediaWire,
}

impl Call for DocsPutMedia {
    const METHOD: &'static str = "docs.put_media";
    type Params = DocsPutMediaParams;
    type Result = DocsPutMediaResult;
}

#[cfg(test)]
#[path = "docs_tests.rs"]
mod tests;
