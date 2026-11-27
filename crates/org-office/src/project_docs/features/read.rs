//! `docs.index`, `docs.list` and `docs.inspect`: reads of one project's tree on one consistent
//! view, each cleared by [`authorize`] first.

use std::collections::BTreeMap;

use org_proto::docs::{
    DocSectionWire, DocsIndexParams, DocsIndexResult, DocsInspectParams, DocsInspectResult,
    DocsListParams, DocsListResult, DOC_LIST_DEFAULT_LIMIT, DOC_LIST_MAX_LIMIT,
};

use super::{fields, wire};
use crate::project_docs::access::{authorize, Denied, DocsAccess};
use crate::project_docs::paths::{self, DocPath};
use crate::project_docs::ports::ProjectDocsRead;

/// `docs.index`: the whole tree, as `index.yaml` records it.
pub fn index<R: ProjectDocsRead>(
    view: &R,
    caller: &str,
    params: &DocsIndexParams,
) -> Result<DocsIndexResult, Denied> {
    let project = authorize(view, caller, &params.project, DocsAccess::Read)?.project;
    let docs = view.project_doc_entries(&project.id)?;
    let mut sections: BTreeMap<String, u64> = BTreeMap::new();
    for doc in &docs {
        *sections
            .entry(doc.path.section().as_str().to_owned())
            .or_default() += 1;
    }
    Ok(DocsIndexResult {
        project_id: project.id.to_string(),
        index_path: paths::index_path(&project.id).as_str().to_owned(),
        sections: sections
            .into_iter()
            .map(|(path, document_count)| DocSectionWire {
                path,
                document_count,
            })
            .collect(),
        documents: docs.iter().map(wire::doc_summary).collect(),
        media: view
            .project_doc_media(&project.id)?
            .iter()
            .map(wire::media)
            .collect(),
    })
}

/// `docs.list`: documents in path order, filtered by section (and below), kind and status,
/// after `cursor`, at most `limit`.
pub fn list<R: ProjectDocsRead>(
    view: &R,
    caller: &str,
    params: &DocsListParams,
) -> Result<DocsListResult, Denied> {
    let project = authorize(view, caller, &params.project, DocsAccess::Read)?.project;
    let limit = params.limit.unwrap_or(DOC_LIST_DEFAULT_LIMIT);
    if !(1..=DOC_LIST_MAX_LIMIT).contains(&limit) {
        return Err(fields::invalid("limit", "1 to 200"));
    }
    let section = params.section.as_deref().map(fields::section).transpose()?;
    let kind = params.kind.as_deref().map(fields::kind).transpose()?;
    let status = params.status.map(fields::status).transpose()?;
    let after = params
        .cursor
        .as_deref()
        .map(|c| DocPath::parse(c).map_err(|e| fields::path_error("cursor", &e)))
        .transpose()?;
    let mut matching = view
        .project_doc_entries(&project.id)?
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
        documents: page.iter().map(wire::doc_summary).collect(),
        next_cursor,
    })
}

/// `docs.inspect`: one document by UUIDv7 or path, with its body.
pub fn inspect<R: ProjectDocsRead>(
    view: &R,
    caller: &str,
    params: &DocsInspectParams,
) -> Result<DocsInspectResult, Denied> {
    let project = authorize(view, caller, &params.project, DocsAccess::Read)?.project;
    let doc = fields::find_doc(view, &project.id, params.document.trim())?;
    Ok(DocsInspectResult {
        document: wire::doc_summary(&doc.entry()),
        body_markdown: doc.body_markdown,
    })
}
