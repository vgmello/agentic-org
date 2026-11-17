//! Browser gateway composition, started only after the verified host handoff.
//!
//! The service consumes an already-bound listener. It never binds a production
//! listener, opens an office database or obtains an office database key.

#![deny(unsafe_code)]

// A release-profile or app-bundle build with debug assertions on would keep the
// unpinned development startup path (release.rs), skipping publisher authentication
// and loaded-image verification: refuse it, whatever turned them on (see build.rs).
// rustdoc compiles with debug assertions on, so documentation and doctests are exempt.
#[cfg(all(
    any(release_profile, bundle_build),
    debug_assertions,
    not(doc),
    not(doctest)
))]
compile_error!(
    "agentic-org-web: refusing a release or app-bundle build with debug assertions on: it \
     would keep the unpinned development startup path. Build it without debug assertions."
);

pub mod assets;
pub mod blocking;
pub mod body;
mod csp;
mod ingress;
mod origin;
pub mod path;
pub mod process;
mod release;
pub mod service;
pub mod sessions;
mod static_ui;
pub mod tls;
pub mod transport;

pub use static_ui::{Hsts, StaticUi};
