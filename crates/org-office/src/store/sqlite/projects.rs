//! SQLite adapter for [`ProjectsRead`] and [`ProjectsWrite`] over the data contract's
//! `projects`, `project_contributing_teams`, `reference_counters`, `reference_allocations`,
//! and `record_refs` tables.

use org_store::{EngineError, Id};
use rusqlite::{types::Value, Connection, Row};

use crate::projects::{
    ports::{ProjectsRead, ProjectsWrite},
    records::{Project, ProjectAllocation, ProjectCounter, ProjectRepositoryLink},
};

use super::{
    organization::{
        all, id, meta, meta_values, number, one, optional_id, optional_id_value, put,
        put_record_ref, run, text, whole, View, META,
    },
    SqliteSnapshot, SqliteTx,
};

const PROJECT: &str = "id, number, slug, name, owner_team_id, archived, development_process_id";
const PROJECT_SCOPE: &str =
    "scope_kind = 'organization' AND scope_id = ?1 AND entity_kind = 'project'";

fn project_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    let archived_int: i64 = row.get(5)?;
    Ok(Project {
        id: id(row, 0)?,
        number: number(row, 1)?,
        slug: row.get(2)?,
        name: row.get(3)?,
        owner_team_id: id(row, 4)?,
        archived: archived_int != 0,
        development_process_id: optional_id(row, 6)?,
        contributing_team_ids: Vec::new(),
        repositories: Vec::new(),
        meta: meta(row, 7)?,
    })
}

fn load_contributing_teams(
    db: &Connection,
    scope: &str,
    project_id: &Id,
) -> Result<Vec<Id>, EngineError> {
    let sql = "SELECT team_id FROM project_contributing_teams \
               WHERE organization_id = ?1 AND project_id = ?2 ORDER BY team_id";
    all(
        db,
        sql,
        vec![text(scope), text(project_id.as_str())],
        |row| id(row, 0),
    )
}

fn load_project_repositories(
    db: &Connection,
    scope: &str,
    project_id: &Id,
) -> Result<Vec<ProjectRepositoryLink>, EngineError> {
    let sql = "SELECT repository_id, integration_branch FROM project_repositories \
               WHERE organization_id = ?1 AND project_id = ?2 ORDER BY repository_id";
    all(
        db,
        sql,
        vec![text(scope), text(project_id.as_str())],
        |row| {
            Ok(ProjectRepositoryLink {
                repository_id: id(row, 0)?,
                integration_branch: row.get(1)?,
            })
        },
    )
}

fn attach_contributing_teams(
    db: &Connection,
    scope: &str,
    mut project: Project,
) -> Result<Project, EngineError> {
    project.contributing_team_ids = load_contributing_teams(db, scope, &project.id)?;
    project.repositories = load_project_repositories(db, scope, &project.id)?;
    Ok(project)
}

fn counter_row(row: &Row<'_>) -> rusqlite::Result<ProjectCounter> {
    Ok(ProjectCounter {
        organization_id: id(row, 0)?,
        last_number: number(row, 1)?,
    })
}

fn allocation_row(row: &Row<'_>) -> rusqlite::Result<ProjectAllocation> {
    Ok(ProjectAllocation {
        organization_id: id(row, 0)?,
        number: number(row, 1)?,
        project_id: id(row, 2)?,
        allocated_at: row.get(3)?,
    })
}

macro_rules! sqlite_projects_read {
    ($($view:ty),+) => {$(
        impl ProjectsRead for $view {
            fn team_task_moves(
                &self,
                chains: &std::collections::BTreeMap<Id, (Vec<Id>, Vec<Id>)>,
            ) -> Result<Vec<crate::organization::ports::TeamHomedMove>, EngineError> {
                crate::tasks::relocation::team_task_moves(self, chains)
            }

            fn project(&self, project_id: &Id) -> Result<Option<Project>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {PROJECT}, {META} FROM projects \
                     WHERE organization_id = ?1 AND id = ?2"
                );
                let Some(project) = one(
                    db,
                    &sql,
                    vec![text(scope), text(project_id.as_str())],
                    project_row,
                )? else {
                    return Ok(None);
                };
                attach_contributing_teams(db, scope, project).map(Some)
            }

            fn project_by_number(&self, num: u64) -> Result<Option<Project>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {PROJECT}, {META} FROM projects \
                     WHERE organization_id = ?1 AND number = ?2"
                );
                let Some(project) = one(db, &sql, vec![text(scope), whole(num)?], project_row)?
                else {
                    return Ok(None);
                };
                attach_contributing_teams(db, scope, project).map(Some)
            }

            fn project_by_slug(&self, slug: &str) -> Result<Option<Project>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {PROJECT}, {META} FROM projects \
                     WHERE organization_id = ?1 AND slug = ?2"
                );
                let Some(project) = one(db, &sql, vec![text(scope), text(slug)], project_row)?
                else {
                    return Ok(None);
                };
                attach_contributing_teams(db, scope, project).map(Some)
            }

            fn projects(&self) -> Result<Vec<Project>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {PROJECT}, {META} FROM projects \
                     WHERE organization_id = ?1 ORDER BY number, id"
                );
                let mut rows = all(db, &sql, vec![text(scope)], project_row)?;
                for project in &mut rows {
                    project.contributing_team_ids =
                        load_contributing_teams(db, scope, &project.id)?;
                    project.repositories = load_project_repositories(db, scope, &project.id)?;
                }
                Ok(rows)
            }

            fn project_counter(&self) -> Result<Option<ProjectCounter>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT organization_id, last_number FROM reference_counters \
                     WHERE organization_id = ?1 AND {PROJECT_SCOPE}"
                );
                one(db, &sql, vec![text(scope)], counter_row)
            }

            fn project_allocations(&self) -> Result<Vec<ProjectAllocation>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT organization_id, number, entity_id, allocated_at \
                     FROM reference_allocations \
                     WHERE organization_id = ?1 AND {PROJECT_SCOPE} ORDER BY number"
                );
                all(db, &sql, vec![text(scope)], allocation_row)
            }
        }
    )+};
}

sqlite_projects_read!(SqliteTx, SqliteSnapshot);

impl ProjectsWrite for SqliteTx {
    fn put_project(&mut self, record: &Project) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let mut values = vec![
            text(scope),
            text(record.id.as_str()),
            whole(record.number)?,
            text(&record.slug),
            text(&record.name),
            text(record.owner_team_id.as_str()),
            Value::Integer(i64::from(record.archived)),
            optional_id_value(record.development_process_id.as_ref()),
        ];
        meta_values(&record.meta, &mut values)?;
        let columns = format!("organization_id, {PROJECT}, schema_version, {META}");
        put(db, "projects", &columns, 2, values)?;
        put_record_ref(db, scope, "project", &record.id)?;

        run(
            db,
            "DELETE FROM project_contributing_teams WHERE organization_id = ?1 AND project_id = ?2",
            vec![text(scope), text(record.id.as_str())],
        )?;
        for team_id in &record.contributing_team_ids {
            run(
                db,
                "INSERT INTO project_contributing_teams (organization_id, project_id, team_id) \
                 VALUES (?1, ?2, ?3)",
                vec![
                    text(scope),
                    text(record.id.as_str()),
                    text(team_id.as_str()),
                ],
            )?;
        }

        run(
            db,
            "DELETE FROM project_repositories WHERE organization_id = ?1 AND project_id = ?2",
            vec![text(scope), text(record.id.as_str())],
        )?;
        for link in &record.repositories {
            run(
                db,
                "INSERT INTO project_repositories \
                 (organization_id, project_id, repository_id, integration_branch) \
                 VALUES (?1, ?2, ?3, ?4)",
                vec![
                    text(scope),
                    text(record.id.as_str()),
                    text(link.repository_id.as_str()),
                    text(&link.integration_branch),
                ],
            )?;
        }
        Ok(())
    }

    fn remove_project(&mut self, project_id: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let pair = || vec![text(scope), text(project_id.as_str())];
        run(
            db,
            "DELETE FROM project_repositories WHERE organization_id = ?1 AND project_id = ?2",
            pair(),
        )?;
        run(
            db,
            "DELETE FROM project_contributing_teams WHERE organization_id = ?1 AND project_id = ?2",
            pair(),
        )?;
        run(
            db,
            "DELETE FROM record_refs WHERE organization_id = ?1 AND kind = 'project' AND id = ?2",
            pair(),
        )?;
        run(
            db,
            "DELETE FROM projects WHERE organization_id = ?1 AND id = ?2",
            pair(),
        )
    }

    fn put_project_counter(&mut self, record: &ProjectCounter) -> Result<(), EngineError> {
        let (db, _) = self.view()?;
        let organization = record.organization_id.as_str();
        let values = vec![
            text(organization),
            text("organization"),
            text(organization),
            text("project"),
            whole(record.last_number)?,
        ];
        let columns = "organization_id, scope_kind, scope_id, entity_kind, last_number";
        put(db, "reference_counters", columns, 4, values)
    }

    fn remove_project_counter(&mut self) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let sql = format!(
            "DELETE FROM reference_counters WHERE organization_id = ?1 AND {PROJECT_SCOPE}"
        );
        run(db, &sql, vec![text(scope)])
    }

    fn put_project_allocation(&mut self, record: &ProjectAllocation) -> Result<(), EngineError> {
        let (db, _) = self.view()?;
        let organization = record.organization_id.as_str();
        let values = vec![
            text(organization),
            text("organization"),
            text(organization),
            text("project"),
            whole(record.number)?,
            text(record.project_id.as_str()),
            text(&record.allocated_at),
        ];
        let columns = "organization_id, scope_kind, scope_id, entity_kind, number, entity_id, \
                       allocated_at";
        put(db, "reference_allocations", columns, 5, values)
    }

    fn remove_project_allocation(&mut self, num: u64) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let sql = format!(
            "DELETE FROM reference_allocations WHERE organization_id = ?1 AND {PROJECT_SCOPE} \
             AND number = ?2"
        );
        run(db, &sql, vec![text(scope), whole(num)?])
    }
}
