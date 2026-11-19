use std::fmt;

use org_proto::org::OrgChange;

use super::view::OrgWrite;

/// A change of a kind this build does not know. A satellite that meets one must stop applying
/// and ask for an update (offices design); HQ never produces one.
#[derive(Debug, PartialEq, Eq)]
pub struct UnknownChange;

impl fmt::Display for UnknownChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a change of a kind this build does not know")
    }
}

impl std::error::Error for UnknownChange {}

/// Applies one change. Mechanical and deterministic: it validates nothing and has no hidden
/// inputs, so replaying a log on an empty state rebuilds exactly the state it produced.
///
/// A change list is applied one change at a time, in order. If one fails with
/// [`UnknownChange`] the earlier ones are already written, so the caller applies a list inside
/// a transaction and rolls it back on error.
pub fn apply(state: &mut dyn OrgWrite, change: &OrgChange) -> Result<(), UnknownChange> {
    match change {
        OrgChange::TeamUpserted { team } => state.put_team(team),
        OrgChange::TeamDeleted { team_id } => state.delete_team(team_id),
        OrgChange::MemberUpserted { member } => state.put_member(member),
        OrgChange::MemberRemoved { member_id } => state.delete_member(member_id),
        OrgChange::MembershipUpserted { membership } => state.put_membership(membership),
        OrgChange::MembershipRemoved { membership_id } => state.delete_membership(membership_id),
        OrgChange::ProjectTeamAdded { project_team } => state.put_project_team(project_team),
        OrgChange::ProjectTeamRemoved { project_team_id } => {
            state.delete_project_team(project_team_id)
        }
        _ => return Err(UnknownChange),
    }
    Ok(())
}
