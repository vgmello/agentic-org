//! Transitioning a task's status (`TASK-49`, `TASK-88`): `task.md` at the task's next
//! revision with its `status_changed` event, answered `durable` (`doc-79`, `ADR-0014`).
//!
//! Completing a task (`done`, resolution `completed` or `fixed`) while a required checklist
//! step of any section is unticked is refused as `checklist_incomplete`, naming the steps,
//! unless a `done_anyway_reason` says why: the reason and the steps are then recorded on the
//! task (`task_details.done_anyway_json`) and in the event, and cleared when the task leaves
//! `done`. Cancelling, and completing with any other resolution, is never gated. A
//! `final_summary` given on a close is set in the same commit, attributed to the caller.

use org_proto::tasks::{TaskStatus, TasksTransitionParams, TASK_DONE_ANYWAY_REASON_MAX_CHARS};
use org_proto::ErrorCode;
use org_store::{Change, Engine, Id, Refusal, UnitOfWork};
use serde_json::{json, Value};

use crate::tasks::ports::TasksRead;
use crate::tasks::records::{ChecklistStepRecord, DoneAnywayRecord, FinalSummaryRecord};
use crate::time::format_rfc3339_ms;

use super::summary::validate_final_summary;
use super::{
    parse_uuid_v7, refuse, require_active_employee, require_task, rewrite_task, storage_refusal,
    validate_i64_u64,
};

/// How many unticked steps a `checklist_incomplete` message names, and how much of each.
const NAMED_STEPS: usize = 10;
const NAMED_STEP_CHARS: usize = 80;

/// Whether completing with `resolution` requires every required step ticked.
fn gated(resolution: &str) -> bool {
    let resolution = resolution.trim();
    resolution.eq_ignore_ascii_case("completed") || resolution.eq_ignore_ascii_case("fixed")
}

/// The refusal naming the unticked required steps, bounded however many there are.
fn incomplete(unticked: &[&ChecklistStepRecord]) -> Refusal {
    let named: Vec<String> = unticked
        .iter()
        .take(NAMED_STEPS)
        .map(|step| {
            let mut text: String = step.text.chars().take(NAMED_STEP_CHARS).collect();
            if step.text.chars().count() > NAMED_STEP_CHARS {
                text.push('…');
            }
            format!("[{}] {text}", step.section.as_str())
        })
        .collect();
    let more = unticked.len().saturating_sub(NAMED_STEPS);
    let tail = if more > 0 {
        format!("; and {more} more")
    } else {
        String::new()
    };
    refuse(
        ErrorCode::ChecklistIncomplete,
        format!(
            "required checklist steps are not done: {}{tail}; tick them or give a done_anyway_reason",
            named.join("; ")
        ),
    )
}

/// Transitions a task's status (`todo`, `in_progress`, `blocked`, `cancelled`, `done`).
pub struct TransitionTask {
    pub actor_member_id: String,
    pub params: TasksTransitionParams,
}

impl TransitionTask {
    pub fn decide<E: Engine>(&self, uow: &mut UnitOfWork<'_, E>) -> Result<(Change, Id), Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let _cmd_id = parse_uuid_v7(&self.params.command_id, "command_id")?;
        let mut task = require_task(uow.view(), &self.params.task)?;
        if task.meta.revision != self.params.expected_revision {
            return Err(refuse(
                ErrorCode::Conflict,
                format!(
                    "stale expected_revision {}: task is at revision {}",
                    self.params.expected_revision, task.meta.revision
                ),
            ));
        }
        let to = self.params.status;
        if to == TaskStatus::Blocked
            && self
                .params
                .blocked_reason
                .as_deref()
                .map(str::trim)
                .is_none_or(str::is_empty)
        {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "transitioning to `blocked` requires a non-empty `blocked_reason`",
            ));
        }
        let summary = self.final_summary()?;
        let reason = self.done_anyway_reason()?;
        let from_status = task.status;
        task.status = to;
        task.blocked_reason = if to == TaskStatus::Blocked {
            self.params
                .blocked_reason
                .as_deref()
                .map(|s| s.trim().to_owned())
        } else {
            None
        };
        let resolution = |default: &str| {
            let given = self.params.resolution.as_deref();
            Some(
                given
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(default)
                    .to_owned(),
            )
        };
        task.resolution = match to {
            TaskStatus::Done => resolution("completed"),
            TaskStatus::Cancelled => resolution("cancelled"),
            _ => None,
        };
        if from_status.is_closed() && !to.is_closed() {
            let det = task.details.get_or_insert_with(Default::default);
            det.reopen_count =
                validate_i64_u64(det.reopen_count.saturating_add(1), "reopen_count")?;
        }
        let done_anyway = match (to, task.resolution.as_deref()) {
            (TaskStatus::Done, Some(res)) if gated(res) => {
                let steps = uow
                    .view()
                    .checklist_steps(&task.id)
                    .map_err(storage_refusal)?;
                let unticked: Vec<&ChecklistStepRecord> =
                    steps.iter().filter(|s| s.required && !s.done).collect();
                match (unticked.is_empty(), reason) {
                    (true, _) => None,
                    (false, None) => return Err(incomplete(&unticked)),
                    (false, Some(reason)) => Some(DoneAnywayRecord {
                        reason,
                        steps: unticked.iter().map(|s| s.id.clone()).collect(),
                    }),
                }
            }
            _ => None,
        };
        // Set on a completion that skipped required steps; cleared by every other transition.
        if let Some(done) = &done_anyway {
            task.details
                .get_or_insert_with(Default::default)
                .done_anyway = Some(done.clone());
        } else if let Some(det) = task.details.as_mut() {
            det.done_anyway = None;
        }

        let at = format_rfc3339_ms(uow.now().0);
        if let Some(text) = &summary {
            task.final_summary = Some(FinalSummaryRecord {
                text: text.clone(),
                at: at.clone(),
                by_id: actor.id.clone(),
            });
        }
        task.meta.touch(&actor.id, &at);
        let done_anyway_event = done_anyway.as_ref().map_or(Value::Null, |d| {
            let steps: Vec<&str> = d.steps.iter().map(Id::as_str).collect();
            json!({ "reason": d.reason, "steps": steps })
        });
        let change = rewrite_task(
            uow.view(),
            &task,
            &actor.id,
            &at,
            &self.params.command_id,
            json!({
                "type": "status_changed",
                "from": from_status.as_str(),
                "to": task.status.as_str(),
                "blocked_reason": task.blocked_reason,
                "resolution": task.resolution,
                "note": self.params.note,
                "done_anyway": done_anyway_event,
                "final_summary": summary,
            }),
        )?;
        Ok((change, task.id))
    }

    /// The closing summary given, trimmed: only a transition to `done` or `cancelled` sets one.
    fn final_summary(&self) -> Result<Option<String>, Refusal> {
        let Some(raw) = self.params.final_summary.as_deref() else {
            return Ok(None);
        };
        if !self.params.status.is_closed() {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "final_summary is set when a task is closed (`done` or `cancelled`)",
            ));
        }
        validate_final_summary(raw).map(Some)
    }

    /// The reason to complete with required steps unticked, trimmed: for `done` only.
    fn done_anyway_reason(&self) -> Result<Option<String>, Refusal> {
        let Some(raw) = self.params.done_anyway_reason.as_deref() else {
            return Ok(None);
        };
        if self.params.status != TaskStatus::Done {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "done_anyway_reason applies only when a task is completed (`done`)",
            ));
        }
        let reason = raw.trim();
        if reason.is_empty() {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "done_anyway_reason cannot be empty",
            ));
        }
        if reason.chars().count() > TASK_DONE_ANYWAY_REASON_MAX_CHARS {
            return Err(refuse(
                ErrorCode::TooLarge,
                format!(
                    "done_anyway_reason exceeds {TASK_DONE_ANYWAY_REASON_MAX_CHARS} characters"
                ),
            ));
        }
        Ok(Some(reason.to_owned()))
    }
}
