//! GNU/Linux has no admission adapter yet: increment X2 writes it (a sealed copy of the
//! verified bytes started with `execveat`, a pidfd, `SO_PEERCRED`). Until then every item
//! is the refusing adapter's, so no child starts and no client is identified on Linux.

#[cfg(any(test, feature = "test-support"))]
pub(super) use super::refused::child_peer;
#[cfg(debug_assertions)]
pub(super) use super::refused::peer_executable;
pub(super) use super::refused::{
    native_peers, own_image, own_signing, peer, peer_satisfies, Admission, Child, StaticImages,
};
