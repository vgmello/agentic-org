//! The organization's structure: teams, members, memberships, projects, the teams working on
//! each project, and each project's areas (see `docs/design/teams-and-org-chart.md` and
//! `docs/design/tasks-projects-docs.md` §1.9, §2.5 and §4.8).
//!
//! Organization data is owned by HQ and copied to every office (offices design). Clients read
//! it from their local office with [`OrgSnapshotCall`], then follow changes with
//! `subscribe { from_position }` and the [`OrgChanged`] event (see [`crate::SubscribeParams`]). Every change is a command that
//! HQ validates. A command carries a client-made `command_id` (an uppercase Crockford ULID, see
//! "Command ids and retries"), client-made ids for anything it creates, and the
//! `expected_version` of what it edits.
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
//! ## Command ids and retries
//! `command_id` is a ULID in its canonical form: exactly 26 **uppercase** Crockford base32
//! characters (`0-9`, `A-Z` without `I`, `L`, `O`, `U`), the first `0` to `7`. Its time part
//! dates the command. Work writes' `change_id` ([`crate::work`]) has the same form.
//!
//! HQ keeps each command's answer, refusals included, until the id's time plus 30 days (offices
//! design, "Dedup window"). A retry with the same `command_id` and the same params gets the
//! stored answer again, without being validated again, even after the state changed. The same
//! `command_id` with different params is refused with `invalid_request` ("command id reused
//! with different params"). Never kept, so a later retry may succeed: the refusals that come
//! before HQ looks at the command itself (`not_authorized`, params that don't decode, the id's
//! format, `expired`, `clock_skew`; see below), `storage_error` and `unavailable`. To try again
//! after any other refusal, use a new `command_id`.
//!
//! ## When several errors apply
//! A command can break more than one rule at once. The answer is the first that applies, in
//! this order:
//! 1. **The receiving office**, before it queues or forwards the command: `not_authorized`
//!    (its check of the connection, "Who may send what"), then `invalid_request` for params
//!    that don't decode into the command.
//! 2. **The stored answer.** HQ looks `command_id` up first: a command it has answered gets
//!    that answer again (or `invalid_request` for different params, above), even when a check
//!    below would now refuse it (a stored command's retry 29.5 days on is still answered).
//! 3. **The `command_id` format**: not a canonical ULID (above) is `invalid_request`.
//! 4. **Age and skew**, by HQ's clock (it keeps a monotonic high-water mark of it, so a clock
//!    that steps back can't reopen the window): `expired` when the id is dated more than 29
//!    days ago, `clock_skew` when it's more than 5 minutes ahead.
//! 5. **The rules**: `not_found`, `invalid_request` (outside a limit, the wrong kind of member
//!    or the wrong state, a value that makes no sense), `invalid_name`, `conflict` (a stale
//!    `expected_version`), `already_exists`.
//!
//! Every command below checks in this one order (its `Errors:` list names the codes it can return,
//! not the order they are checked in). Within step 5, the domain codes (`owner_protected`, `cycle`,
//! `not_a_member`, `not_empty`) come after `invalid_name` and before `conflict`: a rule that would
//! refuse the command at any version is not hidden by a stale one. Among themselves
//! `owner_protected` comes first, so a lead who is the owner is `owner_protected`, not
//! `not_a_member`.
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
//! - **Command ids** (`command_id`, on every command): an uppercase Crockford ULID, 26
//!   characters ("Command ids and retries"); its format is checked after the stored-answer
//!   lookup ("When several errors apply").
//! - **Free text** (`contact`, `title_in_team`): trimmed, then 1 to [`AGENT_TEXT_MAX_CHARS`]
//!   characters with no control characters. To remove a contact use `clear_contact`, not an
//!   empty string.
//! - **An edit that changes nothing still counts.** An update that sets a field to the value it
//!   has (a rename to the same name, a lead set to the current lead), or a
//!   `member.update_person` that gives no field, is accepted and raises the entity's version by
//!   one (and so produces an `org.changed`). Only `member.update_agent` refuses a call that
//!   gives no field. This is about edits: asking for something that already exists (adding a
//!   member to a team they are in, `member.move` to the member's own team) is `already_exists`.
//!
//! ## Projects and areas
//! A project ([`Project`]) has a key that never changes, an optional repository, and areas
//! ([`Area`]). `project.create` makes its **General** area and its project-team links in one
//! `org.changed`; General can be renamed and moved to another team but never deleted. An
//! area's team is always one of its project's teams, or none: while the project has no team,
//! after its team left as the project's last one, or after its team was deleted (the app then
//! asks for one). HQ keeps that true inside the organization event that would break it:
//! - an area lead who leaves the area's team (`team.remove_member`, `member.move`,
//!   `member.offboard`, `member.remove`) stops being its lead, and the area's version goes up;
//! - `project.remove_team` moves the leaving team's areas to `areas_to_team_id`, or leaves
//!   them team-less when it was the project's last team (see [`ProjectRemoveTeam`]);
//! - `team.delete` leaves the areas it owned team-less, with their leads cleared;
//! - `area.set_team` clears a lead who isn't in the new team.
//!
//! Work items (`crate::work`) are office data and follow these changes afterwards, as their own
//! work events (e.g. tasks of a project lose a `team_id` that is no longer one of its teams).

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
    /// members. `Member.name` stays the agent's display name and is not a handle. Two reserved
    /// words, [`OWNER_HANDLE`] and [`OFFICE_HANDLE`], clash with everyone, and a handle that
    /// [`is_shaped_like_an_address`] is `invalid_name`. **The owner's handle is only `owner`:
    /// its display name is exempt from the namespace** (no clash and no address-shape check).
    /// `member.update_person` on the owner never clashes, and `member.add_person` may reuse the
    /// owner's display name.
    pub messaging_name: String,
    /// Whether the agent takes work. Required when adding an agent (`Unknown` and
    /// `Offboarded` are refused: an agent is offboarded with `member.offboard`).
    #[serde(default)]
    pub status: AgentStatus,
    /// When HQ offboarded the agent (RFC 3339 UTC with milliseconds, HQ's clock). HQ keeps it
    /// present exactly when `status` is `offboarded`. Only `member.offboard` sets it: a client
    /// that sends it in a profile (`member.add_agent`) gets `invalid_request`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offboarded_at: Option<String>,
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

/// The longest project key, in characters. A key is 2 to this many lowercase ASCII letters and
/// digits, starting with a letter.
pub const PROJECT_KEY_MAX_CHARS: usize = 10;

/// Project keys no project may have: they would read as another kind of reference
/// (`REQ-14`, `FEAT-0007`, `ADR-0004`, …).
pub const RESERVED_PROJECT_KEYS: &[&str] = &["req", "feat", "adr", "eval", "debt", "lim", "exc"];

/// The longest project `description`, in characters.
pub const PROJECT_DESCRIPTION_MAX_CHARS: usize = 1_000;

/// The longest area name, in characters (after trimming).
pub const AREA_NAME_MAX_CHARS: usize = 40;

/// The branch-name prefixes the office makes and owns: task branches (`task/…`), docs
/// branches (`docs/…`) and the tags it leaves after a merge or an archive (`merged/…`,
/// `archive/…`). A project's `integration_branch` may not start with one
/// ([`has_reserved_branch_prefix`]).
pub const RESERVED_BRANCH_PREFIXES: &[&str] = &["task/", "docs/", "merged/", "archive/"];

/// Whether `name` starts with one of [`RESERVED_BRANCH_PREFIXES`], so it can't be a project's
/// `integration_branch`: `project.create` and `project.update` refuse it with
/// `invalid_request` (docs/design/processes.md, "Project rule"), the one rule the office and
/// the app's mocks are to share.
///
/// The office reads the integration branch as the branch `refs/heads/<name>`, never through
/// git's short-name lookup, so `name` can't reach a tag or another ref. As defence in depth,
/// this is stricter than a plain prefix test: everything is compared ignoring ASCII case (refs
/// can live on a case-insensitive file system), and one leading `refs/heads/` or `heads/` is
/// looked through.
pub fn has_reserved_branch_prefix(name: &str) -> bool {
    let name = ["refs/heads/", "heads/"]
        .iter()
        .find_map(|lead| strip_prefix_ignoring_ascii_case(name, lead))
        .unwrap_or(name);
    RESERVED_BRANCH_PREFIXES
        .iter()
        .any(|prefix| strip_prefix_ignoring_ascii_case(name, prefix).is_some())
}

/// `name` after `prefix`, when it starts with it ignoring ASCII case.
fn strip_prefix_ignoring_ascii_case<'a>(name: &'a str, prefix: &str) -> Option<&'a str> {
    let n = prefix.len();
    (name.len() >= n && name.is_char_boundary(n) && name[..n].eq_ignore_ascii_case(prefix))
        .then(|| &name[n..])
}

/// A project: the full record. It replaced `ProjectSummary` (`{ id, name }`) additively: every
/// other field has a default, so a record from before reads with an empty `key`, no repository,
/// not archived and `version` 0. [`ProjectSummary`] is kept as another name for it.
///
/// `#[non_exhaustive]`: fields may be added. Code outside this crate builds one with
/// [`Project::new`] and sets the fields it needs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Project {
    pub id: String,
    /// The short key used in references and branches (`chk` in `chk-T-12`): 2 to
    /// [`PROJECT_KEY_MAX_CHARS`] lowercase ASCII letters and digits, starting with a letter, not
    /// one of [`RESERVED_PROJECT_KEYS`], unique in the organization. It never changes. Empty
    /// only on a record from before keys existed.
    #[serde(default)]
    pub key: String,
    /// Follows the organization-name rules; unique among projects, ignoring case.
    pub name: String,
    /// At most [`PROJECT_DESCRIPTION_MAX_CHARS`] characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The project's repository, as HQ resolved it from the [`RepoSpec`] it was created with.
    /// Absent: the project has no repository (its tasks and areas work; docs and worktrees
    /// don't).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<ProjectRepo>,
    /// The project's integration branch: task branches start from it, the office reads the
    /// project's instruction files, guidance files and processes from it (and the Docs tab its
    /// docs), and a task branch counts as merged once its work is reachable from it
    /// (docs/design/processes.md, A56 to A58). Absent: the repository's default branch.
    /// Never starts with a reserved prefix ([`has_reserved_branch_prefix`]): `project.create`
    /// and `project.update` refuse one with `invalid_request`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integration_branch: Option<String>,
    /// No new work items in it; everything else still works.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
    /// When HQ created it (RFC 3339 UTC with milliseconds, HQ's clock). Missing on records from
    /// before it existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// 0 only on a record from before projects had versions.
    #[serde(default)]
    pub version: u64,
}

impl Project {
    /// A project with this id and name and every other field at its default.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            ..Self::default()
        }
    }
}

/// The name the project record had before it became [`Project`]. Same type, same wire shape.
pub type ProjectSummary = Project;

/// Where a project's repository is hosted. A value this build doesn't know, in any shape,
/// decodes as [`RepoHost::Unknown`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RepoHost {
    /// A bare repository on HQ's Mac.
    Local,
    /// A GitHub repository.
    Github,
    /// Missing or not known to this build.
    #[default]
    Unknown,
}

impl<'de> Deserialize<'de> for RepoHost {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Like `AgentStatus`: reading never fails on a well-formed document.
        let value = serde_json::Value::deserialize(d)?;
        Ok(match value.as_str() {
            Some("local") => RepoHost::Local,
            Some("github") => RepoHost::Github,
            _ => RepoHost::Unknown,
        })
    }
}

/// A project's repository. It never holds a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRepo {
    #[serde(default)]
    pub host: RepoHost,
    pub default_branch: String,
    /// Local only: the bare repository's absolute path on HQ's Mac. **Visible
    /// organization-wide**, like an agent's `home`. Clients show it and never open it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// GitHub only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<GithubRepo>,
}

/// A GitHub repository, as a [`ProjectRepo`] names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubRepo {
    pub owner: String,
    pub name: String,
    pub repo_id: u64,
    #[serde(default)]
    pub private: bool,
}

/// How `project.create` gets its repository. HQ resolves it into the [`ProjectRepo`] the
/// event carries; a client never sends a `ProjectRepo`. On the wire `{"new_local": {}}` or
/// `{"existing_local": {"path": "<absolute path>"}}`. More kinds (GitHub) are added later, so
/// a server that doesn't know a kind refuses the command with `invalid_request`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RepoSpec {
    /// HQ creates a bare repository with an initial commit on `main` under its workspace,
    /// before the command (recording the intent first and reusing the folder on a retry).
    NewLocal {},
    /// An existing repository on HQ's Mac, checked before the command (it exists, is a git
    /// repository and has its default branch).
    ExistingLocal { path: String },
}

/// A part of a project, for routing bugs: one owning team, an optional lead. Areas are flat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Area {
    pub id: String,
    pub project_id: String,
    /// 1 to [`AREA_NAME_MAX_CHARS`] characters (organization-name rules), unique in its
    /// project among areas that aren't deleted, ignoring case.
    pub name: String,
    /// One of the project's teams. Empty only while the project has no team, or after its team
    /// left as the project's last one or was deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    /// A member of `team_id`; never the owner or an offboarded agent. Empty: the team's lead
    /// stands in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_member_id: Option<String>,
    /// The project's General area: exactly one per project, made by `project.create`, never
    /// deleted.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub general: bool,
    /// A tombstone: a deleted area stays so closed bugs can still show its name.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
    /// Set on a deleted area: where its open bugs went.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced_by: Option<String>,
    pub version: u64,
}

/// Everything the org chart needs, as of `position`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgSnapshot {
    pub position: Position,
    pub teams: Vec<Team>,
    pub members: Vec<Member>,
    pub memberships: Vec<Membership>,
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default)]
    pub project_teams: Vec<ProjectTeam>,
    /// Every project's areas, deleted ones (tombstones) included.
    #[serde(default)]
    pub areas: Vec<Area>,
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

/// `org.snapshot`: the organization's structure from the local office's copy. It needs a
/// bound connection, agent-bound or app-bound, like `who` and `subscribe`: an unbound
/// connection is refused with `not_authorized` (ADR-0003 §2.1.4).
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
            /// Client-made: an uppercase Crockford ULID, 26 characters (`invalid_request`
            /// otherwise). A retry with the same id and params gets the stored answer, refusals
            /// included; the same id with different params is `invalid_request`. See the module
            /// docs, "Command ids and retries".
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
    /// `owner_protected`, `conflict`, `not_found`, `invalid_request` (the member is an offboarded
    /// agent).
    TeamSetLead, "team.set_lead", TeamSetLeadParams {
        team_id: String,
        expected_version: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lead_member_id: Option<String>,
    }
);
command!(
    /// `team.delete`. The areas it owned (through its project links) become team-less, their
    /// leads cleared, in the same `org.changed` (`area_upserted`). Errors: `not_empty` (it owns
    /// teams), `conflict`, `not_found`.
    TeamDelete, "team.delete", TeamDeleteParams {
        team_id: String,
        expected_version: u64,
    }
);
command!(
    /// `team.add_member`. `membership_id` is client-made. Errors: `owner_protected`,
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
    /// makes whoever reported to them (offboarded agents included, since the record goes) report
    /// to the removed member's own manager, or to the owner if they had none. Unlike
    /// `member.offboard`, which leaves offboarded records as they are. It frees the member's handle, so it is
    /// for a member who has no history to keep: that is the CALLER'S duty (HQ sees the
    /// organization's structure, not messages or tasks, and cannot tell). What HQ does refuse is
    /// an offboarded agent, whose record and handle are kept for good (`invalid_request`);
    /// offboard an agent that has worked, remove one that never did. Errors: `not_authorized`,
    /// `owner_protected`, `not_found`, `invalid_request`, `conflict`.
    MemberRemove, "member.remove", MemberRemoveParams {
        member_id: String,
        expected_version: u64,
    }
);
command!(
    /// `member.set_reports_to`: who a member reports to, or none (the owner). Errors:
    /// `cycle` (someone would report to themselves, directly or through others),
    /// `owner_protected` (the owner reports to no one), `conflict`, `not_found`,
    /// `invalid_request` (either member is an offboarded agent).
    MemberSetReportsTo, "member.set_reports_to", MemberSetReportsToParams {
        member_id: String,
        expected_version: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reports_to_member_id: Option<String>,
    }
);
command!(
    /// `member.add_agent`: adds an agent, placed in `office_id`. `member_id` is client-made. The
    /// profile's `runtime` must be a known value (`Unknown` and a non-canonical `Other` such as
    /// `Other("codex")` are `invalid_request`), its `status` must be `active` or `paused`
    /// (`Unknown` and `offboarded` are refused: an agent is offboarded with `member.offboard`),
    /// and `offboarded_at` must be absent. If `run_mode` is `scheduled` a `schedule` is required,
    /// and otherwise there must be none. Errors: `not_authorized`, `invalid_name` (the name, or a
    /// `messaging_name` that breaks its format or [`is_shaped_like_an_address`]),
    /// `invalid_request` (a profile that breaks its limits or rules above, or a `member_id` that
    /// is not [`is_valid_member_id`]), `already_exists` (the id is taken, or `messaging_name` is
    /// already another member's handle: an agent's `messaging_name`, a person's `name`,
    /// [`OWNER_HANDLE`] or [`OFFICE_HANDLE`], ignoring case), `not_found` (no such office).
    /// An `office_id` that isn't a well-formed id (empty, or failing [`is_valid_id`]) is
    /// `invalid_request`, checked before the office is looked up.
    MemberAddAgent, "member.add_agent", MemberAddAgentParams {
        member_id: String,
        name: String,
        office_id: String,
        profile: AgentProfile,
    }
);
command!(
    /// `member.update_agent`: only the fields given change; `clear_focus`, `clear_home` and
    /// `clear_model` remove those fields (giving a field and its `clear_` flag together is
    /// `invalid_request`, and so is a call that gives no field at all; a value equal to the
    /// current one is accepted). A `runtime` that is `Unknown` or a non-canonical `Other` is
    /// refused with `invalid_request`, as in `member.add_agent`. `role` and `title` cannot be cleared. `office_id` does not change here: moving
    /// an agent to another office needs its own command (a future `member.place_agent`; ADR-0003
    /// and the offices design rely on such a move). The run mode is changed with
    /// `member.set_run_mode`, the status with `member.set_agent_status` and `member.offboard`. An
    /// offboarded agent is not edited at all (`invalid_request`): its record, handle included, is
    /// history. Errors: `not_authorized`, `invalid_name` (the name, or a `messaging_name` that
    /// breaks its format or [`is_shaped_like_an_address`]), `already_exists` (`messaging_name`
    /// is already another member's handle, as for
    /// `member.add_agent`), `conflict`, `not_found`, and `invalid_request` if the member is not
    /// an agent (people are edited with `member.update_person`), is offboarded, or a value breaks
    /// its limits.
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
    /// an agent, and an agent that is already offboarded. Errors: `not_authorized`, `not_found`,
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
    /// version. Only members that aren't offboarded are re-pointed: an already offboarded
    /// agent's record is history and keeps its `reports_to_member_id` and version. Existing historical references (authorship, activity) keep pointing at the agent.
    /// Afterwards `team.add_member`, `team.set_lead` and `member.set_reports_to` that name it are
    /// `invalid_request`. Reclaiming the agent's open tasks is part of the work-items design.
    /// Errors: `not_authorized`, `not_found`, `invalid_request` (the member is not an agent, or
    /// is already offboarded), `conflict`.
    MemberOffboard, "member.offboard", MemberOffboardParams {
        member_id: String,
        expected_version: u64,
    }
);
command!(
    /// `member.set_run_mode`: how an agent is run, and when if it is scheduled. App-only (see
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
    /// `project.remove_team`. A project with no team left is unassigned. If the leaving team
    /// owns areas of the project and the project keeps other teams, `areas_to_team_id` is
    /// required and must be one of them: those areas move to it, and their leads who aren't in
    /// it are cleared. If it was the project's last team, `areas_to_team_id` must be absent: the
    /// areas become team-less and their leads are cleared. Errors: `conflict`, `not_found`,
    /// `invalid_request` (`areas_to_team_id` missing, given when not allowed, or not a
    /// remaining team).
    ProjectRemoveTeam, "project.remove_team", ProjectRemoveTeamParams {
        project_team_id: String,
        expected_version: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        areas_to_team_id: Option<String>,
    }
);

/// A project-team link `project.create` makes: `project_team_id` is client-made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectCreateTeam {
    pub project_team_id: String,
    pub team_id: String,
}

command!(
    /// `project.create`: the project, its General area (`general_area_id`, client-made, named
    /// `general_area_name` or "General", owned by the first team in `teams` or by none) and its
    /// project-team links, in one `org.changed` (`project_upserted`, `area_upserted`,
    /// `project_team_added`). `project_id` and the link ids are client-made.
    ///
    /// `key` absent: HQ derives one from the name. Take its ASCII letters and digits,
    /// lowercased, and keep the first 3; then prefix `p` if that doesn't start with a letter
    /// (the prefix doesn't count toward the 3); then pad with `x` to 2 characters; if the key is
    /// taken or reserved, append a digit 2, 3, …. Examples: "Billing" → `bil`, "123 Project" →
    /// `p123`, "Q" → `qx`, "9" → `p9`, "Exc" → `exc2` (`exc` is reserved). The event carries
    /// the key.
    ///
    /// `repo` absent: the project has no repository. `integration_branch` may not start with a
    /// reserved prefix (`task/`, `docs/`, `merged/`, `archive/`, ignoring ASCII case;
    /// [`has_reserved_branch_prefix`]).
    /// App-only, like every command here. Errors: `not_found` (a team), `invalid_request` (key
    /// format or reserved; limits; a repeated team; the repo spec; an `integration_branch` with
    /// a reserved prefix), `invalid_name` (the name or the area name), `already_exists` (an id,
    /// the key, or the name ignoring case).
    ProjectCreate, "project.create", ProjectCreateParams {
        project_id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        teams: Vec<ProjectCreateTeam>,
        general_area_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        general_area_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        repo: Option<RepoSpec>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        integration_branch: Option<String>,
    }
);
command!(
    /// `project.update`: only the fields given change; a field and its `clear_` flag together,
    /// or nothing to change, is `invalid_request`. The key never changes, and the repository
    /// isn't changed here. `integration_branch` may not start with a reserved prefix (ignoring
    /// ASCII case; [`has_reserved_branch_prefix`]). Errors: `not_found`, `invalid_request` (also an
    /// `integration_branch` with a reserved prefix), `invalid_name`, `conflict`,
    /// `already_exists` (the name, ignoring case).
    ProjectUpdate, "project.update", ProjectUpdateParams {
        project_id: String,
        expected_version: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        clear_description: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        integration_branch: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        clear_integration_branch: bool,
    }
);
command!(
    /// `project.archive`: archive (`true`) or unarchive (`false`). An archived project takes no
    /// new work items; everything else still works. Errors: `not_found`, `conflict`.
    ProjectArchive, "project.archive", ProjectArchiveParams {
        project_id: String,
        expected_version: u64,
        archived: bool,
    }
);
command!(
    /// `area.create`. `area_id` is client-made. `team_id` is required when the project has
    /// teams, and must be one of them. Errors: `not_found`, `invalid_request` (the team isn't
    /// the project's, or is missing; the lead is an offboarded agent), `invalid_name`,
    /// `not_a_member` (the lead isn't in the team), `owner_protected` (the lead is the owner),
    /// `already_exists` (the id, or a live area of the project with that name, ignoring case).
    AreaCreate, "area.create", AreaCreateParams {
        area_id: String,
        project_id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        team_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lead_member_id: Option<String>,
    }
);
command!(
    /// `area.rename`. General can be renamed. Errors: `not_found`, `invalid_request` (the area
    /// is deleted), `invalid_name`, `conflict`, `already_exists`.
    AreaRename, "area.rename", AreaRenameParams {
        area_id: String,
        expected_version: u64,
        name: String,
    }
);
command!(
    /// `area.set_team`: one of the project's teams; absent only when the project has no team.
    /// Clears a lead who isn't in the new team. Errors: `not_found`, `invalid_request`,
    /// `conflict`.
    AreaSetTeam, "area.set_team", AreaSetTeamParams {
        area_id: String,
        expected_version: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        team_id: Option<String>,
    }
);
command!(
    /// `area.set_lead`: a member of the area's team, or none (the team's lead stands in).
    /// Errors: `not_found`, `invalid_request` (an offboarded agent), `not_a_member`,
    /// `owner_protected`, `conflict`.
    AreaSetLead, "area.set_lead", AreaSetLeadParams {
        area_id: String,
        expected_version: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lead_member_id: Option<String>,
    }
);
command!(
    /// `area.delete` (phase 2 of the work-items design: a server before it may answer
    /// `unknown_method`). The area becomes a tombstone (`deleted`, `replaced_by` =
    /// `move_open_to_area_id`); its open bugs move there afterwards, as work events. Errors:
    /// `not_found`, `invalid_request` (General; already deleted; the target is the area itself,
    /// deleted, or in another project), `conflict`.
    AreaDelete, "area.delete", AreaDeleteParams {
        area_id: String,
        expected_version: u64,
        move_open_to_area_id: String,
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
    /// A project was created or changed (archiving included).
    ProjectUpserted {
        project: Project,
    },
    /// An area was created or changed. A deleted area is an upsert of its tombstone.
    AreaUpserted {
        area: Area,
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
