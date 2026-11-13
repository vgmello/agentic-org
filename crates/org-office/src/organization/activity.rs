//! Org activity (IA-12): its values and pure rules (verbs, subjects, filters, merge keys).

use std::collections::BTreeMap;

use serde::de::IgnoredAny;
use serde::Deserialize;

/// How many rows below its own row a change looks for its entity's last upsert.
pub(crate) const LOOKBACK_ROWS: usize = 2_000;

/// A change's place in commit order: `index` within the log row `(epoch, seq)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActivityPosition {
    pub epoch: u64,
    pub seq: u64,
    pub index: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivitySubject {
    pub member: Option<String>,
    pub team: Option<String>,
    pub project: Option<String>,
}

/// One org change: ids, verb, times and group (its command) only, never free text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityEntry {
    pub position: ActivityPosition,
    pub verb: String,
    /// The recorded `at`; absent when unparseable (the entry's source is degraded).
    pub at_ms: Option<u64>,
    /// The running minimum of `at_ms` in commit order.
    pub key: u64,
    pub actor: Option<String>,
    pub subject: ActivitySubject,
    pub group: String,
}

/// A team and its direct members as resolved on page 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamFilter {
    pub id: String,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgActivityQuery {
    pub epoch: u64,
    /// The head `seq` page 1 saw.
    pub ceiling: u64,
    /// Resume below the previous page's `scanned_to`.
    pub before: Option<ActivityPosition>,
    /// The previous page's `frontier`; `u64::MAX` on page 1.
    pub prev_key: u64,
    pub member: Option<String>,
    pub team: Option<TeamFilter>,
    pub project: Option<String>,
    pub limit: usize,
    /// At most this many log rows per page.
    pub scan: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityPage {
    pub entries: Vec<ActivityEntry>,
    /// The last scanned (or, past `limit`, returned) position; `None`: nothing scanned.
    pub scanned_to: Option<ActivityPosition>,
    /// The merge key at `scanned_to`.
    pub frontier: u64,
    /// The scan reached the epoch's oldest retained row.
    pub exhausted: bool,
    /// The retained log lacks the genesis.
    pub history_truncated: bool,
}

/// Per project with activity, its newest entry's `at_ms`: `None` when that `at` is unparseable
/// (degraded). A project with no retained activity is absent.
pub type ProjectActivity = BTreeMap<String, Option<u64>>;

#[derive(Debug)]
pub enum ActivityError {
    /// Another epoch, a rollback below the ceiling, or a position no longer retained.
    SnapshotRequired,
    Store(crate::OfficeError),
}

impl From<crate::OfficeError> for ActivityError {
    fn from(error: crate::OfficeError) -> Self {
        Self::Store(error)
    }
}

impl std::fmt::Display for ActivityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SnapshotRequired => f.write_str("the activity history requires a fresh snapshot"),
            Self::Store(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ActivityError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SnapshotRequired => None,
            Self::Store(error) => Some(error),
        }
    }
}

impl OrgActivityQuery {
    /// Criteria combine with AND, each internally OR.
    pub(crate) fn matches(&self, actor: Option<&str>, subject: &ActivitySubject) -> bool {
        let member = |id: &str| actor == Some(id) || subject.member.as_deref() == Some(id);
        self.member.as_deref().is_none_or(member)
            && self.team.as_ref().is_none_or(|team| {
                subject.team.as_deref() == Some(team.id.as_str())
                    || team.members.iter().any(|id| member(id))
            })
            && (self.project.as_deref()).is_none_or(|id| subject.project.as_deref() == Some(id))
    }
}

/// The next merge key: never rising; an unparseable time keeps the previous one.
pub(crate) fn merge_key(previous: u64, at_ms: Option<u64>) -> u64 {
    at_ms.map_or(previous, |at| at.min(previous))
}

/// A logged change as the contract's JSON (`OrgChange`) has it: the same variants and required
/// fields, defaulting only what the contract defaults. Ids, versions and flags are decoded; a
/// required name or kind is only checked to be there.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Change {
    TeamUpserted {
        team: Team,
    },
    TeamDeleted {
        team_id: String,
    },
    MemberUpserted {
        member: Member,
    },
    MemberRemoved {
        member_id: String,
    },
    MembershipUpserted {
        membership: Membership,
    },
    MembershipRemoved {
        membership_id: String,
    },
    ProjectTeamAdded {
        project_team: ProjectTeam,
    },
    ProjectTeamRemoved {
        project_team_id: String,
    },
    ProjectUpserted {
        project: Project,
    },
    AreaUpserted {
        area: Area,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Team {
    id: String,
    #[serde(rename = "name")]
    _name: IgnoredAny,
    version: u64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Member {
    id: String,
    #[serde(rename = "kind")]
    _kind: IgnoredAny,
    #[serde(rename = "name")]
    _name: IgnoredAny,
    version: u64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Membership {
    id: String,
    team_id: String,
    member_id: String,
    version: u64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ProjectTeam {
    id: String,
    project_id: String,
    team_id: String,
    version: u64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Project {
    id: String,
    #[serde(rename = "name")]
    _name: IgnoredAny,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    version: u64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Area {
    id: String,
    project_id: String,
    #[serde(rename = "name")]
    _name: IgnoredAny,
    #[serde(default)]
    deleted: bool,
    version: u64,
}

/// An upsert's version, ids (member, team, project) and `archived` (project) or `deleted` (area).
type Upsert<'a> = (u64, [Option<&'a String>; 3], bool);

/// A change's entity (verb kind, id) and what it upserts; `None` for `Unknown`.
fn facts(change: &Change) -> Option<(&'static str, &String, Option<Upsert<'_>>)> {
    use Change as C;
    let (kind, id, version, ids, flag) = match change {
        C::TeamUpserted { team: e } => ("team", &e.id, e.version, [None, Some(&e.id), None], false),
        C::MemberUpserted { member: e } => {
            ("member", &e.id, e.version, [Some(&e.id), None, None], false)
        }
        C::MembershipUpserted { membership: e } => {
            let ids = [Some(&e.member_id), Some(&e.team_id), None];
            ("membership", &e.id, e.version, ids, false)
        }
        C::ProjectTeamAdded { project_team: e } => {
            let ids = [None, Some(&e.team_id), Some(&e.project_id)];
            ("project_team", &e.id, e.version, ids, false)
        }
        C::ProjectUpserted { project: e } => (
            "project",
            &e.id,
            e.version,
            [None, None, Some(&e.id)],
            e.archived,
        ),
        C::AreaUpserted { area: e } => (
            "area",
            &e.id,
            e.version,
            [None, None, Some(&e.project_id)],
            e.deleted,
        ),
        C::TeamDeleted { team_id: id } => return Some(("team", id, None)),
        C::MemberRemoved { member_id: id } => return Some(("member", id, None)),
        C::MembershipRemoved { membership_id: id } => return Some(("membership", id, None)),
        C::ProjectTeamRemoved {
            project_team_id: id,
        } => return Some(("project_team", id, None)),
        C::Unknown => return None,
    };
    Some((kind, id, Some((version, ids, flag))))
}

/// The entity whose last upsert `describe` needs, and whether the change upserts it.
pub(crate) fn entity(change: &Change) -> Option<((&'static str, &str), bool)> {
    facts(change).map(|(kind, id, upsert)| ((kind, id.as_str()), upsert.is_some()))
}

/// Whether `describe` reads the previous upsert (membership/project-team removals, project edits).
pub(crate) fn needs_previous(change: &Change) -> bool {
    facts(change).is_some_and(|(kind, _, upsert)| match upsert {
        None => !matches!(kind, "team" | "member"),
        Some((version, ..)) => kind == "project" && version != 1,
    })
}

/// The verb and subject of `change`, given its entity's last upsert within the lookback.
pub(crate) fn describe(change: &Change, previous: Option<&Change>) -> (String, ActivitySubject) {
    let Some((kind, id, upsert)) = facts(change) else {
        return ("org.changed".into(), ActivitySubject::default());
    };
    let previous = previous.and_then(facts).and_then(|(.., up)| up);
    let (verb, [member, team, project]) = match (upsert, previous) {
        (None, _) if kind == "team" => ("removed", [None, Some(id), None]),
        (None, _) if kind == "member" => ("removed", [Some(id), None, None]),
        (None, previous) => ("removed", previous.map_or([None; 3], |(_, ids, _)| ids)),
        (Some((_, ids, true)), _) if kind == "area" => ("removed", ids),
        (Some((1, ids, _)), _) => ("added", ids),
        (Some((_, ids, now)), Some((.., was))) if kind == "project" && now != was => {
            (if now { "archived" } else { "unarchived" }, ids)
        }
        (Some((_, ids, _)), _) => ("changed", ids),
    };
    let subject = ActivitySubject {
        member: member.cloned(),
        team: team.cloned(),
        project: project.cloned(),
    };
    (format!("org.{kind}.{verb}"), subject)
}
