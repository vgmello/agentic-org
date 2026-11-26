//! The office's socket: JSON lines (`org_proto::frame`), one thread per connection. It answers
//! `hello`, `org.snapshot`, `msg.bind`, `who`, `msg.send` / `msg.inbox` / `msg.read` /
//! `msg.wait`, `subscribe` and the organization commands, which only an app-bound connection may
//! send, and the work writes made as a bound member; other methods are `unknown_method`. With no
//! `mailbox` or `feed` port (the worker since B1b) `msg.send`/`inbox`/`read`/`wait` and `subscribe` answer
//! `unsupported`, and composition answers work writes, `status.set` and `operation.get` the same
//! way; see [`OfficePorts`]. A `msg.wait` is held on a thread of its own
//! (at most [`MAX_HELD`] per connection), so the connection keeps answering meanwhile. When the
//! server stops, every connection is closed and its thread joined BEFORE the office is, so no thread
//! touches a database that is closing.

mod auth;
mod auth_limits;
mod binding;
mod connection;
mod delegation;
mod dispatch;
mod external;
mod held;
mod host;
mod listener;
mod messaging;
mod org;
mod rpc;
mod sessions;
mod shared;
mod wire;

use std::sync::Arc;

use crate::follow::OrgFeed;
use crate::live::LiveEvents;
use org_mailbox::{Mailbox, MemberDirectory};
use org_office::rules::OrgCommand;
use org_proto::directory::{StatusSetParams, StatusSetResult, WhoParams, WhoResult};
use org_proto::org::CommandAccepted;
use org_proto::projects::{
    ProjectInspectParams, ProjectListParams, ProjectListResult, ProjectRecord,
};
use org_proto::work::WorkAccepted;
use org_proto::{ApiError, OrgSnapshot, Request, Response};
use org_work::{Caller, WorkCall};

use self::dispatch::answer;

pub use self::delegation::{ChiefOfStaffPort, Delegation};
#[cfg(test)]
pub(crate) use self::external::close_after_approval;
pub use self::external::ExternalAuthPort;
pub use self::listener::{bind, run};
pub use self::shared::Shared;

pub(crate) use self::auth::{Binding, LaunchAuth};
pub(crate) use self::shared::{live_owner, live_person};

pub(super) use self::connection::serve_context;
pub(super) use host::HostChannel;

#[cfg(test)]
pub(crate) use self::connection::serve_grant_bound_stream_for_test;

/// Most `msg.wait` calls held at once on one connection; more get `unavailable`.
pub(crate) const MAX_HELD: usize = org_proto::msg::MAX_HELD_CALLS_PER_CONNECTION;

/// Most connections served at once; more are closed at once.
pub(crate) const MAX_CONNECTIONS: usize = 64;

pub(super) enum Context {
    OfficeSocket,
    HostChannel(Box<HostChannel>),
}

/// Reads the organization's snapshot (the office's `org_snapshot`), or says why it cannot.
/// Availability is distinct from storage failure at the organization read boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotFailure {
    Unavailable,
    Storage,
}

pub type SnapshotFn = Box<dyn Fn() -> Result<OrgSnapshot, SnapshotFailure> + Send + Sync>;

/// Runs one organization command from a member (the office's `submit_org` or `check_org` when
/// `dry_run` is true): the member the caller's grant is bound to. HQ's answer, or why it
/// could not answer at all (storage).
pub type SubmitFn = Box<
    dyn Fn(&str, OrgCommand, bool) -> Result<Result<CommandAccepted, ApiError>, String>
        + Send
        + Sync,
>;

/// Reads a retained operation for the acting grant-bound member (`(member, operation_id)`).
pub type OperationFn = Box<
    dyn Fn(&str, &str) -> Result<Result<org_proto::OperationOutcome, ApiError>, String>
        + Send
        + Sync,
>;

/// Answers `who` (the office's `who`): the cards, the caller's mistake, or why it could not
/// answer at all (storage).
pub type WhoFn =
    Box<dyn Fn(&WhoParams) -> Result<Result<WhoResult, ApiError>, String> + Send + Sync>;

/// Sets or clears the given member's status line (the office's `set_status`): the result, the
/// caller's mistake, or why it could not answer at all (storage).
pub type StatusFn = Box<
    dyn Fn(&str, &StatusSetParams) -> Result<Result<StatusSetResult, ApiError>, String>
        + Send
        + Sync,
>;

pub type WorkFn =
    Box<dyn Fn(Caller, WorkCall) -> Result<Result<WorkAccepted, ApiError>, String> + Send + Sync>;

/// Lists projects in this organization (`project.list`).
pub type ProjectsListFn = Box<
    dyn Fn(&ProjectListParams) -> Result<Result<ProjectListResult, ApiError>, String> + Send + Sync,
>;

/// Inspects one project by ID, `P-<number>`, or slug (`project.inspect`).
pub type ProjectInspectFn = Box<
    dyn Fn(&ProjectInspectParams) -> Result<Result<ProjectRecord, ApiError>, String> + Send + Sync,
>;

/// Dispatches C2 role, HR hiring, `may_hire`, team-limit, and agent lifecycle RPCs.
pub type HiringFn = Box<dyn Fn(Option<&str>, &Request) -> Option<Response> + Send + Sync>;

/// Dispatches C3 agent session, grant, and provider CLI enrollment RPCs.
pub type SessionsFn = Box<dyn Fn(Option<&str>, &Request) -> Option<Response> + Send + Sync>;

/// Verifies a managed agent's per-run `AGENT_OS_SESSION_TOKEN` and peer PID ancestry (`CLI-12`, `SUB-3`).
pub type RunBindingVerifyFn =
    Box<dyn Fn(&str, Option<&str>, Option<libc::pid_t>) -> bool + Send + Sync>;

/// Checks whether a bound managed agent still has an active run binding on this office.
pub type RunBindingActiveFn = Box<dyn Fn(&str) -> bool + Send + Sync>;

/// Records an accepted `agent-os` command for a bound managed run (`ADR-0030` decision 8).
pub type RecordCommandFn = Box<dyn Fn(&str, &str) + Send + Sync>;

/// Looks up the lock-free in-memory `AtomicU32` counter of waiting messages/wakes for a bound
/// agent (`TASK-115` AC #6 / `doc-101`). Looked up once on bind; hot-path RPCs do one relaxed load.
pub type WaitingMessagesCounterFn =
    Box<dyn Fn(&str) -> Arc<std::sync::atomic::AtomicU32> + Send + Sync>;

/// Stops an agent's active session and revokes its binding when paused or offboarded (`SUB-5`, `B2 Q9`).
pub type AgentLifecycleHookFn = Box<dyn Fn(&str, &str) + Send + Sync>;

/// Checks whether a member may send a message to `target` (ADR-0029 decision 3 / HIRE-2).
pub type HrMessageGuardFn =
    Box<dyn Fn(&str, &str) -> Result<Result<(), ApiError>, String> + Send + Sync>;

/// Dispatches B6/B7 `dm.open`, `chat.*`, `msg.accept`, and `msg.withdraw` RPCs.
pub type ChatFn = Box<
    dyn Fn(Option<&str>, bool, &std::sync::atomic::AtomicBool, &Request) -> Option<Response>
        + Send
        + Sync,
>;

/// Dispatches B4/B5/B4b unified `tasks.*` RPCs (`TASK-49`, `TASK-77`).
pub type TasksFn = Box<dyn Fn(Option<&str>, &Request) -> Option<Response> + Send + Sync>;

/// Dispatches C7 repository, worktree, and Git push RPCs (`TASK-51`).
pub type GitFn = Box<dyn Fn(Option<&str>, &Request) -> Option<Response> + Send + Sync>;

/// Dispatches C5 organization processes, standards, and built-in skills RPCs (`TASK-44`).
pub type ProcessesFn = Box<dyn Fn(Option<&str>, &Request) -> Option<Response> + Send + Sync>;

/// Dispatches L2/L3 usage, cost accounting, and budget RPCs (`TASK-53`).
pub type CostsFn = Box<dyn Fn(Option<&str>, &Request) -> Option<Response> + Send + Sync>;

/// Dispatches L4 provider connection pools, bindings, model discovery, readiness, and quota RPCs (`TASK-54`).
pub type ProvidersFn = Box<dyn Fn(Option<&str>, &Request) -> Option<Response> + Send + Sync>;

/// What the connections ask of the office.
pub struct OfficePorts {
    pub snapshot: SnapshotFn,
    pub projects_list: ProjectsListFn,
    pub project_inspect: ProjectInspectFn,
    pub hiring: Option<HiringFn>,
    pub sessions: Option<SessionsFn>,
    pub run_binding_verify: Option<RunBindingVerifyFn>,
    pub run_binding_active: Option<RunBindingActiveFn>,
    /// The Chief of Staff's delegation, intersected with her own authority on every call she
    /// makes and on what it answers (`TASK-80.1`). `None`: no member is gated.
    pub delegation: Option<Arc<dyn ChiefOfStaffPort>>,
    /// External (BYOA) agent sign-in, leases and fast tokens (`TASK-137`). `None`: `auth.*`
    /// answers `unsupported` and only managed runs bind.
    pub external_auth: Option<Arc<dyn ExternalAuthPort>>,
    pub record_agent_command: Option<RecordCommandFn>,
    pub waiting_messages_counter: Option<WaitingMessagesCounterFn>,
    pub on_agent_stop: Option<AgentLifecycleHookFn>,
    pub hr_message_guard: Option<HrMessageGuardFn>,
    pub submit: SubmitFn,
    pub work: WorkFn,
    pub operation_get: OperationFn,
    pub who: WhoFn,
    pub status: StatusFn,
    /// The log, for `subscribe`. With `None`, `subscribe` answers `unsupported` and the
    /// inbox sends no `inbox.changed` hints.
    pub feed: Option<Box<dyn OrgFeed>>,
    /// Live events for `subscribe` (messages, status lines).
    pub live: Arc<LiveEvents>,
    /// Messaging: what `msg.send`, `msg.inbox`, `msg.read` and `msg.wait` run on. `None`:
    /// this office does not serve them yet (they answer `unsupported`); `msg.bind` still binds
    /// through `directory`.
    pub mailbox: Option<Arc<Mailbox>>,
    /// B6 chat/channel/DM/accept/events-poll dispatcher.
    pub chat: Option<ChatFn>,
    /// B4/B5/B4b unified typed tasks dispatcher (`TASK-49`, `TASK-77`).
    pub tasks: Option<TasksFn>,
    /// C7 repository, worktree, and Git push dispatcher (`TASK-51`).
    pub git: Option<GitFn>,
    /// C5 organization processes, standards, and built-in skills dispatcher (`TASK-44`).
    pub processes: Option<ProcessesFn>,
    /// L2/L3 usage, cost accounting, and budget controls dispatcher (`TASK-53`).
    pub costs: Option<CostsFn>,
    /// L4 provider pools, office bindings, discovery, readiness, and quota dispatcher (`TASK-54`).
    pub providers: Option<ProvidersFn>,
    /// Multi-office invitation, revocation, and replication status dispatcher (`TASK-52`).
    pub federation: Option<ProvidersFn>,
    /// Who a `msg.bind` names.
    pub directory: Arc<dyn MemberDirectory + Send + Sync>,
}

#[cfg(test)]
mod tests;
