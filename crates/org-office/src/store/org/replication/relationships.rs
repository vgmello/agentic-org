//! Check the rows affected by a replicated change list in its writer transaction.

use std::collections::HashSet;

use org_proto::org::OrgChange;
use org_sync::error::{Refusal, SyncError};
use rusqlite::{Connection, OptionalExtension};

use crate::organization::{is_offboarded, valid_owner};
use crate::store::sync::storage_error;

/// Remember relationships that the delta can remove, and preserve the owner identity.
pub(super) fn before_apply(
    db: &Connection,
    changes: &[OrgChange],
    genesis: bool,
) -> Result<HashSet<String>, SyncError> {
    let mut previous_teams = HashSet::new();
    let mut genesis_owner_created = false;
    for change in changes {
        let membership_id = match change {
            OrgChange::MembershipUpserted { membership } => &membership.id,
            OrgChange::MembershipRemoved { membership_id } => membership_id,
            OrgChange::MemberUpserted { member } => {
                let (was_owner, was_offboarded) = member_protections(db, &member.id)?;
                let can_create_owner = genesis
                    && !genesis_owner_created
                    && !exists(
                        db,
                        "SELECT EXISTS(SELECT 1 FROM members WHERE handle_key IS NULL)",
                        [],
                    )?;
                if (member.owner && (!valid_owner(member) || (!was_owner && !can_create_owner)))
                    || (!member.owner && was_owner)
                    || (was_offboarded && !is_offboarded(member))
                {
                    return Err(Refusal::IntegrityFailure.into());
                }
                genesis_owner_created |= member.owner && !was_owner;
                continue;
            }
            OrgChange::MemberRemoved { member_id } => {
                let (owner, offboarded) = member_protections(db, member_id)?;
                if owner || offboarded {
                    return Err(Refusal::IntegrityFailure.into());
                }
                continue;
            }
            _ => continue,
        };
        if let Some(team_id) = db
            .query_row(
                "SELECT team_id FROM memberships WHERE id = ?1",
                [membership_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?
        {
            previous_teams.insert(team_id);
        }
    }
    Ok(previous_teams)
}

pub(super) fn after_apply(
    db: &Connection,
    changes: &[OrgChange],
    previous_teams: HashSet<String>,
) -> Result<(), SyncError> {
    for change in changes {
        let (query, id) = match change {
            OrgChange::TeamUpserted { team } => (INVALID_TEAM, &team.id),
            OrgChange::TeamDeleted { team_id } => (
                "SELECT NOT EXISTS(SELECT 1 FROM teams WHERE id = ?1) AND (
                    EXISTS(SELECT 1 FROM teams WHERE owner_team_id = ?1) OR
                    EXISTS(SELECT 1 FROM memberships WHERE team_id = ?1) OR
                    EXISTS(SELECT 1 FROM project_teams WHERE team_id = ?1))",
                team_id,
            ),
            OrgChange::MemberUpserted { member } => (INVALID_MEMBER, &member.id),
            OrgChange::MemberRemoved { member_id } => (
                "SELECT NOT EXISTS(SELECT 1 FROM members WHERE id = ?1) AND (
                    EXISTS(SELECT 1 FROM members WHERE reports_to_member_id = ?1) OR
                    EXISTS(SELECT 1 FROM teams WHERE lead_member_id = ?1) OR
                    EXISTS(SELECT 1 FROM memberships WHERE member_id = ?1))",
                member_id,
            ),
            OrgChange::MembershipUpserted { membership } => (
                "SELECT EXISTS(
                    SELECT 1 FROM memberships m WHERE m.id = ?1 AND (
                        NOT EXISTS(SELECT 1 FROM teams WHERE id = m.team_id) OR
                        NOT EXISTS(SELECT 1 FROM members WHERE id = m.member_id
                            AND handle_key IS NOT NULL
                            AND json_extract(body, '$.agent.status') IS NOT 'offboarded')))",
                &membership.id,
            ),
            OrgChange::ProjectTeamAdded { project_team } => (
                "SELECT EXISTS(
                    SELECT 1 FROM project_teams l WHERE l.id = ?1 AND (
                        NOT EXISTS(SELECT 1 FROM projects WHERE id = l.project_id) OR
                        NOT EXISTS(SELECT 1 FROM teams WHERE id = l.team_id)))",
                &project_team.id,
            ),
            _ => continue,
        };
        if exists(db, query, [id])? {
            return Err(Refusal::IntegrityFailure.into());
        }
    }
    // A membership change can invalidate a team lead without touching the team row.
    for team_id in previous_teams {
        if exists(db, INVALID_TEAM, [team_id])? {
            return Err(Refusal::IntegrityFailure.into());
        }
    }
    Ok(())
}

fn member_protections(db: &Connection, id: &str) -> Result<(bool, bool), SyncError> {
    Ok(db
        .query_row(
            "SELECT handle_key IS NULL, json_extract(body, '$.agent.status') IS 'offboarded'
         FROM members WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(storage_error)?
        .unwrap_or_default())
}

fn exists(db: &Connection, query: &str, params: impl rusqlite::Params) -> Result<bool, SyncError> {
    Ok(db
        .query_row(query, params, |row| row.get(0))
        .map_err(storage_error)?)
}

// UNION visits each ancestor at most once, including in a malformed cycle.
const INVALID_TEAM: &str = "WITH RECURSIVE owners(id) AS (
    SELECT owner_team_id FROM teams WHERE id = ?1
    UNION SELECT t.owner_team_id FROM teams t JOIN owners ON owners.id = t.id
) SELECT EXISTS(SELECT 1 FROM teams t WHERE t.id = ?1 AND (
    (t.owner_team_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM teams WHERE id = t.owner_team_id)) OR
    (t.lead_member_id IS NOT NULL AND NOT EXISTS(
        SELECT 1 FROM memberships m JOIN members lead ON lead.id = m.member_id
        WHERE m.team_id = t.id AND m.member_id = t.lead_member_id AND lead.handle_key IS NOT NULL
            AND json_extract(lead.body, '$.agent.status') IS NOT 'offboarded')) OR
    EXISTS(SELECT 1 FROM owners WHERE id = ?1)))";

const INVALID_MEMBER: &str = "WITH RECURSIVE managers(id) AS (
    SELECT reports_to_member_id FROM members WHERE id = ?1
    UNION SELECT m.reports_to_member_id FROM members m JOIN managers ON managers.id = m.id
) SELECT EXISTS(SELECT 1 FROM members m WHERE m.id = ?1 AND (
    (m.reports_to_member_id IS NOT NULL AND NOT EXISTS(
        SELECT 1 FROM members manager WHERE manager.id = m.reports_to_member_id AND (
            json_extract(m.body, '$.agent.status') IS 'offboarded' OR
            json_extract(manager.body, '$.agent.status') IS NOT 'offboarded'))) OR
    EXISTS(SELECT 1 FROM managers WHERE id = ?1) OR
    (json_extract(m.body, '$.agent.status') = 'offboarded' AND (
        EXISTS(SELECT 1 FROM memberships WHERE member_id = ?1) OR
        EXISTS(SELECT 1 FROM teams WHERE lead_member_id = ?1) OR
        EXISTS(SELECT 1 FROM members report WHERE report.reports_to_member_id = ?1
            AND json_extract(report.body, '$.agent.status') IS NOT 'offboarded')))))";
