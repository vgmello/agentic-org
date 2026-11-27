//! Channels: channel creation and membership decisions.

use std::collections::BTreeSet;

use org_proto::chat::{
    ChatChannelCreateParams, ChatChannelCreateResult, ChatChannelMembershipParams,
    ChatChannelMembershipResult, ConversationKind,
};
use org_proto::ErrorCode;
use org_store::{Ack, Change, CoreRead, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};

use crate::messaging::{
    auto_channels::{all_conversations_including_auto, slugify_channel, MAX_CHANNEL_NAME_LEN},
    ports::{MessagingRead, MessagingWrite},
    records::{self, ConversationMembership, ConversationRecord, SCHEMA_VERSION},
};
use crate::organization::ports::OrganizationRead;

use super::conversations::{find_channel, summarize_conversation};
use super::{is_addressable_employee, refusal, resolve_employee};

pub fn decide_channel_create<E>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    caller: &Id,
    params: &ChatChannelCreateParams,
    now_utc: &str,
) -> Result<(Change, ChatChannelCreateResult), Refusal>
where
    E: Engine,
    E::Tx: MessagingWrite,
{
    let slug = slugify_channel(&params.name);
    if slug.is_empty()
        || params.name.trim().is_empty()
        || params.name.trim().len() > MAX_CHANNEL_NAME_LEN
    {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "channel name must be 1..=64 characters",
        ));
    }
    let all = all_conversations_including_auto(uow.view(), now_utc)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to list channels"))?;
    if find_channel(&all, &slug).is_some() {
        return Err(refusal(
            ErrorCode::AlreadyExists,
            format!("channel #{slug} already exists"),
        ));
    }
    let conv_id = match &params.id {
        Some(raw) => Id::parse(raw).map_err(|_| {
            refusal(
                ErrorCode::InvalidRequest,
                "channel id must be a canonical UUIDv7",
            )
        })?,
        None => records::derive_conversation_id(
            "channel",
            &format!("{}:{slug}:{now_utc}", here.organization_id),
        ),
    };
    let path = records::conversation_path(&conv_id);
    let id_taken = all.iter().any(|c| c.id == conv_id)
        || uow
            .view()
            .source_hash(&path)
            .map_err(|_| refusal(ErrorCode::StorageError, "failed to check conversation file"))?
            .is_some();
    if id_taken {
        return Err(refusal(
            ErrorCode::AlreadyExists,
            "channel id already belongs to another conversation",
        ));
    }
    let employees = uow
        .view()
        .employees()
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read employees"))?;
    let mut member_ids = BTreeSet::new();
    member_ids.insert(caller.clone());
    for target in &params.members {
        let emp =
            resolve_employee(&employees, owner, target).map_err(|e| refusal(e.code, e.message))?;
        if is_addressable_employee(emp) {
            member_ids.insert(emp.id.clone());
        }
    }
    let record = ConversationRecord {
        schema_version: SCHEMA_VERSION,
        revision: 1,
        organization_id: here.organization_id.clone(),
        id: conv_id.clone(),
        kind: ConversationKind::Channel,
        name: Some(slug),
        auto_scope: None,
        team_id: None,
        project_id: None,
        archived: false,
        created_at: now_utc.to_owned(),
        memberships: member_ids
            .into_iter()
            .map(|id| ConversationMembership {
                employee_id: id,
                joined_at: now_utc.to_owned(),
                left_at: None,
            })
            .collect(),
        pins: Vec::new(),
    };
    let path = records::conversation_path(&conv_id);
    let bytes = records::encode_json(&record);
    // Enforce encoded OrgEvent headroom (512 KiB limit after base64+JSON framing) at write
    // admission so an accepted HQ channel never stalls `OrgEvent` publication (`TASK-52` P11-#4).
    const MAX_CHANNEL_RECORD_BYTES: usize = 360 * 1024;
    if bytes.len() > MAX_CHANNEL_RECORD_BYTES {
        return Err(refusal(
            ErrorCode::TooLarge,
            "channel definition exceeds replicated event size limit",
        ));
    }
    let summary = summarize_conversation(uow.view(), &record, caller, owner)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to summarize channel"))?;
    Ok((
        Change::new(Ack::Committed).create(path, bytes),
        ChatChannelCreateResult { channel: summary },
    ))
}

pub fn decide_channel_membership<E>(
    uow: &UnitOfWork<'_, E>,
    owner: &Id,
    caller: &Id,
    params: &ChatChannelMembershipParams,
    join: bool,
    now_utc: &str,
) -> Result<(Change, ChatChannelMembershipResult), Refusal>
where
    E: Engine,
    E::Tx: MessagingWrite,
{
    let all = all_conversations_including_auto(uow.view(), now_utc)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to list channels"))?;
    let Some(mut conv) = find_channel(&all, &params.channel) else {
        return Err(refusal(ErrorCode::NotFound, "channel not found"));
    };
    if conv.archived {
        return Err(refusal(ErrorCode::Conflict, "channel is archived"));
    }
    if conv.auto_scope.is_some() {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "automatic organization, team, and project channels cannot be manually joined or left",
        ));
    }
    let existed = uow
        .view()
        .conversation(&conv.id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read channel"))?
        .is_some();

    let mut changed = false;
    if join {
        if let Some(m) = conv
            .memberships
            .iter_mut()
            .find(|m| m.employee_id == *caller)
        {
            if m.left_at.is_some() {
                m.left_at = None;
                m.joined_at = now_utc.to_owned();
                changed = true;
            }
        } else {
            conv.memberships.push(ConversationMembership {
                employee_id: caller.clone(),
                joined_at: now_utc.to_owned(),
                left_at: None,
            });
            changed = true;
        }
    } else if let Some(m) = conv
        .memberships
        .iter_mut()
        .find(|m| m.employee_id == *caller)
    {
        if m.left_at.is_none() {
            m.left_at = Some(now_utc.to_owned());
            changed = true;
        }
    }

    let mut change = Change::new(Ack::Committed);
    if changed {
        conv.revision = conv.revision.saturating_add(1);
        let path = records::conversation_path(&conv.id);
        let bytes = records::encode_json(&conv);
        const MAX_CHANNEL_RECORD_BYTES: usize = 360 * 1024;
        if bytes.len() > MAX_CHANNEL_RECORD_BYTES {
            return Err(refusal(
                ErrorCode::TooLarge,
                "channel definition exceeds replicated event size limit",
            ));
        }
        change = if existed {
            change.put(path, bytes)
        } else {
            change.create(path, bytes)
        };
    }
    let summary = summarize_conversation(uow.view(), &conv, caller, owner)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to summarize channel"))?;
    Ok((
        change,
        ChatChannelMembershipResult {
            channel: summary,
            joined: join,
        },
    ))
}
