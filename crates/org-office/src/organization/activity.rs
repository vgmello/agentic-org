//! Org activity (IA-12): its values and pure rules (verbs, subjects, filters, merge keys).

use org_proto::org::OrgChange;

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

/// An upsert's version, ids (member, team, project) and `archived` (project) or `deleted` (area).
type Upsert<'a> = (u64, [Option<&'a String>; 3], bool);

/// A change's entity (verb kind, id) and what it upserts; `None` for `Unknown`.
fn facts(change: &OrgChange) -> Option<(&'static str, &String, Option<Upsert<'_>>)> {
    use OrgChange as C;
    Some(match change {
        C::TeamUpserted { team: e } => {
            let ids = [None, Some(&e.id), None];
            ("team", &e.id, Some((e.version, ids, false)))
        }
        C::MemberUpserted { member: e } => {
            let ids = [Some(&e.id), None, None];
            ("member", &e.id, Some((e.version, ids, false)))
        }
        C::MembershipUpserted { membership: e } => {
            let ids = [Some(&e.member_id), Some(&e.team_id), None];
            ("membership", &e.id, Some((e.version, ids, false)))
        }
        C::ProjectTeamAdded { project_team: e } => {
            let ids = [None, Some(&e.team_id), Some(&e.project_id)];
            ("project_team", &e.id, Some((e.version, ids, false)))
        }
        C::ProjectUpserted { project: e } => {
            let ids = [None, None, Some(&e.id)];
            ("project", &e.id, Some((e.version, ids, e.archived)))
        }
        C::AreaUpserted { area: e } => {
            let ids = [None, None, Some(&e.project_id)];
            ("area", &e.id, Some((e.version, ids, e.deleted)))
        }
        C::TeamDeleted { team_id: id } => ("team", id, None),
        C::MemberRemoved { member_id: id } => ("member", id, None),
        C::MembershipRemoved { membership_id: id } => ("membership", id, None),
        C::ProjectTeamRemoved {
            project_team_id: id,
        } => ("project_team", id, None),
        _ => return None,
    })
}

/// The entity whose last upsert `describe` needs, and whether the change upserts it.
pub(crate) fn entity(change: &OrgChange) -> Option<((&'static str, &str), bool)> {
    facts(change).map(|(kind, id, upsert)| ((kind, id.as_str()), upsert.is_some()))
}

/// Whether `describe` reads the previous upsert (membership/project-team removals, project edits).
pub(crate) fn needs_previous(change: &OrgChange) -> bool {
    facts(change).is_some_and(|(kind, _, upsert)| match upsert {
        None => !matches!(kind, "team" | "member"),
        Some((version, ..)) => kind == "project" && version != 1,
    })
}

/// The verb and subject of `change`, given its entity's last upsert within the lookback.
pub(crate) fn describe(
    change: &OrgChange,
    previous: Option<&OrgChange>,
) -> (String, ActivitySubject) {
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
