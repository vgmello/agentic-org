//! The office-maintained `docs/index.yaml` of one project, delegated to
//! `org_docs::project_tree::render_project_index`.

pub use org_docs::project_tree::INDEX_MAX_BYTES;
use org_docs::project_tree::{render_project_index, IndexDocRow, IndexMediaRow};
use org_store::Id;

use super::records::{DocEntry, DocMedia};

/// The index file of `project` for exactly these documents and media, in any order.
pub fn render(project: &Id, docs: &[DocEntry], media: &[DocMedia]) -> Vec<u8> {
    let doc_rows: Vec<IndexDocRow<'_>> = docs
        .iter()
        .map(|d| IndexDocRow {
            id: d.id.as_str(),
            path: &d.path,
            kind: &d.kind,
            title: &d.title,
            status: d.status,
            summary: d.summary.as_deref(),
            owner_employee_id: d.owner_employee_id.as_ref().map(Id::as_str),
            revision: d.meta.revision,
        })
        .collect();
    let media_rows: Vec<IndexMediaRow<'_>> = media
        .iter()
        .map(|m| IndexMediaRow {
            id: m.id.as_str(),
            file: &m.file,
            kind: m.kind,
            media_type: &m.media_type,
            bytes: m.bytes,
            sha256: &m.sha256,
            title: &m.title,
            revision: m.meta.revision,
        })
        .collect();
    render_project_index(project.as_str(), &doc_rows, &media_rows)
}
