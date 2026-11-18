//! Fakes and harnesses for the store's own tests and for every family and engine that is
//! tested against it. Never part of a production build.

pub mod custody;
pub mod faults;
pub mod ids;
pub mod rng;

pub use custody::FakeCustody;
pub use faults::{DiskOp, Faults, Target};
pub use ids::{id, identity};
pub use rng::Rng;
