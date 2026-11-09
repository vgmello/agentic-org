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
    AgentStatus, Member, MemberKind, OrgSnapshot, RunMode, Team, OFFICE_HANDLE, OWNER_HANDLE,
};
use org_proto::{ApiError, ErrorCode};

use crate::rules::{is_offboarded, name_key};
use crate::time;

/// What the office knows about one member right now. Runtime data, never in the organization's
/// log. Presence is lost when the office stops; a status line is kept in `office.db` until it
/// lapses.
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

/// The handle a member is addressed by: `owner` for the owner, an agent's `messaging_name`, a
/// person's `name`. `None` for an agent record without its profile.
pub fn handle_of(member: &Member) -> Option<String> {
    if member.owner {
        return Some(OWNER_HANDLE.to_owned());
    }
    match (&member.kind, &member.agent) {
        (MemberKind::Agent, Some(agent)) => Some(agent.messaging_name.clone()),
        (MemberKind::Agent, None) => None,
        _ => Some(member.name.clone()),
    }
}

/// The comparison key of a handle: the lower-cased `messaging_name` of an agent, the name key of
/// a person.
fn handle_key(member: &Member) -> Option<String> {
    if member.owner {
        return None; // the owner's display name is not a handle; only `owner` names it
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
            wanted_team.is_none_or(|team| {
                snapshot
                    .memberships
                    .iter()
                    .any(|ms| ms.member_id == m.id && ms.team_id == team.id)
            })
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
    // What the member reports, else offline. An agent the office runs on call or on a schedule
    // that is not running is `asleep`: derived, never reported, so a reported `offline` for it
    // reads as asleep too.
    let presence = match live.as_ref().map(|r| r.presence) {
        Some(reported) if reported != MemberPresence::Offline => reported,
        _ => match agent.and_then(|a| a.run_mode) {
            Some(RunMode::OnCall | RunMode::Scheduled) => MemberPresence::Asleep,
            _ => MemberPresence::Offline,
        },
    };
    let mut teams: Vec<(String, CardTeam)> = snapshot
        .memberships
        .iter()
        .filter(|ms| ms.member_id == member.id)
        .filter_map(|ms| {
            let team = snapshot.teams.iter().find(|t| t.id == ms.team_id)?;
            Some((
                name_key(&team.name),
                CardTeam {
                    id: team.id.clone(),
                    name: team.name.clone(),
                    title_in_team: ms.title_in_team.clone(),
                },
            ))
        })
        .collect();
    teams.sort_by(|a, b| (&a.0, &a.1.id).cmp(&(&b.0, &b.1.id)));
    DirectoryCard {
        id: member.id.clone(),
        name: member.name.clone(),
        handle: handle_of(member),
        kind: member.kind,
        owner: member.owner,
        agent_status: (member.kind == MemberKind::Agent)
            .then(|| agent.map_or(AgentStatus::Unknown, |a| a.status)),
        role_title: agent.map(|a| a.title.clone()),
        teams: teams.into_iter().map(|(_, team)| team).collect(),
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
    }
}

#[cfg(test)]
mod tests {
    use org_proto::org::{AgentProfile, AgentRuntime, Membership, Position};

    use super::*;

    const NOW: &str = "2026-11-08T12:00:00.000Z";
    const OFFICE: ThisOffice<'static> = ThisOffice {
        id: "office1",
        name: "HQ",
    };

    fn id(n: &str) -> String {
        format!("{n:0<26}")
    }

    fn person(n: &str, name: &str) -> Member {
        Member {
            id: id(n),
            kind: MemberKind::Person,
            name: name.into(),
            contact: None,
            owner: false,
            office_id: None,
            agent: None,
            reports_to_member_id: None,
            created_at: None,
            version: 1,
        }
    }

    fn owner() -> Member {
        Member {
            owner: true,
            ..person("OWNER", "Vinicius")
        }
    }

    fn agent(n: &str, name: &str, handle: &str, status: AgentStatus) -> Member {
        Member {
            kind: MemberKind::Agent,
            office_id: Some("office1".into()),
            agent: Some(AgentProfile {
                role: "engineer".into(),
                title: "Senior Engineer".into(),
                focus: None,
                runtime: AgentRuntime::ClaudeCode,
                model: None,
                run_mode: None,
                schedule: None,
                home: None,
                messaging_name: handle.into(),
                status,
                offboarded_at: None,
            }),
            ..person(n, name)
        }
    }

    fn team(tid: &str, name: &str, owner_team: Option<&str>) -> Team {
        Team {
            id: tid.into(),
            name: name.into(),
            owner_team_id: owner_team.map(Into::into),
            lead_member_id: None,
            version: 1,
        }
    }

    fn joined(ms: &str, tid: &str, member: &str, title: Option<&str>) -> Membership {
        Membership {
            id: ms.into(),
            team_id: tid.into(),
            member_id: id(member),
            title_in_team: title.map(Into::into),
            version: 1,
        }
    }

    /// An owner, a person (Ana), two agents (Pixel active, Gone offboarded), two teams.
    fn org() -> OrgSnapshot {
        let mut pixel = agent("PIXEL", "Pixel the designer", "pixel", AgentStatus::Active);
        pixel.reports_to_member_id = Some(id("ANA"));
        let mut ana = person("ANA", "Ana");
        ana.reports_to_member_id = None;
        OrgSnapshot {
            position: Position { epoch: 1, seq: 9 },
            teams: vec![
                team("t-design", "Design", None),
                team("t-eng", "Engineering", None),
                team("t-eng-sub", "Design", Some("t-eng")),
            ],
            members: vec![
                owner(),
                ana,
                pixel,
                agent("GONE", "Gone", "gone", AgentStatus::Offboarded),
            ],
            memberships: vec![
                joined("m1", "t-design", "PIXEL", Some("Lead designer")),
                joined("m2", "t-eng", "PIXEL", None),
                joined("m3", "t-eng", "ANA", Some("Manager")),
            ],
            projects: vec![],
            project_teams: vec![],
            areas: vec![],
        }
    }

    fn ask(params: WhoParams) -> Result<Vec<String>, ErrorCode> {
        who(&org(), OFFICE, NOW, &NothingLive, &params)
            .map(|r| r.members.into_iter().map(|c| c.id).collect())
            .map_err(|e| e.code)
    }

    fn all() -> Vec<DirectoryCard> {
        who(&org(), OFFICE, NOW, &NothingLive, &WhoParams::default())
            .unwrap()
            .members
    }

    #[test]
    fn everyone_but_the_offboarded_is_listed_by_name_then_id() {
        let names: Vec<_> = all().into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["Ana", "Pixel the designer", "Vinicius"]);
    }

    #[test]
    fn a_card_has_the_handle_kind_title_teams_manager_and_office() {
        let cards = all();
        let pixel = cards.iter().find(|c| c.id == id("PIXEL")).unwrap();
        assert_eq!(pixel.handle.as_deref(), Some("pixel"));
        assert_eq!(pixel.kind, MemberKind::Agent);
        assert_eq!(pixel.agent_status, Some(AgentStatus::Active));
        assert_eq!(pixel.role_title.as_deref(), Some("Senior Engineer"));
        assert!(!pixel.owner);
        // Teams by name, with the title in each.
        let teams: Vec<_> = pixel
            .teams
            .iter()
            .map(|t| (t.name.as_str(), t.title_in_team.as_deref()))
            .collect();
        assert_eq!(
            teams,
            [("Design", Some("Lead designer")), ("Engineering", None)]
        );
        assert_eq!(
            pixel.reports_to.as_ref().unwrap().handle.as_deref(),
            Some("Ana")
        );
        assert_eq!(pixel.reports_to.as_ref().unwrap().name, "Ana");
        assert_eq!(
            pixel.office,
            Some(OfficeRef {
                id: "office1".into(),
                name: "HQ".into()
            })
        );
        assert_eq!(pixel.presence, MemberPresence::Offline);
    }

    #[test]
    fn the_owners_handle_is_owner_and_a_person_has_no_title_agent_status_or_office() {
        let cards = all();
        let owner = cards.iter().find(|c| c.owner).unwrap();
        assert_eq!(owner.handle.as_deref(), Some("owner"));
        assert_eq!(owner.name, "Vinicius");
        assert_eq!(owner.kind, MemberKind::Person);
        assert!(owner.teams.is_empty() && owner.reports_to.is_none());
        let ana = cards.iter().find(|c| c.id == id("ANA")).unwrap();
        assert_eq!(ana.handle.as_deref(), Some("Ana"));
        assert_eq!(
            (&ana.agent_status, &ana.role_title, &ana.office),
            (&None, &None, &None)
        );
        assert_eq!(ana.teams.len(), 1);
        assert_eq!(ana.teams[0].title_in_team.as_deref(), Some("Manager"));
    }

    #[test]
    fn a_member_is_named_by_owner_then_id_then_handle_ignoring_case() {
        let pixel = Ok(vec![id("PIXEL")]);
        let member = |m: &str| {
            ask(WhoParams {
                member: Some(m.into()),
                ..WhoParams::default()
            })
        };
        assert_eq!(member("owner"), Ok(vec![id("OWNER")]));
        assert_eq!(member("OWNER"), Ok(vec![id("OWNER")]));
        assert_eq!(member(&id("PIXEL")), pixel);
        assert_eq!(member("pixel"), pixel);
        assert_eq!(member("PiXeL"), pixel);
        assert_eq!(member("Ana"), Ok(vec![id("ANA")]));
        assert_eq!(member("ana"), Ok(vec![id("ANA")]));
        // A display name is not a handle, and neither is the owner's name.
        assert_eq!(member("Pixel the designer"), Err(ErrorCode::NotFound));
        assert_eq!(member("Vinicius"), Err(ErrorCode::NotFound));
        assert_eq!(member("nobody"), Err(ErrorCode::NotFound));
        // An offboarded agent is not in the directory.
        assert_eq!(member("gone"), Err(ErrorCode::NotFound));
        assert_eq!(member(&id("GONE")), Err(ErrorCode::NotFound));
    }

    #[test]
    fn an_id_beats_a_handle_and_the_office_is_never_a_member() {
        // A person whose NAME equals another member's id cannot capture the lookup by id.
        let mut snapshot = org();
        snapshot
            .members
            .push(person("TRICK", &id("ANA").to_lowercase()));
        assert_eq!(
            resolve_member(&snapshot, &id("ANA"))
                .unwrap()
                .map(|m| m.id.clone()),
            Some(id("ANA"))
        );
        assert!(resolve_member(&snapshot, "office").unwrap().is_none());
        assert!(resolve_member(&snapshot, "OFFICE").unwrap().is_none());
        assert_eq!(
            ask(WhoParams {
                member: Some("Office".into()),
                ..WhoParams::default()
            }),
            Err(ErrorCode::InvalidRequest)
        );
    }

    #[test]
    fn resolve_returns_an_offboarded_agent_as_stored_because_its_handle_stays_reserved() {
        let snapshot = org();
        let gone = resolve_member(&snapshot, "gone").unwrap().unwrap();
        assert!(is_offboarded(gone));
    }

    #[test]
    fn team_by_id_or_by_name_and_kind_filters_combine() {
        let team = |t: &str| {
            ask(WhoParams {
                team: Some(t.into()),
                ..WhoParams::default()
            })
        };
        // By id, and by name ignoring case.
        assert_eq!(team("t-eng"), Ok(vec![id("ANA"), id("PIXEL")]));
        assert_eq!(team("engineering"), Ok(vec![id("ANA"), id("PIXEL")]));
        assert_eq!(team("t-design"), Ok(vec![id("PIXEL")]));
        // "Design" names two teams (one under Engineering): ambiguous, the id works.
        assert_eq!(team("Design"), Err(ErrorCode::InvalidRequest));
        assert_eq!(team("t-eng-sub"), Ok(vec![]));
        assert_eq!(team("Nope"), Err(ErrorCode::NotFound));
        // Kind: agents only; people include the owner.
        let kind = |k| {
            ask(WhoParams {
                kind: Some(k),
                ..WhoParams::default()
            })
        };
        assert_eq!(kind(MemberKind::Agent), Ok(vec![id("PIXEL")]));
        assert_eq!(kind(MemberKind::Person), Ok(vec![id("ANA"), id("OWNER")]));
        assert_eq!(kind(MemberKind::Unknown), Err(ErrorCode::InvalidRequest));
        // Filters narrow each other; a member outside the team is an empty list, not an error.
        let both = |m: &str, t: &str| {
            ask(WhoParams {
                member: Some(m.into()),
                team: Some(t.into()),
                kind: None,
            })
        };
        assert_eq!(both("pixel", "t-design"), Ok(vec![id("PIXEL")]));
        assert_eq!(both("ana", "t-design"), Ok(vec![]));
        assert_eq!(
            ask(WhoParams {
                member: Some("pixel".into()),
                team: Some("t-eng".into()),
                kind: Some(MemberKind::Person),
            }),
            Ok(vec![])
        );
    }

    #[test]
    fn the_errors_come_in_the_documented_order() {
        let long = "x".repeat(MAX_HANDLE_BYTES + 1);
        let at_limit = "x".repeat(MAX_HANDLE_BYTES);
        let params = |member: Option<&str>, team: Option<&str>, kind| WhoParams {
            member: member.map(Into::into),
            team: team.map(Into::into),
            kind,
        };
        // too_large first, whatever else is wrong.
        assert_eq!(
            ask(params(Some(&long), Some("Nope"), Some(MemberKind::Unknown))),
            Err(ErrorCode::TooLarge)
        );
        assert_eq!(
            ask(params(None, Some(&long), None)),
            Err(ErrorCode::TooLarge)
        );
        assert_eq!(
            ask(params(Some(&at_limit), None, None)),
            Err(ErrorCode::NotFound)
        );
        // then invalid_request (kind, office) before not_found.
        assert_eq!(
            ask(params(Some("nobody"), None, Some(MemberKind::Unknown))),
            Err(ErrorCode::InvalidRequest)
        );
        assert_eq!(
            ask(params(Some("office"), Some("Nope"), None)),
            Err(ErrorCode::InvalidRequest)
        );
        // then the member before the team.
        assert_eq!(
            ask(params(Some("nobody"), Some("Design"), None)),
            Err(ErrorCode::NotFound)
        );
        assert_eq!(
            ask(params(Some("pixel"), Some("Design"), None)),
            Err(ErrorCode::InvalidRequest)
        );
    }

    struct Live(Vec<(String, MemberRuntime)>);
    impl RuntimeView for Live {
        fn runtime_of(&self, member_id: &str) -> Option<MemberRuntime> {
            self.0
                .iter()
                .find(|(id, _)| id == member_id)
                .map(|(_, r)| r.clone())
        }
    }

    fn runtime(presence: MemberPresence, status: Option<(&str, &str)>) -> MemberRuntime {
        MemberRuntime {
            presence,
            last_seen_at: Some("2026-11-08T11:59:00.000Z".into()),
            status: status.map(|(text, _)| text.to_owned()),
            status_expires_at: status.map(|(_, at)| at.to_owned()),
            current_task_id: Some("task-1".into()),
        }
    }

    #[test]
    fn presence_and_status_come_from_the_runtime_and_a_lapsed_status_is_absent() {
        let live = Live(vec![
            (
                id("PIXEL"),
                runtime(
                    MemberPresence::Busy,
                    Some(("Drafting the ADR", "2026-11-08T13:00:00.000Z")),
                ),
            ),
            (
                id("ANA"),
                runtime(
                    MemberPresence::Online,
                    Some(("back soon", "2026-11-08T11:00:00.000Z")), // lapsed an hour ago
                ),
            ),
        ]);
        let cards = who(&org(), OFFICE, NOW, &live, &WhoParams::default())
            .unwrap()
            .members;
        let pixel = cards.iter().find(|c| c.id == id("PIXEL")).unwrap();
        assert_eq!(pixel.presence, MemberPresence::Busy);
        assert_eq!(pixel.status.as_deref(), Some("Drafting the ADR"));
        assert_eq!(
            pixel.status_expires_at.as_deref(),
            Some("2026-11-08T13:00:00.000Z")
        );
        assert_eq!(pixel.current_task_id.as_deref(), Some("task-1"));
        assert_eq!(
            pixel.last_seen_at.as_deref(),
            Some("2026-11-08T11:59:00.000Z")
        );
        let ana = cards.iter().find(|c| c.id == id("ANA")).unwrap();
        assert_eq!(ana.presence, MemberPresence::Online);
        assert_eq!((&ana.status, &ana.status_expires_at), (&None, &None));
        // Exactly at the expiry the line has lapsed.
        let at_expiry = Live(vec![(
            id("PIXEL"),
            runtime(MemberPresence::Idle, Some(("x", NOW))),
        )]);
        let cards = who(&org(), OFFICE, NOW, &at_expiry, &WhoParams::default())
            .unwrap()
            .members;
        assert_eq!(
            cards.iter().find(|c| c.id == id("PIXEL")).unwrap().status,
            None
        );
    }

    #[test]
    fn an_office_run_agent_with_no_live_run_is_asleep_and_the_rest_are_offline() {
        let mut snapshot = org();
        let presence_with = |snapshot: &mut OrgSnapshot, mode: Option<RunMode>| {
            snapshot
                .members
                .iter_mut()
                .find(|m| m.id == id("PIXEL"))
                .unwrap()
                .agent
                .as_mut()
                .unwrap()
                .run_mode = mode;
            let cards = who(snapshot, OFFICE, NOW, &NothingLive, &WhoParams::default())
                .unwrap()
                .members;
            let presence = |n: &str| cards.iter().find(|c| c.id == id(n)).unwrap().presence;
            (presence("PIXEL"), presence("ANA"), presence("OWNER"))
        };
        // On call or scheduled, with no live run: asleep (derived, never reported).
        for mode in [RunMode::OnCall, RunMode::Scheduled] {
            assert_eq!(
                presence_with(&mut snapshot, Some(mode)),
                (
                    MemberPresence::Asleep,
                    MemberPresence::Offline,
                    MemberPresence::Offline
                ),
                "{mode:?}"
            );
        }
        // Always-on and hand-run agents are offline, not asleep.
        for mode in [Some(RunMode::AlwaysOn), None] {
            assert_eq!(
                presence_with(&mut snapshot, mode).0,
                MemberPresence::Offline,
                "{mode:?}"
            );
        }
    }

    #[test]
    fn an_agent_in_another_office_shows_its_office_id_and_a_record_without_a_profile_has_no_handle()
    {
        let mut snapshot = org();
        for member in &mut snapshot.members {
            if member.id == id("PIXEL") {
                member.office_id = Some("elsewhere".into());
            }
        }
        snapshot.members.push(Member {
            kind: MemberKind::Agent,
            ..person("BARE", "Bare record")
        });
        let cards = who(&snapshot, OFFICE, NOW, &NothingLive, &WhoParams::default())
            .unwrap()
            .members;
        let pixel = cards.iter().find(|c| c.id == id("PIXEL")).unwrap();
        assert_eq!(pixel.office.as_ref().unwrap().name, "elsewhere");
        let bare = cards.iter().find(|c| c.id == id("BARE")).unwrap();
        assert_eq!(bare.handle, None);
        assert_eq!(bare.agent_status, Some(AgentStatus::Unknown));
        assert_eq!(bare.role_title, None);
    }

    // ---- the review's cases -----------------------------------------------------------------

    #[test]
    fn cards_are_ordered_by_name_ignoring_case_then_by_id() {
        let mut snapshot = org();
        // Pushed so that the order they are stored in is NOT the order they must come out in.
        snapshot.members.push(person("AAA", "ana")); // "ana" == "Ana" ignoring case; AAA < ANA
        snapshot.members.push(person("ZED", "Bob"));
        snapshot.members.push(person("BOB", "bob")); // BOB < ZED
        let cards = who(&snapshot, OFFICE, NOW, &NothingLive, &WhoParams::default())
            .unwrap()
            .members;
        let order: Vec<_> = cards
            .iter()
            .map(|c| (c.name.as_str(), c.id.as_str()))
            .collect();
        assert_eq!(
            order,
            [
                ("ana", id("AAA").as_str()), // the same name as Ana: the id breaks the tie
                ("Ana", id("ANA").as_str()),
                ("bob", id("BOB").as_str()),
                ("Bob", id("ZED").as_str()),
                ("Pixel the designer", id("PIXEL").as_str()),
                ("Vinicius", id("OWNER").as_str()),
            ]
        );
    }

    #[test]
    fn a_cards_teams_are_ordered_by_name_whatever_order_the_memberships_are_in() {
        let mut snapshot = org();
        snapshot.memberships.reverse();
        let cards = who(&snapshot, OFFICE, NOW, &NothingLive, &WhoParams::default())
            .unwrap()
            .members;
        let pixel = cards.iter().find(|c| c.id == id("PIXEL")).unwrap();
        let names: Vec<_> = pixel.teams.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Design", "Engineering"]);
    }

    #[test]
    fn handles_compare_ignoring_case_for_any_script_and_keep_the_case_they_were_given() {
        let mut snapshot = org();
        snapshot
            .members
            .push(agent("REMY", "Remy", "Remy", AgentStatus::Active));
        snapshot.members.push(person("EVA", "Éva"));
        let found = |name: &str| {
            resolve_member(&snapshot, name)
                .unwrap()
                .map(|m| m.id.clone())
        };
        for name in ["remy", "REMY", "Remy", "rEmY"] {
            assert_eq!(found(name), Some(id("REMY")), "{name}");
        }
        for name in ["Éva", "éva", "ÉVA", "éVA"] {
            assert_eq!(found(name), Some(id("EVA")), "{name}");
        }
        let cards = who(&snapshot, OFFICE, NOW, &NothingLive, &WhoParams::default())
            .unwrap()
            .members;
        let remy = cards.iter().find(|c| c.id == id("REMY")).unwrap();
        assert_eq!(remy.handle.as_deref(), Some("Remy")); // as given, not folded
    }

    #[test]
    fn the_word_owner_names_the_owner_even_when_a_member_has_that_id_and_office_names_no_one() {
        let mut snapshot = org();
        let mut sneaky = person("X", "Sneaky");
        sneaky.id = "owner".into(); // not a valid member id, but a stored record might be
        snapshot.members.push(sneaky);
        // A person named "Office" and an agent whose handle is "office" (HQ refuses both; a
        // damaged organization could hold them): neither is found by the reserved word.
        snapshot.members.push(person("OFF", "Office"));
        snapshot
            .members
            .push(agent("OFFA", "Office agent", "office", AgentStatus::Active));
        assert_eq!(
            resolve_member(&snapshot, "owner")
                .unwrap()
                .map(|m| m.id.clone()),
            Some(id("OWNER"))
        );
        for word in ["office", "OFFICE", "Office"] {
            assert!(resolve_member(&snapshot, word).unwrap().is_none(), "{word}");
        }
    }

    #[test]
    fn the_input_is_trimmed_like_the_handles_it_is_compared_with() {
        let snapshot = org();
        let found = |name: &str| {
            resolve_member(&snapshot, name)
                .unwrap()
                .map(|m| m.id.clone())
        };
        assert_eq!(found(" pixel "), Some(id("PIXEL")));
        assert_eq!(found("\t\npixel\r\n"), Some(id("PIXEL")));
        // Only ASCII whitespace is trimmed: a no-break space or an ideographic space is text.
        assert_eq!(found("\u{a0}pixel"), None);
        assert_eq!(found("pixel\u{3000}"), None);
        assert_eq!(found(" owner "), Some(id("OWNER")));
        assert_eq!(found(&format!(" {} ", id("ANA"))), Some(id("ANA")));
        assert!(resolve_member(&snapshot, " office ").unwrap().is_none());
        assert_eq!(
            ask(WhoParams {
                member: Some(" office ".into()),
                ..WhoParams::default()
            }),
            Err(ErrorCode::InvalidRequest)
        );
    }

    #[test]
    fn two_owners_fail_closed_instead_of_picking_one() {
        let mut snapshot = org();
        snapshot.members.push(Member {
            id: id("OWNER2"),
            ..owner()
        });
        assert_eq!(resolve_member(&snapshot, "owner"), Err(AmbiguousHandle));
    }

    #[test]
    fn two_members_with_one_handle_fail_closed_instead_of_picking_one() {
        let mut snapshot = org();
        snapshot
            .members
            .push(agent("TWIN", "Pixel twin", "PIXEL", AgentStatus::Active));
        assert_eq!(resolve_member(&snapshot, "pixel"), Err(AmbiguousHandle));
        // Their ids still name them, and `who` says it cannot answer rather than guess.
        assert!(resolve_member(&snapshot, &id("TWIN")).unwrap().is_some());
        let got = who(
            &snapshot,
            OFFICE,
            NOW,
            &NothingLive,
            &WhoParams {
                member: Some("pixel".into()),
                ..WhoParams::default()
            },
        );
        assert_eq!(got.unwrap_err().code, ErrorCode::StorageError);
    }

    #[test]
    fn a_membership_of_a_team_that_is_gone_is_left_off_the_card() {
        let mut snapshot = org();
        snapshot
            .memberships
            .push(joined("m9", "t-gone", "ANA", None));
        let cards = who(&snapshot, OFFICE, NOW, &NothingLive, &WhoParams::default())
            .unwrap()
            .members;
        let ana = cards.iter().find(|c| c.id == id("ANA")).unwrap();
        assert_eq!(ana.teams.len(), 1);
    }

    #[test]
    fn a_manager_who_is_the_owner_is_no_manager() {
        let mut snapshot = org();
        snapshot
            .members
            .iter_mut()
            .find(|m| m.id == id("ANA"))
            .unwrap()
            .reports_to_member_id = Some(id("OWNER"));
        let cards = who(&snapshot, OFFICE, NOW, &NothingLive, &WhoParams::default())
            .unwrap()
            .members;
        assert!(cards
            .iter()
            .find(|c| c.id == id("ANA"))
            .unwrap()
            .reports_to
            .is_none());
    }

    #[test]
    fn an_office_run_agent_that_reports_offline_still_reads_asleep() {
        let mut snapshot = org();
        snapshot
            .members
            .iter_mut()
            .find(|m| m.id == id("PIXEL"))
            .unwrap()
            .agent
            .as_mut()
            .unwrap()
            .run_mode = Some(RunMode::OnCall);
        // The run just ended: the runtime remembers when it was last seen, and says offline.
        let live = Live(vec![(id("PIXEL"), runtime(MemberPresence::Offline, None))]);
        let cards = who(&snapshot, OFFICE, NOW, &live, &WhoParams::default())
            .unwrap()
            .members;
        let pixel = cards.iter().find(|c| c.id == id("PIXEL")).unwrap();
        assert_eq!(pixel.presence, MemberPresence::Asleep);
        assert!(pixel.last_seen_at.is_some());
        // A live run is not asleep, and a hand-run agent that went offline is offline.
        let busy = Live(vec![(id("PIXEL"), runtime(MemberPresence::Busy, None))]);
        let cards = who(&snapshot, OFFICE, NOW, &busy, &WhoParams::default())
            .unwrap()
            .members;
        assert_eq!(
            cards.iter().find(|c| c.id == id("PIXEL")).unwrap().presence,
            MemberPresence::Busy
        );
        snapshot
            .members
            .iter_mut()
            .find(|m| m.id == id("PIXEL"))
            .unwrap()
            .agent
            .as_mut()
            .unwrap()
            .run_mode = None;
        let cards = who(&snapshot, OFFICE, NOW, &live, &WhoParams::default())
            .unwrap()
            .members;
        assert_eq!(
            cards.iter().find(|c| c.id == id("PIXEL")).unwrap().presence,
            MemberPresence::Offline
        );
    }

    #[test]
    fn a_status_lapses_by_the_instant_not_by_the_text_of_its_time() {
        let status_of = |expires: Option<&str>| {
            let live = Live(vec![(
                id("PIXEL"),
                MemberRuntime {
                    presence: MemberPresence::Busy,
                    last_seen_at: None,
                    status: Some("working".into()),
                    status_expires_at: expires.map(Into::into),
                    current_task_id: None,
                },
            )]);
            let cards = who(&org(), OFFICE, NOW, &live, &WhoParams::default())
                .unwrap()
                .members;
            cards
                .into_iter()
                .find(|c| c.id == id("PIXEL"))
                .unwrap()
                .status
        };
        let live = Some("working".to_owned());
        // NOW is 12:00:00.000Z. The same instant in other spellings has lapsed...
        assert_eq!(status_of(Some("2026-11-08T12:00:00Z")), None);
        assert_eq!(status_of(Some("2026-11-08T14:00:00+02:00")), None);
        // ...and a later one in another spelling has not (13:00Z, written with an offset).
        assert_eq!(status_of(Some("2026-11-08T14:00:00.001+01:00")), live);
        assert_eq!(status_of(Some("2026-11-08T12:00:00.001Z")), live);
        // An expiry that cannot be read counts as lapsed; none at all never lapses.
        assert_eq!(status_of(Some("tomorrow")), None);
        assert_eq!(status_of(Some("")), None);
        assert_eq!(status_of(None), live);
    }
}
