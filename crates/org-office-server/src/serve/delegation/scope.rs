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
//!   conversation is shown only when the caller may see it too (a channel they reach, a DM they
//!   are in). Acting for nobody (a wake), she sees channels only, never a DM's history.
//! - **Her mailbox**: a message is shown only when the caller sent it or received it. Acting
//!   for nobody, none is.
//!
//! A read the gate cannot narrow (its answer does not decode, or the office cannot say what
//! the caller sees) is refused, never passed through.

use org_proto::chat::{ChatChannelSummarize, ChatConversations, ChatInbox, ChatMessages};
use org_proto::hiring::{
    HrHire, HrHireConfirm, HrHireDraft, HrHireInspect, HrHireList, HrHireWithdraw,
};
use org_proto::msg::{MessageHeader, MsgGet, MsgHumanNeedsYou, MsgInbox, MsgList, MsgThreads};
use org_proto::{ApiError, Call, ErrorCode, Outcome, Request, Response};

use super::{denied, ChiefOfStaffPort, Delegation};

/// Refuses a call the parameters of which put it outside what the caller may do; everything
/// else is admitted (a malformed call is left to the method, which refuses it).
pub(super) fn admit(
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
        _ => response,
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
    let visible = match caller {
        Some(caller) => port
            .visible_conversations(caller)
            .map(|all| ids(all, false)),
        None => port.visible_conversations(member).map(|all| ids(all, true)),
    };
    let visible = match visible {
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

/// The IDs of `conversations`, channels only when `channels_only`.
fn ids(
    conversations: Vec<org_proto::chat::ConversationSummary>,
    channels_only: bool,
) -> Vec<String> {
    conversations
        .into_iter()
        .filter(|c| !channels_only || c.kind == org_proto::chat::ConversationKind::Channel)
        .map(|c| c.id)
        .collect()
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
