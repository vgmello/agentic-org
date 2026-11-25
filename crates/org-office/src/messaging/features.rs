//! Engine-independent domain decisions for the messaging family (`doc-90`, `ADR-0003`,
//! `ADR-0014`, `ADR-0024`).
//!
//! Every write commits through `org-store` with `Ack::Committed` (`durable = false`), writing
//! canonical JSON envelopes under `durable/messaging/conversations/...` and projecting into
//! `MessagingWrite` (`protected/messaging.db` on SQLite, `MessagingTables` on Memory).

use std::collections::{BTreeMap, BTreeSet};

use org_proto::chat::{
    AutoScope, ChatChannelCreateParams, ChatChannelCreateResult, ChatChannelMembershipParams,
    ChatChannelMembershipResult, ChatChannelsParams, ChatChannelsResult, ChatConversationsParams,
    ChatConversationsResult, ChatInboxItem, ChatInboxParams, ChatInboxResult, ChatMarkReadParams,
    ChatMarkReadResult, ChatMessageView, ChatMessagesParams, ChatMessagesResult, ChatMuteParams,
    ChatMuteResult, ChatReactParams, ChatReactResult, ChatSendParams, ChatSendResult,
    ConversationKind, ConversationMembershipView, ConversationPinView, ConversationSummary,
    DeliveredBecause, DmOpenParams, DmOpenResult, HandoffAcceptanceView, MentionedMember,
    MsgAcceptParams, MsgAcceptResult, MuteMode, ReactionKind, ReactionView,
};
use org_proto::msg::{
    is_urgent, json_string_len, MessageLifecycleState, MessagePriority, Meta, MsgReplyParams,
    DEFAULT_PAGE_LIMIT, KIND_DELIVERY_FAILED, KIND_MSG, KIND_REPLY, KIND_REQUEST, MAX_BODY_BYTES,
    MAX_ID_BYTES, MAX_META_BYTES, MAX_NOTE_BYTES, MAX_PAGE_LIMIT, MAX_SUBJECT_BYTES,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{Ack, Change, CoreRead, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};

use crate::organization::ports::OrganizationRead;
use crate::organization::records::{Employee, EmployeeStatus, TeamStatus};

use super::{
    mentions::{extract_mentions, MentionCandidate, MentionScan},
    ports::{MessagingRead, MessagingWrite},
    records::{
        self, ConversationMembership, ConversationPreferenceRecord, ConversationReadMarkerRecord,
        ConversationRecord, MessageReactionRecord, MessageRecord, ParticipantAnswerFile,
        ParticipantReactionsFile, ParticipantReceivedSnapshot, ParticipantStateFile,
        RecipientStateRecord, SenderKind, SCHEMA_VERSION,
    },
};

const MAX_GROUP_DM_MEMBERS: usize = 8;
const MAX_CHANNEL_NAME_LEN: usize = 64;

pub(super) fn refusal(code: ErrorCode, msg: impl Into<String>) -> Refusal {
    Refusal::rejected(serde_json::to_vec(&ApiError::new(code, msg)).expect("ApiError serializes"))
}

/// Decodes a `Refusal` encoded by this module back into an `ApiError`.
pub fn decode_refusal(r: &Refusal) -> ApiError {
    match r {
        Refusal::Rejected(bytes) => serde_json::from_slice(bytes)
            .unwrap_or_else(|_| ApiError::new(ErrorCode::InvalidRequest, "request refused")),
        Refusal::Invalid(err) => ApiError::new(ErrorCode::InvalidRequest, err.to_string()),
    }
}

pub(super) fn is_active_employee(emp: &Employee) -> bool {
    matches!(emp.status, EmployeeStatus::Active | EmployeeStatus::Paused)
}

pub(super) fn resolve_employee<'a>(
    employees: &'a [Employee],
    owner: &Id,
    reference: &str,
) -> Result<&'a Employee, ApiError> {
    let trimmed = reference.trim();
    if trimmed.eq_ignore_ascii_case("office") {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "the office cannot be addressed here",
        ));
    }
    if trimmed.eq_ignore_ascii_case("owner") {
        return employees
            .iter()
            .find(|e| e.id == *owner && is_active_employee(e))
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "owner not found"));
    }
    if let Some(by_id) = employees.iter().find(|e| e.id.as_str() == trimmed) {
        return Ok(by_id);
    }
    let key = trimmed.to_lowercase();
    employees
        .iter()
        .find(|e| e.handle.to_lowercase() == key || e.name.trim().to_lowercase() == key)
        .ok_or_else(|| ApiError::new(ErrorCode::UnknownRecipient, "unknown recipient"))
}

/// Enforces `HIRE-2` (`doc-78`, ADR-0029 decision 3): only the owner, an active employee
/// who `may_hire_anywhere` (leads a team with `may_hire == true`), or HR itself may address
/// or `@mention` the builtin HR lead.
pub(super) fn check_hr_access(
    read: &impl MessagingRead,
    employees: &[Employee],
    owner: &Id,
    caller: &Id,
    targets: impl IntoIterator<Item = Id>,
) -> Result<(), Refusal> {
    let hr_id = employees
        .iter()
        .find(|e| e.builtin_key == Some(crate::organization::records::BuiltinKey::Hr))
        .map(|e| &e.id);
    let Some(hr_id) = hr_id else {
        return Ok(());
    };
    if !targets.into_iter().any(|id| id == *hr_id) {
        return Ok(());
    }
    if caller == owner || caller == hr_id {
        return Ok(());
    }
    let Some(caller_emp) = employees
        .iter()
        .find(|e| e.id == *caller && is_active_employee(e))
    else {
        return Err(refusal(
            ErrorCode::NotAuthorized,
            crate::organization::features::HIRE_DENIED,
        ));
    };
    let allowed =
        crate::organization::features::authority::may_hire_anywhere(read, owner, caller_emp)
            .map_err(|_| refusal(ErrorCode::StorageError, "failed to verify hire access"))?;
    if allowed {
        Ok(())
    } else {
        Err(refusal(
            ErrorCode::NotAuthorized,
            crate::organization::features::HIRE_DENIED,
        ))
    }
}

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

fn active_member(conv: &ConversationRecord, employee_id: &Id) -> bool {
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

fn summarize_conversation(
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
    let last_message_at = msgs.last().map(|m| m.created_at.clone());
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
        last_read_message_id: marker.and_then(|m| m.last_read_message_id),
        last_message_at,
        is_member,
    })
}

fn all_conversations_including_auto(
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
        channels.push(summarize_conversation(read, conv, viewer, owner)?);
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
fn find_channel(all: &[ConversationRecord], reference: &str) -> Option<ConversationRecord> {
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

/// Output of committing a `chat.send` message on `org-store`, including the exact headers
/// to emit as `message.received` live events (header-only!) and the recipient employee IDs
/// whose `msg.wait` listeners should be woken.
#[derive(Debug)]
pub struct ChatSendCommitted {
    pub result: ChatSendResult,
    pub inserted: bool,
    pub conversation_kind: ConversationKind,
    pub open_question_asker_id: Option<String>,
    pub message: MessageRecord,
    pub recipients: Vec<(Id, RecipientStateRecord)>,
}

impl ChatSendCommitted {
    /// Builds header-only wire summaries for every delivered recipient (never carries body).
    pub fn delivered_headers(&self) -> Vec<org_proto::msg::MessageHeader> {
        self.recipients
            .iter()
            .map(|(_, state)| to_wire_header(&self.message, Some(state), self.result.local_seq))
            .collect()
    }
}

fn validate_send_sizes(params: &ChatSendParams) -> Result<(), Refusal> {
    if let Some(sub) = &params.subject {
        if json_string_len(sub) > MAX_SUBJECT_BYTES {
            return Err(refusal(ErrorCode::TooLarge, "subject exceeds size limit"));
        }
    }
    if let Some(rep) = &params.reply_to {
        if json_string_len(rep) > MAX_ID_BYTES {
            return Err(refusal(ErrorCode::TooLarge, "reply_to exceeds size limit"));
        }
    }
    if json_string_len(&params.body) > MAX_BODY_BYTES {
        return Err(refusal(ErrorCode::TooLarge, "body exceeds 256 KiB limit"));
    }
    let meta_bytes = serde_json::to_vec(&params.meta)
        .map_err(|_| refusal(ErrorCode::InvalidRequest, "invalid meta"))?
        .len();
    if meta_bytes > MAX_META_BYTES {
        return Err(refusal(ErrorCode::TooLarge, "meta exceeds 64 KiB limit"));
    }
    Ok(())
}

fn validate_chat_send_inputs(params: &ChatSendParams) -> Result<MessagePriority, Refusal> {
    if json_string_len(&params.id) > MAX_ID_BYTES
        || (org_proto::uuid_v7_time_ms(&params.id).is_none()
            && org_proto::ulid_time_ms(&params.id).is_none())
    {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "message id must be a canonical lowercase UUIDv7",
        ));
    }
    if params.kind.is_empty()
        || json_string_len(&params.kind) > MAX_ID_BYTES
        || matches!(params.kind.as_str(), KIND_REQUEST | KIND_DELIVERY_FAILED)
    {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "invalid message kind for send",
        ));
    }
    if params.kind == "announcement" && params.reason.as_deref().is_none_or(|r| r.trim().is_empty())
    {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "announcement messages require --reason",
        ));
    }
    let priority = params.priority.unwrap_or_default();
    if !matches!(
        priority,
        MessagePriority::Normal | MessagePriority::High | MessagePriority::Urgent
    ) {
        return Err(refusal(ErrorCode::InvalidRequest, "invalid priority"));
    }
    validate_send_sizes(params)?;
    Ok(priority)
}

pub(super) fn resolve_send_target(
    read: &impl MessagingRead,
    here: &OfficeIdentity,
    owner: &Id,
    sender: &Employee,
    employees: &[Employee],
    to_raw: &str,
    now_utc: &str,
) -> Result<(ConversationRecord, bool), Refusal> {
    let all = all_conversations_including_auto(read, now_utc)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to load conversations"))?;
    let trimmed = to_raw.trim();
    if trimmed.is_empty() {
        return Err(refusal(ErrorCode::InvalidRequest, "target cannot be empty"));
    }
    // 1. Explicit conversation UUIDv7
    if let Ok(cid) = Id::parse(trimmed) {
        if let Some(conv) = all.iter().find(|c| c.id == cid) {
            let persisted = read
                .conversation(&conv.id)
                .map_err(|_| refusal(ErrorCode::StorageError, "failed to read conversation"))?
                .is_some();
            return Ok((conv.clone(), persisted));
        }
    }
    // 2. Channel by `#name`
    if trimmed.starts_with('#') {
        let conv = find_channel(&all, trimmed)
            .ok_or_else(|| refusal(ErrorCode::NotFound, format!("channel {trimmed} not found")))?;
        let persisted = read
            .conversation(&conv.id)
            .map_err(|_| refusal(ErrorCode::StorageError, "failed to read channel"))?
            .is_some();
        return Ok((conv, persisted));
    }
    // 3. Employee (DM) by owner / id / handle
    if let Ok(target_emp) = resolve_employee(employees, owner, trimmed) {
        if !is_active_employee(target_emp) {
            return Err(refusal(
                ErrorCode::UnknownRecipient,
                "recipient is not active",
            ));
        }
        let dm_id = records::dm_conversation_id(&here.organization_id, &sender.id, &target_emp.id);
        if let Some(conv) = read
            .conversation(&dm_id)
            .map_err(|_| refusal(ErrorCode::StorageError, "failed to read dm"))?
        {
            return Ok((conv, true));
        }
        let mut memberships = vec![ConversationMembership {
            employee_id: sender.id.clone(),
            joined_at: now_utc.to_owned(),
            left_at: None,
        }];
        if target_emp.id != sender.id {
            memberships.push(ConversationMembership {
                employee_id: target_emp.id.clone(),
                joined_at: now_utc.to_owned(),
                left_at: None,
            });
        }
        let conv = ConversationRecord {
            schema_version: SCHEMA_VERSION,
            revision: 1,
            organization_id: here.organization_id.clone(),
            id: dm_id,
            kind: ConversationKind::Dm,
            name: None,
            auto_scope: None,
            team_id: None,
            project_id: None,
            archived: false,
            created_at: now_utc.to_owned(),
            memberships,
            pins: Vec::new(),
        };
        return Ok((conv, false));
    }
    // 4. Bare channel name fallback (e.g., "general")
    if let Some(conv) = find_channel(&all, trimmed) {
        let persisted = read
            .conversation(&conv.id)
            .map_err(|_| refusal(ErrorCode::StorageError, "failed to read channel"))?
            .is_some();
        return Ok((conv, persisted));
    }
    Err(refusal(
        ErrorCode::UnknownRecipient,
        format!("unknown recipient or channel {trimmed:?}"),
    ))
}

fn insert_recipient_reason(
    reasons: &mut BTreeMap<Id, DeliveredBecause>,
    sender_id: &Id,
    emp_id: Id,
    because: DeliveredBecause,
) {
    if emp_id == *sender_id {
        return;
    }
    reasons
        .entry(emp_id)
        .and_modify(|existing| {
            if because < *existing {
                *existing = because;
            }
        })
        .or_insert(because);
}

fn collect_thread_reply_recipients(
    read: &impl MessagingRead,
    conv_id: &Id,
    root_id: &str,
    active_members: &[Id],
    sender_id: &Id,
    reasons: &mut BTreeMap<Id, DeliveredBecause>,
) -> Result<(), Refusal> {
    let conv_msgs = read
        .messages_in_conversation(conv_id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read thread"))?;
    for m in &conv_msgs {
        let in_thread = m.id == root_id || m.thread_root_id.as_deref() == Some(root_id);
        if !in_thread {
            continue;
        }
        if let Some(author) = m
            .sender_employee_id
            .as_ref()
            .filter(|a| active_members.contains(a))
        {
            insert_recipient_reason(
                reasons,
                sender_id,
                author.clone(),
                DeliveredBecause::ThreadReply,
            );
        }
    }
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "recipient reason collection evaluates conversation, sender, members, mentions, and reply/thread context"
)]
pub(crate) fn collect_recipient_reasons(
    read: &impl MessagingRead,
    conv: &ConversationRecord,
    sender_id: &Id,
    active_members: &[Id],
    mentions: &[MentionedMember],
    thread_root_id: Option<&str>,
    kind: &str,
    meta: &Meta,
) -> Result<BTreeMap<Id, DeliveredBecause>, Refusal> {
    let mut reasons: BTreeMap<Id, DeliveredBecause> = BTreeMap::new();
    if matches!(conv.kind, ConversationKind::Dm | ConversationKind::GroupDm) {
        for emp_id in active_members {
            insert_recipient_reason(
                &mut reasons,
                sender_id,
                emp_id.clone(),
                DeliveredBecause::Addressed,
            );
        }
        return Ok(reasons);
    }
    if kind == "handoff" || kind == "request" {
        if let Some(target_id) = meta
            .get("card")
            .and_then(|c| c.get("addressed_to"))
            .and_then(|v| v.as_str())
            .and_then(|s| Id::parse(s).ok())
            .filter(|id| active_members.contains(id))
        {
            insert_recipient_reason(
                &mut reasons,
                sender_id,
                target_id,
                DeliveredBecause::Addressed,
            );
        }
    }
    if let Some(root_id) = thread_root_id {
        collect_thread_reply_recipients(
            read,
            &conv.id,
            root_id,
            active_members,
            sender_id,
            &mut reasons,
        )?;
    }
    for eid in mentions
        .iter()
        .filter_map(|m| Id::parse(&m.employee_id).ok())
        .filter(|id| active_members.contains(id))
    {
        insert_recipient_reason(&mut reasons, sender_id, eid, DeliveredBecause::Mention);
    }
    Ok(reasons)
}

pub(crate) fn upsert_participant_snapshot(
    read: &impl MessagingRead,
    org_id: &Id,
    conv_id: &Id,
    emp_id: &Id,
    state: RecipientStateRecord,
    change: Change,
) -> Result<Change, Refusal> {
    let existing_states = read
        .recipient_states_for_employee(emp_id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read recipient states"))?;
    let mut map: BTreeMap<String, RecipientStateRecord> = existing_states
        .into_iter()
        .filter(|s| s.conversation_id == *conv_id)
        .map(|s| (s.message_id.clone(), s))
        .collect();
    map.insert(state.message_id.clone(), state);
    let snap = ParticipantReceivedSnapshot {
        schema_version: SCHEMA_VERSION,
        revision: map.len() as u64,
        organization_id: org_id.clone(),
        conversation_id: conv_id.clone(),
        employee_id: emp_id.clone(),
        states: map,
    };
    let path = records::participant_received_snapshot_path(conv_id, emp_id);
    let existed = read
        .source_hash(&path)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to check snapshot file"))?
        .is_some();
    let bytes = records::encode_json(&snap);
    Ok(if existed {
        change.put(path, bytes)
    } else {
        change.create(path, bytes)
    })
}

pub(crate) fn build_mention_candidates(
    employees: &[Employee],
    active_members: &[Id],
    owner: &Id,
) -> Vec<MentionCandidate> {
    let mut out = Vec::new();
    for e in employees {
        if !is_active_employee(e) || !active_members.contains(&e.id) {
            continue;
        }
        out.push(MentionCandidate {
            employee_id: e.id.clone(),
            handle: e.handle.clone(),
            display_name: e.name.clone(),
        });
        if e.id == *owner && !e.handle.eq_ignore_ascii_case("owner") {
            out.push(MentionCandidate {
                employee_id: e.id.clone(),
                handle: "owner".into(),
                display_name: e.name.clone(),
            });
        }
    }
    out
}

#[expect(
    clippy::too_many_arguments,
    reason = "replay check compares idempotency record against full send context and caller identity"
)]
fn check_chat_send_replay(
    read: &impl MessagingRead,
    here: &OfficeIdentity,
    owner: &Id,
    sender: &Employee,
    employees: &[Employee],
    params: &ChatSendParams,
    priority: MessagePriority,
    now_utc: &str,
) -> Result<Option<(Change, ChatSendCommitted)>, Refusal> {
    let Some(existing) = read
        .message(&params.id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read message"))?
    else {
        return Ok(None);
    };
    let (target_conv, _) =
        resolve_send_target(read, here, owner, sender, employees, &params.to, now_utc)?;
    let mut expected_meta = params.meta.clone();
    if let Some(reason) = &params.reason {
        expected_meta.insert("reason".into(), serde_json::Value::String(reason.clone()));
    }
    let mut stored_meta = existing.metadata.clone();
    stored_meta.remove("mentions");
    expected_meta.remove("mentions");
    if existing.sender_employee_id.as_ref() != Some(&sender.id)
        || existing.conversation_id != target_conv.id
        || existing.body_markdown != params.body
        || existing.kind != params.kind
        || existing.priority != priority
        || existing.subject != params.subject
        || existing.reply_to_message_id != params.reply_to
        || existing.also_in_channel != params.also_in_channel
        || stored_meta != expected_meta
    {
        return Err(refusal(
            ErrorCode::AlreadyExists,
            "message id already used for different content",
        ));
    }
    let active_members = effective_auto_members(read, &target_conv, owner)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to resolve members"))?;
    let candidates = build_mention_candidates(employees, &active_members, owner);
    let scan = extract_mentions(&params.body, &candidates);
    let thread_id = existing
        .thread_root_id
        .clone()
        .unwrap_or_else(|| existing.id.clone());
    let local_seq = read
        .message_local_seq(&existing.id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read local_seq"))?
        .unwrap_or(existing.origin_sequence);
    let conversation_kind = read
        .conversation(&existing.conversation_id)
        .ok()
        .flatten()
        .map_or(ConversationKind::Channel, |c| c.kind);
    Ok(Some((
        Change::new(Ack::Committed),
        ChatSendCommitted {
            result: ChatSendResult {
                id: existing.id.clone(),
                conversation_id: existing.conversation_id.as_str().to_owned(),
                thread_id,
                local_seq,
                origin_seq: existing.origin_sequence,
                mentions: scan.resolved,
                unmatched_mentions: scan.unmatched,
                recipient_presence: Vec::new(),
                woke_agents: Vec::new(),
            },
            inserted: false,
            conversation_kind,
            open_question_asker_id: None,
            message: existing,
            recipients: Vec::new(),
        },
    )))
}

fn resolve_reply_thread_root(
    read: &impl MessagingRead,
    conv_id: &Id,
    reply_to: Option<&str>,
) -> Result<Option<String>, Refusal> {
    let Some(reply_to) = reply_to else {
        return Ok(None);
    };
    let parent = read
        .message(reply_to)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read reply_to"))?
        .ok_or_else(|| refusal(ErrorCode::NotFound, "reply_to message not found"))?;
    if parent.conversation_id != *conv_id {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "cannot reply across different conversations",
        ));
    }
    if parent.kind == KIND_DELIVERY_FAILED {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "cannot reply to a delivery_failed notice",
        ));
    }
    Ok(Some(parent.thread_root_id.unwrap_or(parent.id)))
}

#[expect(
    clippy::too_many_arguments,
    reason = "message record builder receives explicit identity, conversation, sender, params, priority, and metadata"
)]
fn build_new_message_record(
    here: &OfficeIdentity,
    conv_id: &Id,
    sender: &Employee,
    params: &ChatSendParams,
    priority: MessagePriority,
    meta: Meta,
    thread_root_id: Option<String>,
    origin_seq: u64,
    org_seq: u64,
    now_utc: &str,
) -> MessageRecord {
    let mut record = MessageRecord {
        schema_version: SCHEMA_VERSION,
        organization_id: here.organization_id.clone(),
        id: params.id.clone(),
        conversation_id: conv_id.clone(),
        sender_kind: SenderKind::Employee,
        sender_employee_id: Some(sender.id.clone()),
        sender_office_id: None,
        sender_display_name: sender.name.clone(),
        kind: params.kind.clone(),
        priority,
        subject: params.subject.clone(),
        body_markdown: params.body.clone(),
        metadata: meta,
        thread_root_id,
        reply_to_message_id: params.reply_to.clone(),
        also_in_channel: params.also_in_channel,
        created_at: now_utc.to_owned(),
        expires_at: None,
        ttl_ms: None,
        origin_office_id: here.office_id.clone(),
        origin_incarnation: here.journal_incarnation.as_str().to_owned(),
        origin_sequence: origin_seq,
        org_epoch: 1,
        org_sequence: org_seq,
        content_sha256: String::new(),
        attachments: Vec::new(),
        mailbox_content_hash: None,
    };
    record.content_sha256 = record.compute_content_sha256();
    record
}

/// Resolves the single office owning durable inbox state for `emp_id` (an agent's placed office
/// when hired into an enrolled satellite, else HQ or `here_office_id` in single-office mode,
/// `TASK-52` P5-#3).
pub(super) fn recipient_owner_office_id(
    read: &impl MessagingRead,
    here_office_id: &Id,
    emp_id: &Id,
) -> Id {
    let Ok(offices) = read.offices() else {
        return here_office_id.clone();
    };
    if offices.len() <= 1 {
        return here_office_id.clone();
    }
    let hq_id = offices
        .iter()
        .find(|o| o.role == crate::organization::records::OfficeRole::Hq)
        .map(|o| o.id.clone())
        .unwrap_or_else(|| here_office_id.clone());
    let Ok(hires) = read.hire_requests() else {
        return hq_id;
    };
    for req in hires {
        if req.state == crate::organization::records::HireState::Hired
            && req.employee_id.as_ref() == Some(emp_id)
        {
            if let Some(oid) = req.proposal.office_id.as_deref() {
                if let Some(found) = offices.iter().find(|o| o.id.as_str() == oid) {
                    return found.id.clone();
                }
            }
        }
    }
    hq_id
}

#[expect(
    clippy::too_many_arguments,
    reason = "recipient state builder threads read view, message record, roster, reasons, and delivery metadata"
)]
pub(super) fn append_send_recipient_states(
    read: &impl MessagingRead,
    here: &OfficeIdentity,
    conv_id: &Id,
    record: &MessageRecord,
    employees: &[Employee],
    reasons: BTreeMap<Id, DeliveredBecause>,
    org_seq: u64,
    now_utc: &str,
    mut change: Change,
) -> Result<(Change, Vec<(Id, RecipientStateRecord)>), Refusal> {
    let origin_incarnation = here.journal_incarnation.as_str().to_owned();
    let mut state_seq = read
        .max_state_sequence(&here.office_id, &origin_incarnation)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to allocate state_sequence"))?;
    let mut recipients = Vec::new();
    for (emp_id, because) in reasons {
        let owner_office = recipient_owner_office_id(read, &here.office_id, &emp_id);
        let disp = employees
            .iter()
            .find(|e| e.id == emp_id)
            .map(|e| e.name.clone());
        let is_local_owner = owner_office == here.office_id;
        if is_local_owner {
            state_seq = state_seq.saturating_add(1);
        }
        let state = RecipientStateRecord {
            organization_id: here.organization_id.clone(),
            message_id: record.id.clone(),
            conversation_id: conv_id.clone(),
            employee_id: emp_id.clone(),
            recipient_display_name: disp,
            state: MessageLifecycleState::Unread,
            triage_generation: 0,
            snoozed_until: None,
            snooze_after_task: false,
            snooze_note: None,
            triaged_at: None,
            read_at: None,
            updated_at: now_utc.to_owned(),
            reason: None,
            delivered_because: because,
            related_message_ids: Vec::new(),
            expired_waiting_person: false,
            owner_office_id: owner_office,
            origin_incarnation: origin_incarnation.clone(),
            origin_sequence: state_seq,
            org_epoch: 1,
            org_sequence: org_seq,
        };
        // Persist durable inbox state ONLY at the recipient's single owning office while still
        // returning every recipient in `recipients` for delivery notifications (`TASK-52` P5-#3).
        if is_local_owner {
            change = upsert_participant_snapshot(
                read,
                &here.organization_id,
                conv_id,
                &emp_id,
                state.clone(),
                change,
            )?;
        }
        recipients.push((emp_id, state));
    }
    Ok((change, recipients))
}

#[expect(
    clippy::too_many_arguments,
    reason = "send commit builder coordinates conversation state, record, recipients, and change assembly"
)]
fn build_send_commit_change(
    read: &impl MessagingRead,
    here: &OfficeIdentity,
    owner: &Id,
    conv: &mut ConversationRecord,
    conv_persisted: bool,
    conv_changed: bool,
    sender: &Employee,
    employees: &[Employee],
    active_members: &[Id],
    params: &ChatSendParams,
    priority: MessagePriority,
    scan: MentionScan,
    thread_root_id: Option<String>,
    org_seq: u64,
    now_utc: &str,
) -> Result<(Change, ChatSendCommitted), Refusal> {
    let mut meta = params.meta.clone();
    if !scan.resolved.is_empty() {
        let ids: Vec<serde_json::Value> = scan
            .resolved
            .iter()
            .map(|m| serde_json::Value::String(m.employee_id.clone()))
            .collect();
        meta.insert("mentions".into(), serde_json::Value::Array(ids));
    }
    if let Some(reason) = &params.reason {
        meta.insert("reason".into(), serde_json::Value::String(reason.clone()));
    }
    let origin_incarnation = here.journal_incarnation.as_str();
    let origin_seq = read
        .max_origin_sequence(&here.office_id, origin_incarnation)
        .map_err(|_| {
            refusal(
                ErrorCode::StorageError,
                "failed to allocate origin_sequence",
            )
        })?
        .saturating_add(1);
    let local_seq = read
        .max_local_seq()
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to allocate local_seq"))?
        .saturating_add(1);
    let record = build_new_message_record(
        here,
        &conv.id,
        sender,
        params,
        priority,
        meta,
        thread_root_id.clone(),
        origin_seq,
        org_seq,
        now_utc,
    );
    let mut change = Change::new(Ack::Committed);
    let conv_path = records::conversation_path(&conv.id);
    const MAX_CHANNEL_RECORD_BYTES: usize = 360 * 1024;
    if !conv_persisted {
        let encoded = records::encode_json(conv);
        if encoded.len() > MAX_CHANNEL_RECORD_BYTES {
            return Err(refusal(
                ErrorCode::TooLarge,
                "channel record exceeds replication size limit",
            ));
        }
        change = change.create(conv_path, encoded);
    } else if conv.auto_scope.is_some() && conv_changed {
        conv.revision = conv.revision.saturating_add(1);
        let encoded = records::encode_json(conv);
        if encoded.len() > MAX_CHANNEL_RECORD_BYTES {
            return Err(refusal(
                ErrorCode::TooLarge,
                "channel record exceeds replication size limit",
            ));
        }
        change = change.put(conv_path, encoded);
    }
    change = change.create(
        records::message_path(&conv.id, &record.id),
        records::encode_json(&record),
    );
    let reasons = collect_recipient_reasons(
        read,
        conv,
        &sender.id,
        active_members,
        &scan.resolved,
        thread_root_id.as_deref(),
        &record.kind,
        &record.metadata,
    )?;
    check_hr_access(read, employees, owner, &sender.id, reasons.keys().cloned())?;
    let (change, recipients) = append_send_recipient_states(
        read, here, &conv.id, &record, employees, reasons, org_seq, now_utc, change,
    )?;
    let open_question_asker_id = thread_root_id
        .as_deref()
        .and_then(|root_id| read.message(root_id).ok().flatten())
        .filter(|root| root.kind == KIND_REQUEST && super::asks::is_open_ask_card(read, root))
        .and_then(|root| root.sender_employee_id.map(|id| id.as_str().to_owned()));
    let thread_id = thread_root_id.unwrap_or_else(|| record.id.clone());
    Ok((
        change,
        ChatSendCommitted {
            result: ChatSendResult {
                id: record.id.clone(),
                conversation_id: conv.id.as_str().to_owned(),
                thread_id,
                local_seq,
                origin_seq,
                mentions: scan.resolved,
                unmatched_mentions: scan.unmatched,
                recipient_presence: Vec::new(),
                woke_agents: Vec::new(),
            },
            inserted: true,
            conversation_kind: conv.kind,
            open_question_asker_id,
            message: record,
            recipients,
        },
    ))
}

pub fn decide_chat_send<E>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    caller: &Id,
    params: &ChatSendParams,
    org_seq: u64,
    now_utc: &str,
) -> Result<(Change, ChatSendCommitted), Refusal>
where
    E: Engine,
    E::Tx: MessagingWrite,
{
    let priority = validate_chat_send_inputs(params)?;
    let employees = uow
        .view()
        .employees()
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read employees"))?;
    let sender = employees
        .iter()
        .find(|e| e.id == *caller && is_active_employee(e))
        .ok_or_else(|| refusal(ErrorCode::NotBound, "sender is not an active employee"))?;

    if let Some(replayed) = check_chat_send_replay(
        uow.view(),
        here,
        owner,
        sender,
        &employees,
        params,
        priority,
        now_utc,
    )? {
        return Ok(replayed);
    }

    let (mut conv, conv_persisted) = resolve_send_target(
        uow.view(),
        here,
        owner,
        sender,
        &employees,
        &params.to,
        now_utc,
    )?;
    if conv.archived {
        return Err(refusal(ErrorCode::Conflict, "conversation is archived"));
    }
    let active_members = effective_auto_members(uow.view(), &conv, owner)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to resolve members"))?;
    if !active_members.contains(&sender.id) && sender.id != *owner {
        return Err(refusal(
            ErrorCode::NotAuthorized,
            "sender is not an active member of this conversation",
        ));
    }
    if conv.kind != ConversationKind::Channel {
        let others = active_members
            .iter()
            .filter(|id| *id != &sender.id)
            .cloned();
        check_hr_access(uow.view(), &employees, owner, caller, others)?;
    }

    let mut conv_changed = false;
    if conv.auto_scope.is_none() {
        for eid in &active_members {
            if !conv.memberships.iter().any(|m| m.employee_id == *eid) {
                conv.memberships.push(ConversationMembership {
                    employee_id: eid.clone(),
                    joined_at: now_utc.to_owned(),
                    left_at: None,
                });
                conv_changed = true;
            }
        }
    }

    let thread_root_id =
        resolve_reply_thread_root(uow.view(), &conv.id, params.reply_to.as_deref())?;
    let candidates = build_mention_candidates(&employees, &active_members, owner);
    let scan = extract_mentions(&params.body, &candidates);
    let mentioned_ids: Vec<Id> = scan
        .resolved
        .iter()
        .filter_map(|m| Id::parse(&m.employee_id).ok())
        .collect();
    check_hr_access(uow.view(), &employees, owner, caller, mentioned_ids)?;

    build_send_commit_change(
        uow.view(),
        here,
        owner,
        &mut conv,
        conv_persisted,
        conv_changed,
        sender,
        &employees,
        &active_members,
        params,
        priority,
        scan,
        thread_root_id,
        org_seq,
        now_utc,
    )
}

fn ensure_dm_peer_active(
    conv: &ConversationRecord,
    caller: &Id,
    employees: &[Employee],
) -> Result<(), Refusal> {
    if conv.kind != ConversationKind::Dm {
        return Ok(());
    }
    let mut has_active_peer = false;
    for m in conv
        .memberships
        .iter()
        .filter(|m| m.left_at.is_none() && m.employee_id != *caller)
    {
        if !employees
            .iter()
            .any(|e| e.id == m.employee_id && is_active_employee(e))
        {
            return Err(refusal(
                ErrorCode::UnknownRecipient,
                "recipient is not active",
            ));
        }
        has_active_peer = true;
    }
    if !has_active_peer {
        return Err(refusal(
            ErrorCode::UnknownRecipient,
            "recipient is not active",
        ));
    }
    Ok(())
}

fn resolve_reply_target_conv(
    view: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    to_id: &str,
    now_utc: &str,
) -> Result<ConversationRecord, Refusal> {
    if json_string_len(to_id) > MAX_ID_BYTES {
        return Err(refusal(
            ErrorCode::TooLarge,
            "reply target id exceeds size limit",
        ));
    }
    let parent = view
        .message(to_id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read reply target"))?
        .ok_or_else(|| refusal(ErrorCode::NotFound, "message not found"))?;
    let conv = match view
        .conversation(&parent.conversation_id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read conversation"))?
    {
        Some(conv) => conv,
        None => all_conversations_including_auto(view, now_utc)
            .map_err(|_| refusal(ErrorCode::StorageError, "failed to load conversations"))?
            .into_iter()
            .find(|c| c.id == parent.conversation_id)
            .ok_or_else(|| refusal(ErrorCode::NotFound, "message not found"))?,
    };
    let employees = view
        .employees()
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read employees"))?;
    if !employees
        .iter()
        .any(|e| e.id == *caller && is_active_employee(e))
    {
        return Err(refusal(
            ErrorCode::NotBound,
            "sender is not an active employee",
        ));
    }
    let active_members = effective_auto_members(view, &conv, owner)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to resolve members"))?;
    if !active_members.contains(caller) && *caller != *owner {
        return Err(refusal(ErrorCode::NotFound, "message not found"));
    }
    if parent.kind == KIND_DELIVERY_FAILED || parent.sender_kind == SenderKind::Office {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "cannot reply to an office or delivery_failed notice",
        ));
    }
    ensure_dm_peer_active(&conv, caller, &employees)?;
    Ok(conv)
}

fn mark_reply_parent_read(
    view: &impl MessagingRead,
    org_id: &Id,
    target: (&Id, &Id, &str),
    org_seq: u64,
    now_utc: &str,
    change: Change,
) -> Result<Change, Refusal> {
    let (conv_id, caller, to_id) = target;
    let Some(mut parent_state) = view
        .recipient_state(to_id, caller)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read recipient state"))?
    else {
        return Ok(change);
    };
    if !matches!(
        parent_state.state,
        MessageLifecycleState::Unread | MessageLifecycleState::Snoozed
    ) {
        return Ok(change);
    }
    parent_state.state = MessageLifecycleState::Read;
    parent_state.snoozed_until = None;
    parent_state.snooze_after_task = false;
    parent_state.snooze_note = None;
    parent_state.triaged_at = Some(now_utc.to_owned());
    if parent_state.read_at.is_none() {
        parent_state.read_at = Some(now_utc.to_owned());
    }
    parent_state.updated_at = now_utc.to_owned();
    parent_state.org_sequence = org_seq;
    upsert_participant_snapshot(view, org_id, conv_id, caller, parent_state, change)
}

/// Native conversation-aware `msg.reply`: resolves `params.to_id` in the caller's visible
/// conversation (DM, GroupDM, or Channel), commits a threaded reply through [`decide_chat_send`]
/// (enforcing HR access, mentions, idempotency, and cross-office relay), and atomically marks the
/// answered message `Read` for `caller` when it was `Unread` or `Snoozed`.
pub fn decide_chat_reply<E>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    caller: &Id,
    params: &MsgReplyParams,
    org_seq: u64,
    now_utc: &str,
) -> Result<(Change, ChatSendCommitted), Refusal>
where
    E: Engine,
    E::Tx: MessagingWrite,
{
    let conv = resolve_reply_target_conv(uow.view(), owner, caller, &params.to_id, now_utc)?;
    let send_params = ChatSendParams {
        id: params.id.clone(),
        to: conv.id.as_str().to_owned(),
        kind: KIND_REPLY.to_owned(),
        priority: params.priority,
        subject: params.subject.clone(),
        body: params.body.clone(),
        meta: params.meta.clone(),
        reply_to: Some(params.to_id.clone()),
        also_in_channel: false,
        reason: None,
    };
    let (mut change, committed) =
        decide_chat_send(uow, here, owner, caller, &send_params, org_seq, now_utc)?;
    if committed.inserted {
        change = mark_reply_parent_read(
            uow.view(),
            &here.organization_id,
            (&conv.id, caller, &params.to_id),
            org_seq,
            now_utc,
            change,
        )?;
    }
    Ok((change, committed))
}

fn matches_inbox_state(state: MessageLifecycleState, params: &ChatInboxParams) -> bool {
    match params.state {
        Some(wanted) => state == wanted,
        None => params.include_all || state == MessageLifecycleState::Unread,
    }
}

fn inbox_item_if_matching(
    read: &impl MessagingRead,
    st: RecipientStateRecord,
    params: &ChatInboxParams,
    from_filter: Option<&Id>,
) -> Result<Option<ChatInboxItem>, ApiError> {
    if !matches_inbox_state(st.state, params) {
        return Ok(None);
    }
    if params.because.is_some_and(|b| st.delivered_because != b) {
        return Ok(None);
    }
    let Some(msg) = read
        .message(&st.message_id)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read message"))?
    else {
        return Ok(None);
    };
    if from_filter.is_some_and(|fid| msg.sender_employee_id.as_ref() != Some(fid)) {
        return Ok(None);
    }
    let thread_id = msg.thread_root_id.clone().unwrap_or_else(|| msg.id.clone());
    if params.thread_id.as_deref().is_some_and(|t| thread_id != t) {
        return Ok(None);
    }
    let seq = read
        .message_local_seq(&msg.id)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read local_seq"))?
        .unwrap_or(msg.origin_sequence);
    if params.state != Some(MessageLifecycleState::Snoozed)
        && params.cursor.is_some_and(|c| seq >= c)
    {
        return Ok(None);
    }
    Ok(Some(ChatInboxItem {
        id: msg.id,
        conversation_id: msg.conversation_id.as_str().to_owned(),
        thread_id,
        reply_to: msg.reply_to_message_id,
        from_member_id: msg
            .sender_employee_id
            .map(|i| i.as_str().to_owned())
            .unwrap_or_else(|| "office".into()),
        from_display_name: msg.sender_display_name,
        kind: msg.kind.clone(),
        priority: msg.priority,
        urgent: is_urgent(&msg.kind, msg.priority),
        subject: msg.subject,
        created_at: msg.created_at,
        state: st.state,
        triage_gen: st.triage_generation,
        snoozed_until: st.snoozed_until,
        snooze_after_task: st.snooze_after_task,
        snooze_note: st.snooze_note,
        read_at: st.read_at,
        delivered_because: st.delivered_because,
        related_message_ids: st.related_message_ids,
        reason: st.reason,
        local_seq: seq,
    }))
}

fn sort_chat_inbox_items(items: &mut [ChatInboxItem], state: Option<MessageLifecycleState>) {
    if state == Some(MessageLifecycleState::Snoozed) {
        items.sort_by(compare_snoozed_inbox_items);
    } else {
        items.sort_by(|a, b| b.local_seq.cmp(&a.local_seq).then_with(|| b.id.cmp(&a.id)));
    }
}

fn compare_snoozed_inbox_items(a: &ChatInboxItem, b: &ChatInboxItem) -> std::cmp::Ordering {
    match (&a.snoozed_until, &b.snoozed_until) {
        (Some(x), Some(y)) => {
            let cmp = match (
                crate::time::parse_rfc3339_ms(x),
                crate::time::parse_rfc3339_ms(y),
            ) {
                (Some(xm), Some(ym)) => xm.cmp(&ym),
                _ => x.cmp(y),
            };
            cmp.then_with(|| b.local_seq.cmp(&a.local_seq))
                .then_with(|| b.id.cmp(&a.id))
        }
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => b.local_seq.cmp(&a.local_seq).then_with(|| b.id.cmp(&a.id)),
    }
}

fn paginate_chat_inbox(
    mut items: Vec<ChatInboxItem>,
    params: &ChatInboxParams,
    limit: usize,
) -> ChatInboxResult {
    sort_chat_inbox_items(&mut items, params.state);
    if params.state == Some(MessageLifecycleState::Snoozed) {
        let offset = usize::try_from(params.cursor.unwrap_or(0)).unwrap_or(usize::MAX);
        if offset >= items.len() {
            items.clear();
        } else {
            items.drain(..offset);
        }
        let more = items.len() > limit;
        items.truncate(limit);
        let next_cursor = more.then(|| (offset + items.len()) as u64);
        return ChatInboxResult { items, next_cursor };
    }
    let more = items.len() > limit;
    items.truncate(limit);
    let next_cursor = if more {
        items.last().map(|i| i.local_seq)
    } else {
        None
    };
    ChatInboxResult { items, next_cursor }
}

pub fn list_chat_inbox(
    read: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    params: &ChatInboxParams,
) -> Result<ChatInboxResult, ApiError> {
    if params.state == Some(MessageLifecycleState::Unknown) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "unknown message state filter",
        ));
    }
    let limit = params.limit.unwrap_or(DEFAULT_PAGE_LIMIT);
    if limit == 0 || limit > MAX_PAGE_LIMIT {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "limit must be 1..=100",
        ));
    }
    let employees = read
        .employees()
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read employees"))?;
    let from_filter = match &params.from {
        Some(f) => Some(resolve_employee(&employees, owner, f)?.id.clone()),
        None => None,
    };
    let states = read
        .recipient_states_for_employee(caller)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read inbox"))?;
    let mut items = Vec::new();
    for st in states {
        if let Some(item) = inbox_item_if_matching(read, st, params, from_filter.as_ref())? {
            items.push(item);
        }
    }
    Ok(paginate_chat_inbox(items, params, limit as usize))
}

fn resolve_messages_target(
    read: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    params: &ChatMessagesParams,
    now_utc: &str,
) -> Result<ConversationRecord, ApiError> {
    let all = all_conversations_including_auto(read, now_utc)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to load conversations"))?;
    let conv = if let Some(ref_str) = &params.conversation {
        if let Ok(cid) = Id::parse(ref_str.trim()) {
            all.iter().find(|c| c.id == cid).cloned()
        } else {
            find_channel(&all, ref_str)
        }
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "conversation not found"))?
    } else if let Some(tid) = &params.thread_id {
        let msg = read
            .message(tid)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read thread"))?
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "thread not found"))?;
        all.iter()
            .find(|c| c.id == msg.conversation_id)
            .cloned()
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "conversation not found"))?
    } else {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "pass --conversation or --thread",
        ));
    };
    let active = effective_auto_members(read, &conv, owner)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to resolve members"))?;
    let allowed =
        conv.kind == ConversationKind::Channel || active.contains(caller) || caller == owner;
    if !allowed {
        return Err(ApiError::new(ErrorCode::NotFound, "conversation not found"));
    }
    Ok(conv)
}

fn build_chat_message_view(
    read: &impl MessagingRead,
    m: MessageRecord,
    thread_id: String,
    local_seq: u64,
) -> Result<ChatMessageView, ApiError> {
    let reactions = read
        .reactions_for_message(&m.id)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read reactions"))?
        .into_iter()
        .map(|r| ReactionView {
            employee_id: r.employee_id.as_str().to_owned(),
            reaction: r.reaction,
            created_at: r.created_at,
        })
        .collect();
    let answer_file = read
        .answer_for_message(&m.id)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read acceptance"))?;
    let acceptance = answer_file.as_ref().and_then(|a| {
        Some(HandoffAcceptanceView {
            accepted_by: a.accepted_by.as_ref()?.as_str().to_owned(),
            accepted_at: a.accepted_at.clone()?,
            reason: a.reason.clone(),
        })
    });
    let (answer, withdrawal) = answer_file
        .as_ref()
        .map(super::asks::decode_ask_answer_file)
        .unwrap_or((None, None));
    let mentions = m
        .metadata
        .get("mentions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Ok(ChatMessageView {
        id: m.id,
        conversation_id: m.conversation_id.as_str().to_owned(),
        thread_id,
        reply_to: m.reply_to_message_id,
        also_in_channel: m.also_in_channel,
        sender_kind: m.sender_kind.as_str().to_owned(),
        sender_employee_id: m.sender_employee_id.map(|i| i.as_str().to_owned()),
        sender_display_name: m.sender_display_name,
        kind: m.kind,
        priority: m.priority,
        subject: m.subject,
        body: m.body_markdown,
        meta: m.metadata,
        created_at: m.created_at,
        origin_sequence: local_seq,
        mentions,
        reactions,
        acceptance,
        answer,
        withdrawal,
    })
}

pub fn list_chat_messages(
    read: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    params: &ChatMessagesParams,
    now_utc: &str,
) -> Result<ChatMessagesResult, ApiError> {
    let limit = params.limit.unwrap_or(DEFAULT_PAGE_LIMIT);
    if limit == 0 || limit > MAX_PAGE_LIMIT {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "limit must be 1..=100",
        ));
    }
    let conv = resolve_messages_target(read, owner, caller, params, now_utc)?;
    let raw_msgs = read
        .messages_in_conversation(&conv.id)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read messages"))?;
    let mut views = Vec::new();
    for m in raw_msgs {
        let thread_id = m.thread_root_id.clone().unwrap_or_else(|| m.id.clone());
        if let Some(tid) = &params.thread_id {
            if thread_id != *tid {
                continue;
            }
        }
        let local_seq = read
            .message_local_seq(&m.id)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read local_seq"))?
            .unwrap_or(m.origin_sequence);
        if let Some(cursor) = params.cursor {
            if local_seq >= cursor {
                continue;
            }
        }
        views.push(build_chat_message_view(read, m, thread_id, local_seq)?);
    }
    let more = views.len() > limit as usize;
    if more {
        let skip = views.len() - limit as usize;
        views = views.split_off(skip);
    }
    let next_cursor = if more {
        views.first().map(|m| m.origin_sequence)
    } else {
        None
    };
    Ok(ChatMessagesResult {
        conversation_id: conv.id.as_str().to_owned(),
        messages: views,
        next_cursor,
    })
}

pub(super) fn require_conversation_access(
    read: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    conv_id: &Id,
    now_utc: &str,
) -> Result<(ConversationRecord, bool), Refusal> {
    let persisted = read
        .conversation(conv_id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read conversation"))?;
    let (conv, was_persisted) = match persisted {
        Some(c) => (c, true),
        None => {
            let virtuals = virtual_auto_channels(read, now_utc)
                .map_err(|_| refusal(ErrorCode::StorageError, "failed to load channels"))?;
            let found = virtuals
                .into_iter()
                .find(|c| c.id == *conv_id)
                .ok_or_else(|| refusal(ErrorCode::NotFound, "conversation not found"))?;
            (found, false)
        }
    };
    let active = effective_auto_members(read, &conv, owner)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to resolve members"))?;
    let allowed =
        conv.kind == ConversationKind::Channel || active.contains(caller) || caller == owner;
    if !allowed {
        return Err(refusal(ErrorCode::NotFound, "conversation not found"));
    }
    Ok((conv, was_persisted))
}

pub(super) fn put_or_create_file<E: Engine>(
    uow: &UnitOfWork<'_, E>,
    change: Change,
    path: org_store::DurablePath,
    bytes: Vec<u8>,
) -> Result<Change, Refusal> {
    let existed = uow
        .view()
        .source_hash(&path)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to check durable file"))?
        .is_some();
    Ok(if existed {
        change.put(path, bytes)
    } else {
        change.create(path, bytes)
    })
}

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

fn load_handoff_target(
    read: &impl MessagingRead,
    caller: &Id,
    params: &MsgAcceptParams,
) -> Result<(MessageRecord, RecipientStateRecord), Refusal> {
    if let Some(reason) = &params.reason {
        if json_string_len(reason) > MAX_NOTE_BYTES {
            return Err(refusal(ErrorCode::TooLarge, "reason exceeds size limit"));
        }
    }
    let msg = read
        .message(&params.id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read message"))?
        .ok_or_else(|| refusal(ErrorCode::NotFound, "message not found"))?;
    let state = read
        .recipient_state(&msg.id, caller)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read recipient state"))?
        .ok_or_else(|| {
            refusal(
                ErrorCode::NotFound,
                "handoff message is not addressed to caller",
            )
        })?;
    if msg.kind != "handoff" {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "msg.accept only applies to handoff messages",
        ));
    }
    Ok((msg, state))
}

pub fn decide_accept_handoff<E>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    caller: &Id,
    params: &MsgAcceptParams,
    org_seq: u64,
    now_utc: &str,
) -> Result<(Change, MsgAcceptResult), Refusal>
where
    E: Engine,
    E::Tx: MessagingWrite,
{
    let (msg, mut updated_state) = load_handoff_target(uow.view(), caller, params)?;
    if let Some(existing) = uow
        .view()
        .answer_for_message(&msg.id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read acceptance"))?
    {
        if existing.accepted_by.as_ref() == Some(caller) {
            return Ok((
                Change::new(Ack::Committed),
                MsgAcceptResult {
                    id: msg.id,
                    accepted_by: caller.as_str().to_owned(),
                    accepted_at: existing.accepted_at.unwrap_or_else(|| now_utc.to_owned()),
                    reason: existing.reason,
                },
            ));
        }
        return Err(refusal(
            ErrorCode::Conflict,
            "handoff was already accepted by another member",
        ));
    }
    let answer = ParticipantAnswerFile {
        schema_version: SCHEMA_VERSION,
        revision: 1,
        organization_id: here.organization_id.clone(),
        conversation_id: msg.conversation_id.clone(),
        employee_id: caller.clone(),
        message_id: msg.id.clone(),
        accepted_by: Some(caller.clone()),
        accepted_at: Some(now_utc.to_owned()),
        reason: params.reason.clone(),
        answer: None,
    };
    let ans_path = records::participant_answer_path(&msg.conversation_id, caller, &msg.id);
    let change = put_or_create_file(
        uow,
        Change::new(Ack::Committed),
        ans_path,
        records::encode_json(&answer),
    )?;
    updated_state.state = MessageLifecycleState::Read;
    if updated_state.read_at.is_none() {
        updated_state.read_at = Some(now_utc.to_owned());
    }
    updated_state.updated_at = now_utc.to_owned();
    updated_state.reason = params.reason.clone();
    updated_state.org_sequence = org_seq;
    let change = upsert_participant_snapshot(
        uow.view(),
        &here.organization_id,
        &msg.conversation_id,
        caller,
        updated_state,
        change,
    )?;
    Ok((
        change,
        MsgAcceptResult {
            id: msg.id,
            accepted_by: caller.as_str().to_owned(),
            accepted_at: now_utc.to_owned(),
            reason: params.reason.clone(),
        },
    ))
}

/// Converts a `MessageRecord` + optional `RecipientStateRecord` into an `org_proto::msg::MessageHeader`.
pub fn to_wire_header(
    msg: &MessageRecord,
    state: Option<&RecipientStateRecord>,
    local_seq: u64,
) -> org_proto::msg::MessageHeader {
    let from_member_id = msg
        .sender_employee_id
        .as_ref()
        .map(|i| i.as_str().to_owned())
        .unwrap_or_else(|| org_proto::msg::OFFICE_SENDER.to_owned());
    let to_member_id = state
        .map(|s| s.employee_id.as_str().to_owned())
        .unwrap_or_else(|| msg.conversation_id.as_str().to_owned());
    let to_display_name = state
        .and_then(|s| s.recipient_display_name.clone())
        .unwrap_or_else(|| to_member_id.clone());
    let meta_bytes = if msg.metadata.is_empty() {
        0
    } else {
        serde_json::to_vec(&msg.metadata)
            .map(|b| b.len() as u64)
            .unwrap_or(0)
    };
    org_proto::msg::MessageHeader {
        id: msg.id.clone(),
        thread_id: msg.thread_root_id.clone().unwrap_or_else(|| msg.id.clone()),
        reply_to: msg.reply_to_message_id.clone(),
        from_member_id,
        from_display_name: msg.sender_display_name.clone(),
        to_member_id,
        to_display_name,
        kind: msg.kind.clone(),
        priority: msg.priority,
        urgent: is_urgent(&msg.kind, msg.priority),
        subject: msg.subject.clone(),
        created_at: msg.created_at.clone(),
        expires_at: msg.expires_at.clone(),
        ttl_ms: msg.ttl_ms,
        body_bytes: json_string_len(&msg.body_markdown) as u64,
        meta_bytes,
        local_seq,
        state: state.map_or(MessageLifecycleState::Unread, |s| s.state),
        triage_gen: state.map_or(0, |s| s.triage_generation),
        snoozed_until: state.and_then(|s| s.snoozed_until.clone()),
        snooze_after_task: state.is_some_and(|s| s.snooze_after_task),
        snooze_note: state.and_then(|s| s.snooze_note.clone()),
        read_at: state.and_then(|s| s.read_at.clone()),
        expired_waiting_person: state.is_some_and(|s| s.expired_waiting_person),
    }
}

/// Ensures `msg.kind == KIND_REPLY` or valid DM reply helper for `org-mailbox` integration.
pub fn is_reply_kind(kind: &str) -> bool {
    kind == KIND_REPLY || kind == KIND_MSG
}

/// One validated `answer_candidate` inbox delivery produced by a channel watcher pass
/// (`ADR-0024` d11, `WATCH-1`, `WATCH-3`, `WATCH-7`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerCandidateDelivery {
    pub message_id: String,
    pub asker_id: Id,
    pub question_ids: Vec<String>,
    pub question_context: String,
}

/// Atomically delivers `answer_candidate` inbox entries for one channel watcher pass.
///
/// Idempotent across crash recovery (`WATCH-7`):
/// - Preserves higher-priority `DeliveredBecause` (`Addressed < ThreadReply < Mention < AnswerCandidate`)
///   so a message appears once in the asker's inbox (`WATCH-1`).
/// - Merges matched `question_ids` into `related_message_ids` without duplicates.
/// - Attaches `question_context` in `reason` so an asker with reset context sees the question text
///   beside the entry (`ADR-0024` d11).
/// - Returns only the `(message_id, asker_id)` pairs that received a genuinely new delivery or link.
pub fn deliver_answer_candidates<E: Engine<Tx: MessagingWrite>>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    conversation_id: &Id,
    deliveries: &[AnswerCandidateDelivery],
    org_seq: u64,
    now_utc: &str,
) -> Result<(Change, Vec<(String, Id)>), Refusal> {
    if deliveries.is_empty() {
        return Ok((Change::new(Ack::Committed), Vec::new()));
    }
    let view = uow.view();
    let Some(conv) = all_conversations_including_auto(view, now_utc)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to load conversations"))?
        .into_iter()
        .find(|c| c.id == *conversation_id)
    else {
        return Err(refusal(ErrorCode::NotFound, "conversation not found"));
    };
    let active_members = effective_auto_members(view, &conv, owner)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to resolve channel members"))?;
    let employees = view
        .employees()
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read employees"))?;
    let mut next_state_seq = view
        .max_state_sequence(&here.office_id, here.journal_incarnation.as_str())
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read state sequence"))?;
    let mut snapshots: BTreeMap<Id, ParticipantReceivedSnapshot> = BTreeMap::new();
    let mut newly_delivered = Vec::new();

    for d in deliveries {
        if !active_members.contains(&d.asker_id) {
            continue;
        }
        let Some(msg) = view
            .message(&d.message_id)
            .map_err(|_| refusal(ErrorCode::StorageError, "failed to read message"))?
        else {
            continue;
        };
        if msg.conversation_id != *conversation_id
            || msg.sender_employee_id.as_ref() == Some(&d.asker_id)
        {
            continue;
        }
        let existing = match snapshots
            .get(&d.asker_id)
            .and_then(|snap| snap.states.get(&d.message_id).cloned())
        {
            Some(st) => Some(st),
            None => view
                .recipient_state(&d.message_id, &d.asker_id)
                .map_err(|_| refusal(ErrorCode::StorageError, "failed to read recipient state"))?,
        };
        let Some(record) = merge_answer_candidate_state(
            existing,
            d,
            here,
            conversation_id,
            &employees,
            &mut next_state_seq,
            org_seq,
            now_utc,
        ) else {
            continue;
        };
        if !snapshots.contains_key(&d.asker_id) {
            let loaded = load_participant_snapshot(
                view,
                &here.organization_id,
                conversation_id,
                &d.asker_id,
            )?;
            snapshots.insert(d.asker_id.clone(), loaded);
        }
        if let Some(snap) = snapshots.get_mut(&d.asker_id) {
            snap.states.insert(d.message_id.clone(), record);
        }
        newly_delivered.push((d.message_id.clone(), d.asker_id.clone()));
    }

    let change = commit_participant_snapshots(view, conversation_id, snapshots);
    Ok((change, newly_delivered))
}

#[expect(
    clippy::too_many_arguments,
    reason = "answer candidate merge combines existing state, candidate delivery, office identity, and roster"
)]
fn merge_answer_candidate_state(
    existing: Option<RecipientStateRecord>,
    d: &AnswerCandidateDelivery,
    here: &OfficeIdentity,
    conversation_id: &Id,
    employees: &[Employee],
    next_state_seq: &mut u64,
    org_seq: u64,
    now_utc: &str,
) -> Option<RecipientStateRecord> {
    match existing {
        Some(mut st) => {
            let mut mutated = false;
            let best = st.delivered_because.min(DeliveredBecause::AnswerCandidate);
            if best != st.delivered_because {
                st.delivered_because = best;
                mutated = true;
            }
            for qid in &d.question_ids {
                if !st.related_message_ids.contains(qid) {
                    st.related_message_ids.push(qid.clone());
                    mutated = true;
                }
            }
            if st.reason.is_none() && !d.question_context.is_empty() {
                st.reason = Some(d.question_context.clone());
                mutated = true;
            }
            if mutated {
                st.updated_at = now_utc.to_owned();
                st.org_sequence = org_seq;
                Some(st)
            } else {
                None
            }
        }
        None => {
            *next_state_seq += 1;
            let recipient_name = employees
                .iter()
                .find(|e| e.id == d.asker_id)
                .map(|e| e.name.clone());
            Some(RecipientStateRecord {
                organization_id: here.organization_id.clone(),
                message_id: d.message_id.clone(),
                conversation_id: conversation_id.clone(),
                employee_id: d.asker_id.clone(),
                recipient_display_name: recipient_name,
                state: MessageLifecycleState::Unread,
                triage_generation: 0,
                snoozed_until: None,
                snooze_after_task: false,
                snooze_note: None,
                triaged_at: None,
                read_at: None,
                updated_at: now_utc.to_owned(),
                reason: (!d.question_context.is_empty()).then(|| d.question_context.clone()),
                delivered_because: DeliveredBecause::AnswerCandidate,
                related_message_ids: d.question_ids.clone(),
                expired_waiting_person: false,
                owner_office_id: here.office_id.clone(),
                origin_incarnation: here.journal_incarnation.as_str().to_owned(),
                origin_sequence: *next_state_seq,
                org_epoch: 0,
                org_sequence: org_seq,
            })
        }
    }
}

fn load_participant_snapshot(
    view: &impl MessagingRead,
    organization_id: &Id,
    conversation_id: &Id,
    employee_id: &Id,
) -> Result<ParticipantReceivedSnapshot, Refusal> {
    let states = view
        .recipient_states_for_employee(employee_id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read recipient states"))?
        .into_iter()
        .filter(|s| s.conversation_id == *conversation_id)
        .map(|s| (s.message_id.clone(), s))
        .collect();
    Ok(ParticipantReceivedSnapshot {
        schema_version: SCHEMA_VERSION,
        revision: 1,
        organization_id: organization_id.clone(),
        conversation_id: conversation_id.clone(),
        employee_id: employee_id.clone(),
        states,
    })
}

fn commit_participant_snapshots(
    _view: &impl MessagingRead,
    conversation_id: &Id,
    snapshots: BTreeMap<Id, ParticipantReceivedSnapshot>,
) -> Change {
    let mut change = Change::new(Ack::Committed);
    for (emp_id, mut snap) in snapshots {
        snap.revision = snap.revision.saturating_add(1);
        let path = records::participant_received_snapshot_path(conversation_id, &emp_id);
        change = change.put(path, records::encode_json(&snap));
    }
    change
}

/// Returns the chronological latest `limit` (capped at 200) messages in a channel that `caller`
/// is permitted to read — identical visibility to `chat.messages` / `msg list` (`WATCH-10`).
pub fn channel_messages_for_caller(
    read: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    channel_ref: &str,
    limit: usize,
    now_utc: &str,
) -> Result<(ConversationRecord, Vec<MessageRecord>), ApiError> {
    let params = ChatMessagesParams {
        conversation: Some(channel_ref.to_owned()),
        thread_id: None,
        cursor: None,
        limit: Some(100),
    };
    let conv = resolve_messages_target(read, owner, caller, &params, now_utc)?;
    if conv.kind != ConversationKind::Channel {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "channel summarize only operates on channels",
        ));
    }
    let mut msgs = read
        .messages_in_conversation(&conv.id)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read channel messages"))?;
    let cap = limit.clamp(1, 200);
    if msgs.len() > cap {
        let skip = msgs.len() - cap;
        msgs = msgs.split_off(skip);
    }
    Ok((conv, msgs))
}
