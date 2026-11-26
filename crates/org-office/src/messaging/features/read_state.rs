//! Per-member conversation state: read markers, mutes and reactions.

use org_proto::chat::{
    ChatMarkReadParams, ChatMarkReadResult, ChatMuteParams, ChatMuteResult, ChatReactParams,
    ChatReactResult, MuteMode, ReactionKind,
};
use org_proto::ErrorCode;
use org_store::{Ack, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};

use crate::messaging::{
    ports::{MessagingRead, MessagingWrite},
    records::{
        self, ConversationPreferenceRecord, ConversationReadMarkerRecord, ConversationRecord,
        MessageReactionRecord, MessageRecord, ParticipantReactionsFile, ParticipantStateFile,
        SCHEMA_VERSION,
    },
};

use super::catchup::watermark_boundary;
use super::messages::resolve_conversation_ref;
use super::{put_or_create_file, refusal, require_conversation_access};

/// What `chat.mark_read` does, decided on any consistent view (`TASK-99` rung 2).
#[derive(Debug, Clone)]
pub enum MarkReadPlan {
    /// The marker already covers the message, or there is no message to mark: nothing changes.
    Unchanged(ChatMarkReadResult),
    /// Move the marker forward to message `up_to`.
    Advance {
        conversation: ConversationRecord,
        was_persisted: bool,
        up_to: String,
    },
}

/// Plans moving the caller's read marker to `last_read_message_id`, or to the conversation's
/// latest message when omitted. Monotonic: a message the marker already covers is unchanged.
pub fn plan_mark_read(
    view: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    params: &ChatMarkReadParams,
    now_utc: &str,
) -> Result<MarkReadPlan, Refusal> {
    let target_conv =
        resolve_conversation_ref(view, owner, caller, &params.conversation_id, now_utc)
            .map_err(|e| refusal(e.code, e.message))?;
    let (conversation, was_persisted) =
        require_conversation_access(view, owner, caller, &target_conv.id, now_utc)?;
    let msgs = view
        .messages_in_conversation(&conversation.id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read messages"))?;
    let marker = view
        .read_marker(&conversation.id, caller)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read marker"))?;
    let target = match &params.last_read_message_id {
        Some(mid) => Some(position_in(&msgs, mid)?),
        None => msgs.len().checked_sub(1),
    };
    let boundary = watermark_boundary(&msgs, marker.as_ref());
    let Some(index) = target.filter(|index| index + 1 > boundary) else {
        let (read_at, last_read_message_id) = marker
            .map(|m| (m.read_at, m.last_read_message_id))
            .unwrap_or_default();
        return Ok(MarkReadPlan::Unchanged(ChatMarkReadResult {
            conversation_id: conversation.id.as_str().to_owned(),
            read_at: read_at.unwrap_or_else(|| now_utc.to_owned()),
            last_read_message_id,
            advanced: false,
        }));
    };
    Ok(MarkReadPlan::Advance {
        up_to: msgs[index].id.clone(),
        conversation,
        was_persisted,
    })
}

/// Advances the caller's read marker as [`plan_mark_read`] decides. Only the participant state
/// file is written, so inbox items (`message_recipient_states`) never change; an unchanged
/// marker writes nothing.
pub fn decide_mark_read<E>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    caller: &Id,
    params: &ChatMarkReadParams,
    now_utc: &str,
) -> Result<(Change, ChatMarkReadResult), Refusal>
where
    E: Engine,
    E::Tx: MessagingWrite,
{
    let (conv, was_persisted, up_to) =
        match plan_mark_read(uow.view(), owner, caller, params, now_utc)? {
            MarkReadPlan::Unchanged(result) => return Ok((Change::new(Ack::Committed), result)),
            MarkReadPlan::Advance {
                conversation,
                was_persisted,
                up_to,
            } => (conversation, was_persisted, up_to),
        };
    let conv_id = conv.id.clone();
    let prefs = uow
        .view()
        .preferences(&conv_id, caller)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read preferences"))?;
    let marker = ConversationReadMarkerRecord {
        organization_id: here.organization_id.clone(),
        conversation_id: conv_id.clone(),
        viewer_employee_id: caller.clone(),
        read_at: Some(now_utc.to_owned()),
        last_read_message_id: Some(up_to.clone()),
    };
    let file = ParticipantStateFile {
        schema_version: SCHEMA_VERSION,
        revision: 1,
        organization_id: here.organization_id.clone(),
        conversation_id: conv_id.clone(),
        employee_id: caller.clone(),
        preferences: prefs,
        read_marker: Some(marker),
    };
    let mut change = Change::new(Ack::Committed);
    if !was_persisted {
        change = change.create(
            records::conversation_path(&conv.id),
            records::encode_json(&conv),
        );
    }
    let path = records::participant_state_path(&conv_id, caller);
    change = put_or_create_file(uow, change, path, records::encode_json(&file))?;
    Ok((
        change,
        ChatMarkReadResult {
            conversation_id: conv_id.as_str().to_owned(),
            read_at: now_utc.to_owned(),
            last_read_message_id: Some(up_to),
            advanced: true,
        },
    ))
}

/// Where message `mid` sits in the conversation's canonical order. An unknown message and
/// another conversation's are refused alike, so the refusal reveals nothing about other
/// conversations.
fn position_in(msgs: &[MessageRecord], mid: &str) -> Result<usize, Refusal> {
    msgs.iter().position(|m| m.id == mid).ok_or_else(|| {
        refusal(
            ErrorCode::NotFound,
            "last_read_message_id names no message in this conversation",
        )
    })
}

pub fn decide_mute<E>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    caller: &Id,
    params: &ChatMuteParams,
    now_utc: &str,
) -> Result<(Change, ChatMuteResult), Refusal>
where
    E: Engine,
    E::Tx: MessagingWrite,
{
    if params.mute_mode == MuteMode::Unknown {
        return Err(refusal(ErrorCode::InvalidRequest, "invalid mute_mode"));
    }
    let conv_id = Id::parse(&params.conversation_id)
        .map_err(|_| refusal(ErrorCode::InvalidRequest, "invalid conversation_id"))?;
    let muted_id = Id::parse(&params.muted_employee_id)
        .map_err(|_| refusal(ErrorCode::InvalidRequest, "invalid muted_employee_id"))?;
    let (conv, was_persisted) =
        require_conversation_access(uow.view(), owner, caller, &conv_id, now_utc)?;
    let mut prefs = uow
        .view()
        .preferences(&conv_id, caller)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read preferences"))?;
    let marker = uow
        .view()
        .read_marker(&conv_id, caller)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read marker"))?;
    prefs.retain(|p| p.muted_employee_id != muted_id);
    if params.mute_mode != MuteMode::None {
        prefs.push(ConversationPreferenceRecord {
            muted_employee_id: muted_id.clone(),
            mute_mode: params.mute_mode,
        });
    }
    let file = ParticipantStateFile {
        schema_version: SCHEMA_VERSION,
        revision: 1,
        organization_id: here.organization_id.clone(),
        conversation_id: conv_id.clone(),
        employee_id: caller.clone(),
        preferences: prefs,
        read_marker: marker,
    };
    let mut change = Change::new(Ack::Committed);
    if !was_persisted {
        change = change.create(
            records::conversation_path(&conv.id),
            records::encode_json(&conv),
        );
    }
    let path = records::participant_state_path(&conv_id, caller);
    change = put_or_create_file(uow, change, path, records::encode_json(&file))?;
    Ok((
        change,
        ChatMuteResult {
            conversation_id: conv_id.as_str().to_owned(),
            muted_employee_id: muted_id.as_str().to_owned(),
            mute_mode: params.mute_mode,
        },
    ))
}

pub fn decide_react<E>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    caller: &Id,
    params: &ChatReactParams,
    now_utc: &str,
) -> Result<(Change, ChatReactResult), Refusal>
where
    E: Engine,
    E::Tx: MessagingWrite,
{
    if !matches!(params.reaction, ReactionKind::Seen | ReactionKind::Looking) {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "reaction must be seen or looking",
        ));
    }
    let msg = uow
        .view()
        .message(&params.message_id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read message"))?
        .ok_or_else(|| refusal(ErrorCode::NotFound, "message not found"))?;
    require_conversation_access(uow.view(), owner, caller, &msg.conversation_id, now_utc)?;
    let existing: Vec<MessageReactionRecord> = uow
        .view()
        .reactions_for_message(&msg.id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read reactions"))?
        .into_iter()
        .filter(|r| r.employee_id == *caller)
        .collect();
    let mut updated: Vec<MessageReactionRecord> = existing
        .into_iter()
        .filter(|r| r.reaction != params.reaction)
        .collect();
    let present = !params.remove;
    if present {
        updated.push(MessageReactionRecord {
            organization_id: here.organization_id.clone(),
            message_id: msg.id.clone(),
            employee_id: caller.clone(),
            reaction: params.reaction,
            created_at: now_utc.to_owned(),
        });
    }
    let file = ParticipantReactionsFile {
        schema_version: SCHEMA_VERSION,
        revision: 1,
        organization_id: here.organization_id.clone(),
        conversation_id: msg.conversation_id.clone(),
        employee_id: caller.clone(),
        message_id: msg.id.clone(),
        reactions: updated,
    };
    let path = records::participant_reactions_path(&msg.conversation_id, caller, &msg.id);
    let change = put_or_create_file(
        uow,
        Change::new(Ack::Committed),
        path,
        records::encode_json(&file),
    )?;
    Ok((
        change,
        ChatReactResult {
            message_id: msg.id,
            reaction: params.reaction,
            present,
        },
    ))
}
