//! Test adapters and hooks built from the contracts, for the host's tests
//! (`test-support`). Nothing here is reachable from a production composition.

/// Process admission: a scripted image check, and children a test names itself.
pub mod admission {
    pub use crate::admission::test_support::path_admission;
    pub use crate::admission::test_support::{
        child_peer, scripted_admission, unowned_child, unreleased, Script, Step,
    };
}

/// File names: what this system's filesystems take for one name.
pub mod names {
    pub use crate::names::test_support::equivalent_spellings_are_one_name;
}

/// The directory credential store's write hooks: a step between flush and
/// publication, a step after a name collision, and failing directory flushes.
pub mod credentials {
    pub use crate::credentials::testing::{
        after_collision, before_publish, fail_directory_syncs, take_before_publish,
        take_directory_sync_faults,
    };
}
