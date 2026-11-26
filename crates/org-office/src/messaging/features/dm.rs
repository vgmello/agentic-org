//! Opening (or reopening) direct and group direct messages.

use std::collections::BTreeSet;

use org_proto::chat::{ConversationKind, DmOpenParams, DmOpenResult};
use org_proto::ErrorCode;
use org_store::{Ack, Change, CoreRead, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};

use crate::messaging::{
    ports::{MessagingRead, MessagingWrite},
    records::{self, ConversationMembership, ConversationRecord, SCHEMA_VERSION},
};
use crate::organization::ports::OrganizationRead;
use crate::organization::records::Employee;

use super::channels::virtual_auto_channels;
use super::conversations::summarize_conversation;
use super::{check_hr_access, is_active_employee, refusal, resolve_employee};

const MAX_GROUP_DM_MEMBERS: usize = 8;

fn resolve_dm_members(
    read: &impl MessagingRead,
    employees: &[Employee],
    owner: &Id,
    caller: &Id,
    participants: &[String],
) -> Result<(BTreeSet<Id>, ConversationKind), Refusal> {
    let mut member_ids = BTreeSet::new();
    member_ids.insert(caller.clone());
    for target in participants {
        let emp =
            resolve_employee(employees, owner, target).map_err(|e| refusal(e.code, e.message))?;
        if !is_active_employee(emp) {
            return Err(refusal(
                ErrorCode::UnknownRecipient,
                "participant is not active",
            ));
        }
        member_ids.insert(emp.id.clone());
    }
    if member_ids.len() < 2 {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "a DM requires at least one other participant",
        ));
    }
    if member_ids.len() > MAX_GROUP_DM_MEMBERS {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "a group DM may have at most 8 participants",
        ));
    }
    check_hr_access(read, employees, owner, caller, member_ids.iter().cloned())?;
    let kind = if member_ids.len() == 2 {
        ConversationKind::Dm
    } else {
        ConversationKind::GroupDm
    };
    Ok((member_ids, kind))
}

fn derive_dm_id(
    org_id: &Id,
    kind: ConversationKind,
    explicit: Option<&str>,
    member_ids: &BTreeSet<Id>,
) -> Result<Id, Refusal> {
    if let Some(raw) = explicit {
        return Id::parse(raw).map_err(|_| {
            refusal(
                ErrorCode::InvalidRequest,
                "conversation_id must be a canonical UUIDv7",
            )
        });
    }
    if kind == ConversationKind::Dm {
        let mut iter = member_ids.iter();
        let a = iter.next().expect("2 members");
        let b = iter.next().expect("2 members");
        Ok(records::dm_conversation_id(org_id, a, b))
    } else {
        let key: Vec<&str> = member_ids.iter().map(Id::as_str).collect();
        Ok(records::derive_conversation_id(
            "group_dm",
            &format!("{org_id}:{}", key.join(",")),
        ))
    }
}

fn load_reopenable_dm_prev(
    read: &impl MessagingRead,
    conv_id: &Id,
    kind: ConversationKind,
    member_ids: &BTreeSet<Id>,
    now_utc: &str,
) -> Result<Option<ConversationRecord>, Refusal> {
    let autos = virtual_auto_channels(read, now_utc)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to load auto-channels"))?;
    if autos.iter().any(|c| c.id == *conv_id) {
        return Err(refusal(
            ErrorCode::AlreadyExists,
            "conversation_id belongs to an existing channel",
        ));
    }
    let Some(prev) = read
        .conversation(conv_id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read conversation"))?
    else {
        return Ok(None);
    };
    let prev_members: BTreeSet<Id> = prev
        .memberships
        .iter()
        .filter(|m| m.left_at.is_none())
        .map(|m| m.employee_id.clone())
        .collect();
    if prev.archived && prev.kind == kind && prev_members == *member_ids {
        Ok(Some(prev))
    } else {
        Err(refusal(
            ErrorCode::AlreadyExists,
            "conversation_id already belongs to another conversation",
        ))
    }
}

pub fn decide_dm_open<E>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    caller: &Id,
    params: &DmOpenParams,
    now_utc: &str,
) -> Result<(Change, DmOpenResult), Refusal>
where
    E: Engine,
    E::Tx: MessagingWrite,
{
    let employees = uow
        .view()
        .employees()
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read employees"))?;
    let (member_ids, kind) =
        resolve_dm_members(uow.view(), &employees, owner, caller, &params.participants)?;
    let existing = uow
        .view()
        .conversations()
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read conversations"))?;
    if let Some(found) = existing.iter().find(|c| {
        !c.archived && c.kind == kind && {
            let active: BTreeSet<Id> = c
                .memberships
                .iter()
                .filter(|m| m.left_at.is_none())
                .map(|m| m.employee_id.clone())
                .collect();
            active == member_ids
        }
    }) {
        let summary = summarize_conversation(uow.view(), found, caller, owner)
            .map_err(|_| refusal(ErrorCode::StorageError, "failed to summarize conversation"))?;
        return Ok((
            Change::new(Ack::Committed),
            DmOpenResult {
                conversation: summary,
                created: false,
            },
        ));
    }
    let conv_id = derive_dm_id(
        &here.organization_id,
        kind,
        params.conversation_id.as_deref(),
        &member_ids,
    )?;
    let prev = load_reopenable_dm_prev(uow.view(), &conv_id, kind, &member_ids, now_utc)?;
    let revision = prev.as_ref().map_or(1, |p| p.revision.saturating_add(1));
    let record = ConversationRecord {
        schema_version: SCHEMA_VERSION,
        revision,
        organization_id: here.organization_id.clone(),
        id: conv_id.clone(),
        kind,
        name: params
            .name
            .as_deref()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty()),
        auto_scope: None,
        team_id: None,
        project_id: None,
        archived: false,
        created_at: prev
            .as_ref()
            .map_or_else(|| now_utc.to_owned(), |p| p.created_at.clone()),
        memberships: member_ids
            .iter()
            .map(|id| ConversationMembership {
                employee_id: id.clone(),
                joined_at: now_utc.to_owned(),
                left_at: None,
            })
            .collect(),
        pins: prev.map(|p| p.pins).unwrap_or_default(),
    };
    let path = records::conversation_path(&conv_id);
    let exists = uow
        .view()
        .source_hash(&path)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to check conversation file"))?
        .is_some();
    let bytes = records::encode_json(&record);
    let summary = summarize_conversation(uow.view(), &record, caller, owner)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to summarize conversation"))?;
    let change = if exists {
        Change::new(Ack::Committed).put(path, bytes)
    } else {
        Change::new(Ack::Committed).create(path, bytes)
    };
    Ok((
        change,
        DmOpenResult {
            conversation: summary,
            created: !exists,
        },
    ))
}
