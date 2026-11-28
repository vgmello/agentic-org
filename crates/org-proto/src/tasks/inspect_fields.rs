//! Validated server-side `tasks.inspect` `--fields` section selection (`TASK-90`).

/// Which optional sections `tasks.inspect` should fetch and include on the wire (`TASK-90`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskInspectSelection {
    pub body: bool,
    pub checklist: bool,
    pub comments: bool,
    pub attachments: bool,
    pub watchers: bool,
    pub depends_on: bool,
    pub dependents: bool,
    pub history: bool,
}

impl TaskInspectSelection {
    pub const ALL: Self = Self {
        body: true,
        checklist: true,
        comments: true,
        attachments: true,
        watchers: true,
        depends_on: true,
        dependents: true,
        history: true,
    };

    pub const NONE: Self = Self {
        body: false,
        checklist: false,
        comments: false,
        attachments: false,
        watchers: false,
        depends_on: false,
        dependents: false,
        history: false,
    };

    /// Parses and validates an optional `tasks.inspect` `fields` allowlist, failing closed on
    /// empty or unknown field paths (`TASK-90`).
    pub fn try_from_fields(fields: Option<&[String]>) -> Result<Self, String> {
        let Some(fields) = fields else {
            return Ok(Self::ALL);
        };
        if fields.is_empty() {
            return Err("fields list cannot be empty".to_owned());
        }
        let mut sel = Self::NONE;
        for raw in fields {
            sel.try_apply_field(raw.trim())?;
        }
        Ok(sel)
    }

    pub fn from_fields(fields: Option<&[String]>) -> Self {
        let Some(fields) = fields else {
            return Self::ALL;
        };
        let mut sel = Self::NONE;
        for raw in fields {
            let _ = sel.try_apply_field(raw.trim());
        }
        sel
    }

    fn try_apply_field(&mut self, trimmed: &str) -> Result<(), String> {
        if trimmed.is_empty() || trimmed.split('.').any(str::is_empty) {
            return Err(format!("invalid field path `{trimmed}`"));
        }
        if matches!(trimmed, "body" | "body_markdown" | "task.body_markdown") {
            self.body = true;
            return Ok(());
        }
        let mut parts = trimmed.split('.');
        let root = parts.next().unwrap_or(trimmed);
        let sub = parts.next();
        match root {
            "task" => match sub {
                None => Ok(()),
                Some("body" | "body_markdown") => {
                    self.body = true;
                    Ok(())
                }
                Some(field) if is_task_view_field(field) => Ok(()),
                Some(_) => Err(format!("unknown task field `{trimmed}`")),
            },
            "body" | "body_markdown" => {
                self.body = true;
                Ok(())
            }
            "checklist" => {
                self.checklist = true;
                Ok(())
            }
            "comments" => {
                self.comments = true;
                Ok(())
            }
            "attachments" => {
                self.attachments = true;
                Ok(())
            }
            "watchers" => {
                self.watchers = true;
                Ok(())
            }
            "dependencies" => {
                self.depends_on = true;
                self.dependents = true;
                Ok(())
            }
            "depends_on_tasks" => {
                self.depends_on = true;
                Ok(())
            }
            "dependent_tasks" => {
                self.dependents = true;
                Ok(())
            }
            "history" => {
                self.history = true;
                Ok(())
            }
            other if is_task_view_field(other) => Ok(()),
            _ => Err(format!("unknown inspect field `{trimmed}`")),
        }
    }
}

fn is_task_view_field(name: &str) -> bool {
    matches!(
        name,
        "id" | "office_code"
            | "number"
            | "key"
            | "revision"
            | "team_id"
            | "assigned_team_id"
            | "project_id"
            | "type"
            | "work_type"
            | "title"
            | "status"
            | "assignee_id"
            | "feature_id"
            | "milestone_id"
            | "priority"
            | "effort"
            | "severity"
            | "resolution"
            | "blocked_reason"
            | "blocked_since"
            | "waiting_on_owner"
            | "dependency_blocked"
            | "unmet_blocking_dependencies"
            | "rank_order"
            | "labels"
            | "reference_files"
            | "modified_files"
            | "depends_on"
            | "dependents"
            | "details"
            | "final_summary"
            | "created_at"
            | "created_by_kind"
            | "created_by_id"
            | "updated_at"
            | "updated_by_kind"
            | "updated_by_id"
            | "checklist_total"
            | "checklist_done"
            | "ac_total"
            | "ac_done"
            | "dod_total"
            | "dod_done"
            | "work_steps_total"
            | "work_steps_done"
            | "comment_count"
            | "watcher_count"
    )
}
