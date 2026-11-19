//! In-memory ports, the standard fixture and the generic store conformance suites. No
//! filesystem or live office.

pub mod conformance;
pub mod fixture;
mod memory;
pub mod triage_conformance;

pub use memory::{InMemoryDirectory, InMemoryStore};
