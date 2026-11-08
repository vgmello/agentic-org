//! # org-proto: the contract
//!
//! The wire contract between an office server and its clients: the desktop app (through
//! `org-client`) and, later, the agent CLI (`org-agent`), which will relay messages through
//! the office server. **The most depended-on crate: keep it small, stable, additive-only.**
//! Every change needs ada's review (see AGENTS.md).
//!
//! ## Wire format
//! UTF-8 JSON, one object per line (`\n`), at most [`MAX_FRAME_BYTES`] per line, over the
//! office server's local Unix socket. See [`frame`].
//!
//! - A client sends a [`Request`] with an `id`; the server answers with a [`Response`]
//!   carrying the same `id`. Several requests may be in flight on one connection.
//! - After `subscribe`, the server also pushes [`Event`]s, which have no `id`.
//!   [`ServerMessage`] tells the two apart.
//!
//! ## Typed calls
//! Each method is a type implementing [`Call`] (its name, params and result), and each event
//! a type implementing [`EventKind`]. Clients and servers build and read envelopes through
//! them, so method names and payload shapes live in one place. The app will keep a
//! TypeScript mirror (`apps/desktop/src/lib/proto.ts`) in step by hand for now.
//!
//! ## Client duties
//! - Number requests from 1 and keep ids below 2^53 (id 0 is [`UNREADABLE_REQUEST_ID`]).
//! - Put a timeout on every pending request: a reply can be lost to a line neither side
//!   could read (e.g. nested deeper than serde_json's 128-level limit), and such a reply
//!   falls back to id 0.
//! - Envelopes, params and results are JSON objects. The derived decoders also accept
//!   JSON arrays in their place; don't send them, and don't rely on that.
//!
//! ## Compatibility rules
//! - [`API_VERSION`] changes only for a breaking change. Additive changes keep it.
//! - Additive means: new methods, new events, new optional fields, new error codes.
//! - `event` is a reserved key: a server message carrying it is always an event, so a
//!   response never gains a field named `event`.
//! - Readers ignore unknown fields, ignore unknown events, decode unknown error codes as
//!   [`ErrorCode::Unknown`], and answer unknown methods with [`ErrorCode::UnknownMethod`].

pub mod frame;

mod calls;
mod error;
mod events;
mod types;
mod wire;

pub use calls::{
    Call, Empty, Hello, HelloResult, ListOrganizationsResult, OrganizationCreate,
    OrganizationCreateParams, OrganizationList, Subscribe,
};
pub use error::{ApiError, CallError, ErrorCode};
pub use events::{EventKind, OrganizationCreated};
pub use types::{OfficeInfo, Organization, ORGANIZATION_NAME_MAX_CHARS};
pub use wire::{
    salvage_id, Event, Outcome, Request, Response, ServerMessage, UNREADABLE_REQUEST_ID,
};

/// The protocol version a server reports in `hello`. Bumped only for a breaking change.
pub const API_VERSION: u32 = 1;

/// The largest frame (one JSON line, without its `\n`) either side may send or accept.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[cfg(test)]
mod tests;
