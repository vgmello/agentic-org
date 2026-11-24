//! The projects record family on `org-store` (data contract document 02, implementation plan B3).
//!
//! Owns `state/projects/<project-id>/project.yaml`, `state/identity/counters/organization/<org-id>/project.yaml`,
//! and `state/identity/allocations/organization/<org-id>/project/<number>.yaml`.

pub mod features;
pub mod ports;
pub mod projector;
pub mod records;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
