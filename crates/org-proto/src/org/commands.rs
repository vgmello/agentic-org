//! The organization commands and their answer, [`CommandAccepted`].

use serde::{Deserialize, Serialize};

use super::*;
use crate::calls::Call;

/// What happened to an organization command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CommandStatus {
    /// HQ committed it; `position` says where.
    Committed,
    /// Dry-run validation succeeded without persisting any change; `position` is `None`.
    Checked,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        handle: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        home_team_id: Option<String>,
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
        #[serde(default, skip_serializing_if = "String::is_empty")]
        general_area_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        general_area_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        repo: Option<RepoSpec>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        integration_branch: Option<String>,
        /// Canonical lowercase project slug (B3). Derived from `name` by HQ when omitted.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        slug: Option<String>,
        /// Existing active team accountable for the project (B3). When omitted, HQ creates a
        /// new owning team named after the project under `parent_team_id` (default `TEAM-1`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner_team_id: Option<String>,
        /// Caller-minted UUIDv7 for the new owning team when `owner_team_id` is omitted (B3).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        create_team_id: Option<String>,
        /// Optional parent team for the newly created owning team (defaults to `TEAM-1`, B3).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_team_id: Option<String>,
        /// Optional contributing teams (active teams distinct from the owning team, B3).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        contributing_team_ids: Vec<String>,
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
