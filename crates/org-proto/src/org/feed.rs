//! The organization's feed: `org.snapshot`, and the `org.changed` and `org.rejected`
//! events.

use serde::{Deserialize, Serialize};

use super::projects::{Area, Project, ProjectTeam};
use super::structure::{Member, Position, Team};
use crate::call::{Call, Empty};
use crate::events::EventKind;

/// Everything the org chart needs, as of `position`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgSnapshot {
    pub position: Position,
    pub teams: Vec<Team>,
    pub members: Vec<Member>,
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default)]
    pub project_teams: Vec<ProjectTeam>,
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
        member: Box<Member>,
    },
    MemberRemoved {
        member_id: String,
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
    /// Who sent the command (X-02 §4.2). Absent from servers that predate it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<crate::principal::Actor>,
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
