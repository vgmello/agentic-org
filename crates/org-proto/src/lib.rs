//! `org-proto`: the org API contract between the desktop app and `orgd`.
//!
//! Every type here is what goes over the wire (JSON-RPC 2.0 on `control.sock`), and the UI's
//! TypeScript types are generated from them (see [`typescript`]), so the two can't drift.
//! Changes to this crate need the staff engineer's review.
//!
//! JSON conventions:
//! - field names are snake_case, enums are snake_case strings,
//! - tagged unions use a `kind` field (`#[serde(tag = "kind", rename_all = "snake_case")]`),
//! - optional values are `T | null` (`Option<T>`, serialized as `null`, never omitted); only
//!   optional method params are omitted when unset,
//! - timestamps are RFC 3339 strings in UTC ([`Timestamp`]), dates are `YYYY-MM-DD` ([`IsoDate`]).
//!
//! Identity: every entity the API returns carries `machine_id` (architecture §3). An agent's
//! global key is (machine_id, agent_id). In v1 there is one machine, "This computer".

pub mod error;
pub mod event;
pub mod hire;
pub mod ids;
pub mod model;
pub mod rpc;
pub mod typescript;

pub use error::ApiError;
pub use event::{NeedsYouClearReason, OrgEvent};
pub use hire::{
    FieldError, FileChange, HireField, HirePlan, HirePreview, HireProgress, HireRequest, HireStep,
    HireSuggestion, NewProjectDraft, NewRoleDraft, NewRolePreview, PlannedFile, ProjectChoice,
    ProjectRequest, Proposed, ProposedName, RoleChoice, StepState,
};
pub use ids::{
    AgentId, IsoDate, MachineId, ProjectKey, RoleId, SessionId, Timestamp, WorkspacePath,
};
pub use model::{
    ActivityDetail, ActivityEvent, Agent, AgentStatus, Backlog, ClaudeCodeState, Document,
    DocumentKind, DocumentRef, EmploymentStatus, Environment, Goal, GoalChange, GoalChangeReason,
    GoalState, HrAgent, HrAgentId, HrAgentName, KnownTaskStatus, Machine, MachineConnection,
    NeedsYouItem, NeedsYouKind, Organization, Presence, PresenceSource, Project, ReportsTo, Role,
    Session, SessionChange, SessionKind, SessionOrigin, SessionState, Task, TaskStatus, Tools,
    TurnError,
};
pub use rpc::{
    ActivityParams, AgentIdParams, AgentsParams, ApiInfo, AssignTaskRequest, DismissParams,
    DocumentParams, DocumentsParams, HireParams, Method, MethodName, NeedsYouParams, NoParams,
    OpenSessionResult, ProposeHireResult, ResumeSessionParams, Snapshot, StopSessionParams,
};

/// The org API version. orgd reports it in `hello`, and the app checks it before anything else.
// Open for ada: the architecture (§8) says the app speaks N and N-1; the UI draft says it
// refuses any version but its own.
pub const API_VERSION: u32 = 1;

/// The JSON-RPC notification method that carries an [`OrgEvent`] after `subscribe`.
pub const EVENT_NOTIFICATION: &str = "event";
