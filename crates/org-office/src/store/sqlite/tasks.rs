//! SQLite adapter for [`TasksRead`] and [`TasksWrite`] over the verbatim `schema.sql` tables
//! `tasks`, `task_details`, `task_labels`, `task_files`, `office_task_counters`,
//! `checklist_steps`, `comments`, `watchers`, `links`, `task_events`, and `record_refs`
//! (`TASK-49`, `TASK-77`, `ENG-1`).

use org_proto::tasks::{TaskPriority, TaskStatus, WorkType};
use org_store::{EngineError, Id};
use rusqlite::{
    types::{Type, Value},
    Connection, Row,
};

use crate::tasks::{
    ports::{TasksRead, TasksWrite},
    records::{
        ChecklistStepRecord, OfficeTaskCounterRecord, TaskCommentRecord, TaskDetailsRecord,
        TaskEventRecord, TaskLinkRecord, TaskMeta, TaskRecord, TaskRepositoryRecord,
        TaskWatcherRecord,
    },
};

use super::{
    organization::{
        all, id, meta, meta_values, number, one, optional_id, optional_id_value, put,
        put_record_ref, run, text, whole, View, META,
    },
    SqliteSnapshot, SqliteTx,
};

const TASK_COLS: &str = "id, office_code, number, team_id, assigned_team_id, project_id, \
    type, work_type, title, status, assignee_id, body_markdown, feature_id, milestone_id, \
    priority, effort, severity, resolution, blocked_reason, rank_order";

const DETAIL_COLS: &str = "definition_of_done, what_happened, expected, steps, environment, \
    severity_confirmed, reopen_count, duplicate_of_task_id, closed_reason, affects_feature_id, \
    request_kind, request_text, route_reason, routing_source, routed_by_employee_id, \
    merged_into_task_id, suggested_triager_id, suggested_verifier_id";

fn bad_col(col: usize, msg: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(col, Type::Text, msg.into())
}

fn opt_u64(row: &Row<'_>, col: usize) -> rusqlite::Result<Option<u64>> {
    let val: Option<i64> = row.get(col)?;
    val.map(|v| u64::try_from(v).map_err(|_| bad_col(col, "negative integer")))
        .transpose()
}

fn opt_text_val(val: Option<&str>) -> Value {
    val.map_or(Value::Null, text)
}

fn opt_u64_val(val: Option<u64>) -> Result<Value, EngineError> {
    match val {
        None => Ok(Value::Null),
        Some(n) => whole(n),
    }
}

fn task_row(row: &Row<'_>) -> rusqlite::Result<TaskRecord> {
    let work_type_raw: Option<String> = row.get(7)?;
    let work_type = work_type_raw
        .as_deref()
        .map(|s| WorkType::parse(s).ok_or_else(|| bad_col(7, "invalid work_type")))
        .transpose()?;
    let status_raw: String = row.get(9)?;
    let status = TaskStatus::parse(&status_raw).ok_or_else(|| bad_col(9, "invalid status"))?;
    let prio_raw: String = row.get(14)?;
    let priority = TaskPriority::parse(&prio_raw).ok_or_else(|| bad_col(14, "invalid priority"))?;
    Ok(TaskRecord {
        id: id(row, 0)?,
        office_code: row.get(1)?,
        number: number(row, 2)?,
        team_id: id(row, 3)?,
        assigned_team_id: optional_id(row, 4)?,
        project_id: optional_id(row, 5)?,
        task_type: row.get(6)?,
        work_type,
        title: row.get(8)?,
        status,
        assignee_id: optional_id(row, 10)?,
        body_markdown: row.get(11)?,
        feature_id: optional_id(row, 12)?,
        milestone_id: optional_id(row, 13)?,
        priority,
        effort: opt_u64(row, 15)?,
        severity: row.get(16)?,
        resolution: row.get(17)?,
        blocked_reason: row.get(18)?,
        rank_order: opt_u64(row, 19)?,
        labels: Vec::new(),
        reference_files: Vec::new(),
        modified_files: Vec::new(),
        repositories: Vec::new(),
        details: None,
        meta: TaskMeta::from_org_meta(meta(row, 20)?),
    })
}

fn details_row(row: &Row<'_>) -> rusqlite::Result<TaskDetailsRecord> {
    let sev_int: i64 = row.get(5)?;
    Ok(TaskDetailsRecord {
        definition_of_done: row.get(0)?,
        what_happened: row.get(1)?,
        expected: row.get(2)?,
        steps: row.get(3)?,
        environment: row.get(4)?,
        severity_confirmed: sev_int != 0,
        reopen_count: number(row, 6)?,
        duplicate_of_task_id: optional_id(row, 7)?,
        closed_reason: row.get(8)?,
        affects_feature_id: optional_id(row, 9)?,
        request_kind: row.get(10)?,
        request_text: row.get(11)?,
        route_reason: row.get(12)?,
        routing_source: row.get(13)?,
        routed_by_employee_id: optional_id(row, 14)?,
        merged_into_task_id: optional_id(row, 15)?,
        suggested_triager_id: optional_id(row, 16)?,
        suggested_verifier_id: optional_id(row, 17)?,
    })
}

fn hydrate_task(
    db: &Connection,
    scope: &str,
    mut task: TaskRecord,
) -> Result<TaskRecord, EngineError> {
    task.labels = all(
        db,
        "SELECT label FROM task_labels WHERE organization_id = ?1 AND task_id = ?2 ORDER BY label",
        vec![text(scope), text(task.id.as_str())],
        |r| r.get::<_, String>(0),
    )?;
    let files: Vec<(String, String)> = all(
        db,
        "SELECT role, path FROM task_files WHERE organization_id = ?1 AND task_id = ?2 ORDER BY path",
        vec![text(scope), text(task.id.as_str())],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    )?;
    for (role, path) in files {
        match role.as_str() {
            "reference" => task.reference_files.push(path),
            "modified" => task.modified_files.push(path),
            _ => {}
        }
    }
    task.repositories = all(
        db,
        "SELECT repository_id, branch, integrated_commit FROM task_repositories \
         WHERE organization_id = ?1 AND task_id = ?2 ORDER BY repository_id",
        vec![text(scope), text(task.id.as_str())],
        |r| {
            Ok(TaskRepositoryRecord {
                repository_id: id(r, 0)?,
                branch: r.get(1)?,
                integrated_commit: r.get(2)?,
            })
        },
    )?;
    let det_sql = format!(
        "SELECT {DETAIL_COLS} FROM task_details WHERE organization_id = ?1 AND task_id = ?2"
    );
    task.details = one(
        db,
        &det_sql,
        vec![text(scope), text(task.id.as_str())],
        details_row,
    )?;
    Ok(task)
}

fn counter_row(row: &Row<'_>) -> rusqlite::Result<OfficeTaskCounterRecord> {
    Ok(OfficeTaskCounterRecord {
        office_code: row.get(0)?,
        last_number: number(row, 1)?,
        reserved_through: number(row, 2)?,
    })
}

fn step_row(row: &Row<'_>) -> rusqlite::Result<ChecklistStepRecord> {
    let req_i: i64 = row.get(3)?;
    let done_i: i64 = row.get(4)?;
    Ok(ChecklistStepRecord {
        id: id(row, 0)?,
        position: number(row, 1)?,
        text: row.get(2)?,
        required: req_i != 0,
        done: done_i != 0,
        linked_task_id: optional_id(row, 5)?,
        checked_by_employee_id: optional_id(row, 6)?,
        checked_at: row.get(7)?,
        revision: number(row, 8)?,
    })
}

fn comment_row(row: &Row<'_>) -> rusqlite::Result<TaskCommentRecord> {
    Ok(TaskCommentRecord {
        task_id: id(row, 0)?,
        id: id(row, 1)?,
        position: number(row, 2)?,
        author_id: id(row, 3)?,
        created_at: row.get(4)?,
        body_markdown: row.get(5)?,
    })
}

fn watcher_row(row: &Row<'_>) -> rusqlite::Result<TaskWatcherRecord> {
    Ok(TaskWatcherRecord {
        id: id(row, 0)?,
        watcher_kind: row.get(1)?,
        watcher_id: optional_id(row, 2)?,
    })
}

fn link_row(row: &Row<'_>) -> rusqlite::Result<TaskLinkRecord> {
    Ok(TaskLinkRecord {
        source_id: id(row, 0)?,
        id: id(row, 1)?,
        kind: row.get(2)?,
        target_kind: row.get(3)?,
        target_id: id(row, 4)?,
    })
}

fn event_row(row: &Row<'_>) -> rusqlite::Result<TaskEventRecord> {
    Ok(TaskEventRecord {
        task_id: id(row, 0)?,
        revision: number(row, 1)?,
        schema_version: number(row, 2)?,
        occurred_at: row.get(3)?,
        actor_kind: row.get(4)?,
        actor_id: optional_id(row, 5)?,
        operation_id: row.get(6)?,
        change_json: row.get(7)?,
        source_metadata_json: row.get(8)?,
        payload_sha256: row.get(9)?,
        resource_profile: row.get(10)?,
    })
}

macro_rules! sqlite_tasks_read {
    ($($view:ty),+) => {$(
        impl TasksRead for $view {
            fn task(&self, task_id: &Id) -> Result<Option<TaskRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {TASK_COLS}, {META} FROM tasks \
                     WHERE organization_id = ?1 AND id = ?2"
                );
                let Some(row) = one(db, &sql, vec![text(scope), text(task_id.as_str())], task_row)?
                else {
                    return Ok(None);
                };
                hydrate_task(db, scope, row).map(Some)
            }

            fn task_by_key(
                &self,
                office_code: &str,
                num: u64,
            ) -> Result<Option<TaskRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {TASK_COLS}, {META} FROM tasks \
                     WHERE organization_id = ?1 AND office_code = ?2 AND number = ?3"
                );
                let Some(row) = one(
                    db,
                    &sql,
                    vec![text(scope), text(office_code), whole(num)?],
                    task_row,
                )? else {
                    return Ok(None);
                };
                hydrate_task(db, scope, row).map(Some)
            }

            fn tasks(&self) -> Result<Vec<TaskRecord>, EngineError> {
                let (db, scope) = self.view()?;
                let sql = format!(
                    "SELECT {TASK_COLS}, {META} FROM tasks \
                     WHERE organization_id = ?1 ORDER BY office_code, number, id"
                );
                let rows = all(db, &sql, vec![text(scope)], task_row)?;
                let mut out = Vec::with_capacity(rows.len());
                for row in rows {
                    out.push(hydrate_task(db, scope, row)?);
                }
                Ok(out)
            }

            fn task_ids_by_label(&self, label: &str) -> Result<Vec<Id>, EngineError> {
                let (db, scope) = self.view()?;
                all(
                    db,
                    "SELECT task_id FROM task_labels \
                     WHERE organization_id = ?1 AND label = ?2 ORDER BY task_id",
                    vec![text(scope), text(label)],
                    |r| id(r, 0),
                )
            }

            fn task_label_counts(&self) -> Result<Vec<(String, u64)>, EngineError> {
                let (db, scope) = self.view()?;
                all(
                    db,
                    "SELECT label, COUNT(*) FROM task_labels \
                     WHERE organization_id = ?1 GROUP BY label ORDER BY label",
                    vec![text(scope)],
                    |r| Ok((r.get::<_, String>(0)?, number(r, 1)?)),
                )
            }

            fn office_task_counter(
                &self,
                office_code: &str,
            ) -> Result<Option<OfficeTaskCounterRecord>, EngineError> {
                let (db, scope) = self.view()?;
                one(
                    db,
                    "SELECT office_code, last_number, reserved_through FROM office_task_counters \
                     WHERE organization_id = ?1 AND office_code = ?2",
                    vec![text(scope), text(office_code)],
                    counter_row,
                )
            }

            fn office_task_counters(&self) -> Result<Vec<OfficeTaskCounterRecord>, EngineError> {
                let (db, scope) = self.view()?;
                all(
                    db,
                    "SELECT office_code, last_number, reserved_through FROM office_task_counters \
                     WHERE organization_id = ?1 ORDER BY office_code",
                    vec![text(scope)],
                    counter_row,
                )
            }

            fn checklist_steps(&self, task_id: &Id) -> Result<Vec<ChecklistStepRecord>, EngineError> {
                let (db, scope) = self.view()?;
                all(
                    db,
                    "SELECT id, position, text, required, done, linked_task_id, \
                     checked_by_employee_id, checked_at, revision \
                     FROM checklist_steps \
                     WHERE organization_id = ?1 AND target_kind = 'task' AND target_id = ?2 \
                     ORDER BY position",
                    vec![text(scope), text(task_id.as_str())],
                    step_row,
                )
            }

            fn task_comments(&self, task_id: &Id) -> Result<Vec<TaskCommentRecord>, EngineError> {
                let (db, scope) = self.view()?;
                all(
                    db,
                    "SELECT target_id, id, position, author_id, created_at, body_markdown \
                     FROM comments \
                     WHERE organization_id = ?1 AND target_kind = 'task' AND target_id = ?2 \
                     ORDER BY position",
                    vec![text(scope), text(task_id.as_str())],
                    comment_row,
                )
            }

            fn task_watchers(&self, task_id: &Id) -> Result<Vec<TaskWatcherRecord>, EngineError> {
                let (db, scope) = self.view()?;
                all(
                    db,
                    "SELECT id, watcher_kind, watcher_id FROM watchers \
                     WHERE organization_id = ?1 AND target_kind = 'task' AND target_id = ?2 \
                     ORDER BY id",
                    vec![text(scope), text(task_id.as_str())],
                    watcher_row,
                )
            }

            fn task_links_from(&self, task_id: &Id) -> Result<Vec<TaskLinkRecord>, EngineError> {
                let (db, scope) = self.view()?;
                all(
                    db,
                    "SELECT source_id, id, kind, target_kind, target_id FROM links \
                     WHERE organization_id = ?1 AND source_kind = 'task' AND source_id = ?2 \
                     ORDER BY kind, target_id",
                    vec![text(scope), text(task_id.as_str())],
                    link_row,
                )
            }

            fn task_dependents(&self, task_id: &Id) -> Result<Vec<Id>, EngineError> {
                let (db, scope) = self.view()?;
                all(
                    db,
                    "SELECT source_id FROM links \
                     WHERE organization_id = ?1 AND source_kind = 'task' \
                       AND kind = 'depends_on' AND target_kind = 'task' AND target_id = ?2 \
                     ORDER BY source_id",
                    vec![text(scope), text(task_id.as_str())],
                    |r| id(r, 0),
                )
            }

            fn all_task_links(&self) -> Result<Vec<TaskLinkRecord>, EngineError> {
                let (db, scope) = self.view()?;
                all(
                    db,
                    "SELECT source_id, id, kind, target_kind, target_id FROM links \
                     WHERE organization_id = ?1 AND source_kind = 'task' \
                     ORDER BY source_id, kind, target_id",
                    vec![text(scope)],
                    link_row,
                )
            }

            fn task_events(&self, task_id: &Id) -> Result<Vec<TaskEventRecord>, EngineError> {
                let (db, scope) = self.view()?;
                all(
                    db,
                    "SELECT task_id, revision, schema_version, occurred_at, actor_kind, \
                     actor_id, operation_id, change_json, source_metadata_json, \
                     payload_sha256, resource_profile \
                     FROM task_events \
                     WHERE organization_id = ?1 AND task_id = ?2 ORDER BY revision",
                    vec![text(scope), text(task_id.as_str())],
                    event_row,
                )
            }
        }
    )+};
}

sqlite_tasks_read!(SqliteTx, SqliteSnapshot);

fn put_task_details_row(
    db: &Connection,
    scope: &str,
    task_id: &Id,
    details: Option<&TaskDetailsRecord>,
) -> Result<(), EngineError> {
    let Some(det) = details else {
        return run(
            db,
            "DELETE FROM task_details WHERE organization_id = ?1 AND task_id = ?2",
            vec![text(scope), text(task_id.as_str())],
        );
    };
    let values = vec![
        text(scope),
        text(task_id.as_str()),
        opt_text_val(det.definition_of_done.as_deref()),
        opt_text_val(det.what_happened.as_deref()),
        opt_text_val(det.expected.as_deref()),
        opt_text_val(det.steps.as_deref()),
        opt_text_val(det.environment.as_deref()),
        Value::Integer(i64::from(det.severity_confirmed)),
        whole(det.reopen_count)?,
        optional_id_value(det.duplicate_of_task_id.as_ref()),
        opt_text_val(det.closed_reason.as_deref()),
        optional_id_value(det.affects_feature_id.as_ref()),
        opt_text_val(det.request_kind.as_deref()),
        opt_text_val(det.request_text.as_deref()),
        opt_text_val(det.route_reason.as_deref()),
        opt_text_val(det.routing_source.as_deref()),
        optional_id_value(det.routed_by_employee_id.as_ref()),
        optional_id_value(det.merged_into_task_id.as_ref()),
        optional_id_value(det.suggested_triager_id.as_ref()),
        optional_id_value(det.suggested_verifier_id.as_ref()),
    ];
    let columns = format!("organization_id, task_id, {DETAIL_COLS}");
    put(db, "task_details", &columns, 2, values)
}

impl TasksWrite for SqliteTx {
    fn put_task(&mut self, record: &TaskRecord) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let mut values = vec![
            text(scope),
            text(record.id.as_str()),
            text(&record.office_code),
            whole(record.number)?,
            text(record.team_id.as_str()),
            optional_id_value(record.assigned_team_id.as_ref()),
            optional_id_value(record.project_id.as_ref()),
            text(&record.task_type),
            opt_text_val(record.work_type.map(WorkType::as_str)),
            text(&record.title),
            text(record.status.as_str()),
            optional_id_value(record.assignee_id.as_ref()),
            text(&record.body_markdown),
            optional_id_value(record.feature_id.as_ref()),
            optional_id_value(record.milestone_id.as_ref()),
            text(record.priority.as_str()),
            opt_u64_val(record.effort)?,
            opt_text_val(record.severity.as_deref()),
            opt_text_val(record.resolution.as_deref()),
            opt_text_val(record.blocked_reason.as_deref()),
            opt_u64_val(record.rank_order)?,
        ];
        meta_values(&record.meta.to_org_meta(), &mut values)?;
        let columns = format!("organization_id, {TASK_COLS}, schema_version, {META}");
        put(db, "tasks", &columns, 2, values)?;
        put_record_ref(db, scope, "task", &record.id)?;
        put_task_details_row(db, scope, &record.id, record.details.as_ref())?;

        run(
            db,
            "DELETE FROM task_labels WHERE organization_id = ?1 AND task_id = ?2",
            vec![text(scope), text(record.id.as_str())],
        )?;
        for label in &record.labels {
            run(
                db,
                "INSERT INTO task_labels (organization_id, task_id, label) VALUES (?1, ?2, ?3)",
                vec![text(scope), text(record.id.as_str()), text(label)],
            )?;
        }

        run(
            db,
            "DELETE FROM task_files WHERE organization_id = ?1 AND task_id = ?2",
            vec![text(scope), text(record.id.as_str())],
        )?;
        for path in &record.reference_files {
            run(
                db,
                "INSERT INTO task_files (organization_id, task_id, role, path) \
                 VALUES (?1, ?2, 'reference', ?3)",
                vec![text(scope), text(record.id.as_str()), text(path)],
            )?;
        }
        for path in &record.modified_files {
            run(
                db,
                "INSERT INTO task_files (organization_id, task_id, role, path) \
                 VALUES (?1, ?2, 'modified', ?3)",
                vec![text(scope), text(record.id.as_str()), text(path)],
            )?;
        }

        run(
            db,
            "DELETE FROM task_repositories WHERE organization_id = ?1 AND task_id = ?2",
            vec![text(scope), text(record.id.as_str())],
        )?;
        for repo in &record.repositories {
            run(
                db,
                "INSERT INTO task_repositories \
                 (organization_id, task_id, repository_id, branch, integrated_commit) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                vec![
                    text(scope),
                    text(record.id.as_str()),
                    text(repo.repository_id.as_str()),
                    opt_text_val(repo.branch.as_deref()),
                    opt_text_val(repo.integrated_commit.as_deref()),
                ],
            )?;
        }
        Ok(())
    }

    fn remove_task(&mut self, task_id: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let pair = || vec![text(scope), text(task_id.as_str())];
        run(
            db,
            "DELETE FROM task_workspaces WHERE organization_id = ?1 AND task_id = ?2",
            pair(),
        )?;
        run(
            db,
            "DELETE FROM task_repositories WHERE organization_id = ?1 AND task_id = ?2",
            pair(),
        )?;
        run(
            db,
            "DELETE FROM task_labels WHERE organization_id = ?1 AND task_id = ?2",
            pair(),
        )?;
        run(
            db,
            "DELETE FROM task_files WHERE organization_id = ?1 AND task_id = ?2",
            pair(),
        )?;
        run(
            db,
            "DELETE FROM task_details WHERE organization_id = ?1 AND task_id = ?2",
            pair(),
        )?;
        run(
            db,
            "DELETE FROM checklist_steps WHERE organization_id = ?1 AND target_kind = 'task' AND target_id = ?2",
            pair(),
        )?;
        run(
            db,
            "DELETE FROM comments WHERE organization_id = ?1 AND target_kind = 'task' AND target_id = ?2",
            pair(),
        )?;
        run(
            db,
            "DELETE FROM watchers WHERE organization_id = ?1 AND target_kind = 'task' AND target_id = ?2",
            pair(),
        )?;
        run(
            db,
            "DELETE FROM links WHERE organization_id = ?1 AND source_kind = 'task' AND source_id = ?2",
            pair(),
        )?;
        run(
            db,
            "DELETE FROM record_refs WHERE organization_id = ?1 AND kind = 'task' AND id = ?2",
            pair(),
        )?;
        run(
            db,
            "DELETE FROM tasks WHERE organization_id = ?1 AND id = ?2",
            pair(),
        )
    }

    fn put_office_task_counter(
        &mut self,
        record: &OfficeTaskCounterRecord,
    ) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let values = vec![
            text(scope),
            text(&record.office_code),
            whole(record.last_number)?,
            whole(record.reserved_through)?,
        ];
        put(
            db,
            "office_task_counters",
            "organization_id, office_code, last_number, reserved_through",
            2,
            values,
        )
    }

    fn remove_office_task_counter(&mut self, office_code: &str) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        run(
            db,
            "DELETE FROM office_task_counters WHERE organization_id = ?1 AND office_code = ?2",
            vec![text(scope), text(office_code)],
        )
    }

    fn put_checklist_steps(
        &mut self,
        task_id: &Id,
        steps: &[ChecklistStepRecord],
    ) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        run(
            db,
            "DELETE FROM checklist_steps \
             WHERE organization_id = ?1 AND target_kind = 'task' AND target_id = ?2",
            vec![text(scope), text(task_id.as_str())],
        )?;
        for s in steps {
            run(
                db,
                "INSERT INTO checklist_steps \
                 (organization_id, target_kind, target_id, id, position, text, required, done, \
                  linked_task_id, checked_by_employee_id, checked_at, revision) \
                 VALUES (?1, 'task', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                vec![
                    text(scope),
                    text(task_id.as_str()),
                    text(s.id.as_str()),
                    whole(s.position)?,
                    text(&s.text),
                    Value::Integer(i64::from(s.required)),
                    Value::Integer(i64::from(s.done)),
                    optional_id_value(s.linked_task_id.as_ref()),
                    optional_id_value(s.checked_by_employee_id.as_ref()),
                    opt_text_val(s.checked_at.as_deref()),
                    whole(s.revision)?,
                ],
            )?;
        }
        Ok(())
    }

    fn put_task_comment(&mut self, record: &TaskCommentRecord) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let values = vec![
            text(scope),
            text("task"),
            text(record.task_id.as_str()),
            text(record.id.as_str()),
            whole(record.position)?,
            text(record.author_id.as_str()),
            text(&record.created_at),
            text(&record.body_markdown),
        ];
        put(
            db,
            "comments",
            "organization_id, target_kind, target_id, id, position, author_id, created_at, body_markdown",
            4,
            values,
        )
    }

    fn remove_task_comment(&mut self, task_id: &Id, comment_id: &Id) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        run(
            db,
            "DELETE FROM comments \
             WHERE organization_id = ?1 AND target_kind = 'task' AND target_id = ?2 AND id = ?3",
            vec![
                text(scope),
                text(task_id.as_str()),
                text(comment_id.as_str()),
            ],
        )
    }

    fn put_task_watchers(
        &mut self,
        task_id: &Id,
        watchers: &[TaskWatcherRecord],
    ) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        run(
            db,
            "DELETE FROM watchers \
             WHERE organization_id = ?1 AND target_kind = 'task' AND target_id = ?2",
            vec![text(scope), text(task_id.as_str())],
        )?;
        for w in watchers {
            run(
                db,
                "INSERT INTO watchers \
                 (organization_id, target_kind, target_id, id, watcher_kind, watcher_id) \
                 VALUES (?1, 'task', ?2, ?3, ?4, ?5)",
                vec![
                    text(scope),
                    text(task_id.as_str()),
                    text(w.id.as_str()),
                    text(&w.watcher_kind),
                    optional_id_value(w.watcher_id.as_ref()),
                ],
            )?;
        }
        Ok(())
    }

    fn put_task_links(
        &mut self,
        task_id: &Id,
        links: &[TaskLinkRecord],
    ) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        run(
            db,
            "DELETE FROM links \
             WHERE organization_id = ?1 AND source_kind = 'task' AND source_id = ?2",
            vec![text(scope), text(task_id.as_str())],
        )?;
        for l in links {
            run(
                db,
                "INSERT INTO links \
                 (organization_id, source_kind, source_id, id, kind, target_kind, target_id) \
                 VALUES (?1, 'task', ?2, ?3, ?4, ?5, ?6)",
                vec![
                    text(scope),
                    text(task_id.as_str()),
                    text(l.id.as_str()),
                    text(&l.kind),
                    text(&l.target_kind),
                    text(l.target_id.as_str()),
                ],
            )?;
        }
        Ok(())
    }

    fn put_task_event(&mut self, record: &TaskEventRecord) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        let existing: Option<String> = one(
            db,
            "SELECT payload_sha256 FROM task_events \
             WHERE organization_id = ?1 AND task_kind = 'task' AND task_id = ?2 AND revision = ?3",
            vec![
                text(scope),
                text(record.task_id.as_str()),
                whole(record.revision)?,
            ],
            |row| row.get(0),
        )?;
        if let Some(existing_sha) = existing {
            if existing_sha == record.payload_sha256 {
                return Ok(());
            }
            return Err(EngineError::new(format!(
                "immutable task_event conflict for task {} revision {}",
                record.task_id, record.revision
            )));
        }
        run(
            db,
            "INSERT INTO task_events \
             (organization_id, task_kind, task_id, revision, schema_version, occurred_at, \
              actor_kind, actor_id, operation_id, change_json, source_metadata_json, \
              payload_sha256, resource_profile) \
             VALUES (?1, 'task', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            vec![
                text(scope),
                text(record.task_id.as_str()),
                whole(record.revision)?,
                whole(record.schema_version)?,
                opt_text_val(record.occurred_at.as_deref()),
                text(&record.actor_kind),
                optional_id_value(record.actor_id.as_ref()),
                opt_text_val(record.operation_id.as_deref()),
                text(&record.change_json),
                text(&record.source_metadata_json),
                text(&record.payload_sha256),
                text(&record.resource_profile),
            ],
        )
    }
}
