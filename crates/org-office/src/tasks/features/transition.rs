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
use crate::tasks::records::{
    validate_blocked_reason, validate_transition_note, ChecklistStepRecord, DoneAnywayRecord,
    FinalSummaryRecord,
};
use crate::time::format_rfc3339_ms;

use super::conflict::stale_revision_refusal;
use super::summary::validate_final_summary;
use super::{
    parse_uuid_v7, refuse, require_active_employee, require_local_task_write_authority,
    require_task, rewrite_task, storage_refusal, validate_i64_u64, TaskWriteAuthority,
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
    /// Single-office task write authority verified atomically inside the `UnitOfWork` (`TASK-218`).
    pub authority: TaskWriteAuthority,
}

impl TransitionTask {
    pub fn decide<E: Engine>(&self, uow: &mut UnitOfWork<'_, E>) -> Result<(Change, Id), Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let _cmd_id = parse_uuid_v7(&self.params.command_id, "command_id")?;
        let mut task = require_task(uow.view(), &self.params.task)?;
        require_local_task_write_authority(uow.view(), &self.authority, &task)?;
        if task.meta.revision != self.params.expected_revision {
            return Err(stale_revision_refusal(
                uow.view(),
                &task,
                self.params.expected_revision,
            ));
        }
        let to = self.params.status;
        let (next_blocked_reason, transition_note) = self.resolve_reason_and_note(to)?;
        if to != TaskStatus::Blocked && self.params.waiting_on_owner.is_some() {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "`waiting_on_owner` applies only when transitioning to `blocked`",
            ));
        }
        let summary = self.final_summary()?;
        let reason = self.done_anyway_reason()?;
        let from_status = task.status;
        let prev_block = PriorBlockState {
            reason: task.blocked_reason.clone(),
            waiting_on_owner: task.waiting_on_owner,
            since: task.blocked_since.clone(),
        };
        let at = format_rfc3339_ms(uow.now().0);
        task.status = to;
        self.apply_blocking_state(&mut task, from_status, next_blocked_reason, &at);
        task.resolution = self.resolve_resolution(to);
        if from_status.is_closed() && !to.is_closed() {
            let det = task.details.get_or_insert_with(Default::default);
            det.reopen_count =
                validate_i64_u64(det.reopen_count.saturating_add(1), "reopen_count")?;
            task.final_summary = None;
        }
        let done_anyway = self.evaluate_done_anyway(uow.view(), &task, reason)?;
        apply_done_anyway(&mut task, done_anyway.as_ref());
        if let Some(text) = &summary {
            task.final_summary = Some(FinalSummaryRecord {
                text: text.clone(),
                at: at.clone(),
                by_id: actor.id.clone(),
            });
        }
        task.meta.touch(&actor.id, &at);
        let event = build_status_changed_event(
            from_status,
            &task,
            prev_block,
            transition_note.as_deref(),
            done_anyway.as_ref(),
            summary.as_deref(),
        );
        let change = rewrite_task(
            uow.view(),
            &task,
            &actor.id,
            &at,
            &self.params.command_id,
            event,
        )?;
        Ok((change, task.id))
    }

    fn apply_blocking_state(
        &self,
        task: &mut crate::tasks::records::TaskRecord,
        from_status: TaskStatus,
        next_blocked_reason: Option<String>,
        at: &str,
    ) {
        if task.status != TaskStatus::Blocked {
            // Atomically clear explicit block fields when leaving `blocked` (`TASK-84`).
            task.blocked_reason = None;
            task.blocked_since = None;
            task.waiting_on_owner = false;
            return;
        }
        task.blocked_reason = next_blocked_reason;
        if from_status == TaskStatus::Blocked {
            // In-place `blocked -> blocked`: preserve continuous `blocked_since` span.
            if task.blocked_since.is_none() {
                task.blocked_since = Some(
                    task.meta
                        .updated_at
                        .clone()
                        .unwrap_or_else(|| at.to_owned()),
                );
            }
            if let Some(flag) = self.params.waiting_on_owner {
                task.waiting_on_owner = flag;
            }
        } else {
            task.blocked_since = Some(at.to_owned());
            task.waiting_on_owner = self.params.waiting_on_owner.unwrap_or(false);
        }
    }

    fn resolve_resolution(&self, to: TaskStatus) -> Option<String> {
        let default = match to {
            TaskStatus::Done => "completed",
            TaskStatus::Cancelled => "cancelled",
            _ => return None,
        };
        let given = self
            .params
            .resolution
            .as_deref()
            .filter(|s| !s.trim().is_empty());
        Some(given.unwrap_or(default).to_owned())
    }

    fn evaluate_done_anyway<R: TasksRead>(
        &self,
        view: &R,
        task: &crate::tasks::records::TaskRecord,
        reason: Option<String>,
    ) -> Result<Option<DoneAnywayRecord>, Refusal> {
        let (TaskStatus::Done, Some(res)) = (task.status, task.resolution.as_deref()) else {
            return Ok(None);
        };
        if !gated(res) {
            return Ok(None);
        }
        let steps = view.checklist_steps(&task.id).map_err(storage_refusal)?;
        let unticked: Vec<&ChecklistStepRecord> =
            steps.iter().filter(|s| s.required && !s.done).collect();
        match (unticked.is_empty(), reason) {
            (true, _) => Ok(None),
            (false, None) => Err(incomplete(&unticked)),
            (false, Some(reason)) => Ok(Some(DoneAnywayRecord {
                reason,
                steps: unticked.iter().map(|s| s.id.clone()).collect(),
            })),
        }
    }

    /// Resolves `blocked_reason`, `reason`, and `note` under `TASK-84` rules:
    /// - On `blocked`: requires a valid `1..=1000`-char reason (via `blocked_reason`, `reason`,
    ///   or `note` alone — never both `reason` and `blocked_reason`) and accepts an optional
    ///   supplementary `note` when `blocked_reason` or `reason` is given.
    /// - On any non-blocked status: refuses `blocked_reason` with an actionable hint instead of
    ///   silently dropping it, refuses passing both `reason` and `note`, fails closed on
    ///   whitespace-only `reason`, and accepts an optional transition note via `reason` or `note`.
    fn resolve_reason_and_note(
        &self,
        to: TaskStatus,
    ) -> Result<(Option<String>, Option<String>), Refusal> {
        if to == TaskStatus::Blocked {
            self.resolve_blocked_reason_and_note()
        } else {
            self.resolve_non_blocked_note(to)
        }
    }

    fn resolve_blocked_reason_and_note(&self) -> Result<(Option<String>, Option<String>), Refusal> {
        let map_api = |e: org_proto::ApiError| refuse(e.code, e.message);
        if self.params.reason.is_some() && self.params.blocked_reason.is_some() {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "cannot pass both `reason` and `blocked_reason`",
            ));
        }
        let (raw_blocked, extra_note) = match (
            self.params.blocked_reason.as_deref(),
            self.params.reason.as_deref(),
            self.params.note.as_deref(),
        ) {
            (Some(b), None, n) => (b, n),
            (None, Some(r), n) => (r, n),
            // `note` alone is accepted as an alias for `blocked_reason` on `blocked` transitions
            // so CLI, Tess, and direct RPC stay aligned.
            (None, None, Some(n)) => (n, None),
            (None, None, None) => {
                return Err(refuse(
                    ErrorCode::InvalidRequest,
                    "transitioning to `blocked` requires a non-empty `blocked_reason`",
                ));
            }
            (Some(_), Some(_), _) => unreachable!("rejected above"),
        };
        let blocked_reason = validate_blocked_reason(raw_blocked).map_err(map_api)?;
        let note = extra_note
            .filter(|s| !s.trim().is_empty())
            .map(|s| validate_transition_note(s).map_err(map_api))
            .transpose()?;
        Ok((Some(blocked_reason), note))
    }

    fn resolve_non_blocked_note(
        &self,
        to: TaskStatus,
    ) -> Result<(Option<String>, Option<String>), Refusal> {
        let map_api = |e: org_proto::ApiError| refuse(e.code, e.message);
        if self.params.blocked_reason.is_some() {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                format!(
                    "`blocked_reason` applies only when transitioning to `blocked`, not `{}`; use `reason` or `note` for other status changes",
                    to.as_str()
                ),
            ));
        }
        if self.params.reason.is_some() && self.params.note.is_some() {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "cannot pass both `reason` and `note`",
            ));
        }
        let note = match (self.params.reason.as_deref(), self.params.note.as_deref()) {
            (Some(raw), None) => Some(validate_transition_note(raw).map_err(map_api)?),
            (None, Some(raw)) => {
                if raw.trim().is_empty() {
                    None
                } else {
                    Some(validate_transition_note(raw).map_err(map_api)?)
                }
            }
            _ => None,
        };
        Ok((None, note))
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
        if reason.chars().any(char::is_control) {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "done_anyway_reason cannot contain control characters",
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

struct PriorBlockState {
    reason: Option<String>,
    waiting_on_owner: bool,
    since: Option<String>,
}

fn apply_done_anyway(
    task: &mut crate::tasks::records::TaskRecord,
    done_anyway: Option<&DoneAnywayRecord>,
) {
    // Set on a completion that skipped required steps; cleared by every other transition.
    if let Some(done) = done_anyway {
        task.details
            .get_or_insert_with(Default::default)
            .done_anyway = Some(done.clone());
    } else if let Some(det) = task.details.as_mut() {
        det.done_anyway = None;
    }
}

fn build_status_changed_event(
    from_status: TaskStatus,
    task: &crate::tasks::records::TaskRecord,
    prev: PriorBlockState,
    transition_note: Option<&str>,
    done_anyway: Option<&DoneAnywayRecord>,
    summary: Option<&str>,
) -> Value {
    let done_anyway_event = done_anyway.map_or(Value::Null, |d| {
        let steps: Vec<&str> = d.steps.iter().map(Id::as_str).collect();
        json!({ "reason": d.reason, "steps": steps })
    });
    let mut event = serde_json::Map::new();
    event.insert("type".into(), json!("status_changed"));
    event.insert("from".into(), json!(from_status.as_str()));
    event.insert("to".into(), json!(task.status.as_str()));
    event.insert("blocked_reason".into(), json!(task.blocked_reason));
    event.insert("waiting_on_owner".into(), json!(task.waiting_on_owner));
    if let Some(ref since) = task.blocked_since {
        event.insert("blocked_since".into(), json!(since));
    }
    if from_status == TaskStatus::Blocked {
        event.insert("previous_blocked_reason".into(), json!(prev.reason));
        event.insert(
            "previous_waiting_on_owner".into(),
            json!(prev.waiting_on_owner),
        );
        if task.status != TaskStatus::Blocked {
            if let Some(prev_since) = prev.since {
                event.insert("previous_blocked_since".into(), json!(prev_since));
            }
        }
    }
    event.insert("resolution".into(), json!(task.resolution));
    event.insert("note".into(), json!(transition_note));
    event.insert("done_anyway".into(), done_anyway_event);
    event.insert("final_summary".into(), json!(summary));
    Value::Object(event)
}
