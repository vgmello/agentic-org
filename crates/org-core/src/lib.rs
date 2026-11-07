//! `org-core`: the workspace model of an agent organization, read from the files
//! HR (`hr/hr.py`) and the agents write. Only filesystem I/O; the files are the
//! source of truth.
//!
//! This is the read side: [`Workspace::load`] reads projects, agents, roles, the
//! HR agent and backlogs into a [`WorkspaceModel`], and
//! [`WorkspaceModel::refresh`] re-reads just what one changed file feeds
//! ([`classify`] says what that is). The `parse_*` functions parse one file's
//! text and are pure.
//!
//! Parsing is tolerant: files are written by people and LLMs. A backlog row that
//! can't be read is a [`BacklogItem::Unreadable`]; a missing or garbled file is a
//! [`Problem`] on the model. Only an unusable root is an [`Error`]. No input
//! panics.

mod backlog;
mod brief;
mod error;
mod markdown;
mod model;
mod paths;
mod profile;
mod role;
mod staff;
mod workspace;

pub use backlog::{parse_backlog, parse_status};
pub use brief::{parse_project_brief, TEAM_END, TEAM_START};
pub use error::{Error, Result};
pub use model::*;
pub use paths::{classify, resolve_in_workspace, FileKind};
pub use profile::{parse_agent_profile, parse_project_profile};
pub use role::parse_role;
pub use staff::parse_staff_function;
pub use workspace::{Loaded, Workspace, MAX_FILE_BYTES};
