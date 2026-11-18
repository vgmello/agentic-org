//! Test adapters and hooks built from the contracts, for the host's tests
//! (`test-support`). Nothing here is reachable from a production composition.

/// Process admission: a scripted image check, and children a test names itself.
pub mod admission {
    pub use crate::admission::test_support::{
        child_peer, scripted_admission, unowned_child, unreleased, Script, Step,
    };
}
