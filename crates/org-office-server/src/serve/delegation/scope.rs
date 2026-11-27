//! What one call the Chief of Staff makes for a member may touch and show (`TASK-80.1`): the
//! `caller ∩ tess` rule applied to a call's parameters and to what it answers, where the
//! method's class alone ([`super::access`]) cannot decide.
//!
//! - **Hiring, per team** ([`admit`]): `hr.hire` and `hr.hire.draft` need a caller who may hire
//!   into the request's team; `hr.hire.inspect` and `hr.hire.list` show only hires into teams
//!   the caller may hire into. `hr.hire.confirm` and `hr.hire.withdraw` act on a draft only for
//!   the owner or for the very member she drafted it for (recorded in RAM when the draft
//!   succeeds, [`super::ChiefOfStaffPort::record_draft`]) while that member may still hire
//!   into its team; after an office restart only the owner may.
//! - **Conversations** ([`narrow`]): she is a participant of every member's DM with her, so a
//!   conversation is shown only when the caller may see it too: a channel they reach, or a
//!   conversation they are an active member of (the owner's own list holds every DM in the
//!   organization, so membership, not that list, decides a DM). Acting for nobody (a wake),
//!   she sees channels only, never a DM.
//! - **One conversation or message** ([`admit`]): marking read, muting, reacting, replying,
//!   answering or withdrawing an ask and triaging a message act only on a conversation the
//!   caller sees by the same rule. `dm.open` needs a caller named among its participants,
//!   checked before it runs so a refused call opens or reopens nothing, and answers only a DM
//!   the caller is in.
//! - **Her mailbox**: a message is shown only when the caller sent it or received it. Acting
//!   for nobody, none is.
//!
//! A read the gate cannot narrow (its answer does not decode, or the office cannot say what
//! the caller sees) is refused, never passed through.

use org_proto::ask::{ChatAnswer, MsgWithdraw};
use org_proto::chat::{
    ChatChannelSummarize, ChatConversations, ChatInbox, ChatMarkRead, ChatMessages, ChatMute,
    ChatReact, ConversationKind, ConversationSummary, DmOpen, MsgAccept,
};
use org_proto::hiring::{
    HrHire, HrHireConfirm, HrHireDraft, HrHireInspect, HrHireList, HrHireWithdraw,
};
use org_proto::msg::{
    MessageHeader, MsgDefer, MsgDone, MsgGet, MsgHumanNeedsYou, MsgInbox, MsgList, MsgReply,
    MsgThreads, MsgUnsnooze,
};
use org_proto::{ApiError, Call, ErrorCode, Outcome, Request, Response};

use super::{denied, ChiefOfStaffPort, Delegation};

/// Refuses a call the parameters of which put it outside what the caller may do; everything
/// else is admitted (a malformed call, or one naming an unknown message, is left to the
/// method, which refuses it).
pub(super) fn admit(
    request: &Request,
    port: &dyn ChiefOfStaffPort,
    member: &str,
    delegation: &Delegation,
) -> Result<(), Response> {
    admit_hire(request, port, delegation)?;
    admit_conversation(request, port, member, delegation)
}

/// The hire flow, per team.
fn admit_hire(
    request: &Request,
    port: &dyn ChiefOfStaffPort,
    delegation: &Delegation,
) -> Result<(), Response> {
    let id = request.id;
    let into = |team: &str| require(id, may_hire_into(id, port, delegation, team)?);
    match request.method.as_str() {
        HrHire::METHOD => params::<HrHire>(request).map_or(Ok(()), |p| into(&p.home_team_id)),
        HrHireDraft::METHOD => {
            params::<HrHireDraft>(request).map_or(Ok(()), |p| into(&p.home_team_id))
        }
        HrHireInspect::METHOD => params::<HrHireInspect>(request).map_or(Ok(()), |p| {
            match hire_team(id, port, &p.request_id)? {
                Some(team) => into(&team),
                None => Ok(()),
            }
        }),
        HrHireConfirm::METHOD => params::<HrHireConfirm>(request)
            .map_or(Ok(()), |p| own_draft(id, port, delegation, &p.request_id)),
        HrHireWithdraw::METHOD => params::<HrHireWithdraw>(request)
            .map_or(Ok(()), |p| own_draft(id, port, delegation, &p.request_id)),
        _ => Ok(()),
    }
}

/// Calls that act on one conversation or message: only one the caller sees ([`visible_ids`]).
fn admit_conversation(
    request: &Request,
    port: &dyn ChiefOfStaffPort,
    member: &str,
    delegation: &Delegation,
) -> Result<(), Response> {
    let id = request.id;
    let in_view = |conversation: &str| {
        let visible = visible_ids(port, member, delegation.caller_id.as_deref())
            .map_err(|err| Response::error(id, err))?;
        require(id, visible.iter().any(|v| v == conversation))
    };
    let on_message = |message: &str| match port.message_conversation(message) {
        Ok(Some(conversation)) => in_view(&conversation),
        Ok(None) => Ok(()),
        Err(err) => Err(Response::error(id, err)),
    };
    match request.method.as_str() {
        // She opens or resumes a DM only for a caller who will be in it, checked before it
        // runs, and shows only one they are in ([`narrow`]).
        DmOpen::METHOD => dm_open_includes_caller(id, port, request, delegation),
        ChatMarkRead::METHOD => {
            params::<ChatMarkRead>(request).map_or(Ok(()), |p| in_view(&p.conversation_id))
        }
        ChatMute::METHOD => {
            params::<ChatMute>(request).map_or(Ok(()), |p| in_view(&p.conversation_id))
        }
        ChatReact::METHOD => {
            params::<ChatReact>(request).map_or(Ok(()), |p| on_message(&p.message_id))
        }
        ChatAnswer::METHOD => {
            params::<ChatAnswer>(request).map_or(Ok(()), |p| on_message(&p.ask_id))
        }
        MsgWithdraw::METHOD => {
            params::<MsgWithdraw>(request).map_or(Ok(()), |p| on_message(&p.ask_id))
        }
        MsgReply::METHOD => params::<MsgReply>(request).map_or(Ok(()), |p| on_message(&p.to_id)),
        MsgAccept::METHOD => params::<MsgAccept>(request).map_or(Ok(()), |p| on_message(&p.id)),
        MsgDefer::METHOD => params::<MsgDefer>(request).map_or(Ok(()), |p| on_message(&p.id)),
        MsgDone::METHOD => params::<MsgDone>(request).map_or(Ok(()), |p| on_message(&p.id)),
        MsgUnsnooze::METHOD => params::<MsgUnsnooze>(request).map_or(Ok(()), |p| on_message(&p.id)),
        _ => Ok(()),
    }
}

/// `dm.open`, before it runs: admitted only for a caller one of its participants names, so the
/// DM or group DM it opens, resumes or reopens is one the caller is in. Acting for nobody, or
/// with parameters that do not decode, nothing is opened (fail closed).
fn dm_open_includes_caller(
    id: u64,
    port: &dyn ChiefOfStaffPort,
    request: &Request,
    delegation: &Delegation,
) -> Result<(), Response> {
    let (Some(caller), Some(open)) = (delegation.caller_id.as_deref(), params::<DmOpen>(request))
    else {
        return Err(denied(id));
    };
    for reference in &open.participants {
        let named = port
            .resolve_member(reference)
            .map_err(|err| Response::error(id, err))?;
        if named.as_deref() == Some(caller) {
            return Ok(());
        }
    }
    Err(denied(id))
}

/// Narrows what an admitted call answers; records a draft she made for the caller.
pub(super) fn narrow(
    request: &Request,
    port: &dyn ChiefOfStaffPort,
    member: &str,
    delegation: &Delegation,
    response: Response,
) -> Response {
    if !matches!(response.outcome, Outcome::Result(_)) {
        return response;
    }
    let caller = delegation.caller_id.as_deref();
    let party = |header: &MessageHeader| {
        caller.is_some_and(|c| header.from_member_id == c || header.to_member_id == c)
    };
    match request.method.as_str() {
        HrHireDraft::METHOD => {
            if let (Some(caller), Some(p)) = (caller, params::<HrHireDraft>(request)) {
                port.record_draft(&p.request_id, caller);
            }
            response
        }
        HrHireList::METHOD => {
            let id = response.id;
            rewrite::<HrHireList>(response, |list| {
                let mut kept = Vec::with_capacity(list.hires.len());
                for hire in std::mem::take(&mut list.hires) {
                    if may_hire_into(id, port, delegation, &hire.home_team_id).ok()? {
                        kept.push(hire);
                    }
                }
                list.hires = kept;
                Some(true)
            })
        }
        ChatConversations::METHOD
        | ChatMessages::METHOD
        | ChatInbox::METHOD
        | ChatChannelSummarize::METHOD => {
            narrow_conversations(request, port, member, caller, response)
        }
        MsgInbox::METHOD => rewrite::<MsgInbox>(response, |page| {
            page.headers.retain(party);
            Some(true)
        }),
        MsgList::METHOD => rewrite::<MsgList>(response, |page| {
            page.headers.retain(party);
            Some(true)
        }),
        MsgGet::METHOD => rewrite::<MsgGet>(response, |found| Some(party(&found.message.header))),
        MsgThreads::METHOD => rewrite::<MsgThreads>(response, |page| {
            page.threads
                .retain(|t| caller.is_some_and(|c| t.participants.iter().any(|p| p == c)));
            Some(true)
        }),
        MsgHumanNeedsYou::METHOD => rewrite::<MsgHumanNeedsYou>(response, |needs| {
            needs.items.retain(party);
            let urgent = needs.items.iter().filter(|h| h.urgent).count();
            needs.total_urgent_unread = u32::try_from(urgent).unwrap_or(u32::MAX);
            Some(true)
        }),
        DmOpen::METHOD => narrow_dm_open(caller, response),
        _ => response,
    }
}

/// `dm.open`: the DM she opened or resumed is answered only when the caller is an active
/// member of it; any other (her DM with another member, say) is refused.
fn narrow_dm_open(caller: Option<&str>, response: Response) -> Response {
    let id = response.id;
    let Outcome::Result(value) = &response.outcome else {
        return response;
    };
    match serde_json::from_value::<<DmOpen as Call>::Result>(value.clone()) {
        Ok(opened) if caller.is_some_and(|c| is_active_member(&opened.conversation, c)) => response,
        Ok(_) => denied(id),
        Err(_) => unavailable(id),
    }
}

/// Conversation reads, narrowed to the conversations the caller may see (`caller`), or to
/// channels alone for a wake (`None`).
fn narrow_conversations(
    request: &Request,
    port: &dyn ChiefOfStaffPort,
    member: &str,
    caller: Option<&str>,
    response: Response,
) -> Response {
    let visible = match visible_ids(port, member, caller) {
        Ok(visible) => visible,
        Err(err) => return Response::error(response.id, err),
    };
    let sees = |id: &str| visible.iter().any(|v| v == id);
    match request.method.as_str() {
        ChatConversations::METHOD => rewrite::<ChatConversations>(response, |r| {
            r.conversations.retain(|c| sees(&c.id));
            Some(true)
        }),
        ChatMessages::METHOD => {
            rewrite::<ChatMessages>(response, |r| Some(sees(&r.conversation_id)))
        }
        ChatInbox::METHOD => rewrite::<ChatInbox>(response, |r| {
            r.items.retain(|item| sees(&item.conversation_id));
            Some(true)
        }),
        ChatChannelSummarize::METHOD => {
            rewrite::<ChatChannelSummarize>(response, |r| Some(sees(&r.conversation_id)))
        }
        _ => response,
    }
}

/// The conversations she may show or act on for `caller`: the channels the caller reaches and
/// the conversations the caller is an active member of. The owner's list holds every DM in the
/// organization, so for a DM membership decides, never that list. Acting for nobody (`None`, a
/// wake), the channels she reaches and never a DM.
fn visible_ids(
    port: &dyn ChiefOfStaffPort,
    member: &str,
    caller: Option<&str>,
) -> Result<Vec<String>, ApiError> {
    let listed = port.visible_conversations(caller.unwrap_or(member))?;
    Ok(listed
        .into_iter()
        .filter(|c| {
            c.kind == ConversationKind::Channel || caller.is_some_and(|v| is_active_member(c, v))
        })
        .map(|c| c.id)
        .collect())
}

/// Whether `member` is in `conversation` and has not left it.
fn is_active_member(conversation: &ConversationSummary, member: &str) -> bool {
    conversation
        .members
        .iter()
        .any(|m| m.employee_id == member && m.left_at.is_none())
}

/// Decodes `response`'s result as `C`'s, lets `keep` narrow it, and re-encodes it. `keep`
/// answers `Some(false)` to hide the whole answer (as not found) and `None` when it could not
/// decide (refused). A result that does not decode is refused, never passed through.
fn rewrite<C: Call>(
    response: Response,
    keep: impl FnOnce(&mut C::Result) -> Option<bool>,
) -> Response {
    let id = response.id;
    let Outcome::Result(value) = response.outcome else {
        return response;
    };
    let Ok(mut result) = serde_json::from_value::<C::Result>(value) else {
        return unavailable(id);
    };
    match keep(&mut result) {
        Some(true) => Response::ok::<C>(id, &result).unwrap_or_else(|_| unavailable(id)),
        Some(false) => Response::error(id, ApiError::new(ErrorCode::NotFound, "not found")),
        None => unavailable(id),
    }
}

fn params<C: Call>(request: &Request) -> Option<C::Params> {
    request.params::<C>().ok()
}

fn require(id: u64, allowed: bool) -> Result<(), Response> {
    if allowed {
        Ok(())
    } else {
        Err(denied(id))
    }
}

/// Whether the member she acts for may hire into `team`: the owner into any team, anyone else
/// by their own authority, nobody for a wake.
fn may_hire_into(
    id: u64,
    port: &dyn ChiefOfStaffPort,
    delegation: &Delegation,
    team: &str,
) -> Result<bool, Response> {
    if delegation.caller.is_owner {
        return Ok(true);
    }
    let Some(caller) = delegation.caller_id.as_deref() else {
        return Ok(false);
    };
    port.caller_may_hire_into(caller, team)
        .map_err(|err| Response::error(id, err))
}

fn hire_team(
    id: u64,
    port: &dyn ChiefOfStaffPort,
    request_id: &str,
) -> Result<Option<String>, Response> {
    port.hire_team(request_id)
        .map_err(|err| Response::error(id, err))
}

/// `hr.hire.confirm` / `hr.hire.withdraw`: the owner, or the member she drafted this very hire
/// for while they may still hire into its team. An unknown request is the method's to refuse.
fn own_draft(
    id: u64,
    port: &dyn ChiefOfStaffPort,
    delegation: &Delegation,
    request_id: &str,
) -> Result<(), Response> {
    let Some(team) = hire_team(id, port, request_id)? else {
        return Ok(());
    };
    if delegation.caller.is_owner {
        return Ok(());
    }
    let Some(caller) = delegation.caller_id.as_deref() else {
        return Err(denied(id));
    };
    if port.draft_caller(request_id).as_deref() != Some(caller) {
        return Err(denied(id));
    }
    require(id, may_hire_into(id, port, delegation, &team)?)
}

fn unavailable(id: u64) -> Response {
    Response::error(
        id,
        ApiError::new(
            ErrorCode::Unavailable,
            "the office could not narrow this answer to the member the Chief of Staff acts for",
        ),
    )
}
