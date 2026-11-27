//! `docs.update` and `docs.move`: change a document at its `expected_revision`, or move it to
//! another path; either way the regenerated index lands in the same entry.

use org_proto::docs::{DocsMoveParams, DocsUpdateParams, DocsWriteResult};
use org_store::{Change, Timestamp};

use super::change::TreeEdit;
use super::{fields, reply, wire};
use crate::organization::records::{time_text, By};
use crate::project_docs::access::{authorize, Authorized, Denied, DocsAccess};
use crate::project_docs::paths::DocPath;
use crate::project_docs::ports::ProjectDocsRead;
use crate::project_docs::records::{DocStatus, ProjectDoc};

/// The document `document_id` names in the cleared project, at `expected_revision`.
fn current<R: ProjectDocsRead>(
    view: &R,
    cleared: &Authorized,
    document_id: &str,
    expected_revision: u64,
) -> Result<ProjectDoc, Denied> {
    let id = fields::uuid("document_id", document_id.trim())?;
    let doc = fields::find_doc(view, &cleared.project.id, id.as_str())?;
    fields::revision(doc.meta.revision, expected_revision)?;
    Ok(doc)
}

fn touch(doc: &mut ProjectDoc, cleared: &Authorized, now: Timestamp) {
    doc.meta.revision += 1;
    doc.meta.updated_at = Some(time_text(now));
    doc.meta.updated_by = By::Employee(cleared.caller.id.clone());
}

fn write(view: &impl ProjectDocsRead, doc: ProjectDoc) -> Result<Change, Denied> {
    let mut edit = TreeEdit::load(view, &doc.project_id)?;
    let answer = DocsWriteResult {
        document: wire::doc_summary(&doc.entry()),
    };
    edit.put_doc(doc)?;
    edit.finish(reply(&answer))
}

/// Decides `docs.update`. An absent field is kept; an empty summary or owner clears it (a
/// `current` document's owner is never cleared).
pub fn decide_update<R: ProjectDocsRead>(
    view: &R,
    now: Timestamp,
    caller: &str,
    params: &DocsUpdateParams,
) -> Result<Change, Denied> {
    let cleared = authorize(view, caller, &params.project, DocsAccess::Write)?;
    let before = current(
        view,
        &cleared,
        &params.document_id,
        params.expected_revision,
    )?;
    let mut doc = before.clone();
    if let Some(title) = &params.title {
        doc.title = fields::title(title)?;
    }
    if let Some(kind) = &params.kind {
        doc.kind = fields::kind(kind.trim())?;
    }
    if let Some(status) = params.status {
        doc.status = fields::status(status)?;
    }
    if let Some(summary) = &params.summary {
        doc.summary = fields::summary(Some(summary))?;
    }
    if let Some(owner) = &params.owner_employee_id {
        doc.owner_employee_id = fields::owner(view, Some(owner))?;
        // Clearing a current document's owner is refused, never quietly given to the caller.
        if doc.owner_employee_id.is_none() && doc.status == DocStatus::Current {
            return Err(fields::invalid(
                "owner_employee_id",
                "a current document keeps an owner: name another, or change its status first",
            ));
        }
    }
    if let Some(body) = &params.body_markdown {
        doc.body_markdown = fields::body(body)?;
    }
    if doc.status == DocStatus::Current && doc.owner_employee_id.is_none() {
        doc.owner_employee_id = Some(cleared.caller.id.clone());
    }
    if doc == before {
        return Err(fields::invalid("document", "nothing to change"));
    }
    touch(&mut doc, &cleared, now);
    write(view, doc)
}

/// Decides `docs.move`: to `new_path`, or into `new_section` under the same file name.
pub fn decide_move<R: ProjectDocsRead>(
    view: &R,
    now: Timestamp,
    caller: &str,
    params: &DocsMoveParams,
) -> Result<Change, Denied> {
    let cleared = authorize(view, caller, &params.project, DocsAccess::Write)?;
    let mut doc = current(
        view,
        &cleared,
        &params.document_id,
        params.expected_revision,
    )?;
    let target = match (params.new_path.as_deref(), params.new_section.as_deref()) {
        (Some(path), None) => {
            DocPath::parse(path).map_err(|e| fields::path_error("new_path", &e))?
        }
        (None, Some(section)) => DocPath::join(&fields::section(section)?, doc.path.file_name())
            .map_err(|e| fields::path_error("new_section", &e))?,
        _ => {
            return Err(fields::invalid(
                "new_path",
                "give exactly one of new_path and new_section",
            ))
        }
    };
    if target == doc.path {
        return Err(fields::invalid("new_path", "the document is already there"));
    }
    doc.path = target;
    touch(&mut doc, &cleared, now);
    write(view, doc)
}
