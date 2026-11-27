//! Store `ReadOp` and `StoreOp` adapters for organization processes & standards (`TASK-44`, `C5`).
//!
//! Write decisions live in [`crate::processes::features::write`]; cross-domain consistent read
//! snapshots compose `processes`, `organization`, `projects`, and `tasks` read ports here at the
//! store layer so `processes` stays strictly below `projects` and `tasks` in the module DAG.

use org_proto::processes::ProcessWriteParams;
use org_proto::{ApiError, ErrorCode};
use org_store::{Id, Outcome, Store, StoreError};

use crate::error::decode_api_refusal;
use crate::organization::authority::require_active_employee;
use crate::organization::records::{Employee, Team};
use crate::processes::features::{build_write_operation, decide_structure_write};
use crate::processes::records::StructureDocument;
use crate::projects::records::Project;
use crate::tasks::ports::TasksRead;
use crate::tasks::records::TaskRecord;

use super::{OfficeEngine, ReadOp, StoreOp};

const INACTIVE_PROCESS_ACTOR: &str = "only active employees may access processes";

/// Consistent snapshot of all organization state needed to resolve processes and standards.
#[derive(Debug, Clone)]
pub struct ProcessSnapshot {
    pub caller: Employee,
    pub documents: Vec<StructureDocument>,
    pub teams: Vec<Team>,
    pub projects: Vec<Project>,
    pub employees: Vec<Employee>,
    pub task: Option<TaskRecord>,
}

/// Snapshot loaded for agent launch instruction assembly.
#[derive(Debug, Clone)]
pub struct LaunchProcessSnapshot {
    pub employee: Employee,
    pub documents: Vec<StructureDocument>,
    pub teams: Vec<Team>,
    pub projects: Vec<Project>,
    pub project: Option<Project>,
    pub task: Option<TaskRecord>,
}

/// Snapshot loaded for standing + per-task process delta rendering on a single turn.
#[derive(Debug, Clone)]
pub struct TaskProcessSnapshot {
    pub employee: Employee,
    pub documents: Vec<StructureDocument>,
    pub teams: Vec<Team>,
    pub projects: Vec<Project>,
    /// `(reference, task, the task's project)` for every task that still resolves, in order.
    pub tasks: Vec<(String, TaskRecord, Option<Project>)>,
}

fn employee_by_str<S: TasksRead>(
    read: &S,
    raw: &str,
) -> Result<Result<Employee, ApiError>, StoreError> {
    let not_found = || ApiError::new(ErrorCode::NotFound, format!("employee `{raw}` not found"));
    let Ok(emp_id) = Id::parse(raw) else {
        return Ok(Err(not_found()));
    };
    Ok(read.employee(&emp_id)?.ok_or_else(not_found))
}

fn optional_task_by_ref<S: TasksRead>(
    read: &S,
    task_ref: Option<&str>,
) -> Result<Result<Option<TaskRecord>, ApiError>, StoreError> {
    let Some(tr) = task_ref else {
        return Ok(Ok(None));
    };
    Ok(read
        .resolve_task(tr)?
        .map(Some)
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("task `{tr}` not found"))))
}

fn task_project<S: TasksRead>(
    read: &S,
    project_id: Option<&Id>,
) -> Result<Result<Option<Project>, ApiError>, StoreError> {
    let Some(pid) = project_id else {
        return Ok(Ok(None));
    };
    Ok(read.project(pid)?.map(Some).ok_or_else(|| {
        ApiError::new(
            ErrorCode::NotFound,
            format!("task project `{pid}` not found"),
        )
    }))
}

/// Loads [`ProcessSnapshot`] in one consistent transaction snapshot.
pub struct ProcessSnapshotRead {
    pub caller_id: String,
    pub task_ref: Option<String>,
}

impl ReadOp for ProcessSnapshotRead {
    type Out = Result<ProcessSnapshot, ApiError>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        let caller = match require_active_employee(read, &self.caller_id, INACTIVE_PROCESS_ACTOR) {
            Ok(emp) => emp,
            Err(r) => return Ok(Err(decode_api_refusal(&r, "process operation refused"))),
        };
        let task = match optional_task_by_ref(read, self.task_ref.as_deref())? {
            Ok(task) => task,
            Err(err) => return Ok(Err(err)),
        };
        Ok(Ok(ProcessSnapshot {
            caller,
            documents: read.structure_documents()?,
            teams: read.teams()?,
            projects: read.projects()?,
            employees: read.employees()?,
            task,
        }))
    }
}

/// Loads all documents, teams, projects, and employees for agent launch instruction assembly.
pub struct LaunchProcessSnapshotRead {
    pub employee_id: String,
    pub project_ref: Option<String>,
    pub task_ref: Option<String>,
}

impl ReadOp for LaunchProcessSnapshotRead {
    type Out = Result<LaunchProcessSnapshot, ApiError>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        let employee = match employee_by_str(read, &self.employee_id)? {
            Ok(emp) => emp,
            Err(err) => return Ok(Err(err)),
        };
        let task = match optional_task_by_ref(read, self.task_ref.as_deref())? {
            Ok(task) => task,
            Err(err) => return Ok(Err(err)),
        };
        let project = match self.project_ref.as_deref() {
            Some(pr) => match read.resolve_project(pr)? {
                Some(p) => Some(p),
                None => {
                    return Ok(Err(ApiError::new(
                        ErrorCode::NotFound,
                        format!("project `{pr}` not found"),
                    )));
                }
            },
            None => match task_project(read, task.as_ref().and_then(|t| t.project_id.as_ref()))? {
                Ok(project) => project,
                Err(err) => return Ok(Err(err)),
            },
        };
        Ok(Ok(LaunchProcessSnapshot {
            employee,
            documents: read.structure_documents()?,
            teams: read.teams()?,
            projects: read.projects()?,
            project,
            task,
        }))
    }
}

/// Loads an agent's standing process set and the sets of the tasks one turn names in one read.
pub struct TaskProcessSnapshotRead {
    pub employee_id: String,
    pub task_refs: Vec<String>,
}

impl ReadOp for TaskProcessSnapshotRead {
    type Out = Result<TaskProcessSnapshot, ApiError>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        let employee = match employee_by_str(read, &self.employee_id)? {
            Ok(emp) => emp,
            Err(err) => return Ok(Err(err)),
        };
        let mut tasks = Vec::new();
        for task_ref in &self.task_refs {
            let Some(task) = read.resolve_task(task_ref)? else {
                continue;
            };
            // A live task whose project cannot be loaded has no chain to resolve, and dropping
            // it would silently drop its project's must-follow rules: refuse, as a launch does.
            let project = match task_project(read, task.project_id.as_ref())? {
                Ok(project) => project,
                Err(err) => return Ok(Err(err)),
            };
            tasks.push((task_ref.clone(), task, project));
        }
        Ok(Ok(TaskProcessSnapshot {
            employee,
            documents: read.structure_documents()?,
            teams: read.teams()?,
            projects: read.projects()?,
            tasks,
        }))
    }
}

/// Creates or updates one durable `structure_documents` Markdown file (`process.write`).
pub struct WriteStructureDocumentOp {
    pub actor: String,
    pub owner_id: String,
    pub params: ProcessWriteParams,
}

impl StoreOp for WriteStructureDocumentOp {
    type Out = Result<Result<(String, u64), ApiError>, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let op = match build_write_operation(&self.actor, &self.params) {
            Ok(op) => op,
            Err(err) => return Ok(Err(err)),
        };
        let Self {
            actor,
            owner_id,
            params,
        } = self;
        let reply = match store.submit(Some(op), false, move |uow| {
            decide_structure_write(uow.view(), uow.now().0, &actor, &owner_id, &params)
        }) {
            Ok(r) => r,
            Err(StoreError::Conflict) => {
                return Ok(Err(ApiError::new(
                    ErrorCode::OperationReused,
                    "command_id reused with different request",
                )));
            }
            Err(e) => return Err(e),
        };
        if reply.outcome == Outcome::Rejected {
            let err = serde_json::from_slice::<ApiError>(&reply.bytes)
                .unwrap_or_else(|_| ApiError::new(ErrorCode::InvalidRequest, "write refused"));
            return Ok(Err(err));
        }
        let val: serde_json::Value = serde_json::from_slice(&reply.bytes)
            .map_err(|e| StoreError::Invalid(format!("corrupt process.write reply: {e}")))?;
        let doc_id = val["document_id"].as_str().unwrap_or("").to_owned();
        let rev = val["revision"].as_u64().unwrap_or(1);
        Ok(Ok((doc_id, rev)))
    }
}
