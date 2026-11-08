//! The organization's structure: teams, members, memberships and the teams working on each
//! project (see `docs/design/teams-and-org-chart.md`).
//!
//! Organization data is owned by HQ and copied to every office (offices design). Clients read
//! it from their local office with [`OrgSnapshotCall`], then follow changes with
//! `subscribe { from_position }` and the [`OrgChanged`] event (see [`crate::SubscribeParams`]). Every change is a command that
//! HQ validates. A command carries a client-made `command_id` (a ULID), client-made ids for
//! anything it creates, and the `expected_version` of what it edits.

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
            _ => AgentStatus::Unknown,
        })
    }
}

/// The longest `messaging_name`, in characters.
pub const AGENT_MESSAGING_NAME_MAX_CHARS: usize = 64;

/// The longest `role`, `title`, `focus` and `Other` runtime name, in characters (after
/// trimming).
pub const AGENT_TEXT_MAX_CHARS: usize = 200;

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
    /// Where the agent's files are: an absolute path on its office's machine, at most
    /// [`AGENT_HOME_MAX_BYTES`] bytes, with no control characters (NUL included) and no `..`
    /// component; HQ refuses anything else with `invalid_request`. Clients show it and never
    /// open or interpret it. **It is visible organization-wide**: it is copied to every office
    /// and shown to every client, so it can reveal a user name and folder layout to remote
    /// offices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
    /// The handle this agent is addressed by. 1 to
    /// [`AGENT_MESSAGING_NAME_MAX_CHARS`] ASCII letters, digits, `-`, `_` and `.`, starting
    /// with a letter or digit. Unique among the organization's agents **ignoring case**
    /// (`already_exists` on a clash; a removed agent frees its name, and renaming an agent to
    /// a different capitalisation of its own name is fine). A name that breaks the format is
    /// `invalid_name`.
    pub messaging_name: String,
    /// Whether the agent takes work. Required when adding an agent (`Unknown` is refused).
    #[serde(default)]
    pub status: AgentStatus,
}

/// A member of the organization: an agent or a person. The owner is a person with
/// `owner: true`: in no team, full rights everywhere, always reachable by agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
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
            /// Client-made (a ULID); a retry with the same id gets the same answer.
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
    /// `owner_protected`, `conflict`, `not_found`.
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
    /// `team.add_member`. `membership_id` is client-made. Errors: `already_exists`,
    /// `owner_protected`, `not_found`.
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
    /// `member.add_person`. `member_id` is client-made. Agents are added with
    /// `member.add_agent` instead.
    /// Errors: `invalid_name`, `already_exists` (the id is taken).
    MemberAddPerson, "member.add_person", MemberAddPersonParams {
        member_id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        contact: Option<String>,
    }
);
command!(
    /// `member.update_person`: only the fields given change; `clear_contact` removes the
    /// contact. The owner can edit its own record. Errors: `invalid_name`, `conflict`,
    /// `not_found`, and `invalid_request` if the member is an agent (agents are edited with
    /// `member.update_agent`) or if both `contact` and `clear_contact` are given.
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
    /// `member.remove`: anyone but the owner. Removes their memberships, clears them as lead,
    /// and makes whoever reported to them report to the owner. Errors: `owner_protected`,
    /// `conflict`, `not_found`.
    MemberRemove, "member.remove", MemberRemoveParams {
        member_id: String,
        expected_version: u64,
    }
);
command!(
    /// `member.set_reports_to`: who a member reports to, or none (the owner). Errors:
    /// `cycle` (someone would report to themselves, directly or through others),
    /// `owner_protected` (the owner reports to no one), `conflict`, `not_found`.
    MemberSetReportsTo, "member.set_reports_to", MemberSetReportsToParams {
        member_id: String,
        expected_version: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reports_to_member_id: Option<String>,
    }
);
command!(
    /// `member.add_agent`: adds an agent, placed in `office_id`. `member_id` is client-made.
    /// `profile.runtime` and `profile.status` must be known values (`Unknown` and a
    /// non-canonical `Other` such as `Other("codex")` are `invalid_request`).
    /// Errors: `invalid_name` (the name, or a `messaging_name` that breaks its format),
    /// `invalid_request` (a profile that breaks its limits), `already_exists` (the id is
    /// taken, or `messaging_name` is another agent's, ignoring case), `not_found` (no such
    /// office).
    MemberAddAgent, "member.add_agent", MemberAddAgentParams {
        member_id: String,
        name: String,
        office_id: String,
        profile: AgentProfile,
    }
);
command!(
    /// `member.update_agent`: only the fields given change; `clear_focus` and `clear_home`
    /// remove those fields (giving a field and its `clear_` flag together is
    /// `invalid_request`; so is a call that gives no field at all, though a value equal to the
    /// current one is accepted, and so is a `runtime` that is `Unknown` or a non-canonical
    /// `Other`). `role` and `title` cannot be cleared. `office_id` does
    /// not change here: moving an agent to another office needs its own command (a future
    /// `member.place_agent`; ADR-0003 and the offices design rely on such a move). Errors: `invalid_name` (the name or a `messaging_name` that breaks its
    /// format), `already_exists` (`messaging_name` is another agent's, ignoring case),
    /// `conflict`, `not_found`, and `invalid_request` if the member is not an agent (people
    /// are edited with `member.update_person`) or a value breaks its limits.
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
        home: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        clear_home: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        messaging_name: Option<String>,
    }
);
command!(
    /// `member.set_agent_status`: pause or resume an agent. `status` must be `active` or
    /// `paused`; setting the status it already has is not an error. Errors: `conflict`, `not_found`, and `invalid_request` if the member is not
    /// an agent or `status` is `Unknown`.
    MemberSetAgentStatus, "member.set_agent_status", MemberSetAgentStatusParams {
        member_id: String,
        expected_version: u64,
        status: AgentStatus,
    }
);
command!(
    /// `member.move`: moves a member from one team to another in one step. It ends the
    /// membership `membership_id` and starts `new_membership_id` (client-made) in
    /// `to_team_id`, as ONE `org.changed` whose changes are a `membership_removed` and a
    /// `membership_upserted`, plus a `team_upserted` for the old team if the member led it
    /// (as in `team.remove_member`, leaving clears the lead and raises that team's version).
    /// `title_in_team` is the new membership's title; the old title is not carried over.
    /// `expected_version` is the version of the membership `membership_id`. When several
    /// errors apply, the first of `not_found`, `conflict`, `already_exists` is returned.
    /// Errors: `already_exists` (the member is already in `to_team_id`, which includes moving
    /// to the same team, or `new_membership_id` is taken), `conflict`, `not_found` (no such
    /// membership or team). The owner is in no team, so it has no membership to move.
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

/// One change to the organization's structure, as committed by HQ. Entities are sent whole,
/// as they are after the change. Kinds this build doesn't know decode as
/// [`OrgChange::Unknown`]; a client that sees one should take a fresh snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
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
