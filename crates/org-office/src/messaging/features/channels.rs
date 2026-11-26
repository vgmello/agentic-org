//! Channels: the virtual auto channels of teams and projects, their effective members,
//! and channel creation and membership decisions.

use std::collections::BTreeSet;

use org_proto::chat::{
    AutoScope, ChatChannelCreateParams, ChatChannelCreateResult, ChatChannelMembershipParams,
    ChatChannelMembershipResult, ConversationKind,
};
use org_proto::ErrorCode;
use org_store::{Ack, Change, CoreRead, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};

use crate::messaging::{
    ports::{MessagingRead, MessagingWrite},
    records::{self, ConversationMembership, ConversationRecord, SCHEMA_VERSION},
};
use crate::organization::ports::OrganizationRead;
use crate::organization::records::{Employee, TeamStatus};

use super::conversations::{
    all_conversations_including_auto, find_channel, summarize_conversation,
};
use super::{is_active_employee, refusal, resolve_employee};

const MAX_CHANNEL_NAME_LEN: usize = 64;

/// Synthesizes missing automatic channels (`#general`, team channels, project channels)
/// deterministically from the current organization snapshot. Membership is resolved dynamically
/// via [`effective_auto_members`] so persisted auto-channel records stay compact at any org size
/// (`TASK-52` P12-#2).
pub fn virtual_auto_channels(
    read: &impl MessagingRead,
    now_utc: &str,
) -> Result<Vec<ConversationRecord>, org_store::EngineError> {
    let Some(org) = read.organization()? else {
        return Ok(Vec::new());
    };
    let existing = read.conversations()?;
    let mut out = Vec::new();
    if !existing
        .iter()
        .any(|c| c.auto_scope == Some(AutoScope::Organization))
    {
        out.push(ConversationRecord {
            schema_version: SCHEMA_VERSION,
            revision: 1,
            organization_id: org.id.clone(),
            id: records::derive_conversation_id("auto:org", org.id.as_str()),
            kind: ConversationKind::Channel,
            name: Some("general".into()),
            auto_scope: Some(AutoScope::Organization),
            team_id: None,
            project_id: None,
            archived: false,
            created_at: now_utc.to_owned(),
            memberships: Vec::new(),
            pins: Vec::new(),
        });
    }
    out.extend(virtual_team_channels(read, &org.id, &existing, now_utc)?);
    out.extend(virtual_project_channels(read, &org.id, &existing, now_utc)?);
    Ok(out)
}

fn virtual_team_channels(
    read: &impl MessagingRead,
    org_id: &Id,
    existing: &[ConversationRecord],
    now_utc: &str,
) -> Result<Vec<ConversationRecord>, org_store::EngineError> {
    let mut out = Vec::new();
    for team in read.teams()? {
        if team.status != TeamStatus::Active {
            continue;
        }
        let exists = existing
            .iter()
            .any(|c| c.auto_scope == Some(AutoScope::Team) && c.team_id.as_ref() == Some(&team.id));
        if exists {
            continue;
        }
        out.push(ConversationRecord {
            schema_version: SCHEMA_VERSION,
            revision: 1,
            organization_id: org_id.clone(),
            id: records::derive_conversation_id("auto:team", team.id.as_str()),
            kind: ConversationKind::Channel,
            name: Some(slugify_channel(&team.name)),
            auto_scope: Some(AutoScope::Team),
            team_id: Some(team.id),
            project_id: None,
            archived: false,
            created_at: now_utc.to_owned(),
            memberships: Vec::new(),
            pins: Vec::new(),
        });
    }
    Ok(out)
}

fn virtual_project_channels(
    read: &impl MessagingRead,
    org_id: &Id,
    existing: &[ConversationRecord],
    now_utc: &str,
) -> Result<Vec<ConversationRecord>, org_store::EngineError> {
    let mut out = Vec::new();
    for project in read.projects()? {
        if project.archived {
            continue;
        }
        let exists = existing.iter().any(|c| {
            c.auto_scope == Some(AutoScope::Project) && c.project_id.as_ref() == Some(&project.id)
        });
        if exists {
            continue;
        }
        out.push(ConversationRecord {
            schema_version: SCHEMA_VERSION,
            revision: 1,
            organization_id: org_id.clone(),
            id: records::derive_conversation_id("auto:project", project.id.as_str()),
            kind: ConversationKind::Channel,
            name: Some(project.slug.clone()),
            auto_scope: Some(AutoScope::Project),
            team_id: None,
            project_id: Some(project.id),
            archived: false,
            created_at: now_utc.to_owned(),
            memberships: Vec::new(),
            pins: Vec::new(),
        });
    }
    Ok(out)
}

pub(crate) fn slugify_channel(name: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_owned();
    if trimmed.is_empty() {
        "channel".into()
    } else {
        trimmed.chars().take(MAX_CHANNEL_NAME_LEN).collect()
    }
}

pub(super) fn active_member(conv: &ConversationRecord, employee_id: &Id) -> bool {
    conv.memberships
        .iter()
        .any(|m| m.employee_id == *employee_id && m.left_at.is_none())
}

pub(crate) fn effective_auto_members(
    read: &impl MessagingRead,
    conv: &ConversationRecord,
    owner: &Id,
) -> Result<Vec<Id>, org_store::EngineError> {
    let employees = read.employees()?;
    let active: Vec<&Employee> = employees.iter().filter(|e| is_active_employee(e)).collect();
    match conv.auto_scope {
        Some(AutoScope::Organization) => Ok(active.into_iter().map(|e| e.id.clone()).collect()),
        Some(AutoScope::Team) => {
            let tid = conv.team_id.as_ref();
            Ok(active
                .into_iter()
                .filter(|e| tid == Some(&e.home_team_id) || e.id == *owner)
                .map(|e| e.id.clone())
                .collect())
        }
        Some(AutoScope::Project) => {
            let project = conv
                .project_id
                .as_ref()
                .and_then(|pid| read.project(pid).ok().flatten());
            Ok(active
                .into_iter()
                .filter(|e| {
                    e.id == *owner
                        || project.as_ref().is_some_and(|p| {
                            e.home_team_id == p.owner_team_id
                                || p.contributing_team_ids.contains(&e.home_team_id)
                        })
                })
                .map(|e| e.id.clone())
                .collect())
        }
        None | Some(_) => {
            let active_ids: BTreeSet<&Id> = active.iter().map(|e| &e.id).collect();
            Ok(conv
                .memberships
                .iter()
                .filter(|m| m.left_at.is_none() && active_ids.contains(&m.employee_id))
                .map(|m| m.employee_id.clone())
                .collect())
        }
    }
}

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
        if is_active_employee(emp) {
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
