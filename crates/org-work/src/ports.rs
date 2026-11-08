//! The ports `org-work` defines and the office implements (`store.rs`, server-dev): the
//! organization as the office has applied it ([`OrgView`]), the work storage's write request
//! ([`WorkStore::transact`] and [`WorkTxn`]), its reads ([`WorkReads`]) and the clock
//! ([`Clock`]). All SQL lives behind them (ADR-0002 decision 5).

use std::error::Error;
use std::fmt;
use std::sync::Arc;

use org_proto::org::{Member, Position, Project};
use org_proto::work::{EventRef, ItemKind, Link, WorkEvent, WorkPosition};
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
/// Phase 1 needs these reads; the cascades (`on_org_applied`) add `changes_after`, and bugs add
/// areas and team leads.
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
    /// (removed ones included) or a comment (`already_exists`, §2.3).
    fn id_in_use(&mut self, id: &str) -> Result<bool, StoreError>;
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
}

/// The owning office's clock, in Unix milliseconds. The office keeps it monotonic (offices
/// design); the library also never dates an item's event before its previous one.
pub trait Clock {
    fn now_ms(&self) -> u64;
}
