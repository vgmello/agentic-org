//! The organization family on the office store: its records, ports, projector and
//! operations (one home team per employee, data contract documents 01, 04 and 06), and the
//! directory and snapshot read from it.
pub mod directory;
pub mod features;
pub mod import;
pub(crate) mod names;
pub mod ports;
pub mod projector;
pub mod records;
pub mod snapshot;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
pub mod wire;
