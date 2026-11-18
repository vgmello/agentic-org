//! Fakes and harnesses for the store's own tests and for every family and engine that is
//! tested against it. Never part of a production build.

pub mod clock;
pub mod custody;
pub mod engine_suite;
pub mod faults;
pub mod ids;
pub mod memory;
pub mod rng;

pub use clock::{FixedClock, SteppingClock};
pub use custody::FakeCustody;
pub use faults::{DiskOp, Faults, Target};
pub use ids::{id, identity};
pub use memory::{FamilyTable, MemoryEngine, MemorySnapshot, MemoryTx};
pub use rng::Rng;

/// A canonical listing of every row of a projection, sorted. Compared only between two
/// projections of the same engine.
pub trait Dump {
    fn dump(&self) -> Vec<String>;
}
