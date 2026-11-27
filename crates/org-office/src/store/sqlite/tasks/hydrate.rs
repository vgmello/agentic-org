//! Hydration of task rows with their labels, files, repositories and details: one statement
//! per child table for a whole set of tasks, never one per task (`TASK-105`).

use std::collections::BTreeMap;

use org_store::{EngineError, Id};
use rusqlite::{types::Value, Connection};

use super::super::organization::{id, text};
use super::rows::details_row;
use super::{fetch, id_list, CHUNK, DETAIL_COLS};
use crate::tasks::records::{TaskDetailsRecord, TaskRecord, TaskRepositoryRecord};

/// Which tasks' children a hydration reads.
#[derive(Clone, Copy)]
pub(super) enum Owners {
    /// Every task of the organization: each child table is read whole.
    Organization,
    /// Only the tasks being hydrated: their IDs are bound, [`CHUNK`] at a time.
    Listed,
}

#[derive(Default)]
struct Children {
    labels: Vec<String>,
    files: Vec<(String, String)>,
    repositories: Vec<TaskRepositoryRecord>,
    details: Option<TaskDetailsRecord>,
}

/// `tasks` with their children filled in, in the order given.
pub(super) fn hydrate(
    db: &Connection,
    scope: &str,
    mut tasks: Vec<TaskRecord>,
    owners: Owners,
) -> Result<Vec<TaskRecord>, EngineError> {
    if tasks.is_empty() {
        return Ok(tasks);
    }
    let mut children: BTreeMap<Id, Children> = BTreeMap::new();
    match owners {
        Owners::Organization => read_children(db, scope, None, &mut children)?,
        Owners::Listed => {
            let ids: Vec<Id> = tasks.iter().map(|task| task.id.clone()).collect();
            for chunk in ids.chunks(CHUNK) {
                read_children(db, scope, Some(chunk), &mut children)?;
            }
        }
    }
    for task in &mut tasks {
        let Some(found) = children.remove(&task.id) else {
            continue;
        };
        task.labels = found.labels;
        for (role, path) in found.files {
            match role.as_str() {
                "reference" => task.reference_files.push(path),
                "modified" => task.modified_files.push(path),
                _ => {}
            }
        }
        task.repositories = found.repositories;
        task.details = found.details;
    }
    Ok(tasks)
}

/// Reads the children of `ids` (of every task when `None`) into `out`: four statements.
fn read_children(
    db: &Connection,
    scope: &str,
    ids: Option<&[Id]>,
    out: &mut BTreeMap<Id, Children>,
) -> Result<(), EngineError> {
    let mut values: Vec<Value> = vec![text(scope)];
    let only = match ids {
        Some(ids) => format!(" AND task_id IN {}", id_list(ids, &mut values)),
        None => String::new(),
    };
    let labels = fetch(
        db,
        &format!(
            "SELECT task_id, label FROM task_labels \
             WHERE organization_id = ?1{only} ORDER BY task_id, label"
        ),
        values.clone(),
        |r| Ok((id(r, 0)?, r.get::<_, String>(1)?)),
    )?;
    for (task, label) in labels {
        out.entry(task).or_default().labels.push(label);
    }
    let files = fetch(
        db,
        &format!(
            "SELECT task_id, role, path FROM task_files \
             WHERE organization_id = ?1{only} ORDER BY task_id, path"
        ),
        values.clone(),
        |r| Ok((id(r, 0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
    )?;
    for (task, role, path) in files {
        out.entry(task).or_default().files.push((role, path));
    }
    let repositories = fetch(
        db,
        &format!(
            "SELECT task_id, repository_id, branch, integrated_commit FROM task_repositories \
             WHERE organization_id = ?1{only} ORDER BY task_id, repository_id"
        ),
        values.clone(),
        |r| {
            let repository = TaskRepositoryRecord {
                repository_id: id(r, 1)?,
                branch: r.get(2)?,
                integrated_commit: r.get(3)?,
            };
            Ok((id(r, 0)?, repository))
        },
    )?;
    for (task, repository) in repositories {
        out.entry(task).or_default().repositories.push(repository);
    }
    // `task_id` follows the 18 detail columns `details_row` reads from 0.
    let details = fetch(
        db,
        &format!(
            "SELECT {DETAIL_COLS}, task_id FROM task_details WHERE organization_id = ?1{only}"
        ),
        values,
        |r| Ok((id(r, 18)?, details_row(r)?)),
    )?;
    for (task, found) in details {
        out.entry(task).or_default().details = Some(found);
    }
    Ok(())
}
