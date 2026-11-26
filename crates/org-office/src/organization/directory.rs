//! The directory (`who`): a card for each member, built from the organization as one consistent
//! snapshot plus what the office knows at runtime (presence, a status line). Pure: no storage and
//! no clock of its own, so the same snapshot and `now` always give the same cards.
//!
//! Members are named as in messaging's addressing rules: `owner` first, then an exact id, then a
//! handle ignoring case (an agent's `messaging_name`, a person's `name`). The reserved word
//! `office` never names a member, and display names are never handles.

use org_proto::directory::{
    CardTeam, DirectoryCard, MemberPresence, MemberRef, OfficeRef, WhoParams, WhoResult,
};
use org_proto::msg::MAX_HANDLE_BYTES;
use org_proto::org::{
    AgentStatus, Member, MemberKind, OrgSnapshot, Team, OFFICE_HANDLE, OWNER_HANDLE,
};
use org_proto::{ApiError, ErrorCode};

use super::names::name_key;

/// Whether `member` is an agent that was offboarded: a record that is history.
fn is_offboarded(member: &Member) -> bool {
    member
        .agent
        .as_ref()
        .is_some_and(|agent| agent.status == AgentStatus::Offboarded)
}
use crate::time;

/// What the office knows about one member right now. Runtime data, never in the organization's
/// log. Presence is lost when the office stops; a status line lapses at its expiry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRuntime {
    pub presence: MemberPresence,
    pub last_seen_at: Option<String>,
    /// The member's own status line and when it lapses (RFC 3339 UTC, as `now` is).
    pub status: Option<String>,
    pub status_expires_at: Option<String>,
    pub current_task_id: Option<String>,
}

/// Where cards get presence and status from.
pub trait RuntimeView {
    fn runtime_of(&self, member_id: &str) -> Option<MemberRuntime>;
}

/// Nothing is live: everyone reads as offline (or asleep, for an on-call or scheduled agent).
pub struct NothingLive;

impl RuntimeView for NothingLive {
    fn runtime_of(&self, _member_id: &str) -> Option<MemberRuntime> {
        None
    }
}

/// This office, for the `office` on an agent's card.
#[derive(Debug, Clone, Copy)]
pub struct ThisOffice<'a> {
    pub id: &'a str,
    pub name: &'a str,
}

/// The handle a member is addressed by: `owner` for the owner, an agent's `messaging_name`
/// or `handle`, or a person's `handle`/`name`. `None` for an agent record without either.
pub fn handle_of(member: &Member) -> Option<String> {
    if member.owner {
        return Some(OWNER_HANDLE.to_owned());
    }
    if let Some(handle) = &member.handle {
        return Some(handle.clone());
    }
    match (&member.kind, &member.agent) {
        (MemberKind::Agent, Some(agent)) => Some(agent.messaging_name.clone()),
        (MemberKind::Agent, None) => None,
        _ => Some(member.name.clone()),
    }
}

/// The comparison key of a handle: the lower-cased handle/messaging_name, or name key of
/// a person.
fn handle_key(member: &Member) -> Option<String> {
    if member.owner {
        return None; // the owner's display name is not a handle; only `owner` names it
    }
    if let Some(handle) = &member.handle {
        return Some(handle.to_lowercase());
    }
    match (&member.kind, &member.agent) {
        (MemberKind::Agent, Some(agent)) => Some(agent.messaging_name.to_lowercase()),
        (MemberKind::Agent, None) => None,
        _ => Some(name_key(&member.name)),
    }
}

/// An addressing input without the ASCII whitespace around it (space, tab, newline, form feed,
/// carriage return): every address (`who`, `msg.bind`'s member, a recipient) is trimmed so, and
/// only so, before it is looked up. Other Unicode spaces are part of the text.
pub fn trim_address(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_ascii_whitespace())
}

/// Two members have the same handle: a record that breaks HQ's uniqueness (a replayed or damaged
/// organization). Callers fail closed rather than pick one.
#[derive(Debug, PartialEq, Eq)]
pub struct AmbiguousHandle;

/// The member `id_or_handle` names, as messaging addresses them (the input is trimmed, as HQ
/// compares handles trimmed): `owner` (any case) first, then an exact id, then a handle ignoring
/// case. The member is returned as stored, an offboarded agent included (its handle stays
/// reserved): the caller decides what a status means. `office` is reserved and names no one.
/// `Err` when two members have the handle.
pub fn resolve_member<'a>(
    snapshot: &'a OrgSnapshot,
    id_or_handle: &str,
) -> Result<Option<&'a Member>, AmbiguousHandle> {
    let wanted = trim_address(id_or_handle);
    if wanted.eq_ignore_ascii_case(OWNER_HANDLE) {
        // Two owners cannot be told apart: fail closed rather than pick one.
        let mut owners = snapshot.members.iter().filter(|m| m.owner);
        return match (owners.next(), owners.next()) {
            (found, None) => Ok(found),
            (_, Some(_)) => Err(AmbiguousHandle),
        };
    }
    if wanted.eq_ignore_ascii_case(OFFICE_HANDLE) {
        return Ok(None);
    }
    if let Some(by_id) = snapshot.members.iter().find(|m| m.id == wanted) {
        return Ok(Some(by_id));
    }
    // Compared as stored handles are keyed (lower-cased); `wanted` is already trimmed.
    let key = wanted.to_lowercase();
    let mut named = snapshot
        .members
        .iter()
        .filter(|m| handle_key(m).is_some_and(|k| k == key));
    match (named.next(), named.next()) {
        (found, None) => Ok(found),
        (_, Some(_)) => Err(AmbiguousHandle),
    }
}

fn member_ref(member: &Member) -> MemberRef {
    MemberRef {
        id: member.id.clone(),
        name: member.name.clone(),
        handle: handle_of(member),
    }
}

/// `who`: the cards of everyone in the organization who matches `params`, ordered by display
/// name (ignoring case) and then id. Offboarded agents are not listed, and naming one is
/// `not_found`.
///
/// Errors, in this order: `too_large` (a `member` or `team` over [`MAX_HANDLE_BYTES`]);
/// `invalid_request` (an `Unknown` kind, `office` as the member); the member (`not_found`, or
/// `storage_error` when two members have the handle); then the team (`not_found`, or
/// `invalid_request` for a name that matches more than one team). The member is looked up before
/// the team, so an unknown member with an ambiguous team is `not_found`.
pub fn who(
    snapshot: &OrgSnapshot,
    office: ThisOffice<'_>,
    now: &str,
    runtime: &dyn RuntimeView,
    params: &WhoParams,
) -> Result<WhoResult, ApiError> {
    for text in [&params.member, &params.team].into_iter().flatten() {
        if text.len() > MAX_HANDLE_BYTES {
            return Err(ApiError::new(ErrorCode::TooLarge, "that name is too long"));
        }
    }
    if params.kind == Some(MemberKind::Unknown) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "kind must be agent or person",
        ));
    }
    if params
        .member
        .as_deref()
        .is_some_and(|m| trim_address(m).eq_ignore_ascii_case(OFFICE_HANDLE))
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "the office is not a member",
        ));
    }
    let wanted_member = match params.member.as_deref() {
        Some(name) => {
            let found = resolve_member(snapshot, name)
                .map_err(|_| {
                    ApiError::new(ErrorCode::StorageError, "two members share that handle")
                })?
                .filter(|m| !is_offboarded(m))
                .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "no such member"))?;
            Some(found.id.as_str())
        }
        None => None,
    };
    let wanted_team = match params.team.as_deref() {
        Some(name) => Some(resolve_team(snapshot, name)?),
        None => None,
    };

    let mut cards: Vec<(String, &Member)> = snapshot
        .members
        .iter()
        .filter(|m| !is_offboarded(m))
        .filter(|m| wanted_member.is_none_or(|id| m.id == id))
        .filter(|m| match params.kind {
            Some(kind) => m.kind == kind,
            None => true,
        })
        .filter(|m| {
            wanted_team.is_none_or(|team| m.home_team_id.as_deref() == Some(team.id.as_str()))
        })
        .map(|m| (name_key(&m.name), m))
        .collect();
    cards.sort_by(|a, b| (&a.0, &a.1.id).cmp(&(&b.0, &b.1.id)));
    Ok(WhoResult {
        members: cards
            .into_iter()
            .map(|(_, m)| card(snapshot, office, now, runtime, m))
            .collect(),
    })
}

/// A team named by id (exactly) or by name (ignoring case). A name that two teams share, under
/// different owners, is ambiguous: the caller passes the id.
fn resolve_team<'a>(snapshot: &'a OrgSnapshot, name: &str) -> Result<&'a Team, ApiError> {
    if let Some(by_id) = snapshot.teams.iter().find(|t| t.id == name) {
        return Ok(by_id);
    }
    let key = name_key(name);
    let mut matching = snapshot.teams.iter().filter(|t| name_key(&t.name) == key);
    match (matching.next(), matching.next()) {
        (Some(team), None) => Ok(team),
        (Some(_), Some(_)) => Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "more than one team has that name: use its id",
        )),
        (None, _) => Err(ApiError::new(ErrorCode::NotFound, "no such team")),
    }
}

fn card(
    snapshot: &OrgSnapshot,
    office: ThisOffice<'_>,
    now: &str,
    runtime: &dyn RuntimeView,
    member: &Member,
) -> DirectoryCard {
    let live = runtime.runtime_of(&member.id);
    // A status line lapses: absent once it expires. Times are compared as instants, never as
    // text (`...00Z` and `...00.000Z` are one instant, an offset moves it); one that cannot be
    // read counts as lapsed.
    let now_ms = time::parse_rfc3339_ms(now);
    let status_live = live.as_ref().is_some_and(|r| {
        r.status.is_some()
            && match r.status_expires_at.as_deref() {
                None => true,
                Some(at) => match (time::parse_rfc3339_ms(at), now_ms) {
                    (Some(at), Some(now)) => at > now,
                    _ => false,
                },
            }
    });
    let agent = member
        .agent
        .as_ref()
        .filter(|_| member.kind == MemberKind::Agent);
    // ADR-0028 decision 5 (AVAIL-4): an agent that is not working is always Available,
    // whether or not its process is running; paused agents show Paused; no Asleep/Offline
    // state is shown for an agent. People remain Online or Offline.
    let presence = if let Some(a) = agent {
        if a.status == AgentStatus::Paused {
            MemberPresence::Paused
        } else {
            match live.as_ref().map(|r| r.presence) {
                Some(
                    reported @ (MemberPresence::Busy
                    | MemberPresence::NeedsYou
                    | MemberPresence::WaitingForAnswer
                    | MemberPresence::UnableToRun
                    | MemberPresence::WithYou),
                ) => reported,
                _ => MemberPresence::Available,
            }
        }
    } else {
        match live.as_ref().map(|r| r.presence) {
            Some(reported) if reported != MemberPresence::Offline => reported,
            _ => MemberPresence::Offline,
        }
    };
    let teams: Vec<CardTeam> = member
        .home_team_id
        .as_deref()
        .and_then(|home_team_id| snapshot.teams.iter().find(|t| t.id == home_team_id))
        .map(|team| {
            vec![CardTeam {
                id: team.id.clone(),
                name: team.name.clone(),
                title_in_team: member.title_in_team.clone(),
            }]
        })
        .unwrap_or_default();
    DirectoryCard {
        id: member.id.clone(),
        name: member.name.clone(),
        handle: handle_of(member),
        kind: member.kind,
        owner: member.owner,
        agent_status: (member.kind == MemberKind::Agent)
            .then(|| agent.map_or(AgentStatus::Unknown, |a| a.status)),
        role_title: agent.map(|a| a.title.clone()),
        teams,
        reports_to: member
            .reports_to_member_id
            .as_deref()
            .and_then(|id| snapshot.members.iter().find(|m| m.id == id))
            // Reporting to the owner is "no manager"; HQ stores it so, and a card holds to it.
            .filter(|manager| !manager.owner)
            .map(member_ref),
        office: member.office_id.as_ref().map(|id| OfficeRef {
            id: id.clone(),
            // This office knows its own name only: another office is shown by its id for now
            // (the organization carries no office names; they come with the sync work).
            name: if id == office.id {
                office.name.to_owned()
            } else {
                id.clone()
            },
        }),
        presence,
        last_seen_at: live.as_ref().and_then(|r| r.last_seen_at.clone()),
        status: live
            .as_ref()
            .filter(|_| status_live)
            .and_then(|r| r.status.clone()),
        status_expires_at: live
            .as_ref()
            .filter(|_| status_live)
            .and_then(|r| r.status_expires_at.clone()),
        current_task_id: live.as_ref().and_then(|r| r.current_task_id.clone()),
        managed_home: None,
    }
}

#[cfg(test)]
mod tests;
