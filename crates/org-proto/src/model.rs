//! The organization as the API returns it: machine, workspace, projects, roles, agents, live
//! presence and sessions, goals, backlogs, the Needs-you inbox, activity and documents.
//!
//! Sources (spec desktop-app-v1 §2.6): files are the truth (agent.json, BACKLOG.md, briefs,
//! roles/*.toml); orgd adds presence, sessions, the inbox and the activity log.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ts_rs::TS;

use crate::ids::{
    AgentId, IsoDate, MachineId, ProjectKey, RoleId, SessionId, Timestamp, WorkspacePath,
};

// ─── Machine and organization ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MachineConnection {
    Connected,
    Reconnecting,
    Unreachable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Machine {
    pub machine_id: MachineId,
    /// "This computer" for the local machine.
    pub label: String,
    pub is_local: bool,
    pub connection: MachineConnection,
    /// When `connection` isn't `connected`: the last time we heard from it.
    pub last_seen_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Organization {
    pub machine_id: MachineId,
    /// Display name (OQ-13). Falls back to the workspace folder name.
    pub name: String,
    /// Absolute path on `machine_id`.
    pub workspace_path: String,
    /// `workspace.toml` `format`.
    pub format: u32,
}

// ─── Claude Code environment (first run, banners) ───────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClaudeCodeState {
    Ok { version: String },
    Missing,
    TooOld { version: String, minimum: String },
    SignedOut { version: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Environment {
    pub machine_id: MachineId,
    pub claude_code: ClaudeCodeState,
    pub checked_at: Timestamp,
}

// ─── Projects and roles ─────────────────────────────────────────────────────

/// `projects/<key>/project.json` plus what the brief and roster say.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Project {
    pub machine_id: MachineId,
    pub key: ProjectKey,
    pub name: String,
    pub description: String,
    pub created_on: IsoDate,
    /// `projects/<key>/CLAUDE.md`
    pub brief_path: WorkspacePath,
    /// Agents in the roster block, in roster order.
    pub roster: Vec<AgentId>,
}

/// A tool allowlist, or every tool (`inherit`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Tools {
    /// All tools (the role spec says `inherit`).
    Inherit,
    List {
        tools: Vec<String>,
    },
}

/// `hr/roles/<id>.toml`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Role {
    pub machine_id: MachineId,
    pub id: RoleId,
    pub title: String,
    pub department: String,
    /// Phrased "Use for …".
    pub summary: String,
    pub responsibilities: Vec<String>,
    pub boundaries: Vec<String>,
    pub tools: Tools,
    pub directories: Vec<String>,
    pub path: WorkspacePath,
}

// ─── Agents ─────────────────────────────────────────────────────────────────

/// Who an agent reports to: the owner (`"user"`, shown as "You") or another agent's id.
/// On the wire it is a plain string.
// Rust: equality and hashing go by that string, so `Agent("user")` equals `User`; build
// values with `From` to get the canonical variant.
#[derive(Debug, Clone, TS)]
#[ts(type = "\"user\" | string")]
pub enum ReportsTo {
    User,
    Agent(AgentId),
}

impl ReportsTo {
    pub const USER: &'static str = "user";

    pub fn as_str(&self) -> &str {
        match self {
            ReportsTo::User => Self::USER,
            ReportsTo::Agent(id) => id.as_str(),
        }
    }
}

impl From<&str> for ReportsTo {
    fn from(value: &str) -> Self {
        ReportsTo::from(value.to_owned())
    }
}

impl From<String> for ReportsTo {
    fn from(value: String) -> Self {
        if value == Self::USER {
            ReportsTo::User
        } else {
            ReportsTo::Agent(AgentId(value))
        }
    }
}

impl PartialEq for ReportsTo {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for ReportsTo {}

impl std::hash::Hash for ReportsTo {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl fmt::Display for ReportsTo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ReportsTo {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ReportsTo {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(ReportsTo::from)
    }
}

/// `agent.json` `status`: whether the agent is on the team.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum EmploymentStatus {
    Active,
    Offboarded,
}

/// The `agent.json` record, as written by HR. Read-only in the app (changes go through HR).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Agent {
    pub machine_id: MachineId,
    pub id: AgentId,
    /// Display name as written, e.g. "Eden", "ada".
    pub name: String,
    pub project: ProjectKey,
    pub role: RoleId,
    pub title: String,
    pub department: String,
    pub focus: Option<String>,
    /// `user` (shown as "You") or an AgentId.
    pub reports_to: ReportsTo,
    pub model: String,
    pub tools: Tools,
    pub status: EmploymentStatus,
    pub hired_on: IsoDate,
    pub offboarded_on: Option<IsoDate>,
    /// `projects/<key>/agents/<id>`
    pub home: WorkspacePath,
    /// `.claude/agents/<id>.md`
    pub registration: WorkspacePath,
}

/// The HR agent's id, always `"hr"`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub enum HrAgentId {
    #[default]
    #[serde(rename = "hr")]
    Hr,
}

impl HrAgentId {
    /// The HR agent's id as an [`AgentId`] (for presence, sessions and the like).
    pub fn agent_id(self) -> AgentId {
        AgentId::from("hr")
    }
}

/// The HR agent's display name, always `"HR"`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub enum HrAgentName {
    #[default]
    #[serde(rename = "HR")]
    Hr,
}

/// The HR agent is not a project member; it shows as a staff node beside the owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HrAgent {
    pub machine_id: MachineId,
    #[ts(inline)]
    pub id: HrAgentId,
    #[ts(inline)]
    pub name: HrAgentName,
    pub description: String,
    /// `.claude/agents/hr.md`
    pub registration: WorkspacePath,
}

/// Exactly one per agent. Precedence: unreachable > needs_you > working > idle > offline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    NeedsYou,
    Working,
    Idle,
    Offline,
    Unreachable,
}

/// How a session runs (spec §2.6, R#10). `background`: a Claude Code background session, which
/// the app can attach to (show in the Session panel) wherever it was started. `interactive`: a
/// session in a terminal (e.g. `bin/agent`), which the app can't show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    Interactive,
    Background,
}

/// Who started the session: the app, or the owner outside it (a terminal, `bin/agent`, plain `claude`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SessionOrigin {
    App,
    Outside,
}

/// Where a presence status comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PresenceSource {
    /// From the presence mod's events.
    Presence,
    /// From Claude Code's session list and liveness checks only (no session, a session without
    /// the mods, or one still starting); a `poll` Needs you ("waiting") can be up to 10 s late.
    Poll,
}

/// Live status of one agent, from presence events + liveness (orgd's status engine). Carries
/// what the UI needs to label it: whether the session can be shown (`session_kind`), whether it
/// can report Needs you (`has_presence`), and how fresh the status is (`as_of`, `source`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Presence {
    pub machine_id: MachineId,
    pub agent_id: AgentId,
    pub status: AgentStatus,
    /// When `status` last changed.
    pub since: Timestamp,
    /// Why the agent is Needs you: the kinds of its open items that hold the status, oldest
    /// first, without repeats. Empty unless `status` is `needs_you`. `blocked_task` never holds it.
    pub reasons: Vec<NeedsYouKind>,
    /// The live session (starting or running), if any.
    pub session_id: Option<SessionId>,
    /// The live session's kind; null without one.
    pub session_kind: Option<SessionKind>,
    /// The live session loaded the app's mods, so it can report Needs you. False without one.
    pub has_presence: bool,
    pub source: PresenceSource,
    /// When orgd last confirmed this status. While the machine isn't `connected` it stops moving:
    /// the UI shows the status muted, "as of <time>" (spec §2.7).
    pub as_of: Timestamp,
}

// ─── Sessions ───────────────────────────────────────────────────────────────

/// The error a session's last turn ended on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TurnError {
    pub at: Timestamp,
    pub message: String,
}

/// "Stopped with an error" is not a state of its own: the agent's turn ended on an error, but
/// the session stays open and live (spec §3.8, SESS-11). It shows as `running` with `error`
/// set until the owner next writes to the agent, which clears it (and its Needs-you item).
/// A Stop the owner started is always `stopped`, never `ended_unexpectedly` (R#24).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionState {
    Starting,
    Running {
        error: Option<TurnError>,
    },
    Stopped {
        stopped_at: Timestamp,
    },
    EndedUnexpectedly {
        at: Timestamp,
        exit_reason: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Session {
    pub machine_id: MachineId,
    pub id: SessionId,
    /// Claude Code's 8-hex short id. `claude attach` accepts only this (spike S2, 2026-11-07),
    /// and `--bg` ignores `--session-id`, so orgd stores both. Used only to attach the panel,
    /// so only when `kind` is `background`.
    pub attach_id: String,
    pub agent_id: AgentId,
    /// `background` can be attached (shown in the panel); `interactive` can't.
    pub kind: SessionKind,
    pub origin: SessionOrigin,
    /// The app's mods are loaded (presence reports from it), so it can raise Needs you.
    pub has_presence: bool,
    pub state: SessionState,
    pub started_at: Timestamp,
    /// The last presence event from the session ("last active"); null when none arrived.
    pub last_event_at: Option<Timestamp>,
    /// The `bin/agent` equivalent, for "Copy command".
    pub command: String,
}

// ─── Goals and tasks ────────────────────────────────────────────────────────

/// The live goal's state. The goals mod reports only `active` and `paused` (spec §2.6, OQ-7);
/// `met` and `cleared` are kept for when it emits those, and then show as history (activity),
/// never as a live state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum GoalState {
    Active,
    Met,
    Paused,
    Cleared,
}

/// The live goal of an agent's running session, from goals mod events. Only what the mod
/// reports: no condition text, check count or check reason (OQ-7, OQ-31), and no link to a
/// backlog row (R#33). What the goal is comes from the backlog instead: the "DoD (goal)" text
/// in the Notes of the agent's `in-progress` rows, which the UI derives from `Backlog`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Goal {
    pub machine_id: MachineId,
    pub agent_id: AgentId,
    pub session_id: SessionId,
    pub state: GoalState,
    /// The pause event's count ("paused after 8 unmet checks in a row"); null unless paused.
    pub paused_after_checks: Option<u32>,
    pub updated_at: Timestamp,
}

/// Known values; anything else is shown as written with a neutral chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum KnownTaskStatus {
    Todo,
    InProgress,
    Blocked,
    Done,
}

impl KnownTaskStatus {
    pub const ALL: [KnownTaskStatus; 4] = [
        KnownTaskStatus::Todo,
        KnownTaskStatus::InProgress,
        KnownTaskStatus::Blocked,
        KnownTaskStatus::Done,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            KnownTaskStatus::Todo => "todo",
            KnownTaskStatus::InProgress => "in-progress",
            KnownTaskStatus::Blocked => "blocked",
            KnownTaskStatus::Done => "done",
        }
    }
}

/// A task's status as written in the file: a `KnownTaskStatus` or any other string.
/// On the wire it is a plain string.
// Rust: equality and hashing go by that string, so `Other("todo")` equals `Known(Todo)`;
// build values with `parse` or `From` to get the canonical variant.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum TaskStatus {
    Known(KnownTaskStatus),
    Other(#[ts(type = "string & {}")] String),
}

impl TaskStatus {
    /// Reads a status as written. Known values become [`TaskStatus::Known`], so a value never
    /// changes variant across a JSON round trip.
    pub fn parse(value: &str) -> Self {
        // Through serde, so a new KnownTaskStatus variant is recognized without touching this.
        serde_json::from_value(serde_json::Value::String(value.to_owned()))
            .map_or_else(|_| TaskStatus::Other(value.to_owned()), TaskStatus::Known)
    }

    /// The status as written.
    pub fn as_str(&self) -> &str {
        match self {
            TaskStatus::Known(known) => known.as_str(),
            TaskStatus::Other(other) => other,
        }
    }
}

impl From<&str> for TaskStatus {
    fn from(value: &str) -> Self {
        TaskStatus::parse(value)
    }
}

impl From<String> for TaskStatus {
    fn from(value: String) -> Self {
        TaskStatus::parse(&value)
    }
}

impl PartialEq for TaskStatus {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for TaskStatus {}

impl std::hash::Hash for TaskStatus {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

/// One row of `tasks/BACKLOG.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Task {
    pub machine_id: MachineId,
    pub agent_id: AgentId,
    /// The `#` column.
    pub number: u32,
    pub text: String,
    /// As written in the file (KnownTaskStatus or any other string).
    pub status: TaskStatus,
    /// `user` or an agent name, as written.
    pub requested_by: String,
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Backlog {
    Ok {
        machine_id: MachineId,
        agent_id: AgentId,
        path: WorkspacePath,
        tasks: Vec<Task>,
    },
    Unreadable {
        machine_id: MachineId,
        agent_id: AgentId,
        path: WorkspacePath,
        /// Plain words, e.g. "the task table is malformed".
        reason: String,
    },
}

// ─── Needs you ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum NeedsYouKind {
    Permission,
    Question,
    /// A background session without the app's mods that Claude Code's session list reports as
    /// waiting on the owner (spec §2.4, §2.6, SESS-12). Polled, so it can be up to 10 s late.
    /// Clears when the agent continues.
    Waiting,
    GoalPaused,
    StoppedWithError,
    EndedUnexpectedly,
    /// On hold in v1 (spec §2.4, HOME-5, OQ-12): kept in the contract, never produced yet.
    BlockedTask,
}

/// One inbox item. The bell, Home › Needs you, the nav badge and the dock badge are all
/// counts of open (not dismissed) items. Never contains tool inputs or outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct NeedsYouItem {
    pub machine_id: MachineId,
    pub id: String,
    pub agent_id: AgentId,
    pub kind: NeedsYouKind,
    /// When the agent started waiting.
    pub since: Timestamp,
    pub session_id: Option<SessionId>,
    /// `permission`: tool name only, e.g. "Bash".
    pub tool_name: Option<String>,
    /// `question`: the session's notification text; errors: the message; `blocked_task`: the task
    /// text. Null for `goal_paused` (the mod gives no text; the UI shows the backlog's definition
    /// of done) and `waiting` (Claude Code's list gives none).
    pub detail: Option<String>,
    /// `blocked_task` only.
    pub task_number: Option<u32>,
    /// Only `ended_unexpectedly` and `blocked_task` can be dismissed (spec §2.4). The others
    /// clear when the agent carries on (`stopped_with_error` and `goal_paused`: when the owner
    /// next writes to it) and can't be dismissed.
    pub dismissible: bool,
    pub dismissed_at: Option<Timestamp>,
}

// ─── Activity ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SessionChange {
    Started,
    Resumed,
    Stopped,
    StoppedWithError,
    EndedUnexpectedly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum GoalChange {
    Set,
    Met,
    Paused,
    Cleared,
}

/// Why a goal changed, for the activity line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum GoalChangeReason {
    /// The goals mod paused it after `paused_after_checks` unmet checks in a row ("Goal paused").
    UnmetChecks,
    /// The session stopped while the goal was paused, so nothing is left to answer in
    /// ("Goal paused · session stopped", spec §2.4).
    SessionStopped,
}

/// One entry of the activity log: the common fields plus a `kind`-tagged detail, flattened
/// into one JSON object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ActivityEvent {
    pub machine_id: MachineId,
    pub id: String,
    pub at: Timestamp,
    #[serde(flatten)]
    pub detail: ActivityDetail,
}

/// What happened, by `kind`. Flattened into [`ActivityEvent`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActivityDetail {
    AgentHired {
        agent_id: AgentId,
    },
    AgentOffboarded {
        agent_id: AgentId,
    },
    SessionChanged {
        agent_id: AgentId,
        session_id: SessionId,
        change: SessionChange,
    },
    GoalChanged {
        agent_id: AgentId,
        session_id: SessionId,
        /// `set`, `met` and `cleared` once the goals mod emits them (OQ-7); today only `paused`.
        change: GoalChange,
        reason: Option<GoalChangeReason>,
        /// `paused` for `unmet_checks`: the pause event's count. Otherwise null.
        paused_after_checks: Option<u32>,
    },
    TaskAdded {
        agent_id: AgentId,
        task_number: u32,
        text: String,
    },
    TaskStatusChanged {
        agent_id: AgentId,
        task_number: u32,
        from: String,
        to: String,
    },
    JournalEntryAdded {
        agent_id: AgentId,
        date: IsoDate,
        path: WorkspacePath,
    },
    ProjectCreated {
        project_key: ProjectKey,
    },
}

// ─── Files (journal, memory, knowledge, briefs) ─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Journal,
    Memory,
    Knowledge,
    Brief,
    Instructions,
    Role,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DocumentRef {
    pub machine_id: MachineId,
    pub path: WorkspacePath,
    pub kind: DocumentKind,
    /// Owning agent, if the file lives in an agent's home.
    pub agent_id: Option<AgentId>,
    pub title: String,
    pub modified_at: Timestamp,
    pub size_bytes: u64,
}

/// A file's content. Markdown is rendered by the UI with raw HTML off and sanitized.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Document {
    #[serde(flatten)]
    pub meta: DocumentRef,
    pub content: String,
}
