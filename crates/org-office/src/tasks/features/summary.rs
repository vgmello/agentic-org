//! A task's attributed closing summary (`TASK-88`, `tasks.final_summary`): set by closing the
//! task with one ([`super::transition`]) or by this call, at the task's next revision, with
//! who wrote it and when. Nothing else writes it: comments, watches, checklist ticks and other
//! edits leave it as it is.

use org_proto::tasks::{TasksFinalSummaryParams, TASK_FINAL_SUMMARY_MAX_CHARS};
use org_proto::ErrorCode;
use org_store::{Change, Engine, Id, Refusal, UnitOfWork};
use serde_json::json;

use crate::tasks::ports::TasksRead;
use crate::tasks::records::FinalSummaryRecord;
use crate::time::format_rfc3339_ms;

use super::attachments::stale_revision_refusal;
use super::{parse_uuid_v7, refuse, require_active_employee, require_task, rewrite_task};

/// The summary trimmed: refused when empty, over [`TASK_FINAL_SUMMARY_MAX_CHARS`], or
/// containing control characters other than newline/carriage-return/tab (including NUL, which
/// truncates SQLite `length()`).
pub(crate) fn validate_final_summary(raw: &str) -> Result<String, Refusal> {
    let text = raw.trim();
    if text.is_empty() {
        return Err(refuse(
            ErrorCode::InvalidRequest,
            "final_summary cannot be empty",
        ));
    }
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(refuse(
            ErrorCode::InvalidRequest,
            "final_summary cannot contain control characters",
        ));
    }
    if text.chars().count() > TASK_FINAL_SUMMARY_MAX_CHARS {
        return Err(refuse(
            ErrorCode::TooLarge,
            format!("final_summary exceeds {TASK_FINAL_SUMMARY_MAX_CHARS} characters"),
        ));
    }
    Ok(text.to_owned())
}

/// Sets a task's final summary at `expected_revision`, attributed to the caller.
pub struct SetFinalSummary {
    pub actor_member_id: String,
    pub params: TasksFinalSummaryParams,
}

impl SetFinalSummary {
    pub fn decide<E: Engine>(&self, uow: &mut UnitOfWork<'_, E>) -> Result<(Change, Id), Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let _cmd_id = parse_uuid_v7(&self.params.command_id, "command_id")?;
        let mut task = require_task(uow.view(), &self.params.task)?;
        if !task.status.is_closed() {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "final_summary can only be set on a closed task (`done` or `cancelled`)",
            ));
        }
        if task.meta.revision != self.params.expected_revision {
            return Err(stale_revision_refusal(
                uow.view(),
                &task,
                self.params.expected_revision,
            ));
        }
        let text = validate_final_summary(&self.params.final_summary)?;
        let at = format_rfc3339_ms(uow.now().0);
        task.final_summary = Some(FinalSummaryRecord {
            text: text.clone(),
            at: at.clone(),
            by_id: actor.id.clone(),
        });
        task.meta.touch(&actor.id, &at);
        let change = rewrite_task(
            uow.view(),
            &task,
            &actor.id,
            &at,
            &self.params.command_id,
            json!({ "type": "final_summary_set", "final_summary": text }),
        )?;
        Ok((change, task.id))
    }
}
