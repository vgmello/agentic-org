//! # org-proto: the contract
//!
//! The wire contract between this machine's servers and their clients (the desktop app
//! through `org-client`, and the agent CLI `org-agent`). There are two kinds of server:
//! - **The host** (`org-office-host`), the one login service on a machine. It supervises the
//!   office servers and answers `hello`, `organization.create`, `organization.list`,
//!   `office.list`, `office.app_token`, `office.restart` and `subscribe` (it emits
//!   `organization.created` and `office.status_changed`).
//! - **An office server** (`org-office-server`), one process per office. An organization's
//!   first office is its HQ. It answers `hello` with its identity, the organization's calls
//!   ([`org`]: teams, members, projects and areas), the work calls ([`work`]: tasks and their
//!   checklists, links, comments and activity), and later agent messaging.
//!
//! ## Who may call what
//! The wire never carries the caller's identity: the office knows whose connection a call came
//! in on. **App-only** calls are refused on any other connection with `not_authorized`: every
//! organization command ([`org`]) and `work.delete` ([`work`]). Every work call needs a bound
//! connection (`not_bound` otherwise), and acts as the member it is bound as.
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
//! TypeScript mirror (`apps/desktop/src/lib/proto.ts`) in step by hand for now.
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

// No unsafe code, except the one function that zeroes an `AppToken`'s bytes (see `token`).
// One test there also allows it, to read a token's bytes after it's dropped.
#![deny(unsafe_code)]

pub mod directory;
pub mod frame;
pub mod location;
pub mod msg;
pub mod org;
pub mod work;

mod calls;
mod error;
mod events;
mod name;
mod token;
mod types;
mod wire;

pub use calls::{
    Call, Empty, Hello, HelloResult, ListOfficesResult, ListOrganizationsResult, OfficeAppToken,
    OfficeAppTokenParams, OfficeAppTokenResult, OfficeHello, OfficeList, OfficeRestart,
    OfficeRestartParams, OrganizationCreate, OrganizationCreateParams, OrganizationList, Subscribe,
    SubscribeParams,
};
pub use error::{ApiError, CallError, ErrorCode};
pub use events::{EventKind, OfficeStatusChanged, OfficeStatusChangedData, OrganizationCreated};
pub use name::{NameError, OrganizationName};
pub use org::{
    is_shaped_like_an_address, is_valid_id, is_valid_member_id, AgentProfile, AgentRuntime,
    AgentStatus, Area, CommandAccepted, CommandStatus, GithubRepo, Member, MemberKind, Membership,
    OrgChange, OrgChanged, OrgChangedData, OrgRejected, OrgRejectedData, OrgSnapshot,
    OrgSnapshotCall, Position, Project, ProjectCreateTeam, ProjectRepo, ProjectSummary,
    ProjectTeam, RepoHost, RepoSpec, RunMode, Schedule, Team, AGENT_HOME_MAX_BYTES,
    AGENT_MESSAGING_NAME_MAX_CHARS, AGENT_TEXT_MAX_CHARS, AGENT_TIME_ZONE_MAX_CHARS,
    AREA_NAME_MAX_CHARS, ID_MAX_CHARS, MEMBER_ID_CHARS, OFFICE_HANDLE, OWNER_HANDLE,
    PROJECT_DESCRIPTION_MAX_CHARS, PROJECT_KEY_MAX_CHARS, RESERVED_PROJECT_KEYS, RUN_HANDLE_PREFIX,
};
pub use token::{AppToken, APP_TOKEN_BYTES};
pub use types::{
    OfficeInfo, OfficeRole, OfficeStatus, OfficeStatusReason, Organization,
    ORGANIZATION_NAME_MAX_CHARS,
};
pub use wire::{
    salvage_id, Event, Outcome, Request, Response, ServerMessage, UNREADABLE_REQUEST_ID,
};
pub use work::{
    ChecklistCounts, ChecklistStep, CommentKind, DocRef, DocWritten, DoneAnyway, DraftTask,
    Estimate, EventRef, Feature, FeatureOutcome, FeatureStatus, FieldChange, ItemKind, ItemStatus,
    ItemTime, Link, LinkKind, LinkTarget, NewStep, Priority, RequestKind, RequestOutcome,
    Resolution, RoutedBy, Severity, Shortcut, TimeFigures, VerifyOutcome, WaitReason, WorkAccepted,
    WorkActivity, WorkActivityParams, WorkActivityResult, WorkChange, WorkChanged, WorkChangedData,
    WorkEvent, WorkEventKind, WorkGet, WorkGetParams, WorkGetResult, WorkItem, WorkItemSummary,
    WorkList, WorkListParams, WorkListResult, WorkPosition, WorkSnapshot, WorkSnapshotParams,
    WorkSnapshotResult, WorkStatus, WorkTimes, WorkTimesParams, WorkTimesResult,
    WORK_BLOCKED_REASON_MAX_CHARS, WORK_CHANGED_MAX_BYTES, WORK_DESCRIPTION_MAX_CHARS,
    WORK_DOD_MAX_CHARS, WORK_ENVIRONMENT_MAX_CHARS, WORK_GET_MAX_LISTED, WORK_MAX_DRAFT_TASKS,
    WORK_MAX_LINKS, WORK_MAX_STEPS, WORK_PAGE_MAX, WORK_PARAMS_MAX_BYTES, WORK_REASON_MAX_CHARS,
    WORK_REQUEST_TEXT_MAX_CHARS, WORK_RESPONSE_MAX_BYTES, WORK_STEP_TEXT_MAX_CHARS,
    WORK_TEXT_MAX_CHARS, WORK_TITLE_MAX_CHARS,
};

/// The protocol version a server reports in `hello`. Bumped only for a breaking change.
pub const API_VERSION: u32 = 1;

/// The largest frame (one JSON line, without its `\n`) either side may send or accept.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[cfg(test)]
mod tests;
