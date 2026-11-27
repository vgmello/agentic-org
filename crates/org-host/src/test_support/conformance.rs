//! Shared engine-independent host-storage conformance suites (ADR-0018 ENG-3).
//!
//! `org-host` runs every case against its in-memory adapters (`MemoryHostStore`,
//! `MemoryAccounts`, `MemoryLinks`, `MemorySessions`); `org-host-sqlite` runs the exact same
//! 49 cases against real SQLite `HostDb` registries.

pub mod accounts;
pub mod host_store;
pub mod links;
pub mod sessions;
