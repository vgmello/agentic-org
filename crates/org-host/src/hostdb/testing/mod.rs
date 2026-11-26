//! Test-only adapters for the host-registry contract: [`memory`] is the in-memory
//! [`HostStore`](crate::hostdb::HostStore) the conformance suite runs beside SQLite.

pub(crate) mod memory;
