//! Live caller-visibility preflight for messaging `suggestion.create` targets (`chat.*`, `msg.*`),
//! mirroring `serve::delegation::scope` so non-visible conversations or DMs never pass preflight.

use std::collections::HashSet;

use org_proto::chat::{ChatConversationsParams, ConversationKind, ConversationSummary};
use org_proto::{ApiError, ErrorCode};
use serde_json::{Map, Value};

use super::{PreflightContext, PreflightOutcome};
use crate::{unavailable, TessService};

fn non_empty_str<'a>(
    input: &'a Map<String, Value>,
    keys: &[&str],
    missing: &str,
) -> Result<&'a str, ApiError> {
    keys.iter()
        .find_map(|k| input.get(*k))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::new(ErrorCode::InvalidRequest, missing))
}

fn list_all_conversations(
    svc: &TessService,
    member_id: &str,
    tag: &str,
    err_msg: &str,
) -> Result<Vec<ConversationSummary>, ApiError> {
    Ok(svc
        .office()
        .list_conversations(
            member_id,
            ChatConversationsParams {
                include_archived: true,
            },
        )
        .map_err(|e| unavailable(tag, &e, err_msg))??
        .conversations)
}

pub(super) fn is_messaging_target_call(call: &str) -> bool {
    matches!(
        call,
        "chat.send"
            | "chat.ask"
            | "chat.mark_read"
            | "chat.channel_join"
            | "chat.channel_leave"
            | "msg.accept"
            | "msg.defer"
            | "msg.done"
            | "msg.unsnooze"
            | "msg.withdraw"
    )
}

pub(super) fn preflight_messaging(
    svc: &TessService,
    ctx: &PreflightContext<'_>,
    call: &str,
    input: &Map<String, Value>,
) -> Result<PreflightOutcome, ApiError> {
    let visible = caller_visible_conversations(svc, ctx.caller_id, ctx.tess_id)?;
    match call {
        "chat.send" | "chat.ask" => {
            let to = non_empty_str(input, &["to"], "missing recipient `to`")?;
            verify_send_target(svc, ctx, &visible, to)?;
            if call == "chat.send" {
                if let Some(reply_to) = input.get("reply_to").and_then(Value::as_str) {
                    verify_message_visible(svc, &visible, reply_to)?;
                }
            }
            Ok(PreflightOutcome::ok(call, "messaging target verified"))
        }
        "chat.mark_read" => {
            let target = non_empty_str(
                input,
                &["channel", "conversation_id"],
                "missing target conversation",
            )?;
            if !visible.iter().any(|c| c.id == target) {
                verify_channel_named(svc, ctx.tess_id, &visible, target)?;
            }
            if let Some(up_to) = input
                .get("up_to")
                .or_else(|| input.get("last_read_message_id"))
                .and_then(Value::as_str)
            {
                verify_message_visible(svc, &visible, up_to)?;
            }
            Ok(PreflightOutcome::ok(call, "conversation target verified"))
        }
        "chat.channel_join" | "chat.channel_leave" => {
            let channel = non_empty_str(input, &["channel"], "missing target channel")?;
            let by_id = visible
                .iter()
                .any(|c| c.kind == ConversationKind::Channel && c.id == channel);
            if !by_id {
                verify_channel_named(svc, ctx.tess_id, &visible, channel)?;
            }
            Ok(PreflightOutcome::ok(call, "channel target verified"))
        }
        "msg.accept" | "msg.defer" | "msg.done" | "msg.unsnooze" | "msg.withdraw" => {
            let msg_id = non_empty_str(input, &["id", "ask_id"], "missing target message ID")?;
            verify_message_visible(svc, &visible, msg_id)?;
            Ok(PreflightOutcome::ok(call, "message target verified"))
        }
        _ => Ok(PreflightOutcome::ok(call, "schema and authority verified")),
    }
}

pub(super) fn caller_visible_conversations(
    svc: &TessService,
    caller_id: &str,
    tess_id: &str,
) -> Result<Vec<ConversationSummary>, ApiError> {
    let caller_convs = list_all_conversations(
        svc,
        caller_id,
        "tess_calls.visible_conversations",
        "unable to inspect conversations",
    )?;
    let tess_visible_ids: HashSet<String> = list_all_conversations(
        svc,
        tess_id,
        "tess_calls.tess_conversations",
        "unable to inspect conversations",
    )?
    .into_iter()
    .map(|c| c.id)
    .collect();
    Ok(caller_convs
        .into_iter()
        .filter(|c| {
            tess_visible_ids.contains(&c.id)
                && (c.kind == ConversationKind::Channel
                    || (is_active_member(c, caller_id) && is_active_member(c, tess_id)))
        })
        .collect())
}

fn is_active_member(conversation: &ConversationSummary, member: &str) -> bool {
    conversation
        .members
        .iter()
        .any(|m| m.employee_id == member && m.left_at.is_none())
}

fn verify_send_target(
    svc: &TessService,
    ctx: &PreflightContext<'_>,
    visible: &[ConversationSummary],
    to: &str,
) -> Result<(), ApiError> {
    if visible.iter().any(|c| c.id == to) {
        return Ok(());
    }
    if !to.starts_with('#') {
        let resolved = svc.office().resolve_member_id(to).map_err(|e| {
            unavailable(
                "tess_calls.resolve_member",
                &e,
                "unable to resolve recipient",
            )
        })?;
        if let Some(member_id) = resolved {
            if member_id == ctx.caller_id {
                return Ok(());
            }
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "caller may not send to another member's direct conversation through Tess",
            ));
        }
    }
    verify_channel_named(svc, ctx.tess_id, visible, to)
}

fn verify_channel_named(
    svc: &TessService,
    tess_id: &str,
    visible: &[ConversationSummary],
    reference: &str,
) -> Result<(), ApiError> {
    let needle = reference.trim_start_matches('#').to_lowercase();
    let named: Vec<String> = list_all_conversations(
        svc,
        tess_id,
        "tess_calls.tess_conversations",
        "unable to inspect channels",
    )?
    .into_iter()
    .filter(|c| {
        c.kind == ConversationKind::Channel
            && c.name
                .as_deref()
                .is_some_and(|n| n.to_lowercase() == needle)
    })
    .map(|c| c.id)
    .collect();
    if !named.is_empty()
        && named.iter().all(|id| {
            visible
                .iter()
                .any(|c| c.kind == ConversationKind::Channel && c.id == *id)
        })
    {
        return Ok(());
    }
    Err(ApiError::new(
        ErrorCode::NotAuthorized,
        "target conversation or recipient is not visible to caller",
    ))
}

fn verify_message_visible(
    svc: &TessService,
    visible: &[ConversationSummary],
    msg_id: &str,
) -> Result<(), ApiError> {
    let conv_id = svc.office().message_conversation_id(msg_id).map_err(|e| {
        unavailable(
            "tess_calls.message_conversation",
            &e,
            "unable to inspect target message",
        )
    })?;
    if conv_id
        .as_deref()
        .is_some_and(|cid| visible.iter().any(|c| c.id == cid))
    {
        return Ok(());
    }
    Err(ApiError::new(
        ErrorCode::NotFound,
        "target message not found or not visible to caller",
    ))
}
