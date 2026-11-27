//! Creating a task (`TASK-49`, `TASK-77`, `doc-77`, `doc-80`, `ADR-0019`): its office-coded
//! key is minted locally from the office's own counter, `local/offices/<office-id>/
//! task-counter.yaml`, written in the same entry as the task's `task.md` and its revision-1
//! `created` history event.
//!
//! The counter reserves numbers a block at a time (`ADR-0019`): `last_number <=
//! reserved_through`, and a number above `reserved_through` is only issued with a new
//! reservation answered `durable`. A create inside a reserved block whose reservation is
//! flushed is answered `committed`; one inside a block another create reserved but has not
//! flushed yet is `durable` too, so a power cut never takes a reservation back while an
//! answer names a number inside it. After a stop that was not clean the next number is past
//! the reservation, so a number an answer named but a crash lost is never issued again; a
//! clean stop that comes first spends the reservation durably ([`TaskNumbering::settled`]).
//!
//! An office whose counter is missing or behind the numbers its code already has cannot
//! prove its sequence (a lost or restored-from-backup `task-counter.yaml`): it refuses to
//! create rather than reissue or reconstruct a number (`doc-80`).
//!
//! The task's definition of done (`TASK-88`) is copied into its `checklist.yaml` `dod`
//! section as required steps in the same entry: the create's own `dod`, or else its project's
//! `default_dod`, or else the organization's, as this office's projection has them. So a
//! create works offline, and a later change to a default never changes a task already made.
//! Each step's ID derives from the task and the item's index, so a retry makes the same one.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use org_proto::attachments::{check_task_attachment_ids, AttachmentTargetKind};
use org_proto::tasks::{
    normalize_dod_items, ChecklistSection, TaskPriority, TaskStatus, TasksCreateParams,
    TASK_MAX_DEPENDENCIES,
};
use org_proto::ErrorCode;
use org_store::{Ack, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};

use crate::organization::records::EmployeeKind;
use crate::tasks::ports::TasksRead;
use crate::tasks::records::{
    self, normalize_file_paths, normalize_labels, ChecklistStepRecord, OfficeTaskCounterRecord,
    TaskLinkRecord, TaskMeta, TaskRecord,
};
use crate::time::format_rfc3339_ms;

use super::attachments::PreparedTaskFiles;
use super::core::{derive_link_id, derived_id};
use super::write::{child_bytes, task_location, too_deep, Writer};
use super::{
    parse_details, parse_uuid_v7, refuse, require_active_employee, require_office_code,
    require_task, storage_refusal, validate_active_employee, validate_active_team, validate_body,
    validate_task_type, validate_title,
};

/// How many task numbers one durable reservation covers (`ADR-0019`): a create inside the
/// reserved block is answered `committed`; the create that needs the next block waits for
/// its reservation to be `durable`.
pub const TASK_NUMBER_BLOCK: u64 = 32;

/// What one open store knows of its office's task-number reservation beyond the projection
/// (`ADR-0019`), shared by every task create it runs. Every decision reads the projection
/// as the store's single writer leaves it, so concurrent creates never jump twice.
#[derive(Debug, Default)]
pub struct TaskNumbering {
    /// After a stop that was not clean: the counter as the store opened. Numbers up to its
    /// `reserved_through` may have been issued and lost, so the first create jumps past it.
    unsettled: Option<OfficeTaskCounterRecord>,
    /// The office code and highest `reserved_through` known flushed.
    durable: Mutex<Option<(String, u64)>>,
}

impl TaskNumbering {
    /// The numbering of a store that opened with `counter` (what is on disk is flushed),
    /// after a clean stop or not.
    pub fn opened(counter: Option<OfficeTaskCounterRecord>, stopped_cleanly: bool) -> Self {
        let durable = counter
            .as_ref()
            .map(|c| (c.office_code.clone(), c.reserved_through));
        Self {
            unsettled: counter.filter(|_| !stopped_cleanly),
            durable: Mutex::new(durable),
        }
    }

    /// Past what the next number must be after a stop that was not clean: the opening
    /// counter's reservation, until a create has issued past it.
    fn jump_floor(&self, counter: &OfficeTaskCounterRecord) -> Option<u64> {
        let opened = self.unsettled.as_ref()?;
        let reserved = opened.reserved_through;
        (opened.office_code == counter.office_code && counter.last_number <= reserved)
            .then_some(reserved)
    }

    /// The highest `reserved_through` of `office_code` known flushed.
    fn durable_through(&self, office_code: &str) -> u64 {
        let durable = self.durable.lock().unwrap_or_else(|p| p.into_inner());
        durable
            .as_ref()
            .filter(|(code, _)| code == office_code)
            .map_or(0, |(_, through)| *through)
    }

    /// Records that `counter` is flushed: a create that issued it was answered `durable`.
    pub fn flushed(&self, counter: &OfficeTaskCounterRecord) {
        let mut durable = self.durable.lock().unwrap_or_else(|p| p.into_inner());
        let through = match durable.as_ref() {
            Some((code, through)) if *code == counter.office_code => {
                (*through).max(counter.reserved_through)
            }
            _ => counter.reserved_through,
        };
        *durable = Some((counter.office_code.clone(), through));
    }

    /// The counter a clean stop must leave in place of `counter`, if any: after a stop
    /// that was not clean and before any create, the numbers up to the reservation may
    /// have been issued and lost, so they are spent, durably, before the stop is clean
    /// (`ADR-0019` Decision 7). Otherwise the next opening would issue them again.
    pub fn settled(&self, counter: &OfficeTaskCounterRecord) -> Option<OfficeTaskCounterRecord> {
        let reserved = self.jump_floor(counter)?;
        (counter.last_number < reserved).then(|| OfficeTaskCounterRecord {
            last_number: reserved,
            ..counter.clone()
        })
    }
}

/// Creates a unified task (`task`, `bug`, `request`, ...), minting its office-coded key locally.
pub struct CreateTask {
    pub actor_member_id: String,
    pub params: TasksCreateParams,
    /// The store's task numbering: whether numbers up to the counter's reservation may have
    /// been issued by a run that did not stop cleanly, and how far a reservation is flushed.
    pub numbering: Arc<TaskNumbering>,
    /// Validated, blob-promoted files to bind onto the new task (`TASK-98.4`).
    pub prepared_files: PreparedTaskFiles,
}

/// What a decided create leaves: its change, the task, the agent it newly assigns, and the
/// office counter after it.
pub struct CreatedTask {
    pub change: Change,
    pub task_id: Id,
    pub assigned_agent: Option<Id>,
    pub counter: OfficeTaskCounterRecord,
}

/// The office's counter after issuing one more number, and the class the create is
/// answered in: `durable` when it reserves the next block or its block is not known
/// flushed, `committed` inside a flushed one.
fn issue_number<R: TasksRead>(
    view: &R,
    office_code: &str,
    numbering: &TaskNumbering,
) -> Result<(OfficeTaskCounterRecord, Ack), Refusal> {
    let prev = view
        .office_task_counter(office_code)
        .map_err(storage_refusal)?;
    let issued = view.max_task_number(office_code).map_err(storage_refusal)?;
    let unproven = match &prev {
        None => issued > 0,
        Some(c) => c.last_number < issued || c.reserved_through < issued,
    };
    if unproven {
        return Err(refuse(
            ErrorCode::Unavailable,
            format!(
                "this office cannot prove its task numbering for office code `{office_code}`: \
                 its task counter is missing or behind T-{office_code}{issued}, so it needs a \
                 new office code before it creates tasks"
            ),
        ));
    }
    let (floor, reserved) = match &prev {
        Some(c) => {
            let floor = numbering
                .jump_floor(c)
                .map_or(c.last_number, |r| c.last_number.max(r));
            (floor, c.reserved_through)
        }
        None => (0, 0),
    };
    let next = super::validate_i64_u64(floor + 1, "task number")?;
    let (reserved_through, ack) = if next > reserved {
        let through = next
            .saturating_add(TASK_NUMBER_BLOCK - 1)
            .min(i64::MAX as u64);
        (through, Ack::Durable)
    } else if next > numbering.durable_through(office_code) {
        (reserved, Ack::Durable)
    } else {
        (reserved, Ack::Committed)
    };
    let counter = OfficeTaskCounterRecord {
        office_code: office_code.to_owned(),
        last_number: next,
        reserved_through,
    };
    Ok((counter, ack))
}

impl CreateTask {
    pub fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
        here: &OfficeIdentity,
    ) -> Result<CreatedTask, Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let _cmd_id = parse_uuid_v7(&self.params.command_id, "command_id")?;
        let task_id = parse_uuid_v7(&self.params.task_id, "task_id")?;
        if uow
            .view()
            .task(&task_id)
            .map_err(storage_refusal)?
            .is_some()
        {
            return Err(refuse(
                ErrorCode::AlreadyExists,
                format!("task `{task_id}` already exists"),
            ));
        }
        let Scope {
            team_id,
            project_id,
            project_dod,
        } = resolve_scope(uow.view(), &self.params)?;
        let assigned_team_id = match self.params.assigned_team_id.as_deref() {
            Some(tid) => Some(validate_active_team(uow.view(), tid, "assigned_team_id")?),
            None => None,
        };
        let (assignee_id, assigned_agent) = match self.params.assignee_id.as_deref() {
            Some(eid) => {
                let emp = validate_active_employee(uow.view(), eid, "assignee_id")?;
                let agent = (emp.kind == EmployeeKind::Agent).then(|| emp.id.clone());
                (Some(emp.id), agent)
            }
            None => (None, None),
        };
        let title = validate_title(&self.params.title)?;
        let body_markdown = validate_body(&self.params.body_markdown)?;
        let task_type = match self.params.task_type.as_deref() {
            Some(t) => validate_task_type(t)?,
            None => "task".to_owned(),
        };
        let labels =
            normalize_labels(&self.params.labels).map_err(|e| refuse(e.code, e.message))?;
        let reference_files = normalize_file_paths(&self.params.reference_files, "reference")
            .map_err(|e| refuse(e.code, e.message))?;
        let modified_files = normalize_file_paths(&self.params.modified_files, "modified")
            .map_err(|e| refuse(e.code, e.message))?;
        let details = match self.params.details.as_ref() {
            Some(w) => Some(parse_details(uow.view(), &task_id, w)?),
            None => None,
        };
        let links = build_initial_depends_on(uow.view(), &task_id, &self.params.depends_on)?;
        let dod = definition_of_done(
            uow.view(),
            &task_id,
            self.params.dod.as_deref(),
            project_dod,
        )?;

        // Mint final `T-<CODE><N>` reference locally using this office's active office_code.
        let office_code = require_office_code(uow.view(), &here.office_id)?;
        let (counter, ack) = issue_number(uow.view(), &office_code, &self.numbering)?;
        let next_number = counter.last_number;

        if let Some(effort) = self.params.effort {
            super::validate_i64_u64(effort, "effort")?;
        }
        if let Some(rank_order) = self.params.rank_order {
            super::validate_i64_u64(rank_order, "rank_order")?;
        }
        if let Err(err) = check_task_attachment_ids(&self.params.attachment_ids) {
            return Err(refuse(err.code, err.message));
        }
        let at = format_rfc3339_ms(uow.now().0);
        let attachments = if self.params.attachment_ids.is_empty() {
            Vec::new()
        } else {
            self.prepared_files.to_records(
                AttachmentTargetKind::Task,
                &task_id,
                &self.params.command_id,
                &at,
            )?
        };
        let task = TaskRecord {
            id: task_id.clone(),
            office_code: office_code.clone(),
            number: next_number,
            team_id,
            assigned_team_id,
            project_id,
            task_type: task_type.clone(),
            work_type: self.params.work_type,
            title: title.clone(),
            status: TaskStatus::Todo,
            assignee_id,
            body_markdown,
            feature_id: None,
            milestone_id: None,
            priority: self.params.priority.unwrap_or(TaskPriority::Normal),
            effort: self.params.effort,
            severity: self.params.severity.clone(),
            resolution: None,
            blocked_reason: None,
            rank_order: self.params.rank_order,
            labels,
            reference_files,
            modified_files,
            repositories: Vec::new(),
            details,
            final_summary: None,
            meta: TaskMeta::created(&actor.id, &at),
        };

        let location = task_location(uow.view(), &task)?;
        let writer = Writer::new(uow.view(), &actor.id, &at, &self.params.command_id)?;
        let known_facts = records::task_md::task_fields(&task, &links, &attachments);
        let mut created = records::created_change(known_facts, &task.body_markdown);
        let mut change = Change::new(ack).put(
            records::counter_path(&here.office_id),
            records::encode_counter(&here.office_id, &counter),
        );
        if !dod.is_empty() {
            let path = records::checklist_path(&location, &task_id).map_err(too_deep)?;
            change = change.put(path, child_bytes(records::encode_checklist(&dod))?);
            let copied: Vec<serde_json::Value> = dod
                .iter()
                .map(|s| serde_json::json!({ "step_id": s.id.as_str(), "text": s.text }))
                .collect();
            created["dod"] = serde_json::Value::Array(copied);
        }
        let change =
            writer.task_and_event(change, &location, &task, &links, &attachments, created)?;
        Ok(CreatedTask {
            change,
            task_id,
            assigned_agent,
            counter,
        })
    }
}

/// The ID of the `index`-th item of a task's definition of done copied at create.
pub(crate) fn dod_step_id(task: &Id, index: usize) -> Id {
    let index = (index as u64).to_be_bytes();
    derived_id(&[
        b"task-checklist-step-v1:",
        task.as_str().as_bytes(),
        b":",
        ChecklistSection::Dod.as_str().as_bytes(),
        b":",
        &index,
    ])
}

/// The `dod` steps a new task starts with: `explicit` when given (`[]` for none), else its
/// project's default when the project names one, else the organization's.
fn definition_of_done<R: TasksRead>(
    view: &R,
    task_id: &Id,
    explicit: Option<&[String]>,
    project_dod: Option<Vec<String>>,
) -> Result<Vec<ChecklistStepRecord>, Refusal> {
    let items = match (explicit, project_dod) {
        (Some(items), _) => normalize_dod_items(items).map_err(|e| refuse(e.code, e.message))?,
        (None, Some(items)) => items,
        (None, None) => view
            .organization()
            .map_err(storage_refusal)?
            .and_then(|org| org.default_dod)
            .unwrap_or_default(),
    };
    Ok(items
        .into_iter()
        .enumerate()
        .map(|(index, text)| ChecklistStepRecord {
            id: dod_step_id(task_id, index),
            section: ChecklistSection::Dod,
            position: index as u64,
            text,
            required: true,
            done: false,
            linked_task_id: None,
            checked_by_employee_id: None,
            checked_at: None,
            revision: 1,
        })
        .collect())
}

/// Where a new task belongs: its owning team, its project, and that project's own
/// `default_dod` when it names one (`TASK-88`).
struct Scope {
    team_id: Id,
    project_id: Option<Id>,
    project_dod: Option<Vec<String>>,
}

fn resolve_scope<R: TasksRead>(view: &R, params: &TasksCreateParams) -> Result<Scope, Refusal> {
    if let Some(proj_ref) = params.project_id.as_deref() {
        let project = view
            .resolve_project(proj_ref)
            .map_err(storage_refusal)?
            .ok_or_else(|| {
                refuse(
                    ErrorCode::NotFound,
                    format!("project `{proj_ref}` not found"),
                )
            })?;
        if project.archived {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                format!("project `{proj_ref}` is archived"),
            ));
        }
        if let Some(team_str) = params.team_id.as_deref() {
            let explicit_team = validate_active_team(view, team_str, "team_id")?;
            if explicit_team != project.owner_team_id {
                return Err(refuse(
                    ErrorCode::InvalidRequest,
                    "a project task's owning team_id must match the project's owner_team_id",
                ));
            }
        }
        return Ok(Scope {
            team_id: project.owner_team_id,
            project_id: Some(project.id),
            project_dod: project.default_dod,
        });
    }
    let Some(team_str) = params.team_id.as_deref() else {
        return Err(refuse(
            ErrorCode::InvalidRequest,
            "either `team_id` or `project_id` is required",
        ));
    };
    let team_id = validate_active_team(view, team_str, "team_id")?;
    Ok(Scope {
        team_id,
        project_id: None,
        project_dod: None,
    })
}

fn build_initial_depends_on<R: TasksRead>(
    view: &R,
    task_id: &Id,
    refs: &[String],
) -> Result<Vec<TaskLinkRecord>, Refusal> {
    if refs.len() > TASK_MAX_DEPENDENCIES {
        return Err(refuse(
            ErrorCode::TooLarge,
            format!("at most {TASK_MAX_DEPENDENCIES} dependencies allowed"),
        ));
    }
    let mut seen = BTreeSet::new();
    let mut links = Vec::new();
    for (idx, dep_ref) in refs.iter().enumerate() {
        let target = require_task(view, dep_ref)?;
        if &target.id == task_id {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "a task cannot depend on itself",
            ));
        }
        if seen.insert(target.id.clone()) {
            links.push(TaskLinkRecord {
                source_id: task_id.clone(),
                id: derive_link_id(task_id, &target.id, idx as u32),
                kind: "depends_on".into(),
                target_kind: "task".into(),
                target_id: target.id,
            });
        }
    }
    Ok(links)
}
