//! [`TasksWrite`] for the writer's transaction.

use org_proto::tasks::WorkType;
use org_store::{EngineError, Id};
use rusqlite::{types::Value, Connection};

use super::super::{
    organization::{
        meta_values, one, optional_id_value, put, put_record_ref, run, text, whole, View, META,
    },
    SqliteTx,
};
use super::{opt_text_val, opt_u64_val, DETAIL_COLS, TASK_COLS};
use org_office::organization_family::records::canonical_json;
use org_office::tasks::{
    ports::TasksWrite,
    records::{
        ChecklistStepRecord, FinalSummaryRecord, OfficeTaskCounterRecord, TaskCommentRecord,
        TaskDetailsRecord, TaskEventRecord, TaskLinkRecord, TaskRecord, TaskWatcherRecord,
    },
};

/// `{"reason": ..., "steps": [...]}`, canonical, or null (`TASK-88`).
fn done_anyway_value(details: &TaskDetailsRecord) -> Value {
    details.done_anyway.as_ref().map_or(Value::Null, |done| {
        let steps: Vec<&str> = done.steps.iter().map(Id::as_str).collect();
        text(&canonical_json(
            &serde_json::json!({"reason": done.reason, "steps": steps}),
        ))
    })
}

/// The four `final_summary*` values of a task row, all null or all set (`TASK-88`).
fn final_summary_values(summary: Option<&FinalSummaryRecord>, values: &mut Vec<Value>) {
    match summary {
        None => values.extend([Value::Null, Value::Null, Value::Null, Value::Null]),
        Some(s) => values.extend([
            text(&s.text),
            text(&s.at),
            text(FinalSummaryRecord::BY_KIND),
            text(s.by_id.as_str()),
        ]),
    }
}

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
        done_anyway_value(det),
    ];
    let columns = format!("organization_id, task_id, {DETAIL_COLS}");
    put(db, "task_details", &columns, 2, values)
}

impl TasksWrite for SqliteTx {
    fn put_task(&mut self, record: &TaskRecord) -> Result<(), EngineError> {
        let (db, scope) = self.view()?;
        // Effective revision (`doc-79`): `max(task.md revision, latest event revision)`.
        let latest: Option<i64> = one(
            db,
            "SELECT MAX(revision) FROM task_events \
             WHERE organization_id = ?1 AND task_kind = 'task' AND task_id = ?2",
            vec![text(scope), text(record.id.as_str())],
            |row| row.get(0),
        )?
        .flatten();
        let latest = latest.and_then(|n| u64::try_from(n).ok()).unwrap_or(0);
        let mut meta = record.meta.to_org_meta();
        meta.revision = meta.revision.max(latest);
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
        final_summary_values(record.final_summary.as_ref(), &mut values);
        values.push(opt_text_val(record.blocked_since.as_deref()));
        values.push(Value::Integer(i64::from(record.waiting_on_owner)));
        meta_values(&meta, &mut values)?;
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
                 (organization_id, target_kind, target_id, id, section, position, text, \
                  required, done, linked_task_id, checked_by_employee_id, checked_at, revision) \
                 VALUES (?1, 'task', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                vec![
                    text(scope),
                    text(task_id.as_str()),
                    text(s.id.as_str()),
                    text(s.section.as_str()),
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
             WHERE organization_id = ?1 AND source_kind = 'task' AND source_id = ?2 \
               AND kind = 'depends_on'",
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
        )?;
        // A child-only change (`doc-79`) advances the task's effective revision without
        // rewriting `task.md`.
        run(
            db,
            "UPDATE tasks SET revision = ?3 \
             WHERE organization_id = ?1 AND id = ?2 AND revision < ?3",
            vec![
                text(scope),
                text(record.task_id.as_str()),
                whole(record.revision)?,
            ],
        )
    }
}
