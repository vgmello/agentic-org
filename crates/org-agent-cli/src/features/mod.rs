//! The bounded-context vertical slices of `agent-os`. Each owns its commands' rows, field schema,
//! `Use when:` guidance, handlers and tests; names only itself and `runtime`; and is composed by
//! the registry, never by another slice (`crates/org-platform/tests/vsa_structure.rs`).

pub(crate) mod auth;
pub(crate) mod budget;
pub(crate) mod files;
pub(crate) mod git;
pub(crate) mod hiring;
pub(crate) mod messaging;
pub(crate) mod office;
pub(crate) mod organization;
pub(crate) mod processes;
pub(crate) mod projects;
pub(crate) mod providers;
pub(crate) mod search;
pub(crate) mod session;
pub(crate) mod tess;
pub(crate) mod work;
