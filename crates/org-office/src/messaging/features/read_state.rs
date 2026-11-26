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
        self, ConversationPreferenceRecord, ConversationReadMarkerRecord, MessageReactionRecord,
        ParticipantReactionsFile, ParticipantStateFile, SCHEMA_VERSION,
    },
};

use super::{put_or_create_file, refusal, require_conversation_access};

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
    let conv_id = Id::parse(&params.conversation_id)
        .map_err(|_| refusal(ErrorCode::InvalidRequest, "invalid conversation_id"))?;
    let (conv, was_persisted) =
        require_conversation_access(uow.view(), owner, caller, &conv_id, now_utc)?;
    if let Some(mid) = &params.last_read_message_id {
        let msg = uow
            .view()
            .message(mid)
            .map_err(|_| refusal(ErrorCode::StorageError, "failed to read message"))?
            .ok_or_else(|| refusal(ErrorCode::NotFound, "last_read_message_id not found"))?;
        if msg.conversation_id != conv.id {
            return Err(refusal(
                ErrorCode::InvalidRequest,
                "last_read_message_id does not belong to this conversation",
            ));
        }
    }
    let prefs = uow
        .view()
        .preferences(&conv_id, caller)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read preferences"))?;
    let marker = ConversationReadMarkerRecord {
        organization_id: here.organization_id.clone(),
        conversation_id: conv_id.clone(),
        viewer_employee_id: caller.clone(),
        read_at: Some(now_utc.to_owned()),
        last_read_message_id: params.last_read_message_id.clone(),
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
            last_read_message_id: params.last_read_message_id.clone(),
        },
    ))
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
