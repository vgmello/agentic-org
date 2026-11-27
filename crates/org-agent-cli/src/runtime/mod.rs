//! The shared headless kernel of `agent-os`: transport, binding, input, keys, sources and help.
//! It names nothing outside `runtime`: every feature slice depends on it, never the reverse
//! (`crates/org-platform/tests/vsa_structure.rs`).

pub(crate) mod binding;
pub(crate) mod card;
pub(crate) mod domain;
pub(crate) mod file_guard;
pub(crate) mod file_transfer;
pub(crate) mod help;
pub(crate) mod input;
pub(crate) mod keys;
pub(crate) mod office;
pub(crate) mod sources;
pub(crate) mod transport;
