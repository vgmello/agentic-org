//! The organization's structure: positions, teams, members, handles and ids, and agent
//! profiles.

use serde::{Deserialize, Serialize};

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
    Gemini,
    Kimi,
    Pi,
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
                "gemini" => AgentRuntime::Gemini,
                "kimi" => AgentRuntime::Kimi,
                "pi" => AgentRuntime::Pi,
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
/// (any case), or is exactly 26 ASCII letters and digits or a canonical UUIDv7 (the shapes of a
/// member id, [`is_valid_member_id`]; a lenient ULID decoder reads I, L, O and U too, so none is
/// excluded),
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

/// The length of a member id in its ULID form.
pub const MEMBER_ID_CHARS: usize = 26;

/// Whether `id` is a valid member id: exactly [`MEMBER_ID_CHARS`] ASCII letters and digits (the
/// shape of a ULID, which is what clients make), or a canonical lowercase UUIDv7
/// ([`crate::is_uuid_v7`]), the form the host mints for the person who creates an organization.
/// `member.add_person` and `member.add_agent` refuse anything else with `invalid_request`. No
/// handle has either shape ([`is_shaped_like_an_address`]), so an id never equals a handle, and
/// a lookup by id before handle cannot be captured by a name.
pub fn is_valid_member_id(id: &str) -> bool {
    (id.len() == MEMBER_ID_CHARS && id.bytes().all(|b| b.is_ascii_alphanumeric()))
        || crate::is_uuid_v7(id)
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
    /// The handle this agent is addressed by (`--to <handle>` on `agent-os`). 1 to
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

/// An agent's profile as a client sends it in `member.add_agent` (X-02 §4.2, §4.3): an
/// [`AgentProfile`] without `home`, which the office manages.
///
/// **It serializes byte-identically to [`AgentProfile`]** for every input it accepts: the same
/// fields in the same order, with the same `skip_serializing_if` rules, and `home` is never
/// written. HQ's command fingerprint hashes the serialized params, so a command stored before the
/// switch keeps its fingerprint. A present `home`, any value, `null` included, is refused while
/// it decodes ([`crate::retired::home`]), so a server answers `invalid_request`.
///
/// The field docs and limits are [`AgentProfile`]'s.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProfileInput {
    pub role: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    #[serde(default)]
    pub runtime: AgentRuntime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_mode: Option<RunMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<Schedule>,
    /// Retired: the office manages the home. Always `None`.
    #[serde(default, skip_serializing, deserialize_with = "crate::retired::home")]
    pub home: Option<crate::retired::Retired>,
    pub messaging_name: String,
    #[serde(default)]
    pub status: AgentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offboarded_at: Option<String>,
}

impl From<AgentProfileInput> for AgentProfile {
    /// The profile HQ stores for this input: `home` is absent.
    fn from(input: AgentProfileInput) -> Self {
        AgentProfile {
            role: input.role,
            title: input.title,
            focus: input.focus,
            runtime: input.runtime,
            model: input.model,
            run_mode: input.run_mode,
            schedule: input.schedule,
            home: None,
            messaging_name: input.messaging_name,
            status: input.status,
            offboarded_at: input.offboarded_at,
        }
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
    /// The member's unique lowercase handle (e.g. `hr`, `alice`), when provided by `org-store`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    /// The employee's single home team ID (Doc 01).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_team_id: Option<String>,
    /// The employee's optional referenced shared role ID (`roles/<id>.md`, Doc 04).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_id: Option<String>,
    /// The employee's optional title within its home team.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_in_team: Option<String>,
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
