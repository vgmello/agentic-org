//! Organization replication policy. No filesystem, networking, threads, or ambient clock.
//! Public adapter contracts are introduced only after lead/consumer agreement.
#![deny(unsafe_code)]

pub mod error;
pub mod messages;
mod ordering;
pub mod organization;
pub mod versions;
pub mod work;

#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
