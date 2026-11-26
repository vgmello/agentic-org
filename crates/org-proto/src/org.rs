//! The organization's structure: teams, members, projects, the teams working on
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
//! in on (a person acts through an owner grant on a host channel, X-02 §4.1), enforces this before it
//! queues or forwards the command, and HQ relies on that.
//!
//! ## Command ids and retries
//! `command_id` is a ULID in its canonical form: exactly 26 **uppercase** Crockford base32
//! characters (`0-9`, `A-Z` without `I`, `L`, `O`, `U`), the first `0` to `7`. Its time part
//! dates the command. Work writes' `change_id` ([`crate::work`]) has the same form. Until the
//! wire moves to UUIDv7, the office stores each command under a UUIDv7 that keeps all but the
//! ULID's lowest 6 random bits (`org_office::organization_family::operation_id`), so a client
//! must not use a monotonic ULID generator: two commands in one millisecond could map to one id,
//! and once the first is recorded the second is refused as a reused command id or, if it comes
//! from the same member with the same params, gets the first's stored answer without running.
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
//!   `project.add_team`'s `project_team_id`, and so on): 1 to [`ID_MAX_CHARS`] ASCII letters, digits, `-` or `_`
//!   ([`is_valid_id`]). A ULID fits. An id that names something that already exists is only
//!   looked up (`not_found`).
//! - **Member ids** (`member.add_person`'s and `member.add_agent`'s `member_id`): exactly
//!   [`MEMBER_ID_CHARS`] ASCII letters and digits, the shape of a ULID, or a canonical
//!   lowercase UUIDv7, the form of an organization creator's id ([`is_valid_member_id`]).
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
//! - an area lead who leaves the area's team (`member.move`,
//!   `member.offboard`, `member.remove`) stops being its lead, and the area's version goes up;
//! - `project.remove_team` moves the leaving team's areas to `areas_to_team_id`, or leaves
//!   them team-less when it was the project's last team (see [`ProjectRemoveTeam`]);
//! - `team.delete` leaves the areas it owned team-less, with their leads cleared;
//! - `area.set_team` clears a lead who isn't in the new team.
//!
//! Work items (`crate::work`) are office data and follow these changes afterwards, as their own
//! work events (e.g. tasks of a project lose a `team_id` that is no longer one of its teams).

mod commands;
mod feed;
mod projects;
mod structure;

pub use commands::{
    AreaCreate, AreaCreateParams, AreaDelete, AreaDeleteParams, AreaRename, AreaRenameParams,
    AreaSetLead, AreaSetLeadParams, AreaSetTeam, AreaSetTeamParams, CommandAccepted, CommandStatus,
    MemberAddAgent, MemberAddAgentParams, MemberAddPerson, MemberAddPersonParams, MemberMove,
    MemberMoveParams, MemberOffboard, MemberOffboardParams, MemberRemove, MemberRemoveParams,
    MemberSetAgentStatus, MemberSetAgentStatusParams, MemberSetReportsTo, MemberSetReportsToParams,
    MemberSetRunMode, MemberSetRunModeParams, MemberUpdateAgent, MemberUpdateAgentParams,
    MemberUpdatePerson, MemberUpdatePersonParams, ProjectAddTeam, ProjectAddTeamParams,
    ProjectArchive, ProjectArchiveParams, ProjectCreate, ProjectCreateParams, ProjectCreateTeam,
    ProjectRemoveTeam, ProjectRemoveTeamParams, ProjectUpdate, ProjectUpdateParams, TeamCreate,
    TeamCreateParams, TeamDelete, TeamDeleteParams, TeamMove, TeamMoveParams, TeamRename,
    TeamRenameParams, TeamSetLead, TeamSetLeadParams, TeamSetTitle, TeamSetTitleParams,
};
pub use feed::{
    OrgChange, OrgChanged, OrgChangedData, OrgRejected, OrgRejectedData, OrgSnapshot,
    OrgSnapshotCall,
};
pub use projects::{
    has_reserved_branch_prefix, Area, GithubRepo, Project, ProjectRepo, ProjectSummary,
    ProjectTeam, RepoHost, RepoSpec, AREA_NAME_MAX_CHARS, PROJECT_DESCRIPTION_MAX_CHARS,
    PROJECT_KEY_MAX_CHARS, RESERVED_BRANCH_PREFIXES, RESERVED_PROJECT_KEYS,
};
pub use structure::{
    is_shaped_like_an_address, is_valid_id, is_valid_member_id, AgentProfile, AgentProfileInput,
    AgentRuntime, AgentStatus, Member, MemberKind, Position, RunMode, Schedule, Team,
    AGENT_HOME_MAX_BYTES, AGENT_MESSAGING_NAME_MAX_CHARS, AGENT_TEXT_MAX_CHARS,
    AGENT_TIME_ZONE_MAX_CHARS, ID_MAX_CHARS, MEMBER_ID_CHARS, OFFICE_HANDLE, OWNER_HANDLE,
    RUN_HANDLE_PREFIX,
};
