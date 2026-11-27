//! `docs.create`: a new document in a project's tree, and the regenerated index, in one entry.

use org_proto::docs::{DocsCreateParams, DocsWriteResult};
use org_store::{Change, Id, Timestamp};

use super::change::TreeEdit;
use super::{fields, reply, wire};
use crate::organization::records::{By, Meta};
use crate::project_docs::access::{authorize, Denied, DocsAccess};
use crate::project_docs::ports::ProjectDocsRead;
use crate::project_docs::records::{DocStatus, ProjectDoc};

/// Decides `docs.create` for `caller` on `view`; `id` is the new document's ID.
pub fn decide<R: ProjectDocsRead>(
    view: &R,
    now: Timestamp,
    caller: &str,
    params: &DocsCreateParams,
    id: Id,
) -> Result<Change, Denied> {
    let cleared = authorize(view, caller, &params.project, DocsAccess::Write)?;
    let kind = fields::kind(params.kind.trim())?;
    let title = fields::title(&params.title)?;
    let path = fields::new_path(params.path.as_deref(), params.section.as_deref(), &title)?;
    let summary = fields::summary(params.summary.as_deref())?;
    let body_markdown = fields::body(&params.body_markdown)?;
    let status = params
        .status
        .map(fields::status)
        .transpose()?
        .unwrap_or(DocStatus::Draft);
    let mut owner_employee_id = fields::owner(view, params.owner_employee_id.as_deref())?;
    if status == DocStatus::Current && owner_employee_id.is_none() {
        owner_employee_id = Some(cleared.caller.id.clone());
    }
    let mut edit = TreeEdit::load(view, &cleared.project.id)?;
    let doc = ProjectDoc {
        id,
        project_id: cleared.project.id,
        path,
        kind,
        title,
        status,
        summary,
        owner_employee_id,
        body_markdown,
        meta: Meta::created(&By::Employee(cleared.caller.id), now),
    };
    let answer = DocsWriteResult {
        document: wire::doc_summary(&doc.entry()),
    };
    edit.put_doc(doc)?;
    edit.finish(reply(&answer))
}
