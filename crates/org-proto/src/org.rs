//! The organization's structure: teams, members, memberships and the teams working on each
//! project (see `docs/design/teams-and-org-chart.md`).
//!
//! Organization data is owned by HQ and copied to every office (offices design). Clients read
//! it from their local office with [`OrgSnapshotCall`], then follow changes with
//! `subscribe { from_position }` and the [`OrgChanged`] event (see [`crate::SubscribeParams`]). Every change is a command that
//! HQ validates. A command carries a client-made `command_id` (a ULID), client-made ids for
//! anything it creates, and the `expected_version` of what it edits.
//!
//! ## Who may send what
//! Every command in this module is for the app. A command that arrives on a connection that
//! isn't the app's is refused with `not_authorized` unless a row of ADR-0003 §2.1.4 allows it
//! (none does today). The refusal applies to every command here, though only the ones below
//! that change who works for the organization name `not_authorized` in their `Errors:` lists. In
//! particular `member.add_agent`, `member.update_agent`, `member.set_agent_status`,
//! `member.set_run_mode`, `member.offboard` and `member.remove` change who works for the
//! organization and what it spends, and some cannot be undone. The wire
//! carries no caller identity: the office that receives a command knows whose connection it came
//! in on (the app's connection is bound with the app token, ADR-0003), enforces this before it
//! queues or forwards the command, and HQ relies on that.
//!
//! **One exception, for hiring.** `member.add_agent` is also accepted from a connection bound as
//! the built-in HR lead (its run, with its session token), and only when it names an APPROVED
//! hire proposal (`hire_proposal_id`, see [`HireProposal`]) whose facts match the params; the
//! proposal moves to `provisioned` in the same event. Nothing else is open to it except
//! `hire.request` (on behalf of a requester) and `hire.fail`. `hire.request` is for the app and
//! for agents; `hire.approve` and `hire.reject` are for the proposal's routed approver and the
//! owner (see their docs). The wire still carries no identity: the office knows whose connection
//! it is, and HQ relies on that.
//!
//! ## When several errors apply
//! A command can break more than one rule at once. HQ returns the first of: `not_authorized`,
//! `not_found`, `invalid_request` (unreadable, outside a limit, the wrong kind of member or the
//! wrong state, a value that makes no sense), `invalid_name`, `conflict` (a stale
//! `expected_version`), `already_exists`. Every command below checks in this one order (its
//! `Errors:` list names the codes it can return, not the order they are checked in). The
//! domain codes (`owner_protected`, `builtin_protected`, `cycle`, `not_a_member`, `not_empty`)
//! come after
//! `invalid_name` and before `conflict`: a rule that would refuse the command at any version
//! is not hidden by a stale one. Among themselves `owner_protected` comes first, then
//! `builtin_protected`, so a lead
//! who is the owner is `owner_protected`, not `not_a_member`.
//!
//! ## Limits every command shares
//! All are `invalid_request`, so a client can check them before sending:
//! - **Client-made ids**, the ones a command creates (`team.create`'s `team_id`,
//!   `team.add_member`'s and `member.move`'s new `membership_id`, `project.add_team`'s
//!   `project_team_id`, and so on): 1 to [`ID_MAX_CHARS`] ASCII letters, digits, `-` or `_`
//!   ([`is_valid_id`]). A ULID fits. An id that names something that already exists is only
//!   looked up (`not_found`).
//! - **Member ids** (`member.add_person`'s and `member.add_agent`'s `member_id`): exactly
//!   [`MEMBER_ID_CHARS`] ASCII letters and digits, the shape of a ULID ([`is_valid_member_id`]).
//!   No handle has this shape, so an id and a handle never collide.
//! - **Command ids** (`command_id`, on every command): the same, [`is_valid_id`].
//! - **Free text** (`contact`, `title_in_team`): trimmed, then 1 to [`AGENT_TEXT_MAX_CHARS`]
//!   characters with no control characters. To remove a contact use `clear_contact`, not an
//!   empty string.
//! - **An edit that changes nothing still counts.** An update that sets a field to the value it
//!   has (a rename to the same name, a lead set to the current lead), or a
//!   `member.update_person` that gives no field, is accepted and raises the entity's version by
//!   one (and so produces an `org.changed`). Only `member.update_agent` refuses a call that
//!   gives no field. This is about edits: asking for something that already exists (adding a
//!   member to a team they are in, `member.move` to the member's own team) is `already_exists`.

use serde::{Deserialize, Serialize};

use crate::calls::{Call, Empty};
use crate::events::EventKind;

/// A position in the organization's event log: `(epoch, seq)`, compared in that order.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct Position {
    pub epoch: u64,
    pub seq: u64,
}

/// A team. Teams own teams (a tree); that's the only structure ("division" and "guild" aren't
/// concepts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Team {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_team_id: Option<String>,
    /// Optional; an agent or a person who's a member of this team. Also leads its projects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_member_id: Option<String>,
    pub version: u64,
}

/// Whether a member is an AI agent or a human. Unknown values decode as
/// [`MemberKind::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MemberKind {
    Agent,
    Person,
    #[serde(other)]
    Unknown,
}

/// The program an agent runs on.
///
/// Never resend a decoded profile verbatim: an older client that decoded a newer runtime holds
/// `Unknown`, and sending that back would overwrite the real value (HQ refuses `Unknown`
/// where it is a command's input, but a cache written back by the client would not be a
/// command). Commands carry only the fields to change.
///
/// On the wire: `"claude_code"`, `"codex"`, `"antigravity"`, or `{"other": "<name>"}` for a
/// runtime the sender knows by name but this enum has no variant for. Reading never fails:
/// any string or object this build does not recognise, including a shape a newer build may
/// add, decodes as [`AgentRuntime::Unknown`], so one new runtime cannot make an older client
/// reject a whole member, change or snapshot.
///
/// Writers use the named variant when there is one and never send `Unknown`; HQ refuses both
/// with `invalid_request`. A name is non-canonical, and refused, when it equals a named
/// runtime ignoring case and treating `-`, `_` and spaces alike ("Codex", "claude-code").
/// An `Other` name has no control characters.
///
/// Decoding goes through `serde_json::Value`, so it needs a self-describing format; the
/// protocol is JSON.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AgentRuntime {
    ClaudeCode,
    Codex,
    Antigravity,
    /// Any other runtime, by name (1 to [`AGENT_TEXT_MAX_CHARS`] characters).
    Other(String),
    /// Missing or not known to this build.
    #[default]
    Unknown,
}

impl<'de> Deserialize<'de> for AgentRuntime {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Read the shape first, then decide: this never fails on a well-formed document.
        let value = serde_json::Value::deserialize(d)?;
        Ok(match &value {
            serde_json::Value::String(name) => match name.as_str() {
                "claude_code" => AgentRuntime::ClaudeCode,
                "codex" => AgentRuntime::Codex,
                "antigravity" => AgentRuntime::Antigravity,
                _ => AgentRuntime::Unknown,
            },
            serde_json::Value::Object(map) => match map.get("other") {
                Some(serde_json::Value::String(name)) => AgentRuntime::Other(name.clone()),
                _ => AgentRuntime::Unknown,
            },
            _ => AgentRuntime::Unknown,
        })
    }
}

/// Whether an agent takes work. A value this build doesn't know, in any shape, decodes as
/// [`AgentStatus::Unknown`]; writers never send it (HQ refuses it with `invalid_request`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AgentStatus {
    /// Takes work and may message and be messaged.
    Active,
    /// Takes no new work; stays a member and keeps its place in teams. Resumed with
    /// `member.set_agent_status`. What a paused agent still receives (messages, tasks) is
    /// decided by ADR-0003 and the work-items design; this contract only carries the flag.
    Paused,
    /// Left the organization (`member.offboard`). A state, not a deletion: the record stays
    /// for history and its handle stays reserved. Only `member.offboard` sets it, and it is
    /// final in this contract: no command brings an agent back. It has no memberships, leads
    /// no team and manages no one (`member.offboard` ends those), and the team and reporting
    /// commands refuse it. What it still receives (messages, tasks) is decided by ADR-0003
    /// and the work-items design. A build older than this word reads it as `Unknown`, so
    /// a client must not treat `Unknown` as "working".
    Offboarded,
    /// Missing or not known to this build.
    #[default]
    Unknown,
}

impl<'de> Deserialize<'de> for AgentStatus {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Like `AgentRuntime`: reading never fails on a well-formed document.
        let value = serde_json::Value::deserialize(d)?;
        Ok(match value.as_str() {
            Some("active") => AgentStatus::Active,
            Some("paused") => AgentStatus::Paused,
            Some("offboarded") => AgentStatus::Offboarded,
            _ => AgentStatus::Unknown,
        })
    }
}

/// How an agent is run. A value this build doesn't know, in any shape, decodes as
/// [`RunMode::Unknown`]; writers never send it (HQ refuses it with `invalid_request`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RunMode {
    /// Kept running: it takes work and messages as they come.
    AlwaysOn,
    /// Started when there is something for it to do.
    OnCall,
    /// Started on a schedule (see [`Schedule`]); a scheduled agent must have one.
    Scheduled,
    /// Missing or not known to this build.
    #[default]
    Unknown,
}

impl<'de> Deserialize<'de> for RunMode {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Like `AgentStatus`: reading never fails on a well-formed document.
        let value = serde_json::Value::deserialize(d)?;
        Ok(match value.as_str() {
            Some("always_on") => RunMode::AlwaysOn,
            Some("on_call") => RunMode::OnCall,
            Some("scheduled") => RunMode::Scheduled,
            _ => RunMode::Unknown,
        })
    }
}

/// When a [`RunMode::Scheduled`] agent runs. Kept as two validated strings for now, so the
/// shape can grow (a structured form, an end date) without breaking readers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    /// What to run when: plain words ("weekdays at 9") or a cron-like expression. HQ trims it,
    /// then checks it is 1 to [`AGENT_TEXT_MAX_CHARS`] characters with no control characters,
    /// and stores it as trimmed. It does not interpret it: the office that runs the agent does,
    /// and shows a spec it can't read. Empty only if a newer writer left it out.
    #[serde(default)]
    pub spec: String,
    /// The IANA time zone the spec is in, e.g. `Europe/Lisbon`. 1 to
    /// [`AGENT_TIME_ZONE_MAX_CHARS`] characters: ASCII letters, digits, `_`, `+`, `-` and `/`,
    /// as `/`-separated names with no empty name, no leading or trailing `/`, and no `.` or
    /// `..` (so it can never name a path). It is only ever looked up by a time-zone library,
    /// never joined to a file path. Empty only if a newer writer left it out.
    #[serde(default)]
    pub time_zone: String,
}

/// The word that always addresses the organization's owner (ADR-0003). It is reserved: no
/// other member may have it as a handle (see [`AgentProfile::messaging_name`]).
pub const OWNER_HANDLE: &str = "owner";

/// The word that addresses the office itself in messaging (ADR-0003). Reserved like
/// [`OWNER_HANDLE`]: no member may have it as a handle.
pub const OFFICE_HANDLE: &str = "office";

/// The handle of the built-in HR lead (see [`BuiltinRole`]). Reserved like [`OWNER_HANDLE`]: no
/// other member may have it as a handle, and the HR lead's `messaging_name` is exactly this.
pub const HR_HANDLE: &str = "hr";

/// A handle may not start with this (any case): `run:<id>` addresses a run in messaging, so a
/// person named `Run: 1` would shadow it. See [`is_shaped_like_an_address`].
pub const RUN_HANDLE_PREFIX: &str = "run:";

/// Whether `handle` (trimmed, as a person's `name` or an agent's `messaging_name` is compared)
/// looks like something messaging already addresses by: it starts with [`RUN_HANDLE_PREFIX`]
/// (any case), or is exactly 26 ASCII letters and digits (the shape of every member id,
/// [`is_valid_member_id`]; a lenient ULID decoder reads I, L, O and U too, so none is excluded),
/// so a handle can never shadow a member id or a run. HQ refuses such a
/// handle with `invalid_name`, for agents and for people. The reserved words [`OWNER_HANDLE`]
/// and [`OFFICE_HANDLE`] are a different rule: they are `already_exists`, as if another
/// member held them.
pub fn is_shaped_like_an_address(handle: &str) -> bool {
    let handle = handle.trim();
    let prefix = RUN_HANDLE_PREFIX.len();
    let run = handle.len() >= prefix
        && handle.is_char_boundary(prefix)
        && handle[..prefix].eq_ignore_ascii_case(RUN_HANDLE_PREFIX);
    // Compared by their lower-cased key: U+212A (KELVIN SIGN) lower-cases to ASCII `k`.
    run || is_valid_member_id(handle) || is_valid_member_id(&handle.to_lowercase())
}

/// The length of a member id.
pub const MEMBER_ID_CHARS: usize = 26;

/// Whether `id` is a valid member id: exactly [`MEMBER_ID_CHARS`] ASCII letters and digits (the
/// shape of a ULID, which is what clients make). `member.add_person` and `member.add_agent`
/// refuse anything else with `invalid_request`. No handle has this shape
/// ([`is_shaped_like_an_address`]), so an id never equals a handle, and a lookup by id before
/// handle cannot be captured by a name.
pub fn is_valid_member_id(id: &str) -> bool {
    id.len() == MEMBER_ID_CHARS && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The longest client-made id, in characters (see [`is_valid_id`]).
pub const ID_MAX_CHARS: usize = 64;

/// Whether `id` is a valid client-made id: 1 to [`ID_MAX_CHARS`] ASCII letters, digits, `-` or
/// `_`. HQ refuses anything else with `invalid_request`.
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= ID_MAX_CHARS
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The longest `messaging_name`, in characters.
pub const AGENT_MESSAGING_NAME_MAX_CHARS: usize = 64;

/// The longest `role`, `title`, `focus`, `model`, `Other` runtime name, `schedule.spec`, and
/// `contact` and `title_in_team`, in characters (after trimming).
pub const AGENT_TEXT_MAX_CHARS: usize = 200;

/// The longest `schedule.time_zone`, in characters.
pub const AGENT_TIME_ZONE_MAX_CHARS: usize = 64;

/// The longest `home`, in bytes.
pub const AGENT_HOME_MAX_BYTES: usize = 1024;

/// What makes a member an agent: who it is at work and how it runs. Part of the member record,
/// so it has the member's version.
///
/// Limits (HQ enforces them; a client may pre-check): `role`, `title` and `focus` are trimmed,
/// at most [`AGENT_TEXT_MAX_CHARS`] characters, with no control characters, and `role` and
/// `title` are not empty (`invalid_request` otherwise).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProfile {
    /// The agent's role, e.g. "software engineer".
    pub role: String,
    /// Its job title, e.g. "Senior Backend Engineer".
    pub title: String,
    /// What it concentrates on, if anything in particular.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    /// What the agent runs on. Required when adding an agent (`Unknown` is refused).
    #[serde(default)]
    pub runtime: AgentRuntime,
    /// The model the runtime uses (its model id, e.g. `claude-sonnet-5-5`), if the agent sets
    /// one. 1 to [`AGENT_TEXT_MAX_CHARS`] characters, trimmed, no control characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// How the agent is run. Missing means not office-run: a person starts it. Changed only
    /// with `member.set_run_mode`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_mode: Option<RunMode>,
    /// When a scheduled agent runs. HQ keeps it present exactly when `run_mode` is `scheduled`;
    /// readers must not rely on that (a mode this build can't read is `Unknown`, and it may
    /// come with or without a schedule).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<Schedule>,
    /// Where the agent's files are: an absolute path on its office's machine, at most
    /// [`AGENT_HOME_MAX_BYTES`] bytes, with no control characters (NUL included) and no `..`
    /// component; HQ refuses anything else with `invalid_request`. Clients show it and never
    /// open or interpret it. **It is visible organization-wide**: it is copied to every office
    /// and shown to every client, so it can reveal a user name and folder layout to remote
    /// offices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
    /// The handle this agent is addressed by (`--to <handle>` on `org-agent`). 1 to
    /// [`AGENT_MESSAGING_NAME_MAX_CHARS`] ASCII letters, digits, `-`, `_` and `.`, starting
    /// with a letter or digit. Unique among the organization's agents **ignoring case**
    /// (`already_exists` on a clash). A removed agent frees its name; an offboarded one keeps
    /// it for good. Renaming an agent to a different capitalisation of its own name is fine.
    /// A name that breaks the format is `invalid_name`.
    ///
    /// **One handle namespace.** Handles are compared ignoring case across ALL members: every
    /// agent's `messaging_name`, every person's `name` (the owner's apart, see below), and the
    /// reserved [`OWNER_HANDLE`], which no member other than the owner may use. HQ rejects a clash with `already_exists` in
    /// either direction (an agent whose `messaging_name` equals a person's `name`, or a person
    /// whose `name` equals an agent's `messaging_name`), so `--to <handle>` never names two
    /// members. `Member.name` stays the agent's display name and is not a handle. Three reserved
    /// words, [`OWNER_HANDLE`], [`OFFICE_HANDLE`] and [`HR_HANDLE`] (which is the built-in HR
    /// lead's own `messaging_name`), clash with every other member, and a handle that
    /// [`is_shaped_like_an_address`] is `invalid_name`. **The owner's handle is only `owner`:
    /// its display name is exempt from the namespace** (no clash and no address-shape check).
    /// `member.update_person` on the owner never clashes, and `member.add_person` may reuse the
    /// owner's display name.
    pub messaging_name: String,
    /// Whether the agent takes work. Required when adding an agent (`Unknown` and
    /// `Offboarded` are refused: an agent is offboarded with `member.offboard`).
    #[serde(default)]
    pub status: AgentStatus,
    /// Whether the agent may approve hire proposals routed to it (see [`HireProposal`]). Set by
    /// the owner ONLY, with `member.set_can_approve_hires` (or by the app's `member.add_agent`
    /// at hire): `member.update_agent` cannot change it, an agent can never give it to itself or
    /// to the agents it hires, and the HR lead never has it. Default false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub can_approve_hires: bool,
    /// When HQ offboarded the agent (RFC 3339 UTC with milliseconds, HQ's clock). HQ keeps it
    /// present exactly when `status` is `offboarded`. Only `member.offboard` sets it: a client
    /// that sends it in a profile (`member.add_agent`) gets `invalid_request`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offboarded_at: Option<String>,
}

/// What a built-in member is. A role this build doesn't know decodes as
/// [`BuiltinRole::Unknown`]; writers never send it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BuiltinRole {
    /// The organization's one HR lead: an agent that only provisions agents (it proposes and
    /// sets up hires and cleans up after offboarding). Created with the organization.
    HrLead,
    /// Missing or not known to this build.
    #[default]
    Unknown,
}

impl<'de> Deserialize<'de> for BuiltinRole {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(d)?;
        Ok(match value.as_str() {
            Some("hr_lead") => BuiltinRole::HrLead,
            _ => BuiltinRole::Unknown,
        })
    }
}

/// A member of the organization: an agent or a person. The owner is a person with
/// `owner: true`: in no team, full rights everywhere, always reachable by agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    /// The member's id, made by the client (or, for the owner, by the office): every one,
    /// the owner's included, satisfies [`is_valid_member_id`].
    pub id: String,
    pub kind: MemberKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub owner: bool,
    /// Set on a built-in member, like `owner` is on the owner: the organization's HR lead
    /// ([`BuiltinRole::HrLead`]), an agent made with the organization (one per organization, a
    /// server-made id, `messaging_name` [`HR_HANDLE`], run mode `on_call`). It is in no team, has
    /// no reports and no manager, cannot be offboarded or removed or renamed, and the owner can
    /// change only its `runtime`, `model` and run mode: anything else is
    /// [`crate::ErrorCode::BuiltinProtected`]. Absent on every other member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin: Option<BuiltinRole>,
    /// Agents only: the office the agent is placed in. People aren't placed in offices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub office_id: Option<String>,
    /// Agents only: who the agent is and how it runs. HQ keeps it present exactly when `kind`
    /// is `agent`; readers must still cope with a record that breaks that (older data, or a
    /// build that doesn't know the kind) and never treat it as an error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentProfile>,
    /// Who this member reports to (an agent or a person). Empty means directly to the owner;
    /// setting it to the owner's own id is stored as empty, so each line has one form.
    /// Reporting lines are separate from teams. The owner reports to no one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reports_to_member_id: Option<String>,
    /// When HQ created the member (RFC 3339 UTC with milliseconds, HQ's clock): for an agent,
    /// the day it was hired. Set by HQ and never changed; missing on records from before it
    /// existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    pub version: u64,
}

/// A member in a team. A member can be in any number of teams.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Membership {
    pub id: String,
    pub team_id: String,
    pub member_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_in_team: Option<String>,
    pub version: u64,
}

/// A team working on a project. Usually one per project; several teams can share one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectTeam {
    pub id: String,
    pub project_id: String,
    pub team_id: String,
    pub version: u64,
}

/// What the charts need to show a project. The full project record comes with the GitHub
/// contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
}

/// Everything the org chart needs, as of `position`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgSnapshot {
    pub position: Position,
    pub teams: Vec<Team>,
    pub members: Vec<Member>,
    pub memberships: Vec<Membership>,
    #[serde(default)]
    pub projects: Vec<ProjectSummary>,
    #[serde(default)]
    pub project_teams: Vec<ProjectTeam>,
    /// Every hire proposal, newest last.
    #[serde(default)]
    pub hire_proposals: Vec<HireProposal>,
}

/// What happened to an organization command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CommandStatus {
    /// HQ committed it; `position` says where.
    Committed,
    /// HQ is unreachable, so this satellite queued it in its outbox. It's **pending**: not
    /// in the copy yet. The outcome arrives later, matched by `command_id`: an `org.changed`
    /// event if HQ commits it, or an `org.rejected` event (shown as blocked) if not.
    Queued,
    #[serde(other)]
    Unknown,
}

/// The answer to every organization command. An error response means HQ rejected it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandAccepted {
    pub status: CommandStatus,
    /// Set when `status` is `committed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
}

/// `org.snapshot`: the organization's structure from the local office's copy.
pub struct OrgSnapshotCall;

impl Call for OrgSnapshotCall {
    const METHOD: &'static str = "org.snapshot";
    type Params = Empty;
    type Result = OrgSnapshot;
}

macro_rules! command {
    ($(#[$doc:meta])* $call:ident, $method:literal, $params:ident { $($(#[$fattr:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        $(#[$doc])*
        pub struct $call;

        #[doc = concat!("Params of `", $method, "`.")]
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        pub struct $params {
            /// Client-made (a ULID; [`is_valid_id`], `invalid_request` otherwise); a retry with the
            /// same id gets the same answer.
            pub command_id: String,
            $($(#[$fattr])* pub $field: $ty,)*
        }

        impl Call for $call {
            const METHOD: &'static str = $method;
            type Params = $params;
            type Result = CommandAccepted;
        }
    };
}

command!(
    /// `team.create`. `team_id` is client-made. Errors: `invalid_name`, `already_exists` (a
    /// sibling has the name, ignoring case, or the id is taken), `not_found` (no such owner
    /// team).
    TeamCreate, "team.create", TeamCreateParams {
        team_id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner_team_id: Option<String>,
    }
);
command!(
    /// `team.rename`. Errors: `invalid_name`, `already_exists`, `conflict`, `not_found`.
    TeamRename, "team.rename", TeamRenameParams {
        team_id: String,
        expected_version: u64,
        name: String,
    }
);
command!(
    /// `team.move`: a new owner, or none for top level. Errors: `cycle`, `already_exists`,
    /// `conflict`, `not_found`.
    TeamMove, "team.move", TeamMoveParams {
        team_id: String,
        expected_version: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner_team_id: Option<String>,
    }
);
command!(
    /// `team.set_lead`: a member of the team, or none. Errors: `not_a_member`,
    /// `owner_protected`, `builtin_protected`, `conflict`, `not_found`, `invalid_request` (the member is an offboarded
    /// agent).
    TeamSetLead, "team.set_lead", TeamSetLeadParams {
        team_id: String,
        expected_version: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lead_member_id: Option<String>,
    }
);
command!(
    /// `team.delete`. Errors: `not_empty` (it owns teams), `conflict`, `not_found`.
    TeamDelete, "team.delete", TeamDeleteParams {
        team_id: String,
        expected_version: u64,
    }
);
command!(
    /// `team.add_member`. `membership_id` is client-made. Errors: `owner_protected`,
    /// `builtin_protected` (the HR lead is in no team),
    /// `not_found`, `invalid_request` (the member is an offboarded agent), `already_exists`.
    TeamAddMember, "team.add_member", TeamAddMemberParams {
        membership_id: String,
        team_id: String,
        member_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title_in_team: Option<String>,
    }
);
command!(
    /// `team.remove_member`. Removing the lead clears the lead. Errors: `conflict`,
    /// `not_found`.
    TeamRemoveMember, "team.remove_member", TeamRemoveMemberParams {
        membership_id: String,
        expected_version: u64,
    }
);
command!(
    /// `team.set_title`. Errors: `conflict`, `not_found`.
    TeamSetTitle, "team.set_title", TeamSetTitleParams {
        membership_id: String,
        expected_version: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title_in_team: Option<String>,
    }
);
command!(
    /// `member.add_person`. `member_id` is client-made and must be [`is_valid_member_id`]
    /// (`invalid_request`). Agents are added with `member.add_agent` instead.
    /// Errors: `invalid_request`, `invalid_name` (also a name that
    /// [`is_shaped_like_an_address`]), `already_exists`
    /// (the id is taken, or the name is already another member's handle: an agent's
    /// `messaging_name`, [`OWNER_HANDLE`] or [`OFFICE_HANDLE`], ignoring case; people's names
    /// also clash with each other, but not with the owner's display name).
    MemberAddPerson, "member.add_person", MemberAddPersonParams {
        member_id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        contact: Option<String>,
    }
);
command!(
    /// `member.update_person`: only the fields given change; `clear_contact` removes the
    /// contact. The owner can edit its own record, and its display name is not a handle, so
    /// renaming the owner never clashes. Errors: `invalid_name` (as for `member.add_person`,
    /// except that the owner is exempt from the address-shape part), `already_exists` (the new
    /// name is already another member's handle, as for `member.add_person`),
    /// `conflict`, `not_found`, and `invalid_request` if the member is an agent (agents are
    /// edited with `member.update_agent`) or if both `contact` and `clear_contact` are given.
    MemberUpdatePerson, "member.update_person", MemberUpdatePersonParams {
        member_id: String,
        expected_version: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        contact: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        clear_contact: bool,
    }
);
command!(
    /// `member.remove`: anyone but the owner. Removes their memberships, clears them as lead, and
    /// makes whoever reported to them report to the removed member's own manager, or to the owner
    /// if they had none (the same as `member.offboard`). It frees the member's handle, so it is
    /// for a member who has no history to keep: that is the CALLER'S duty (HQ sees the
    /// organization's structure, not messages or tasks, and cannot tell). What HQ does refuse is
    /// an offboarded agent, whose record and handle are kept for good (`invalid_request`);
    /// offboard an agent that has worked, remove one that never did. Errors: `not_authorized`,
    /// `builtin_protected` (the HR lead is never removed),
    /// `owner_protected`, `not_found`, `invalid_request`, `conflict`.
    MemberRemove, "member.remove", MemberRemoveParams {
        member_id: String,
        expected_version: u64,
    }
);
command!(
    /// `member.set_reports_to`: who a member reports to, or none (the owner). Errors:
    /// `cycle` (someone would report to themselves, directly or through others),
    /// `owner_protected` (the owner reports to no one), `builtin_protected` (the HR lead has no
    /// manager and no reports), `conflict`, `not_found`, `invalid_request` (either member is an
    /// offboarded agent).
    MemberSetReportsTo, "member.set_reports_to", MemberSetReportsToParams {
        member_id: String,
        expected_version: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reports_to_member_id: Option<String>,
    }
);
command!(
    /// `member.add_agent`: adds an agent, placed in `office_id`. `member_id` is client-made.
    /// App-only, except for the built-in HR lead: its connection may add an agent when
    /// `hire_proposal_id` names an `approved` [`HireProposal`] and the params match its facts
    /// (name, role, title, runtime, model, run mode, and the team and manager if it named them),
    /// and then `profile.can_approve_hires` must be false; in the same event the proposal becomes
    /// `provisioned` with this member as `provisioned_member_id`. Any other use of
    /// `hire_proposal_id` (the app may name one too) is held to the same match; a proposal that
    /// is not `approved` is `invalid_request`, one that does not exist `not_found`. A profile
    /// with `can_approve_hires` true is accepted from the app only (`not_authorized` otherwise). The
    /// profile's `runtime` must be a known value (`Unknown` and a non-canonical `Other` such as
    /// `Other("codex")` are `invalid_request`), its `status` must be `active` or `paused`
    /// (`Unknown` and `offboarded` are refused: an agent is offboarded with `member.offboard`),
    /// and `offboarded_at` must be absent. If `run_mode` is `scheduled` a `schedule` is required,
    /// and otherwise there must be none. Errors: `not_authorized`, `invalid_name` (the name, or a
    /// `messaging_name` that breaks its format or [`is_shaped_like_an_address`]),
    /// `invalid_request` (a profile that breaks its limits or rules above, or a `member_id` that
    /// is not [`is_valid_member_id`], or a `hire_proposal_id` whose proposal is not approved or does
    /// not match), `already_exists` (the id is taken, or `messaging_name` is
    /// already another member's handle: an agent's `messaging_name`, a person's `name`,
    /// [`OWNER_HANDLE`] or [`OFFICE_HANDLE`], ignoring case), `not_found` (no such office).
    MemberAddAgent, "member.add_agent", MemberAddAgentParams {
        member_id: String,
        name: String,
        office_id: String,
        profile: AgentProfile,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hire_proposal_id: Option<String>,
    }
);
command!(
    /// `member.update_agent`: only the fields given change; `clear_focus`, `clear_home` and
    /// `clear_model` remove those fields (giving a field and its `clear_` flag together is
    /// `invalid_request`; so is a call that gives no field at all, though a value equal to the
    /// current one is accepted, and so is a `runtime` that is `Unknown` or a non-canonical
    /// `Other`). `role` and `title` cannot be cleared. `office_id` does not change here: moving
    /// an agent to another office needs its own command (a future `member.place_agent`; ADR-0003
    /// and the offices design rely on such a move). The run mode is changed with
    /// `member.set_run_mode`, the status with `member.set_agent_status` and `member.offboard`. An
    /// offboarded agent is not edited at all (`invalid_request`): its record, handle included, is
    /// history. Errors: `not_authorized`, `invalid_name` (the name, or a `messaging_name` that
    /// breaks its format or [`is_shaped_like_an_address`]), `already_exists` (`messaging_name`
    /// is already another member's handle, as for
    /// `member.add_agent`), `conflict`, `not_found`, and `invalid_request` if the member is not
    /// an agent (people are edited with `member.update_person`), is offboarded, or a value breaks
    /// its limits. On the built-in HR lead only `runtime` and `model` may be given; any other
    /// field is `builtin_protected`. `can_approve_hires` is never changed here
    /// (`member.set_can_approve_hires`).
    MemberUpdateAgent, "member.update_agent", MemberUpdateAgentParams {
        member_id: String,
        expected_version: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        focus: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        clear_focus: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        runtime: Option<AgentRuntime>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        clear_model: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        home: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        clear_home: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        messaging_name: Option<String>,
    }
);
command!(
    /// `member.set_agent_status`: pause or resume an agent. `status` must be `active` or
    /// `paused`; setting the status it already has is not an error. Anything else is
    /// `invalid_request`: `offboarded` (use `member.offboard`), `Unknown`, a member that is not
    /// an agent, and an agent that is already offboarded. Errors: `not_authorized`,
    /// `builtin_protected` (the HR lead is never paused), `not_found`,
    /// `invalid_request`, `conflict`.
    MemberSetAgentStatus, "member.set_agent_status", MemberSetAgentStatusParams {
        member_id: String,
        expected_version: u64,
        status: AgentStatus,
    }
);
command!(
    /// `member.offboard`: an agent leaves the organization. App-only (see "Who may send what").
    /// Offboarding is a STATE, not a deletion: HQ sets the agent's `status` to `offboarded` and
    /// `offboarded_at` to its own clock, and the record and its handle (`messaging_name`) stay.
    /// The handle stays reserved PERMANENTLY: no command frees it, so history addressed to it can
    /// never meet a different agent. Live relations end, as in `member.remove`, in the same
    /// `org.changed`: the agent's memberships are removed, the teams it led lose their lead
    /// (`team_upserted`), and whoever reported to it now reports to the agent's own manager, or
    /// to the owner if it had none (`member_upserted`); each affected team and member gets a new
    /// version. Existing historical references (authorship, activity) keep pointing at the agent.
    /// Afterwards `team.add_member`, `team.set_lead` and `member.set_reports_to` that name it are
    /// `invalid_request`. Reclaiming the agent's open tasks is part of the work-items design.
    /// Errors: `not_authorized`, `builtin_protected` (the HR lead is never offboarded),
    /// `not_found`, `invalid_request` (the member is not an agent, or
    /// is already offboarded), `conflict`.
    MemberOffboard, "member.offboard", MemberOffboardParams {
        member_id: String,
        expected_version: u64,
    }
);
command!(
    /// `member.set_run_mode`: how an agent is run, and when if it is scheduled (the built-in HR
    /// lead's too: a run mode is one of the three things the owner may change on it). App-only (see
    /// "Who may send what"). `run_mode` is `always_on`, `on_call`, `scheduled`, or left out: left
    /// out means "not office-run", the agent is started by a person (this is how an agent is
    /// taken out of the office's control). `Unknown` is `invalid_request`. `schedule` is required
    /// for `scheduled` and must be absent otherwise (`invalid_request`); its text follows the
    /// [`Schedule`] rules. Setting the mode an agent already has is accepted. Errors:
    /// `not_authorized`, `not_found`, `invalid_request` (the member is not an agent, is
    /// offboarded, or a value breaks the rules above), `conflict`.
    MemberSetRunMode, "member.set_run_mode", MemberSetRunModeParams {
        member_id: String,
        expected_version: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_mode: Option<RunMode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        schedule: Option<Schedule>,
    }
);
command!(
    /// `member.set_can_approve_hires`: the owner lets an agent approve hire proposals (`value`
    /// true) or stops it. App-only, and only the owner's connection: an agent can never set it,
    /// on itself or anyone. Setting the value the agent already has is accepted. Errors:
    /// `not_authorized`, `not_found`, `invalid_request` (the member is not an agent, or is
    /// offboarded), `builtin_protected` (the HR lead never approves hires), `conflict`.
    MemberSetCanApproveHires, "member.set_can_approve_hires", MemberSetCanApproveHiresParams {
        member_id: String,
        expected_version: u64,
        value: bool,
    }
);
command!(
    /// `member.move`: moves a member from one team to another in one step. It ends the membership
    /// `membership_id` and starts `new_membership_id` (client-made) in `to_team_id`, as ONE
    /// `org.changed` whose changes are a `membership_removed` and a `membership_upserted`, plus a
    /// `team_upserted` for the old team if the member led it (as in `team.remove_member`, leaving
    /// clears the lead and raises that team's version). `title_in_team` is the new membership's
    /// title; the old title is not carried over. `expected_version` is the version of the
    /// membership `membership_id`. Errors: `not_found` (no such membership or team), `conflict`,
    /// `already_exists` (the member is already in `to_team_id`, which includes moving to the same
    /// team, or `new_membership_id` is taken). The owner is in no team, so it has no membership
    /// to move.
    MemberMove, "member.move", MemberMoveParams {
        membership_id: String,
        expected_version: u64,
        new_membership_id: String,
        to_team_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title_in_team: Option<String>,
    }
);
command!(
    /// `project.add_team`. `project_team_id` is client-made. Errors: `already_exists`,
    /// `not_found`.
    ProjectAddTeam, "project.add_team", ProjectAddTeamParams {
        project_team_id: String,
        project_id: String,
        team_id: String,
    }
);
command!(
    /// `project.remove_team`. A project with no team left is unassigned. Errors:
    /// `conflict`, `not_found`.
    ProjectRemoveTeam, "project.remove_team", ProjectRemoveTeamParams {
        project_team_id: String,
        expected_version: u64,
    }
);

/// Where a hire proposal is. A state this build doesn't know decodes as [`HireState::Unknown`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum HireState {
    /// Being drafted by the HR lead; not yet sent to an approver.
    Draft,
    /// Waiting for its approver (`approver_id`).
    Waiting,
    /// Approved; the HR lead provisions the agent next.
    Approved,
    /// Rejected by the approver or the owner.
    Rejected,
    /// The agent exists (`provisioned_member_id`).
    Provisioned,
    /// Provisioning failed (`failure`).
    Failed,
    /// Missing or not known to this build.
    #[default]
    Unknown,
}

impl<'de> Deserialize<'de> for HireState {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(d)?;
        Ok(match value.as_str() {
            Some("draft") => HireState::Draft,
            Some("waiting") => HireState::Waiting,
            Some("approved") => HireState::Approved,
            Some("rejected") => HireState::Rejected,
            Some("provisioned") => HireState::Provisioned,
            Some("failed") => HireState::Failed,
            _ => HireState::Unknown,
        })
    }
}

/// What a hire would create: the facts the approver sees, and that `member.add_agent` must match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HireFacts {
    /// The new agent's display name.
    pub name: String,
    pub role: String,
    pub title: String,
    /// The team it joins, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    /// Who it reports to, if not the owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reports_to_member_id: Option<String>,
    pub runtime: AgentRuntime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// How it will run; absent means "not office-run".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_mode: Option<RunMode>,
    /// The estimated cost per month at API prices, in cents. The approver sees it; HQ does not
    /// check it against anything.
    #[serde(default)]
    pub est_cost_cents_month: u64,
}

/// A request to hire an agent, and what became of it. Organization data (HQ's), carried in
/// `org.changed` as [`OrgChange::HireProposalUpserted`] and in [`OrgSnapshot`].
///
/// **Routing.** HQ sets `approver_id` when the request is made: the nearest manager up the
/// requester's reporting line (the requester's own manager first, never the requester) that is an
/// agent with `can_approve_hires`, not offboarded and not built in; if there is none, the owner.
/// A request by the owner is approved at once (`decided_by` the owner, no reason). The owner can
/// approve or reject any proposal at any time. The HR lead never approves, and the requester
/// never approves their own request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HireProposal {
    pub id: String,
    /// Who asked: a member id (an agent, or the owner).
    pub requester_id: String,
    /// Who it waits for (the owner's id when it comes to the owner). Empty while a draft.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approver_id: Option<String>,
    pub state: HireState,
    pub facts: HireFacts,
    /// Why the requester wants it, in their words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Who approved or rejected it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
    /// The reason given with the decision (required for an approval or rejection by someone
    /// other than the owner acting on its own request).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_reason: Option<String>,
    /// The agent that was made, once `provisioned`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provisioned_member_id: Option<String>,
    /// Why provisioning failed, once `failed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    /// RFC 3339 UTC with milliseconds, HQ's clock.
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<String>,
    pub version: u64,
}

command!(
    /// `hire.request`: asks to hire an agent. `proposal_id` is client-made ([`is_valid_id`]).
    /// The requester is the caller (the connection's bound member: an agent, or the owner through
    /// the app); the built-in HR lead may name another with `on_behalf_of` (a member id) when it
    /// drafts a request an agent made in words, and nobody else may (`not_authorized`). HQ
    /// creates the proposal `waiting` for its routed approver (see [`HireProposal`]), or
    /// `approved` at once when the owner asks. `facts` are checked like `member.add_agent`'s
    /// profile (`invalid_name` for the name, `invalid_request` for limits and `Unknown` values),
    /// `team_id` and `reports_to_member_id` must exist (`not_found`) and not be the built-in HR
    /// lead (`builtin_protected`). Errors: `not_authorized`, `not_found`, `invalid_request`,
    /// `invalid_name`, `builtin_protected`, `already_exists` (the proposal id is taken).
    HireRequest, "hire.request", HireRequestParams {
        proposal_id: String,
        facts: HireFacts,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        on_behalf_of: Option<String>,
    }
);
command!(
    /// `hire.approve`: approves a `waiting` proposal. Only its routed approver (`approver_id`) or
    /// the owner may (`not_authorized`); never the requester (the owner acting on its own request
    /// excepted: that is approved at once by `hire.request`), never the HR lead. `reason` is
    /// required ([`AGENT_TEXT_MAX_CHARS`], trimmed, no control characters). A proposal that is
    /// not `waiting` is `invalid_request`. Errors: `not_authorized`, `not_found`,
    /// `invalid_request`, `conflict`.
    HireApprove, "hire.approve", HireApproveParams {
        proposal_id: String,
        expected_version: u64,
        reason: String,
    }
);
command!(
    /// `hire.reject`: rejects a `waiting` proposal, under the same rules as `hire.approve`.
    /// Errors: `not_authorized`, `not_found`, `invalid_request`, `conflict`.
    HireReject, "hire.reject", HireRejectParams {
        proposal_id: String,
        expected_version: u64,
        reason: String,
    }
);
command!(
    /// `hire.fail`: the HR lead says provisioning an `approved` proposal failed, with `reason`
    /// (the proposal becomes `failed`). Only the built-in HR lead's connection may
    /// (`not_authorized`). Errors: `not_authorized`, `not_found`, `invalid_request` (not
    /// `approved`, or a bad reason), `conflict`.
    HireFail, "hire.fail", HireFailParams {
        proposal_id: String,
        expected_version: u64,
        reason: String,
    }
);

/// One change to the organization's structure, as committed by HQ. Entities are sent whole,
/// as they are after the change. Kinds this build doesn't know decode as
/// [`OrgChange::Unknown`]; a client that sees one should take a fresh snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
// A member carries an agent profile, so `MemberUpserted` is much larger than the others. These
// are short-lived wire values; boxing would change the public type for no practical gain.
#[allow(clippy::large_enum_variant)]
pub enum OrgChange {
    TeamUpserted {
        team: Team,
    },
    TeamDeleted {
        team_id: String,
    },
    MemberUpserted {
        member: Member,
    },
    MemberRemoved {
        member_id: String,
    },
    MembershipUpserted {
        membership: Membership,
    },
    MembershipRemoved {
        membership_id: String,
    },
    ProjectTeamAdded {
        project_team: ProjectTeam,
    },
    ProjectTeamRemoved {
        project_team_id: String,
    },
    HireProposalUpserted {
        proposal: HireProposal,
    },
    #[serde(other)]
    Unknown,
}

/// The data of an `org.changed` event: the changes one command made, at `position`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgChangedData {
    pub position: Position,
    /// The command that made these changes, so a client can match a queued command to its
    /// commit.
    pub command_id: String,
    pub changes: Vec<OrgChange>,
}

/// `org.changed`: sent to subscribers for every committed organization command.
pub struct OrgChanged;

impl EventKind for OrgChanged {
    const NAME: &'static str = "org.changed";
    type Data = OrgChangedData;
}

/// The data of an `org.rejected` event: HQ rejected a command that was queued while it was
/// unreachable. Rejections are recorded like commits (offices design), so every office sees
/// them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgRejectedData {
    pub position: Position,
    pub command_id: String,
    pub error: crate::ApiError,
}

/// `org.rejected`: sent to subscribers when HQ rejects a command.
pub struct OrgRejected;

impl EventKind for OrgRejected {
    const NAME: &'static str = "org.rejected";
    type Data = OrgRejectedData;
}
