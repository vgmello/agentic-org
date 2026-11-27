//! Narrow federation test hooks exposed under `feature = "testkit"` for downstream integration
//! suites (such as `org-office-server`). Never compiled into production builds.

pub use crate::stream::HqPeerRegistry;

pub mod federation_link {
    pub use crate::link::StaticPeer;

    pub mod org_events {
        pub use crate::link::org_events::{
            apply_satellite_org_event_frames, replicated_files_op_id,
        };
    }
}

pub mod snapshot_transfer {
    pub use crate::snapshot_transfer::sync_initial_snapshot_in_batches;
}
