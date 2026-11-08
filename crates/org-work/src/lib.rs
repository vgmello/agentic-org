//! # org-work: the work items' rules
//!
//! Tasks and bugs (and, in a later phase, feature requests), their checklists, links, comments
//! and events, as `docs/design/tasks-projects-docs.md` (W7) describes them. A **pure domain
//! library**: it has no SQL, no git, no network and no clock of its own. It speaks the wire
//! contract's types ([`org_proto::work`]) and defines the ports the office implements
//! (`store.rs`, server-dev): [`OrgView`], [`WorkStore`] / [`WorkTxn`], [`WorkReads`] and
//! [`Clock`].
//!
//! ## One write, one transaction
//! [`WorkService::call`] hands [`execute`] to [`WorkStore::transact`] as a re-runnable closure.
//! Inside it the library **decides first** (reads the item, checks every rule in the design's
//! error order, computes the whole outcome, automatic changes and bookkeeping included) and only
//! **then writes**: the items with their events ([`WorkTxn::put_item`]), the feed row
//! ([`WorkTxn::record_change`]) and the answer for retries ([`WorkTxn::put_change_result`]). A
//! refusal writes only its answer. The result, [`TxnOutput`], carries the answer and the
//! `work.changed` parts to send after the commit.
//!
//! ## What phase 1 covers
//! - Tasks: `task.create`, `task.update`, `task.assign`, `task.set_status` (the state machine
//!   and Done's required-step guard, "done anyway" with a reason).
//! - Checklists: add, edit, check, move, remove, make a step into a task, unlink it. A made-task
//!   step ticks only when its task is Done · Completed and unticks on a reopen (automatic,
//!   undoable for an hour).
//! - `work.comment` (questions to the owner open a waiting span), `work.watch`, `work.link` /
//!   `work.unlink` (`relates_to`), `work.undo`, `work.delete` (a tombstone, app-only).
//! - Per-project `T-n` numbers, allocated in the create's transaction.
//! - `change_id` idempotency (answers stored and replayed, refusals included, matched by a
//!   params hash) and `expected_version` with per-field conflicts.
//! - Time figures from events ([`time_figures`]).
//!
//! ## What phase 2 adds: bugs
//! - `bug.report`, `bug.update`, `bug.triage`, `bug.assign`, `bug.mark_fixed`, `bug.verify`,
//!   `bug.close` (duplicate, won't fix, can't reproduce) and `bug.reopen`, with the bug state
//!   machine (`new → triaged → in_progress → fixed → closed`), severity and priority, per-project
//!   `B-n` numbers and the same required-step guard as Done.
//! - Fix tasks: `fixes` links (`task.create { fixes }`, `work.link`) and the bug that follows
//!   them (In progress, Fixed, back to In progress), each an automatic event with `work.undo`.
//! - Areas as bugs reference them (`OrgView::area`, a live area of the bug's project; General by
//!   default), the suggested triager and verifier, and the triage waiting span: a `new` bug the
//!   owner would triage waits on the owner, in the time figures.
//! - [`reconcile_bugs`]: the cascades for open bugs after an organization change (a deleted
//!   area's open bugs move to its replacement; triage spans follow the suggested triager).
//!
//! Not yet: the rest of the organization cascades (`on_org_applied`: members, teams, the org
//! cursor and the catch-up inside every write), ownership hand-overs, `work.set_current` and
//! paged reads (`work.snapshot`, `work.list`), which the office builds on these ports.
//!
//! ## Dependencies
//! `org-proto` (the vocabulary), `serde`, `serde_json` and `sha2` (the params hash). The
//! `testkit` feature adds in-memory implementations of every port.

#![deny(unsafe_code)]

mod aspects;
mod bugs;
mod call;
mod cascades;
mod checklist;
mod common;
mod engine;
mod fail;
mod feed;
mod fixes;
mod hash;
mod ids;
mod items;
mod kpi;
mod model;
mod ports;
mod refs;
mod service;
mod states;
mod tasks;
mod text;
mod time;
mod undo;
mod waiting;

#[cfg(any(test, feature = "testkit"))]
pub mod testkit;

#[cfg(test)]
mod tests;

pub use call::{Caller, WorkCall};
pub use cascades::reconcile_bugs;
pub use engine::{
    execute, TxnOutput, CHANGE_ID_MAX_AGE_MS, CHANGE_ID_MAX_AHEAD_MS, CHANGE_RESULT_KEEP_MS,
};
pub use hash::{canonical_json, params_hash};
pub use ids::{is_ulid, short_id, ulid_from_parts, ulid_time_ms};
pub use items::DOC_KEY_MAX_CHARS;
pub use kpi::{time_figures, TimeTally};
pub use model::{
    work_item, BugFields, ItemReadParts, OpenWait, StoredComment, StoredItem, StoredLink,
    StoredResult, StoredStep, TaskFields,
};
pub use ports::{Clock, NumberScope, OrgView, StoreError, TxnFn, WorkReads, WorkStore, WorkTxn};
pub use refs::{resolve_item, ItemRef};
pub use service::{read_item, WorkService};
pub use states::{is_task_status, is_terminal, starts_cycle, task_move_allowed};
pub use time::{format_ms, parse_ms};
pub use undo::UNDO_WINDOW_MS;
