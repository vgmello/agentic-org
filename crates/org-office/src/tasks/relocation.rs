//! Moving a team's projectless task folders with the team (`MoveTeam`, `doc-77`): every
//! projectless task folder of the team and of every team below it moves to the folder the
//! new chain names, in the same journal entry, beside the team's process and standard
//! documents. The organization's `TeamDependents` asks for these moves through the
//! projects family's `ProjectsRead::team_task_moves`, which every engine view answers with
//! [`team_task_moves`].
//!
//! The decision has the projection, not the files: each file is written again from its
//! records, at the same name in the new folder. Only files that are there move, so a task's
//! `checklist.yaml` or `operations.yaml` moves exactly when the task has one. `task.md` is
//! written at the task's effective revision, which its history already reaches, so a
//! rebuild keeps the same revision (`doc-79`).

use std::collections::BTreeMap;

use org_proto::attachments::{
    AttachmentTargetKind, TASK_ATTACHMENTS_MAX, TASK_COMMENT_ATTACHMENTS_MAX,
};
use org_store::{DurablePath, EngineError, Id};

use super::{
    ports::TasksRead,
    records::{self, Kind, Location, TaskRecord},
};
use crate::organization::ports::TeamHomedMove;

/// Every file of every projectless task folder homed in a team of `chains` (team ID to its
/// old and new chain), with its old path, its new path and its bytes.
pub fn team_task_moves<T: TasksRead + ?Sized>(
    view: &T,
    chains: &BTreeMap<Id, (Vec<Id>, Vec<Id>)>,
) -> Result<Vec<TeamHomedMove>, EngineError> {
    let mut moves = Vec::new();
    for task in view.tasks()? {
        if task.project_id.is_some() {
            continue;
        }
        let Some((from, to)) = chains.get(&task.team_id) else {
            continue;
        };
        let (from, to) = (Location::Team(from.clone()), Location::Team(to.clone()));
        moves.extend(task_folder_moves(view, &task, &from, &to)?);
    }
    Ok(moves)
}

fn encoding(reason: String) -> EngineError {
    EngineError::new(format!("re-encoding a moved task file: {reason}"))
}

/// Every file of `task`'s folder in `from`, with its path in `to` and its bytes.
pub(crate) fn task_folder_moves<T: TasksRead + ?Sized>(
    view: &T,
    task: &TaskRecord,
    from: &Location,
    to: &Location,
) -> Result<Vec<TeamHomedMove>, EngineError> {
    let id = &task.id;
    let folder = DurablePath::parse(&records::task_folder(from, id))
        .map_err(|e| EngineError::new(format!("a task folder: {e}")))?;
    let mut moves = Vec::new();
    for path in view.sources_under(&folder)? {
        let Some(kind) = records::kind_of(&path) else {
            return Err(EngineError::new(format!("{path} is not a task file")));
        };
        let (target, bytes) = match kind {
            Kind::Task(_) => {
                let links = view.task_links_from(id)?;
                let attachments = view.attachments_for_target(
                    AttachmentTargetKind::Task,
                    Some(id),
                    None,
                    TASK_ATTACHMENTS_MAX,
                )?;
                let bytes =
                    records::encode_task_md(task, &links, &attachments).map_err(encoding)?;
                (records::task_md_path(to, id), bytes)
            }
            Kind::Comment(_, comment) => {
                let comments = view.task_comments(id)?;
                let Some(record) = comments.iter().find(|c| c.id == comment) else {
                    return Err(EngineError::new(format!("{path} has no comment record")));
                };
                let attachments = view.attachments_for_target(
                    AttachmentTargetKind::TaskComment,
                    Some(&comment),
                    None,
                    TASK_COMMENT_ATTACHMENTS_MAX,
                )?;
                let bytes = records::encode_comment(record, &attachments).map_err(encoding)?;
                (records::comment_path(to, id, &comment), bytes)
            }
            Kind::Checklist(_) => {
                let bytes =
                    records::encode_checklist(&view.checklist_steps(id)?).map_err(encoding)?;
                (records::checklist_path(to, id), bytes)
            }
            Kind::Operations(_) => {
                let bytes =
                    records::encode_operations(&view.task_watchers(id)?).map_err(encoding)?;
                (records::operations_path(to, id), bytes)
            }
            Kind::Event(..) => return Err(EngineError::new("history is not in a task folder")),
        };
        moves.push(TeamHomedMove {
            from: Ok(path),
            to: target.ok(),
            bytes,
        });
    }
    Ok(moves)
}
