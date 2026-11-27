//! The organization family on the office store: its records, ports, projector and
//! operations (one home team per employee, data contract documents 01, 04 and 06), and the
//! directory read from it. The protocol's `org.snapshot` and structure commands, which span
//! this family and the projects built on it, are `crate::structure`.
pub mod chief_of_staff;
pub mod directory;
pub mod features;
pub mod import;
pub(crate) mod names;
pub(crate) mod ops;
pub mod owner;
pub mod ports;
pub mod projector;
pub mod records;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
