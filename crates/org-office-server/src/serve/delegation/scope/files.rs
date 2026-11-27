//! The files the Chief of Staff touches for a member (`TASK-240`). A file bound to a message,
//! an ask or an answer is in that message's conversation, and she inspects, reads, lists,
//! edits or re-attaches it only for a caller who sees that conversation too
//! ([`super::visible_ids`]). She is in every member's DM with her and the owner is not, so
//! even acting for the owner she never opens a file another member sent her.
//!
//! - **Before a call runs** ([`admit`]): `attachment.inspect`, `attachment.read_chunk` and
//!   `attachment.update_meta` by the file's conversation; `attachment.attach` by its source
//!   file's and by the message or ask it names as its target; `attachment.list` of a
//!   message's or ask's files by that message's.
//! - **On what it answers** ([`narrow`]): `attachment.list` and `attachment.inspect` keep only
//!   files of messages and asks in a conversation the caller sees.
//!
//! Any other file (a project's, the organization's, a private one) is the policy's to decide.
//! A call whose parameters do not decode is refused, and so is one the office cannot check.

use org_proto::attachments::{
    AttachmentAttach, AttachmentInspect, AttachmentList, AttachmentReadChunk, AttachmentTargetKind,
    AttachmentUpdateMeta, AttachmentView,
};
use org_proto::{Call, Request, Response};

use super::{denied, params, rewrite, View};

/// Refuses a call on a file of a message, ask or answer the caller does not see.
pub(super) fn admit(request: &Request, view: &View<'_>) -> Result<(), Response> {
    match request.method.as_str() {
        AttachmentInspect::METHOD => {
            file(view, &decoded::<AttachmentInspect>(request)?.attachment_id)
        }
        AttachmentReadChunk::METHOD => file(
            view,
            &decoded::<AttachmentReadChunk>(request)?.attachment_id,
        ),
        AttachmentUpdateMeta::METHOD => file(
            view,
            &decoded::<AttachmentUpdateMeta>(request)?.attachment_id,
        ),
        AttachmentAttach::METHOD => {
            let p = decoded::<AttachmentAttach>(request)?;
            if let Some(source) = &p.source_attachment_id {
                file(view, source)?;
            }
            of_message(view, Some(p.target_kind), p.target_id.as_deref())
        }
        AttachmentList::METHOD => {
            let p = decoded::<AttachmentList>(request)?;
            of_message(view, p.target_kind, p.target_id.as_deref())
        }
        _ => Ok(()),
    }
}

/// Keeps only the files of messages and asks in a conversation the caller sees.
pub(super) fn narrow(request: &Request, view: &View<'_>, response: Response) -> Response {
    match request.method.as_str() {
        AttachmentList::METHOD => rewrite::<AttachmentList>(response, |list| {
            let mut kept = Vec::with_capacity(list.attachments.len());
            for file in std::mem::take(&mut list.attachments) {
                if sees(view, &file)? {
                    kept.push(file);
                }
            }
            list.attachments = kept;
            Some(true)
        }),
        AttachmentInspect::METHOD => {
            rewrite::<AttachmentInspect>(response, |found| sees(view, &found.attachment))
        }
        _ => response,
    }
}

fn decoded<C: Call>(request: &Request) -> Result<C::Params, Response> {
    params::<C>(request).ok_or_else(|| denied(request.id))
}

fn of_a_message(kind: AttachmentTargetKind) -> bool {
    matches!(
        kind,
        AttachmentTargetKind::Message | AttachmentTargetKind::AskAnswer
    )
}

/// The file `attachment_id`: one of a message, ask or answer only in a conversation the caller
/// sees; any other (or an unknown one) is the method's to decide.
fn file(view: &View<'_>, attachment_id: &str) -> Result<(), Response> {
    match view.port.attachment_conversation(attachment_id) {
        Ok(Some(conversation)) => view.conversation(&conversation),
        Ok(None) => Ok(()),
        Err(err) => Err(view.fail(err)),
    }
}

/// A message's or an ask's files as a call's target: only of one in a conversation the caller
/// sees. Naming none, or one the office does not know, is refused.
fn of_message(
    view: &View<'_>,
    kind: Option<AttachmentTargetKind>,
    target_id: Option<&str>,
) -> Result<(), Response> {
    if !kind.is_some_and(of_a_message) {
        return Ok(());
    }
    let Some(message) = target_id else {
        return Err(denied(view.id));
    };
    match view.port.message_conversation(message) {
        Ok(Some(conversation)) => view.conversation(&conversation),
        Ok(None) => Err(denied(view.id)),
        Err(err) => Err(view.fail(err)),
    }
}

/// Whether the caller may be shown `file`; `None` when the office cannot say.
fn sees(view: &View<'_>, file: &AttachmentView) -> Option<bool> {
    if !of_a_message(file.target_kind) {
        return Some(true);
    }
    let message = file.target_id.as_deref()?;
    let conversation = view.port.message_conversation(message).ok()??;
    let visible = view.visible().ok()?;
    Some(visible.contains(&conversation))
}
