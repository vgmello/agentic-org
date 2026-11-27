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

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_proto::attachments::{
    AttachmentAttach, AttachmentInspect, AttachmentList, AttachmentReadChunk, AttachmentRender,
    AttachmentRenderParams, AttachmentTargetKind, AttachmentUpdateMeta, AttachmentView,
    RenderSourceKind, ATTACHMENT_CHUNK_MAX_BASE64_CHARS,
};
use org_proto::{ApiError, Call, ErrorCode, Outcome, Request, Response};

use super::{denied, params, rewrite, View};
use crate::serve::delegation::{ChiefOfStaffPort, Delegation, DELEGATION_DENIED};

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
        AttachmentRender::METHOD => {
            let p = decoded::<AttachmentRender>(request)?;
            admit_render(view, &p)
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

/// Single choke-point enforcing `caller ∩ tess` for one attachment ID under Chief-of-Staff
/// delegation (used both pre-dispatch in [`admit`] and post-assembly inside `attachment.render`).
pub(crate) fn check_delegated_attachment(
    port: &dyn ChiefOfStaffPort,
    member: &str,
    delegation: &Delegation,
    attachment_id: &str,
) -> Result<(), ApiError> {
    let view = View::new(0, port, member, delegation.caller_id.as_deref());
    file(&view, attachment_id).map_err(|resp| match resp.outcome {
        Outcome::Error(err) => err,
        Outcome::Result(_) => ApiError::new(ErrorCode::NotAuthorized, DELEGATION_DENIED),
    })
}

fn admit_render(view: &View<'_>, p: &AttachmentRenderParams) -> Result<(), Response> {
    let kind = match p.source_kind {
        RenderSourceKind::Markdown => org_doc_render::SourceKind::Markdown,
        RenderSourceKind::Html => org_doc_render::SourceKind::Html,
    };
    for att_id in org_doc_render::extract_attachment_refs(kind, &p.source, p.css.as_deref()) {
        file(view, &att_id)?;
    }
    admit_render_chunk_refs(view, kind, p)?;
    of_message(view, p.target_kind, p.target_id.as_deref())
}

fn admit_render_chunk_refs(
    view: &View<'_>,
    kind: org_doc_render::SourceKind,
    p: &AttachmentRenderParams,
) -> Result<(), Response> {
    let src_chunk = decode_utf8_chunk(p.source_chunk_base64.as_deref());
    let css_chunk = decode_utf8_chunk(p.css_chunk_base64.as_deref());
    if src_chunk.is_some() || css_chunk.is_some() {
        let src = src_chunk.as_deref().unwrap_or("");
        for att_id in org_doc_render::extract_attachment_refs(kind, src, css_chunk.as_deref()) {
            file(view, &att_id)?;
        }
    }
    Ok(())
}

fn decode_utf8_chunk(raw: Option<&str>) -> Option<String> {
    let text = raw?;
    if text.len() > ATTACHMENT_CHUNK_MAX_BASE64_CHARS {
        return None;
    }
    let bytes = STANDARD.decode(text.as_bytes()).ok()?;
    String::from_utf8(bytes).ok()
}

/// The file `attachment_id`: one of a message, ask or answer only in a conversation the caller
/// sees; any other (or an unknown one) is the method's to decide.
pub(super) fn file(view: &View<'_>, attachment_id: &str) -> Result<(), Response> {
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
