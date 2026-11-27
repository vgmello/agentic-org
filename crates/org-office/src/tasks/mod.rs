//! Unified typed tasks family on the office store (`TASK-49` B4/B5 + `TASK-77` B4b +
//! `TASK-87`; `doc-77`, `doc-79`, `doc-80`, `doc-84`, `ADR-0014`, `ADR-0019`, `ADR-0031`).
//!
//! Engine-independent: runs identically on `MemoryEngine` and `SqliteEngine` (`ENG-1`).

pub mod features;
#[cfg(any(test, feature = "testkit"))]
pub mod ops;
#[cfg(not(any(test, feature = "testkit")))]
pub(crate) mod ops;
pub mod ports;
pub mod projector;
pub mod query;
pub mod records;
pub mod relocation;
pub mod replica;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
