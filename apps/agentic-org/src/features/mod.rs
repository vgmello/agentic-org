//! The vertical slices of `agentic-org`: one module per noun, one file per verb. A slice uses
//! the shared leaf modules (`args`, `error`, `host`, `selection`, `completers`) and never
//! another slice.

pub(crate) mod admin;
pub(crate) mod agent;
pub(crate) mod backup;
pub(crate) mod completion;
pub(crate) mod context;
pub(crate) mod diagnostics;
pub(crate) mod office;
pub(crate) mod org;
pub(crate) mod project;
pub(crate) mod provider;
pub(crate) mod recovery;
pub(crate) mod release;
pub(crate) mod repo;
pub(crate) mod serve;
pub(crate) mod service;
pub(crate) mod setup;
pub(crate) mod sign_in;
pub(crate) mod status;
