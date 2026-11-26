//! Identity mirror helpers inside `protected/messaging.db`: organization, team, project,
//! employee and office rows plus automatic conversation membership.

use org_store::EngineError;
use rusqlite::Connection;

use super::super::organization::{run, text};

// ---- Identity mirror helpers inside protected/messaging.db ----

pub(super) fn ensure_org(db: &Connection, org_id: &str) -> Result<(), EngineError> {
    run(
        db,
        "INSERT OR IGNORE INTO messaging_organizations (id) VALUES (?1)",
        vec![text(org_id)],
    )
}

pub(super) fn ensure_team(db: &Connection, org_id: &str, team_id: &str) -> Result<(), EngineError> {
    ensure_org(db, org_id)?;
    run(
        db,
        "INSERT OR IGNORE INTO messaging_teams (organization_id, id) VALUES (?1, ?2)",
        vec![text(org_id), text(team_id)],
    )
}

pub(super) fn ensure_project(
    db: &Connection,
    org_id: &str,
    project_id: &str,
) -> Result<(), EngineError> {
    ensure_org(db, org_id)?;
    run(
        db,
        "INSERT OR IGNORE INTO messaging_projects (organization_id, id) VALUES (?1, ?2)",
        vec![text(org_id), text(project_id)],
    )
}

pub(super) fn ensure_employee(
    db: &Connection,
    org_id: &str,
    employee_id: &str,
) -> Result<(), EngineError> {
    ensure_org(db, org_id)?;
    run(
        db,
        "INSERT OR IGNORE INTO messaging_employees (organization_id, id) VALUES (?1, ?2)",
        vec![text(org_id), text(employee_id)],
    )
}

pub(super) fn ensure_office(
    db: &Connection,
    org_id: &str,
    office_id: &str,
) -> Result<(), EngineError> {
    ensure_org(db, org_id)?;
    run(
        db,
        "INSERT OR IGNORE INTO messaging_offices (organization_id, id) VALUES (?1, ?2)",
        vec![text(org_id), text(office_id)],
    )
}

pub(super) fn ensure_auto_conversation_membership(
    db: &Connection,
    org_id: &str,
    conversation_id: &str,
    employee_id: &str,
    joined_at: &str,
) -> Result<(), EngineError> {
    run(
        db,
        "INSERT OR IGNORE INTO conversation_memberships \
           (organization_id, conversation_id, employee_id, joined_at, left_at) \
         SELECT organization_id, id, ?3, ?4, NULL \
         FROM conversations \
         WHERE organization_id = ?1 AND id = ?2 AND auto_scope IS NOT NULL",
        vec![
            text(org_id),
            text(conversation_id),
            text(employee_id),
            text(joined_at),
        ],
    )
}
