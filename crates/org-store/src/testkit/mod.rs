//! Fakes and harnesses for the store's own tests and for every family and engine that is
//! tested against it. Never part of a production build.

pub mod clock;
pub mod custody;
pub mod engine_suite;
pub mod faults;
pub mod harness;
pub mod ids;
pub mod memory;
pub mod probe;
pub mod rng;

pub use clock::{FixedClock, SteppingClock};
pub use custody::FakeCustody;
pub use faults::{CrashPoint, DiskOp, Faults, FlushSite, Target};
pub use harness::{rebuild_equivalence, Command, FamilyScript};
pub use ids::{id, identity};
pub use memory::{FamilyTable, MemoryEngine, MemorySnapshot, MemoryTx};
pub use probe::Probe;
pub use rng::Rng;

/// How long a test waits for a held flush or a held confirmation to arrive before it
/// fails. A failure bound, never a wait on the passing path: when the thread meant to
/// reach the hold dies first, the test fails here instead of hanging the run.
pub const HELD_BOUND: std::time::Duration = std::time::Duration::from_secs(30);

/// A canonical listing of every row of a projection, sorted. Compared only between two
/// projections of the same engine.
pub trait Dump {
    fn dump(&self) -> Vec<String>;
}

/// Composes a store on a disk that fails and dies as `faults` says.
pub fn with_faults<E: crate::Engine>(
    mut parts: crate::StoreParts<E>,
    faults: &std::sync::Arc<Faults>,
) -> crate::StoreParts<E> {
    parts.disk = faults.disk();
    parts.hooks = std::sync::Arc::clone(faults) as std::sync::Arc<dyn crate::store::Hooks>;
    parts
}
