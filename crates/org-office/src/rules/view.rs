use org_proto::org::{Member, Membership, ProjectTeam, Team};

/// Read access to the organization's current state, as the rules need it.
///
/// Everything returns owned values so a SQL-backed implementation can answer from a transaction
/// without borrowing it. Name and handle lookups take a key made by [`crate::rules::name_key`]
/// (lower-cased the Unicode way, which SQL's ASCII-only `lower()` is not), so a SQL-backed
/// implementation stores that key in a column and compares it, rather than lower-casing in SQL.
/// See the handle namespace in `org_proto::org`.
pub trait OrgView {
    fn team(&self, id: &str) -> Option<Team>;
    /// The team named `name_key` (see `OrganizationName::key`) among the teams `owner_team_id`
    /// owns; `None` as the owner means a top-level team.
    fn team_named(&self, owner_team_id: Option<&str>, name_key: &str) -> Option<Team>;
    fn teams_owned_by(&self, team_id: &str) -> Vec<Team>;
    fn teams_led_by(&self, member_id: &str) -> Vec<Team>;

    fn member(&self, id: &str) -> Option<Member>;
    /// The member whose person `name` or agent `messaging_name` has the key `handle_key`: the
    /// trimmed name lower-cased for a person, the `messaging_name` lower-cased for an agent. The
    /// owner's display name is NOT a handle: the owner is never returned here. An agent that was
    /// offboarded IS returned: its handle stays reserved for good (a removed member frees its
    /// handle; an offboarded one does not). (The reserved word `owner` is not looked up here;
    /// the rules handle it.)
    fn member_with_handle(&self, handle_key: &str) -> Option<Member>;
    /// Members that report directly to `member_id`.
    fn reports_of(&self, member_id: &str) -> Vec<Member>;

    fn membership(&self, id: &str) -> Option<Membership>;
    fn membership_of(&self, team_id: &str, member_id: &str) -> Option<Membership>;
    fn memberships_of_team(&self, team_id: &str) -> Vec<Membership>;
    fn memberships_of_member(&self, member_id: &str) -> Vec<Membership>;

    /// Whether `office_id` is an office of this organization. Phase 1 has one, HQ itself; offices
    /// that enrol later are added by the sync work.
    fn office_exists(&self, office_id: &str) -> bool;

    fn project_exists(&self, project_id: &str) -> bool;
    fn project_team(&self, id: &str) -> Option<ProjectTeam>;
    fn project_team_of(&self, project_id: &str, team_id: &str) -> Option<ProjectTeam>;
    fn project_teams_of_team(&self, team_id: &str) -> Vec<ProjectTeam>;
}

/// Write access, used only by [`fn@crate::rules::apply`]. Each call is a plain upsert or delete.
pub trait OrgWrite {
    fn put_team(&mut self, team: &Team);
    fn delete_team(&mut self, id: &str);
    fn put_member(&mut self, member: &Member);
    fn delete_member(&mut self, id: &str);
    fn put_membership(&mut self, membership: &Membership);
    fn delete_membership(&mut self, id: &str);
    fn put_project_team(&mut self, project_team: &ProjectTeam);
    fn delete_project_team(&mut self, id: &str);
}
