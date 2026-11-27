//! Conversation summaries and listings, including the virtual auto channels.

use org_proto::chat::{
    ChatChannelsParams, ChatChannelsResult, ChatConversationsParams, ChatConversationsResult,
    ConversationKind, ConversationMembershipView, ConversationPinView, ConversationSummary,
};
use org_proto::msg::MessageLifecycleState;
use org_store::Id;

use crate::messaging::{ports::MessagingRead, records::ConversationRecord};

use super::auto_channels::{active_member, effective_auto_members, virtual_auto_channels};
use super::catchup::{new_messages, watermark_boundary};
use super::is_active_employee;

pub(super) fn summarize_conversation(
    read: &impl MessagingRead,
    conv: &ConversationRecord,
    viewer: &Id,
    owner: &Id,
) -> Result<ConversationSummary, org_store::EngineError> {
    let states = read.recipient_states_for_employee(viewer)?;
    let unread_count = states
        .iter()
        .filter(|s| s.conversation_id == conv.id && s.state == MessageLifecycleState::Unread)
        .count() as u64;
    let marker = read.read_marker(&conv.id, viewer)?;
    let msgs = read.messages_in_conversation(&conv.id)?;
    let boundary = watermark_boundary(&msgs, marker.as_ref());
    let new_message_count = new_messages(&msgs, boundary, viewer).count() as u64;
    let (last_message_id, last_message_at) = msgs
        .last()
        .map(|m| (m.id.clone(), m.created_at.clone()))
        .unzip();
    let (last_read_message_id, last_read_at) = marker
        .map(|m| (m.last_read_message_id, m.read_at))
        .unwrap_or_default();
    let effective = effective_auto_members(read, conv, owner)?;
    let is_member = effective.contains(viewer) || active_member(conv, viewer);

    let mut members: Vec<ConversationMembershipView> = conv
        .memberships
        .iter()
        .map(|m| ConversationMembershipView {
            employee_id: m.employee_id.as_str().to_owned(),
            joined_at: m.joined_at.clone(),
            left_at: m.left_at.clone(),
        })
        .collect();
    if conv.auto_scope.is_some() {
        for eid in effective {
            if !members.iter().any(|m| m.employee_id == eid.as_str()) {
                members.push(ConversationMembershipView {
                    employee_id: eid.as_str().to_owned(),
                    joined_at: conv.created_at.clone(),
                    left_at: None,
                });
            }
        }
    }

    Ok(ConversationSummary {
        id: conv.id.as_str().to_owned(),
        kind: conv.kind,
        name: conv.name.clone(),
        auto_scope: conv.auto_scope,
        team_id: conv.team_id.as_ref().map(|i| i.as_str().to_owned()),
        project_id: conv.project_id.as_ref().map(|i| i.as_str().to_owned()),
        archived: conv.archived,
        revision: conv.revision,
        created_at: conv.created_at.clone(),
        members,
        pins: conv
            .pins
            .iter()
            .map(|p| ConversationPinView {
                message_id: p.message_id.clone(),
                pinned_by: p.pinned_by.as_str().to_owned(),
                pinned_at: p.pinned_at.clone(),
            })
            .collect(),
        unread_count,
        new_message_count,
        last_read_message_id,
        last_read_at,
        last_message_id,
        last_message_at,
        is_member,
    })
}

pub(super) fn all_conversations_including_auto(
    read: &impl MessagingRead,
    now_utc: &str,
) -> Result<Vec<ConversationRecord>, org_store::EngineError> {
    let mut all = read.conversations()?;
    all.extend(virtual_auto_channels(read, now_utc)?);
    all.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(all)
}

/// Whether `caller` may read `conv`'s messages: every channel, and a DM or group DM whose
/// active (effective) member the caller is, or any of them for the organization's owner. The
/// one rule every read of messages applies (`chat.messages`, channel summaries, search).
///
/// Only a channel has an auto scope (`messaging-schema.sql`), so a DM's effective members are
/// its current members who are active employees: the caller's own membership and record
/// decide, read once.
pub(crate) fn may_read(
    read: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    conv: &ConversationRecord,
) -> Result<bool, org_store::EngineError> {
    if conv.kind == ConversationKind::Channel || caller == owner {
        return Ok(true);
    }
    if conv.auto_scope.is_some() {
        return Ok(effective_auto_members(read, conv, owner)?.contains(caller));
    }
    let member = conv
        .memberships
        .iter()
        .any(|m| m.employee_id == *caller && m.left_at.is_none());
    Ok(member
        && read
            .employee(caller)?
            .is_some_and(|e| is_active_employee(&e)))
}

/// Every conversation (the virtual auto channels included) whose messages `caller` may read,
/// by [`may_read`].
pub fn readable_conversations(
    read: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    now_utc: &str,
) -> Result<Vec<ConversationRecord>, org_store::EngineError> {
    let mut readable = Vec::new();
    for conv in all_conversations_including_auto(read, now_utc)? {
        if may_read(read, owner, caller, &conv)? {
            readable.push(conv);
        }
    }
    Ok(readable)
}

pub fn list_channels(
    read: &impl MessagingRead,
    viewer: &Id,
    owner: &Id,
    params: &ChatChannelsParams,
    now_utc: &str,
) -> Result<ChatChannelsResult, org_store::EngineError> {
    let all = all_conversations_including_auto(read, now_utc)?;
    let mut channels = Vec::new();
    for conv in &all {
        if conv.kind != ConversationKind::Channel {
            continue;
        }
        if conv.archived && !params.include_archived {
            continue;
        }
        let summary = summarize_conversation(read, conv, viewer, owner)?;
        if params.unread_only && !summary.has_unread() {
            continue;
        }
        channels.push(summary);
    }
    Ok(ChatChannelsResult { channels })
}

pub fn list_conversations(
    read: &impl MessagingRead,
    viewer: &Id,
    owner: &Id,
    params: &ChatConversationsParams,
    now_utc: &str,
) -> Result<ChatConversationsResult, org_store::EngineError> {
    let all = all_conversations_including_auto(read, now_utc)?;
    let mut conversations = Vec::new();
    for conv in &all {
        if conv.archived && !params.include_archived {
            continue;
        }
        let summary = summarize_conversation(read, conv, viewer, owner)?;
        let visible =
            conv.kind == ConversationKind::Channel || summary.is_member || *viewer == *owner;
        if visible {
            conversations.push(summary);
        }
    }
    Ok(ChatConversationsResult { conversations })
}

/// Resolves a channel by UUIDv7 ID, `#name`, or plain name (case-insensitive).
pub(super) fn find_channel(
    all: &[ConversationRecord],
    reference: &str,
) -> Option<ConversationRecord> {
    let trimmed = reference.trim();
    if let Ok(id) = Id::parse(trimmed) {
        if let Some(found) = all
            .iter()
            .find(|c| c.kind == ConversationKind::Channel && c.id == id)
        {
            return Some(found.clone());
        }
    }
    let clean = trimmed.trim_start_matches('#').to_lowercase();
    all.iter()
        .find(|c| {
            c.kind == ConversationKind::Channel
                && c.name.as_deref().is_some_and(|n| n.to_lowercase() == clean)
        })
        .cloned()
}
