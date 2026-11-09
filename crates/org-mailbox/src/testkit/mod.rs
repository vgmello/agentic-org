//! In-memory ports, the standard fixture and the generic store conformance suite. No
//! filesystem or live office.

pub mod conformance;
pub mod fixture;
mod memory;

pub use memory::{InMemoryDirectory, InMemoryStore};
