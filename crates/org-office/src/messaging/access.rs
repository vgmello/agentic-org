//! Who may read a conversation's messages: the one rule every read of messages applies
//! (`chat.messages`, channel summaries, search), and the narrow capability another domain
//! (search, `TASK-91`) asks of messaging instead of reaching into its features.

use org_proto::chat::ConversationKind;
use org_store::{EngineError, Id};

use super::features::{
    all_conversations_including_auto, effective_auto_members, is_active_employee,
};
use super::ports::MessagingRead;
use super::records::ConversationRecord;

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
            .is_some_and(|e| is_active_employee(&e)))
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
