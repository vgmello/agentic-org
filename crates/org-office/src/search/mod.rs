//! Unified cross-entity search (`TASK-91`): tasks, organization processes and standards,
//! projects, members, teams and roles, the chat messages the caller may read, and, asked for
//! on their own, the caller's permitted private notes and decisions (`ADR-0031`).
//!
//! A search domain module with no records of its own: it indexes the other families'
//! records ([`documents`]), and its consumer-owned ports ([`ports`]) are answered by three
//! physically separate stores that never see one another's documents. On SQLite those are
//! adapter-private FTS5 tables in `office.db`, in `protected/messaging.db`, and in HQ's
//! disposable `protected/private/task-notes-search.db` (`store/sqlite/search/`); the memory
//! engine scans the same documents in pure Rust ([`testkit`]). Both read words the same way
//! ([`text`]), and filter, score and order by the same rules ([`plan`], [`rank`]).

pub mod documents;
pub mod features;
pub mod plan;
pub mod ports;
pub mod rank;
pub mod records;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
pub mod text;
