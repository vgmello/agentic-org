//! Automatic channels: the virtual `#general`, team and project channels synthesized from the
//! organization snapshot, their effective members, and channel-name slugs.

use std::collections::BTreeSet;

use org_proto::chat::{AutoScope, ConversationKind};
use org_store::Id;

use crate::messaging::{
    ports::MessagingRead,
    records::{self, ConversationRecord, SCHEMA_VERSION},
};
use crate::organization::records::{Employee, TeamStatus};

use super::is_active_employee;

pub(super) const MAX_CHANNEL_NAME_LEN: usize = 64;

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
    let owner = crate::organization::owner::effective_owner_id(read, owner)?;
    let employees = read.employees()?;
    let active: Vec<&Employee> = employees.iter().filter(|e| is_active_employee(e)).collect();
    match conv.auto_scope {
        Some(AutoScope::Organization) => Ok(active.into_iter().map(|e| e.id.clone()).collect()),
        Some(AutoScope::Team) => {
            let tid = conv.team_id.as_ref();
            Ok(active
                .into_iter()
                .filter(|e| tid == Some(&e.home_team_id) || e.id == owner)
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
                    e.id == owner
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
