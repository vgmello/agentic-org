//! # org-proto: the contract
//!
//! The wire contract between this machine's servers and their clients (the desktop app
//! through `org-client`, and the agent CLI `agent-os`). There are two kinds of server:
//! - **The host** (`org-host`), the one login service on a machine. It supervises the
//!   office servers and answers `hello`, `organization.list`,
//!   `office.list`, `office.restart` and `subscribe` (it emits
//!   `organization.created` and `office.status_changed`).
//!   Its `hello` lists in `capabilities` the optional calls and events it implements
//!   ([`capability`]), so a client checks before relying on one.
//! - **An office server** (`agentic-org-office`), one process per office. An organization's
//!   first office is its HQ. The current worker answers `hello` with its identity, supports
//!   `msg.bind` and `who`, and serves authorized organization requests ([`org`]). Typed tasks
//!   are the `tasks.*` calls in [`tasks`]. The retired phase-1 work calls (`task.*`, `bug.*`,
//!   `checklist.*` and `work.*`) are no longer part of the contract: an office answers them
//!   [`ErrorCode::UnknownMethod`] and a host channel refuses them.
//!
//! ## Who may call what
//! The wire never carries the caller's identity: the office knows whose connection a call came
//! in on. **App-only** calls are refused on any other connection with `not_authorized`: every
//! organization command ([`org`]).
//! On the host, for an app-only call (`office.restart`), the client rule: a
//! client reads `hello`'s answer before sending an app-only call, and sends nothing else on that
//! connection until its answer arrives. First thing in the app's main, before any output or
//! threads: fstat fds 0-2 and dup2 `/dev/null` over any that is a socket; the app never reads or
//! writes an fd it didn't open. The host check: the host accepts an app-only call only if it
//! arrived after the connection's accept was sampled and nothing follows its request line
//! (otherwise `not_authorized`).
//!
//! **The most depended-on crate: keep it small, stable, additive-only.** Every change needs
//! ada's review (see AGENTS.md).
//!
//! ## Wire format
//! UTF-8 JSON, one object per line (`\n`), at most [`MAX_FRAME_BYTES`] per line, over the
//! server's local Unix socket, found through [`location`]. See [`frame`].
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
//! TypeScript mirror (`apps/agentic-org-ui/src/lib/proto.ts`) in step by hand for now.
//!
//! Agent messaging on an office server (ADR-0003) is in [`msg`]; the organization directory
//! (`who`), status lines (`status.set`) and presence are in [`directory`].
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
//! - Version 1 is **unreleased**. It was revised on 2026-11-08 for the office model (the host,
//!   office servers, `OfficeInfo`, `HelloResult.host`), before any client shipped. From the
//!   first release on, these rules apply strictly.
//! - Additive means: new methods, new events, new optional fields, new error codes.
//! - `event` is a reserved key: a server message carrying it is always an event, so a
//!   response never gains a field named `event`.
//! - Readers ignore unknown fields, ignore unknown events, decode unknown error codes as
//!   [`ErrorCode::Unknown`], and answer unknown methods with [`ErrorCode::UnknownMethod`].
//!
//! ## Version 2 (X-02), additive so far
//! The shapes of version 2 are here alongside version 1, and [`API_VERSION`] stays 1 until the
//! migration's move 3:
//! - `hello` takes [`HelloParams`] (`api`, an [`ApiRange`]) and negotiates within
//!   [`SUPPORTED_API`]; v2 results carry `instance_id` ([`HelloResult::instance_id`]).
//! - On host-made channels, a request acts under a [`Principal`] ([`Request::principal`]): a
//!   grant the host MAC'd over [`grant_mac_input`].
//! - [`retired`] sentinels for the retired inputs, and [`AgentProfileInput`], the profile
//!   `member.add_agent` takes; move 2a puts both on the params.
//! - [`ManagedHome`] on `who`'s cards, [`Actor`] on `org.changed`, the
//!   [`OperationGet`] lookup, and the codes `unsupported_version` and `operation_reused`.
//!
//! Its vectors are in `test-vectors/v2/proto-*.json`.

// No unsafe code: a secret's bytes are zeroed through `zeroize` (see `token`).
#![forbid(unsafe_code)]

mod identity;
pub mod selected_bootstrap;
pub use identity::{is_uuid_v7, uuid_v7_time_ms};
pub mod ask;
pub mod attachments;
pub mod budgets;
pub mod chat;
pub mod costs;
pub mod directory;
pub mod docs;
pub mod external_auth;
pub mod federation;
pub mod frame;
pub mod git;
pub mod hiring;
pub mod inbox;
pub mod location;
pub mod msg;
pub mod org;
pub mod private_notes;
pub mod processes;
pub mod projects;
pub mod providers;
pub mod retired;
pub mod reviews;
pub mod rfc3339;
pub mod search;
pub mod sessions;
pub mod tasks;
pub mod tess;
pub mod tz;
pub mod web;

mod call;
mod calls;
mod error;
mod events;
mod name;
mod operation;
mod principal;
mod recovery;
mod token;
mod types;
mod ulid;
mod wire;

pub use calls::{
    capability, ApiRange, Call, Empty, Hello, HelloParams, HelloResult, ListOfficesResult,
    ListOrganizationsResult, OfficeHello, OfficeList, OfficeRestart, OfficeRestartParams,
    OrganizationList, Subscribe, SubscribeParams,
};
pub use directory::ManagedHome;
pub use error::{ApiError, CallError, ConflictDetail, ErrorCode, ErrorHint};
pub use events::{EventKind, OfficeStatusChanged, OfficeStatusChangedData, OrganizationCreated};
pub use name::{NameError, OrganizationName};
pub use operation::{OperationGet, OperationGetParams, OperationKind, OperationOutcome};
pub use org::{
    has_reserved_branch_prefix, is_shaped_like_an_address, is_valid_id, is_valid_member_id,
    AgentProfile, AgentProfileInput, AgentRuntime, AgentStatus, Area, CommandAccepted,
    CommandStatus, GithubRepo, Member, MemberKind, OrgChange, OrgChanged, OrgChangedData,
    OrgRejected, OrgRejectedData, OrgSnapshot, OrgSnapshotCall, Position, Project,
    ProjectCreateTeam, ProjectRepo, ProjectSummary, ProjectTeam, RepoHost, RepoSpec, RunMode,
    Schedule, Team, AGENT_HOME_MAX_BYTES, AGENT_MESSAGING_NAME_MAX_CHARS, AGENT_TEXT_MAX_CHARS,
    AGENT_TIME_ZONE_MAX_CHARS, AREA_NAME_MAX_CHARS, ID_MAX_CHARS, MEMBER_ID_CHARS, OFFICE_HANDLE,
    OWNER_HANDLE, PROJECT_DESCRIPTION_MAX_CHARS, PROJECT_KEY_MAX_CHARS, RESERVED_BRANCH_PREFIXES,
    RESERVED_PROJECT_KEYS, RUN_HANDLE_PREFIX,
};
pub use principal::{
    grant_mac_input, Actor, ActorKind, Grant, GrantClaims, MalformedPrincipal, Principal,
    GRANT_MAC_DOMAIN,
};
pub use recovery::RecoveryCode;
pub use retired::Retired;
pub use rfc3339::{canonicalize_rfc3339_utc, format_rfc3339_ms, parse_rfc3339_ms};
pub use token::zero as zero_bytes;
pub use token::{
    GrantKey, GrantMac, OfficeSecretsKey, GRANT_KEY_BYTES, GRANT_MAC_BYTES,
    OFFICE_SECRETS_KEY_BYTES,
};
pub use types::{
    OfficeInfo, OfficeRole, OfficeStatus, OfficeStatusReason, Organization,
    ORGANIZATION_NAME_MAX_CHARS,
};
pub use ulid::ulid_time_ms;
pub use wire::{
    salvage_id, Event, Outcome, Request, Response, ServerMessage, UNREADABLE_REQUEST_ID,
};

/// The protocol version a server reports in `hello`. Bumped only for a breaking change.
pub const API_VERSION: u32 = 1;

/// The `org-proto` versions this build speaks, for `hello`'s negotiation (X-02 §3): version 1
/// and the additive version 2. Move 3 of the migration narrows it to `{2, 2}`, together with
/// [`API_VERSION`] becoming 2.
pub const SUPPORTED_API: ApiRange = ApiRange { min: 1, max: 2 };

/// The largest frame (one JSON line, without its `\n`) either side may send or accept.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod v2_tests;
