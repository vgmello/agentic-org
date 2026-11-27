//! What the Chief of Staff sends and asks for a member (`TASK-240`), checked before it runs: a
//! message or an ask goes only where the caller is, and carries staged files only for the
//! owner.
//!
//! - **Where it goes**: `to`, read in the order a send resolves it, names a conversation the
//!   caller sees (a channel they reach, a DM or group DM they are an active member of), a
//!   channel they reach by `#name` or bare name, or the caller themself (her DM with them).
//!   Any other member is refused: acting for one member she never writes into another
//!   member's DM with her (the owner's included), and acting for nobody into no DM at all. A
//!   `reply_to` must be in a conversation the caller sees, as `msg.reply` must.
//! - **Files**: a message, ask or answer naming staged uploads is admitted only while she
//!   acts for the live owner, the only member she may stage uploads for (`attachment.upload_*`
//!   needs the owner): no other member's turn and no wake ever binds them. The office checks
//!   the upload itself too: it is bound only for the member it was staged for
//!   (`org_office::attachments::staging`).
//! - **No hold**: `msg.ask` must not hold. `chat.ask` never holds for her: the office answers
//!   it at once (`chat_calls::ask`), as `chat.wait_ask` is never hers.
//!
//! A call whose parameters do not decode is refused here (fail closed).

use org_proto::ask::{ChatAnswer, ChatAsk};
use org_proto::chat::{ChatSend, ConversationKind};
use org_proto::msg::{MsgAsk, MsgReply, MsgSend};
use org_proto::tasks::{TasksComment, TasksCreate, TasksUpdate};
use org_proto::{Call, Request, Response};

use super::files::file as visible_file;
use super::{denied, params, require, Delegation, View};

/// Refuses a send, ask, reply, answer, or task/comment mutation carrying attachments when it goes
/// where the caller is not, binds files for anyone but the live owner, or re-attaches a message
/// file the caller does not see (`TASK-240`, `TASK-98.4`).
pub(super) fn admit(
    request: &Request,
    view: &View<'_>,
    delegation: &Delegation,
) -> Result<(), Response> {
    let files = |ids: &[String]| -> Result<(), Response> {
        owner_files(view.id, delegation, ids)?;
        for id in ids {
            visible_file(view, id)?;
        }
        Ok(())
    };
    match request.method.as_str() {
        ChatSend::METHOD => {
            let p = decoded::<ChatSend>(request)?;
            files(&p.attachment_ids)?;
            send(view, &p.to, p.reply_to.as_deref())
        }
        MsgSend::METHOD => {
            let p = decoded::<MsgSend>(request)?;
            files(&p.attachment_ids)?;
            send(view, &p.to, p.reply_to.as_deref())
        }
        ChatAsk::METHOD => {
            let p = decoded::<ChatAsk>(request)?;
            files(&p.attachment_ids)?;
            send(view, &p.to, None)
        }
        MsgAsk::METHOD => {
            let p = decoded::<MsgAsk>(request)?;
            files(&p.attachment_ids)?;
            require(view.id, p.wait_ms == 0)?;
            send(view, &p.to, None)
        }
        // Where a reply or an answer goes is its parent's conversation ([`super::admit`]).
        MsgReply::METHOD => files(&decoded::<MsgReply>(request)?.attachment_ids),
        ChatAnswer::METHOD => files(&decoded::<ChatAnswer>(request)?.attachment_ids),
        TasksCreate::METHOD => files(&decoded::<TasksCreate>(request)?.attachment_ids),
        TasksUpdate::METHOD => files(&decoded::<TasksUpdate>(request)?.add_attachments),
        TasksComment::METHOD => files(&decoded::<TasksComment>(request)?.attachment_ids),
        _ => Ok(()),
    }
}

fn decoded<C: Call>(request: &Request) -> Result<C::Params, Response> {
    params::<C>(request).ok_or_else(|| denied(request.id))
}

/// Staged files ride a message, ask or answer only while she acts for the live owner.
fn owner_files(
    id: u64,
    delegation: &Delegation,
    attachment_ids: &[String],
) -> Result<(), Response> {
    let caller = &delegation.caller;
    let owner = caller.active && caller.is_person && caller.is_owner;
    require(id, attachment_ids.is_empty() || owner)
}

/// A send to `to`, in reply to `reply_to`.
fn send(view: &View<'_>, to: &str, reply_to: Option<&str>) -> Result<(), Response> {
    addressed(view, to)?;
    match reply_to {
        Some(parent) => view.message(parent),
        None => Ok(()),
    }
}

/// `to`, read in the order a send resolves it: a conversation by ID, a channel by `#name`, a
/// member (her DM with them), a channel by bare name.
fn addressed(view: &View<'_>, to: &str) -> Result<(), Response> {
    let to = to.trim();
    if view.visible()?.iter().any(|v| v == to) {
        return Ok(());
    }
    if !to.starts_with('#') {
        let named = view.port.resolve_member(to).map_err(|e| view.fail(e))?;
        if let Some(member) = named {
            return require(view.id, view.caller == Some(member.as_str()));
        }
    }
    channel_named(view, to)
}

/// A channel by name (`#name` or `name`, in any case): admitted only when she reaches a
/// channel of that name and every one she reaches is one the caller sees.
fn channel_named(view: &View<'_>, reference: &str) -> Result<(), Response> {
    let name = reference.trim_start_matches('#').to_lowercase();
    let hers = view
        .port
        .visible_conversations(view.member)
        .map_err(|e| view.fail(e))?;
    let named: Vec<String> = hers
        .into_iter()
        .filter(|c| {
            c.kind == ConversationKind::Channel
                && c.name.as_deref().is_some_and(|n| n.to_lowercase() == name)
        })
        .map(|c| c.id)
        .collect();
    let visible = view.visible()?;
    require(
        view.id,
        !named.is_empty() && named.iter().all(|c| visible.contains(c)),
    )
}
