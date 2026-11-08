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
    /// `member.add_person`. `member_id` is client-made. Agents join through hiring instead.
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
    /// `not_found`, and `invalid_request` if the member is an agent (agents are edited
    /// through hiring) or if both `contact` and `clear_contact` are given.
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
