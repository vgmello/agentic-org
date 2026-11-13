//! The ports `org-work` defines and the office implements (`store.rs`, server-dev): the
//! organization as the office has applied it ([`OrgView`]), the work storage's write request
//! ([`WorkStore::transact`] and [`WorkTxn`]), its reads ([`WorkReads`]), the owner's activity
//! feed ([`WorkActivityReads`]) and the clock ([`Clock`]). All SQL lives behind them (ADR-0002
//! decision 5).

use std::error::Error;
use std::fmt;
use std::sync::Arc;

use org_proto::org::{Area, Member, Position, Project};
use org_proto::work::{EventRef, ItemKind, Link, LinkKind, WorkEvent, WorkPosition};
use serde::{Deserialize, Serialize};

use crate::engine::TxnOutput;
use crate::model::{StoredItem, StoredResult};

/// A storage failure. The cause is for the server's log; `Display` never shows it (no paths,
/// no SQL), so it is safe to answer a client with as `storage_error`.
#[derive(Debug)]
pub struct StoreError(Box<dyn Error + Send + Sync>);

impl StoreError {
    pub fn new(cause: impl Into<Box<dyn Error + Send + Sync>>) -> Self {
        Self(cause.into())
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the office could not read or write its work storage")
    }
}

impl Error for StoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.0.as_ref())
    }
}

/// Read-only view of the organization: `org.db` at the position the office has applied (never
/// queued commands). One `OrgView` is one `org.db` read transaction, opened by
/// [`WorkStore::transact`] and held for its duration, so every check and the recorded
/// organization position come from the same point in time.
///
/// Phase 1 needs the member and project reads; bugs (phase 2) add areas and team leads. The
/// cascades (`on_org_applied`) will add `changes_after`.
pub trait OrgView {
    /// The organization position this view shows. Recorded on every event (`org_position`).
    fn applied_position(&self) -> Result<Position, StoreError>;
    /// A member by id, whatever its state (an offboarded agent included). A removed person is
    /// gone: `None`.
    fn member(&self, id: &str) -> Result<Option<Member>, StoreError>;
    /// The owner's member id.
    fn owner_id(&self) -> Result<String, StoreError>;
    fn project(&self, id: &str) -> Result<Option<Project>, StoreError>;
    /// A project by its key (`chk`), for item references such as `chk-T-12`.
    fn project_by_key(&self, key: &str) -> Result<Option<Project>, StoreError>;
    /// The ids of the teams working on the project.
    fn project_teams(&self, project_id: &str) -> Result<Vec<String>, StoreError>;
    /// An area by id, whatever its state: a deleted one is still here as a tombstone, with
    /// `replaced_by` (closed bugs keep showing its name).
    fn area(&self, id: &str) -> Result<Option<Area>, StoreError>;
    /// The id of the project's General area (every project has exactly one, never deleted).
    fn general_area(&self, project_id: &str) -> Result<Option<String>, StoreError>;
    /// A team's lead, if it has one.
    fn team_lead(&self, team_id: &str) -> Result<Option<String>, StoreError>;
}

/// A numbering scope (§2.6): tasks and bugs per project, requests per organization. Numbers
/// start at 1, have no gaps at assignment and are never reused.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NumberScope {
    Tasks { project_id: String },
    Bugs { project_id: String },
    Requests,
}

impl NumberScope {
    /// The scope an item of `kind` in `project_id` is numbered in, if it is numbered at all.
    pub fn of(kind: ItemKind, project_id: Option<&str>) -> Option<Self> {
        match (kind, project_id) {
            (ItemKind::Task, Some(p)) => Some(Self::Tasks {
                project_id: p.to_owned(),
            }),
            (ItemKind::Bug, Some(p)) => Some(Self::Bugs {
                project_id: p.to_owned(),
            }),
            (ItemKind::Request, _) => Some(Self::Requests),
            _ => None,
        }
    }

    /// A stable text key for a storage column (`task:<project id>`, `bug:<project id>`,
    /// `request`).
    pub fn key(&self) -> String {
        match self {
            Self::Tasks { project_id } => format!("task:{project_id}"),
            Self::Bugs { project_id } => format!("bug:{project_id}"),
            Self::Requests => "request".to_owned(),
        }
    }
}

/// What a write request may read and write, inside [`WorkStore::transact`]. Every read a
/// decision depends on goes through here, so no decision is made on state another write changed
/// in between.
pub trait WorkTxn {
    /// An item by id, deleted ones included.
    fn get(&mut self, id: &str) -> Result<Option<StoredItem>, StoreError>;
    /// The item holding the checklist step `step_id`, live or removed.
    fn item_of_step(&mut self, step_id: &str) -> Result<Option<StoredItem>, StoreError>;
    /// The item holding the link `link_id` (its `from` item), live or removed.
    fn item_of_link(&mut self, link_id: &str) -> Result<Option<StoredItem>, StoreError>;
    /// Whether a client-made id is taken: an item, a step (removed ones included), a link
    /// (removed ones included) or a comment (`already_exists`, §2.3). The four share one id
    /// space, so a store keeps them in one unique index (a `work_ids` table, or a union over
    /// the item, step, link and comment rows); §3.3's sketch has no comment table yet.
    fn id_in_use(&mut self, id: &str) -> Result<bool, StoreError>;
    /// The items that have a live link of `kind` to the item `target_id` (a bug's fix tasks:
    /// the tasks with a `fixes` link to it), deleted ones included.
    fn linked_to(&mut self, target_id: &str, kind: LinkKind)
        -> Result<Vec<StoredItem>, StoreError>;
    /// The bugs that aren't closed or deleted, of `project_id` or of every project (the
    /// bug cascades of [`crate::reconcile_bugs`]).
    fn open_bugs(&mut self, project_id: Option<&str>) -> Result<Vec<StoredItem>, StoreError>;
    /// The item's events after `after_version`, oldest first.
    fn events_since(
        &mut self,
        target_id: &str,
        after_version: u64,
    ) -> Result<Vec<WorkEvent>, StoreError>;
    /// The stored answer to `change_id`, if any (expired ones may already be purged).
    fn change_result(&mut self, change_id: &str) -> Result<Option<StoredResult>, StoreError>;

    /// The next number in `scope` (from 1, no gaps, never reused). HQ only.
    fn allocate_number(&mut self, scope: &NumberScope) -> Result<u64, StoreError>;
    /// Writes the item's whole new state (core, task fields, checklist with removed steps,
    /// out-links with removed ones, watchers, comment references) and appends `events`. The
    /// store checks that `events` continue the stored version without a gap
    /// (`events[0].version == stored version + 1`, 0 for a new item, each one more than the
    /// last, and `item.version` equal to the last) and fails otherwise. Events are immutable.
    fn put_item(&mut self, item: &StoredItem, events: &[WorkEvent]) -> Result<(), StoreError>;
    /// Stores an answer for retries (§2.3). The store may purge it after
    /// `result.expires_at_ms`.
    fn put_change_result(
        &mut self,
        change_id: &str,
        result: &StoredResult,
    ) -> Result<(), StoreError>;
    /// Appends the call's one origin row (the next `origin_seq`, `org_position`) with one
    /// reference per committed `(target, version)` (§2.7), and its one feed row; returns the
    /// feed position.
    fn record_change(
        &mut self,
        org_position: Position,
        refs: &[EventRef],
        change_id: Option<&str>,
    ) -> Result<WorkPosition, StoreError>;
}

/// One write request's body. It is `Fn`, not `FnOnce`: when SQLite ends a batch's transaction,
/// the writer runs every request in it again (ADR-0002), each time with a fresh [`OrgView`].
/// So it has no effect outside its [`WorkTxn`].
pub type TxnFn =
    dyn Fn(&dyn OrgView, &mut dyn WorkTxn) -> Result<TxnOutput, StoreError> + Send + Sync;

/// The office's work storage.
pub trait WorkStore {
    /// Runs `f` as one write request on the office's single writer, inside its own SAVEPOINT
    /// (ADR-0002 decision 4): the reads and writes in `f` see one consistent state, and either
    /// all of `f`'s writes commit or none do. `Err` from `f` rolls them back. The result is
    /// reported after the batch's commit.
    fn transact(&self, f: Arc<TxnFn>) -> Result<TxnOutput, StoreError>;
}

/// Reads from the read pool (no writer involved), for `work.get`, `work.activity` and
/// `work.times`.
pub trait WorkReads {
    /// An item by id, deleted ones included.
    fn get(&self, id: &str) -> Result<Option<StoredItem>, StoreError>;
    /// The item numbered `number` in `scope`.
    fn by_number(&self, scope: &NumberScope, number: u64)
        -> Result<Option<StoredItem>, StoreError>;
    /// The items whose short id is `short` (lower case).
    fn by_short_id(&self, short: &str) -> Result<Vec<StoredItem>, StoreError>;
    /// The target's events after `after_version`, oldest first, at most `limit`.
    fn events(
        &self,
        target_id: &str,
        after_version: u64,
        limit: u32,
    ) -> Result<Vec<WorkEvent>, StoreError>;
    /// Live links whose target is the item `target_id` (its backlinks), at most `limit`.
    fn backlinks(&self, target_id: &str, limit: u32) -> Result<Vec<Link>, StoreError>;
    /// The items that have a live link of `kind` to the item `target_id`, deleted ones
    /// included (a bug's fix tasks, for its summary's `fixed_by`).
    fn linked_to(&self, target_id: &str, kind: LinkKind) -> Result<Vec<StoredItem>, StoreError>;
}

/// A work event's place in commit order: its call's feed row, then its target, then its
/// version. The owner's activity feed lists events with all three descending, which is this
/// type's `Ord` reversed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkActivityPosition {
    pub feed_seq: u64,
    pub target_id: String,
    pub version: u64,
}

/// One page of the owner's work activity (IA-12 decision 7). Filters combine with AND; each is
/// internally OR over the actor and the event's member fields ([`Self::involves`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkActivityQuery {
    /// The incarnation page 1 saw (`None`: the office had none yet).
    pub incarnation: Option<String>,
    /// The newest feed row the cursor may show, fixed by page 1.
    pub ceiling: u64,
    /// Exclusive: the page starts just after (older than) this position.
    pub before: Option<WorkActivityPosition>,
    /// The merge key of the last entry the cursor emitted; `u64::MAX` on page 1.
    pub prev_key: u64,
    pub member: Option<String>,
    /// A team's direct members, resolved on page 1.
    pub team_members: Option<Vec<String>>,
    /// Items currently in this project.
    pub project: Option<String>,
    /// At most this many entries (0 counts as 1).
    pub limit: u32,
    /// At most this many feed rows started, matching or not (0 counts as 1). The rest of the row `before`
    /// points into doesn't count again, so every page that scans moves on.
    pub scan: u32,
}

impl WorkActivityQuery {
    /// Whether the event passes the `member` and `team_members` filters: the actor or any
    /// member field (assignees, scopers, the member waited on or watching, a question's
    /// addressee) is the member, and one of them is in the team.
    pub fn involves(&self, event: &WorkEvent) -> bool {
        let people = event_members(event);
        self.member
            .as_ref()
            .is_none_or(|m| people.contains(&m.as_str()))
            && self
                .team_members
                .as_ref()
                .is_none_or(|team| people.iter().any(|p| team.iter().any(|t| t == p)))
    }
}

fn event_members(event: &WorkEvent) -> Vec<&str> {
    use org_proto::work::WorkEventKind as K;
    let fields: [Option<&String>; 2] = match &event.change {
        K::Assigned { from, to } => [from.as_ref(), to.as_ref()],
        K::Triaged { assignee_id, .. } => [assignee_id.as_ref(), None],
        K::Routed {
            scoper_from,
            scoper_to,
            ..
        } => [scoper_from.as_ref(), scoper_to.as_ref()],
        K::WaitingStarted { on_member_id, .. } | K::WaitingEnded { on_member_id, .. } => {
            [Some(on_member_id), None]
        }
        K::Watched { member_id, .. } => [Some(member_id), None],
        K::Comment { asks, .. } => [asks.as_ref(), None],
        _ => [None, None],
    };
    event
        .actor_id
        .iter()
        .chain(fields.into_iter().flatten())
        .map(String::as_str)
        .collect()
}

/// One work event in the activity feed, with its merge key: the running minimum of the
/// events' times in commit order, seeded by the query's `prev_key`, so it never rises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkActivityEntry {
    pub position: WorkActivityPosition,
    pub event: WorkEvent,
    /// `None`: the event's `at` doesn't parse. Its key is then the previous key, and the
    /// caller marks the source degraded.
    pub at_ms: Option<u64>,
    pub key: u64,
}

/// What one activity read returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkActivityPage {
    /// Newest first, at most `limit`.
    pub entries: Vec<WorkActivityEntry>,
    /// Where the next page resumes (its `before`): the last scanned event, or the last
    /// returned one when matches were left over. `None`: nothing was scanned.
    pub scanned_to: Option<WorkActivityPosition>,
    /// The merge key at `scanned_to` (`prev_key` when nothing was scanned).
    pub frontier: u64,
    /// The walk reached the start of the feed with every match returned.
    pub exhausted: bool,
}

/// Why an activity read failed.
#[derive(Debug)]
pub enum WorkActivityError {
    /// The office's incarnation is not the cursor's: start again from page 1.
    SnapshotRequired,
    /// `before` names no stored event of this office (above the ceiling, a missing feed row,
    /// a target not in that row, or a version not committed in it): a bad cursor, not a
    /// storage failure.
    InvalidPosition,
    Store(StoreError),
}

impl fmt::Display for WorkActivityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SnapshotRequired => f.write_str("the work history was replaced; start again"),
            Self::InvalidPosition => f.write_str("the activity position is not a stored one"),
            Self::Store(error) => fmt::Display::fmt(error, f),
        }
    }
}

impl Error for WorkActivityError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::SnapshotRequired | Self::InvalidPosition => None,
            Self::Store(error) => Some(error),
        }
    }
}

impl From<StoreError> for WorkActivityError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

/// The owner's work activity, read from the read pool. Each call is one read transaction
/// and never writes.
pub trait WorkActivityReads {
    /// Page 1's fixed point: the stored incarnation (read, never made) and the newest feed row
    /// (0: none), which becomes the cursor's ceiling.
    fn head(&self) -> Result<(Option<String>, u64), WorkActivityError>;
    /// One page, newest first. [`WorkActivityError::SnapshotRequired`] when the stored
    /// incarnation differs from the query's, except that none → one is compatible while the
    /// ceiling is 0; [`WorkActivityError::InvalidPosition`] when `before` isn't a position
    /// this office stored at or below the ceiling (`(feed_seq, "", 0)`: the start of a row).
    fn activity_page(
        &self,
        query: &WorkActivityQuery,
    ) -> Result<WorkActivityPage, WorkActivityError>;
    /// For each project, the newest event in commit order, at or below `ceiling`, of the
    /// items currently in it.
    fn latest_by_project(
        &self,
        ceiling: u64,
        project_ids: &[String],
    ) -> Result<Vec<(String, LatestActivity)>, WorkActivityError>;
}

/// A project's latest work activity ([`WorkActivityReads::latest_by_project`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatestActivity {
    /// No event of an item currently in the project.
    None,
    /// The newest event's time, in Unix milliseconds.
    At(u64),
    /// The newest event's `at` doesn't parse: the caller marks the source degraded.
    Unknown,
}

/// The owning office's clock, in Unix milliseconds. The office keeps it monotonic (offices
/// design); the library also never dates an item's event before its previous one.
pub trait Clock {
    fn now_ms(&self) -> u64;
}
