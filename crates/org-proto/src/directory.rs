//! The organization directory and presence: who's in the organization, where, and what
//! they're doing (`docs/adr/0003-agent-messaging-office-first.md` §2.4).
//!
//! Served by the office runtime, not the messaging library. Cards combine the office's copy
//! of the organization (see [`crate::org`]) with live presence and each agent's self-reported
//! status line. Presence and status are runtime data: never in the organization's event log.
//! Members are named as in [`crate::msg`]'s addressing rules. Deliberate differences from
//! the ADR text are listed in [`crate::msg`]'s module docs.

use serde::{Deserialize, Serialize};

use crate::calls::Call;
use crate::events::EventKind;
use crate::org::{AgentStatus, MemberKind};

/// The longest status line, in characters (Unicode scalar values), after trimming.
pub const STATUS_MAX_CHARS: usize = 140;
/// The longest status line, in UTF-8 bytes of the trimmed text (not JSON-encoded bytes, unlike
/// [`crate::msg`]'s limits). UTF-8 is enough here because a status is a single line with no
/// control characters, so its JSON encoding stays close to its UTF-8 size.
pub const STATUS_MAX_BYTES: usize = 256;
/// How long a status line lasts when `ttl_ms` is omitted: 8 hours.
pub const DEFAULT_STATUS_TTL_MS: u64 = 8 * 3_600_000;
/// The longest `ttl_ms` a status line may have: 24 hours. More is `invalid_request`.
pub const MAX_STATUS_TTL_MS: u64 = 24 * 3_600_000;

/// What a member is doing right now. `online` is for people; the other values are for
/// agents (ADR-0003 §2.4, as amended by the agent-sessions design), and `offline` for both.
/// Unknown values decode as [`MemberPresence::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MemberPresence {
    /// A person with the app open. People only.
    Online,
    /// Running a turn.
    Busy,
    /// Between turns, or holding an `org-agent` wait.
    Idle,
    /// Waiting for the owner: an approval or input.
    NeedsYou,
    /// A person has taken over the session.
    WithYou,
    /// An on-call or scheduled agent with no live run (derived, not reported).
    Asleep,
    /// No live session (an agent), or the app isn't open (a person).
    Offline,
    /// A value this build doesn't know (sent by a newer peer).
    #[serde(other)]
    Unknown,
}

/// A team on a directory card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardTeam {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_in_team: Option<String>,
}

/// A member named on a card, e.g. who someone reports to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberRef {
    pub id: String,
    /// The display name (`Member.name`), not a handle.
    pub name: String,
    /// The handle to address this member by: an agent's
    /// [`messaging_name`](crate::org::AgentProfile::messaging_name), a person's `name`, and
    /// for the owner always [`OWNER_HANDLE`](crate::OWNER_HANDLE) (`owner`), never their
    /// name. Absent when the member has none (e.g. an agent record without its profile). At
    /// most [`MAX_HANDLE_BYTES`](crate::msg::MAX_HANDLE_BYTES).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
}

/// An office named on a card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeRef {
    pub id: String,
    /// The office's display name. For an office other than the one answering, it is the
    /// office's id until the organization carries office names (the registry, offices design
    /// phase 2); show it as is.
    pub name: String,
}

/// Where an agent's files are, as its owning office reports it (X-02 §8): read-only status, not
/// part of the agent's profile. The office manages the folder; clients show the path and never
/// open or interpret it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedHome {
    /// The launcher's effective `<instance root>/agents/<office id>/<member id>/`, an absolute
    /// path on the owning office's machine.
    pub path: String,
    /// The office that manages it: the one the agent is placed in.
    pub office_id: String,
    /// When the office observed the path: RFC 3339 UTC with milliseconds.
    pub observed_at: String,
}

/// One member's directory card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryCard {
    pub id: String,
    /// The display name (`Member.name`). For an agent it is not a handle; use `handle`.
    pub name: String,
    /// The handle to address this member by: an agent's
    /// [`messaging_name`](crate::org::AgentProfile::messaging_name), a person's `name`, and
    /// for the owner always [`OWNER_HANDLE`](crate::OWNER_HANDLE) (`owner`), never their
    /// name. Absent when the member has none (e.g. an agent record without its profile). At
    /// most [`MAX_HANDLE_BYTES`](crate::msg::MAX_HANDLE_BYTES).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    pub kind: MemberKind,
    /// The organization's owner (a person).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub owner: bool,
    /// Agents only: whether it takes work
    /// ([`AgentProfile::status`](crate::org::AgentProfile::status)). `who` never lists an
    /// [`Offboarded`](crate::org::AgentStatus::Offboarded) agent, so this is `active`,
    /// `paused` or `unknown`; a client must not treat `unknown` as working. Absent for
    /// people; on an agent's card, absent (an older server, or a record without its profile)
    /// means the same as `unknown`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_status: Option<AgentStatus>,
    /// For an agent, its job title ([`AgentProfile::title`](crate::org::AgentProfile::title)).
    /// Absent for people, whom the organization model gives no title. Per-team titles are in
    /// `teams`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_title: Option<String>,
    #[serde(default)]
    pub teams: Vec<CardTeam>,
    /// Absent for the owner, and for anyone who reports directly to the owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reports_to: Option<MemberRef>,
    /// Agents only: the office the agent is placed in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub office: Option<OfficeRef>,
    pub presence: MemberPresence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<String>,
    /// The member's status line, absent once it expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_task_id: Option<String>,
    /// Agents only: the home its owning office manages (X-02 §8). Sent only to a human
    /// principal on a host-made channel, and only for agents placed in the answering office;
    /// absent otherwise, and from servers that predate it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_home: Option<ManagedHome>,
}

/// `who`: directory cards for everyone in the organization, or those matching the filters.
/// Agent-bound or app-bound. Offboarded agents are left out: they're no longer in the
/// organization's directory (a `member` naming one is `not_found`).
///
/// Errors: `not_found` (`member` or `team` names nothing), `invalid_request` (`office`, an
/// `Unknown` kind, or a team name that matches more than one team), `too_large` (`member` or
/// `team` over [`MAX_HANDLE_BYTES`](crate::msg::MAX_HANDLE_BYTES)), `not_bound`,
/// `storage_error`.
pub struct Who;

/// `who`'s params. `{}` lists everyone; filters combine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhoParams {
    /// One member (see [`crate::msg`]'s addressing rules).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    /// A team's id or name (ignoring case): its members. Team names are unique only among
    /// siblings, so a name that matches more than one team is `invalid_request`: pass the id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    /// Only agents, or only people (the owner included). There is no owner kind: find the
    /// owner with `member: "owner"` ([`OWNER_HANDLE`](crate::OWNER_HANDLE)). `Unknown` is
    /// refused (`invalid_request`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<MemberKind>,
}

/// `who`'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhoResult {
    /// Unbounded today; paging it would come as an additive change.
    pub members: Vec<DirectoryCard>,
}

impl Call for Who {
    const METHOD: &'static str = "who";
    type Params = WhoParams;
    type Result = WhoResult;
}

/// `status.set`: set or clear the bound agent's own status line. **Agent-bound only.**
///
/// The text is trimmed and must be a single line with no control characters, at most
/// [`STATUS_MAX_CHARS`] characters and [`STATUS_MAX_BYTES`] UTF-8 bytes. Text that trims to
/// empty clears the line, like `text: None`, and so does a `ttl_ms` of 0.
///
/// A line also holds no character that hides text or reorders it: the Unicode line and
/// paragraph separators (U+2028, U+2029), the zero-width space (U+200B), the bidirectional
/// embeddings, overrides and isolates (U+202A-202E, U+2066-2069), the word joiner and invisible
/// operators (U+2060-2064), the deprecated formatting characters (U+206A-206F), the byte order
/// mark (U+FEFF), the invisible fillers (U+115F, U+1160, U+180E, U+3164, U+FFA0), the
/// interlinear annotation marks (U+FFF9-FFFB), the tag block (U+E0000-E007F) and the variation
/// selectors supplement (U+E0100-E01EF). The zero-width joiner and non-joiner and
/// the left-to-right and right-to-left marks are allowed: emoji sequences and Persian, Indic
/// and mixed-direction text need them.
///
/// Errors: `too_large` (`text` over a limit), `invalid_request` (control characters, line
/// breaks or hidden-text characters, or `ttl_ms` over [`MAX_STATUS_TTL_MS`]), `not_bound` (unbound, or bound
/// app-wide), `not_authorized` (bound as a person or the owner), `storage_error`.
pub struct StatusSet;

/// `status.set`'s params. `{}` clears the status line.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusSetParams {
    /// The new line; omitted, or empty after trimming, clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// How long it lasts. Omitted: [`DEFAULT_STATUS_TTL_MS`]. 0 clears the line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_ms: Option<u64>,
}

/// `status.set`'s result: the line as stored, or both absent when cleared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusSetResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

impl Call for StatusSet {
    const METHOD: &'static str = "status.set";
    type Params = StatusSetParams;
    type Result = StatusSetResult;
}

/// `member.presence_changed`: a member's presence or status line changed. Debounced to at
/// most one per member per second. Sent to bound connections only (member-bound or app-wide),
/// never to unbound ones.
pub struct MemberPresenceChanged;

/// The data of a `member.presence_changed` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberPresenceChangedData {
    pub member_id: String,
    pub presence: MemberPresence,
    pub last_seen_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_task_id: Option<String>,
}

impl EventKind for MemberPresenceChanged {
    const NAME: &'static str = "member.presence_changed";
    type Data = MemberPresenceChangedData;
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use serde::de::DeserializeOwned;
    use serde_json::{json, Value};

    use super::*;

    /// Asserts `value` serializes to exactly `expected` and decodes back to `value`.
    fn check<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T, expected: Value) {
        assert_eq!(serde_json::to_value(value).unwrap(), expected);
        assert_eq!(&serde_json::from_value::<T>(expected).unwrap(), value);
    }

    fn card() -> DirectoryCard {
        DirectoryCard {
            id: "01M6W".into(),
            name: "Agy".into(),
            handle: Some("agy-senior-dev".into()),
            kind: MemberKind::Agent,
            owner: false,
            agent_status: Some(AgentStatus::Paused),
            role_title: Some("Senior Backend Engineer".into()),
            teams: vec![CardTeam {
                id: "01M6T".into(),
                name: "Platform".into(),
                title_in_team: Some("Messaging Engineer".into()),
            }],
            reports_to: Some(MemberRef {
                id: "01M6A".into(),
                name: "Ada".into(),
                handle: Some("ac-staff-engineer-ada".into()),
            }),
            office: Some(OfficeRef {
                id: "01M6O".into(),
                name: "HQ".into(),
            }),
            presence: MemberPresence::Busy,
            last_seen_at: Some("2026-11-08T05:08:00.000Z".into()),
            status: Some("Drafting ADR-0003".into()),
            status_expires_at: Some("2026-11-08T13:08:00.000Z".into()),
            current_task_id: None,
            managed_home: None,
        }
    }

    #[test]
    fn who_wire_shape() {
        check(&WhoParams::default(), json!({}));
        assert_eq!(
            serde_json::from_value::<WhoParams>(json!({})).unwrap(),
            WhoParams::default()
        );
        check(
            &WhoParams {
                member: Some("ada".into()),
                team: Some("Platform".into()),
                kind: Some(MemberKind::Person),
            },
            json!({"member": "ada", "team": "Platform", "kind": "person"}),
        );
        check(
            &WhoResult {
                members: vec![card()],
            },
            json!({"members": [{
                "id": "01M6W",
                "name": "Agy",
                "handle": "agy-senior-dev",
                "kind": "agent",
                "agent_status": "paused",
                "role_title": "Senior Backend Engineer",
                "teams": [{"id": "01M6T", "name": "Platform", "title_in_team": "Messaging Engineer"}],
                "reports_to": {"id": "01M6A", "name": "Ada", "handle": "ac-staff-engineer-ada"},
                "office": {"id": "01M6O", "name": "HQ"},
                "presence": "busy",
                "last_seen_at": "2026-11-08T05:08:00.000Z",
                "status": "Drafting ADR-0003",
                "status_expires_at": "2026-11-08T13:08:00.000Z"
            }]}),
        );
        // The owner: no team, office, manager or title; `teams` is still sent, and may be
        // missing. The owner's handle is always `owner`, never their name.
        let owner = DirectoryCard {
            id: "01M6X".into(),
            name: "Vinicius".into(),
            handle: Some(crate::OWNER_HANDLE.into()),
            kind: MemberKind::Person,
            owner: true,
            agent_status: None,
            role_title: None,
            teams: vec![],
            reports_to: None,
            office: None,
            presence: MemberPresence::Online,
            last_seen_at: None,
            status: None,
            status_expires_at: None,
            current_task_id: Some("T-1".into()),
            managed_home: None,
        };
        check(
            &owner,
            json!({
                "id": "01M6X", "name": "Vinicius", "handle": "owner", "kind": "person",
                "owner": true, "teams": [], "presence": "online",
                "current_task_id": "T-1"
            }),
        );
        let decoded: DirectoryCard = serde_json::from_value(json!({
            "id": "01M6X", "name": "Vinicius", "handle": "owner", "kind": "person",
            "owner": true, "presence": "online", "current_task_id": "T-1",
            "pronouns": "he/him"
        }))
        .unwrap();
        assert_eq!(decoded, owner);
    }

    #[test]
    fn card_agent_status_values() {
        for (status, name) in [
            (AgentStatus::Active, "active"),
            (AgentStatus::Paused, "paused"),
        ] {
            let mut c = card();
            c.agent_status = Some(status);
            assert_eq!(serde_json::to_value(&c).unwrap()["agent_status"], name);
            assert_eq!(
                serde_json::from_value::<DirectoryCard>(serde_json::to_value(&c).unwrap()).unwrap(),
                c
            );
        }
        // Any other value, in any shape, reads as `Unknown` (never as working) and never
        // fails the card, except `offboarded`, which `who` never sends but still decodes;
        // `null` reads as absent.
        for (value, expected) in [
            (json!("sabbatical"), Some(AgentStatus::Unknown)),
            (json!(5), Some(AgentStatus::Unknown)),
            (json!({"status": "active"}), Some(AgentStatus::Unknown)),
            (json!("offboarded"), Some(AgentStatus::Offboarded)),
            (Value::Null, None),
        ] {
            let mut v = serde_json::to_value(card()).unwrap();
            v["agent_status"] = value.clone();
            assert_eq!(
                serde_json::from_value::<DirectoryCard>(v)
                    .unwrap()
                    .agent_status,
                expected,
                "{value}"
            );
        }
    }

    #[test]
    fn member_ref_handle_is_optional_and_unknown_kind_decodes() {
        check(
            &MemberRef {
                id: "01M6A".into(),
                name: "Ada".into(),
                handle: None,
            },
            json!({"id": "01M6A", "name": "Ada"}),
        );
        // The server refuses it (`invalid_request`), so it must decode to be refused.
        assert_eq!(
            serde_json::from_value::<WhoParams>(json!({"kind": "robot"}))
                .unwrap()
                .kind,
            Some(MemberKind::Unknown)
        );
    }

    #[test]
    fn status_set_wire_shape() {
        check(&StatusSetParams::default(), json!({}));
        assert_eq!(
            serde_json::from_value::<StatusSetParams>(json!({})).unwrap(),
            StatusSetParams::default()
        );
        check(
            &StatusSetParams {
                text: Some("Reviewing PR 12".into()),
                ttl_ms: Some(DEFAULT_STATUS_TTL_MS),
            },
            json!({"text": "Reviewing PR 12", "ttl_ms": 28800000}),
        );
        check(
            &StatusSetResult {
                status: Some("Reviewing PR 12".into()),
                expires_at: Some("2026-11-08T13:08:00.000Z".into()),
            },
            json!({"status": "Reviewing PR 12", "expires_at": "2026-11-08T13:08:00.000Z"}),
        );
        check(
            &StatusSetResult {
                status: None,
                expires_at: None,
            },
            json!({}),
        );
    }

    #[test]
    fn presence_changed_wire_shape() {
        check(
            &MemberPresenceChangedData {
                member_id: "01M6W".into(),
                presence: MemberPresence::NeedsYou,
                last_seen_at: "2026-11-08T05:08:00.000Z".into(),
                status: None,
                status_expires_at: None,
                current_task_id: None,
            },
            json!({
                "member_id": "01M6W", "presence": "needs_you",
                "last_seen_at": "2026-11-08T05:08:00.000Z"
            }),
        );
        check(
            &MemberPresenceChangedData {
                member_id: "01M6W".into(),
                presence: MemberPresence::WithYou,
                last_seen_at: "2026-11-08T05:08:00.000Z".into(),
                status: Some("Pairing".into()),
                status_expires_at: Some("2026-11-08T13:08:00.000Z".into()),
                current_task_id: Some("T-9".into()),
            },
            json!({
                "member_id": "01M6W", "presence": "with_you",
                "last_seen_at": "2026-11-08T05:08:00.000Z", "status": "Pairing",
                "status_expires_at": "2026-11-08T13:08:00.000Z", "current_task_id": "T-9"
            }),
        );
    }

    #[test]
    fn presence_values_on_the_wire() {
        for (presence, name) in [
            (MemberPresence::Online, "online"),
            (MemberPresence::Busy, "busy"),
            (MemberPresence::Idle, "idle"),
            (MemberPresence::NeedsYou, "needs_you"),
            (MemberPresence::WithYou, "with_you"),
            (MemberPresence::Asleep, "asleep"),
            (MemberPresence::Offline, "offline"),
            // Re-serializes as "unknown" (a CLI printing it shows that).
            (MemberPresence::Unknown, "unknown"),
        ] {
            assert_eq!(serde_json::to_value(presence).unwrap(), name);
            assert_eq!(
                serde_json::from_value::<MemberPresence>(name.into()).unwrap(),
                presence
            );
        }
        assert_eq!(
            serde_json::from_value::<MemberPresence>(json!("away")).unwrap(),
            MemberPresence::Unknown
        );
    }
}
