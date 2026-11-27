//! Who may read a conversation's messages or post files into it: the one rule every read of
//! messages applies (`chat.messages`, channel summaries, search, attachments), and the narrow
//! non-`features` capability other domains ask of messaging.

use std::collections::BTreeSet;

use org_proto::chat::ConversationKind;
use org_store::{EngineError, Id};

pub(crate) use super::auto_channels::resolve_employee;
use super::auto_channels::{
    all_conversations_including_auto, effective_auto_members, is_addressable_employee,
};
use super::ports::MessagingRead;
use super::records::ConversationRecord;

/// Whether `caller` may read `conv`'s messages: every channel, and a DM or group DM whose
/// active (effective) member the caller is, or any of them for the organization's owner. The
/// one rule every read of messages applies (`chat.messages`, channel summaries, search).
///
/// Only a channel has an auto scope (`messaging-schema.sql`), so a DM's effective members are
/// its current members who are addressable employees: the caller's own membership and record
/// decide, read once.
pub(crate) fn may_read(
    read: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    conv: &ConversationRecord,
) -> Result<bool, EngineError> {
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
            .is_some_and(|e| is_addressable_employee(&e)))
}

/// Every conversation (the virtual auto channels included) whose messages `caller` may read,
/// by [`may_read`].
pub fn readable_conversations(
    read: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    now_utc: &str,
) -> Result<Vec<ConversationRecord>, EngineError> {
    let mut readable = Vec::new();
    for conv in all_conversations_including_auto(read, now_utc)? {
        if may_read(read, owner, caller, &conv)? {
            readable.push(conv);
        }
    }
    Ok(readable)
}

/// Who may read a conversation's files and post into it, as the policy needs it: its effective
/// members, and whether it is a channel (which every employee reads). `None` when it is not
/// held here.
pub fn conversation_access(
    read: &impl MessagingRead,
    conversation_id: &Id,
    owner: &Id,
) -> Result<Option<(BTreeSet<String>, bool)>, EngineError> {
    let Some(conversation) = read.conversation(conversation_id)? else {
        return Ok(None);
    };
    let members = effective_auto_members(read, &conversation, owner)?;
    Ok(Some((
        members.iter().map(Id::to_string).collect(),
        conversation.kind == ConversationKind::Channel,
    )))
}
