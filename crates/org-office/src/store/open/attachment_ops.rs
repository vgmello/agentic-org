//! Store operations of the shared attachments family (`TASK-98.1`, ADR-0037): writing a
//! standalone file's canonical YAML through the journal, and the snapshot reads the
//! attachment facade and its authorization policy need.

use std::collections::BTreeSet;

use org_proto::attachments::{AttachmentTargetKind, AttachmentView};
use org_proto::{ApiError, ErrorCode};
use org_store::{Ack, Actor, Change, Id, Operation, Outcome, Refusal, Store, StoreError};

use crate::attachments::ports::AttachmentsRead;
use crate::attachments::records::{self, AttachmentRecord};
use crate::authz::{ParentFacts, PrincipalContext, PrincipalKind, ResourceRef};
use crate::organization::records::{EmployeeKind, EmployeeStatus};
use crate::tasks::ports::TasksRead;

use super::{OfficeEngine, ReadOp, StoreOp};

/// Whether a write makes a new file or replaces an existing one's metadata.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AttachmentWriteMode {
    Create,
    Update,
}

/// Writes one standalone attachment file (project or organization) through the journal.
pub struct AttachmentWriteOp {
    pub operation: Operation,
    pub record: AttachmentRecord,
    pub mode: AttachmentWriteMode,
}

fn refused(code: ErrorCode, message: &str) -> Refusal {
    Refusal::rejected(serde_json::to_vec(&ApiError::new(code, message)).expect("an error encodes"))
}

fn decide<S: TasksRead + AttachmentsRead>(
    view: &S,
    record: &AttachmentRecord,
    mode: AttachmentWriteMode,
) -> Result<Change, Refusal> {
    let Some(path) = records::record_path(record) else {
        return Err(refused(
            ErrorCode::Unsupported,
            "this target has no standalone attachment file",
        ));
    };
    let existing = view.attachment(&record.id)?;
    match (mode, &existing) {
        (AttachmentWriteMode::Create, Some(_)) => {
            return Err(refused(ErrorCode::AlreadyExists, "attachment id in use"));
        }
        (AttachmentWriteMode::Update, None) => {
            return Err(refused(ErrorCode::NotFound, "attachment not found"));
        }
        (AttachmentWriteMode::Update, Some(existing))
            if existing.target_kind != record.target_kind
                || existing.target_id != record.target_id =>
        {
            return Err(refused(ErrorCode::Conflict, "an attachment never moves"));
        }
        _ => {}
    }
    // The policy read a snapshot; the writer checks the project again where it decides.
    if let (AttachmentTargetKind::Project, Some(project)) = (record.target_kind, &record.target_id)
    {
        match view.project(project)? {
            None => return Err(refused(ErrorCode::NotFound, "project not found")),
            Some(project) if project.archived && mode == AttachmentWriteMode::Create => {
                return Err(refused(ErrorCode::NotAuthorized, "the project is archived"));
            }
            Some(_) => {}
        }
    }
    let reply = serde_json::to_vec(&record.to_view()).expect("a view encodes");
    let bytes = records::encode(record);
    let change = Change::new(Ack::Durable);
    let change = match mode {
        AttachmentWriteMode::Create => change.create(path, bytes),
        AttachmentWriteMode::Update => change.put(path, bytes),
    };
    Ok(change.reply(reply))
}

impl StoreOp for AttachmentWriteOp {
    type Out = Result<Result<AttachmentView, ApiError>, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let Self {
            operation,
            record,
            mode,
        } = self;
        let reply = match store.submit(Some(operation), false, move |uow| {
            decide(uow.view(), &record, mode)
        }) {
            Ok(reply) => reply,
            Err(StoreError::Conflict) => {
                return Ok(Err(ApiError::new(
                    ErrorCode::OperationReused,
                    "command_id reused with a different request",
                )));
            }
            Err(error) => return Err(error),
        };
        if reply.outcome == Outcome::Rejected {
            let refusal = serde_json::from_slice::<ApiError>(&reply.bytes)
                .unwrap_or_else(|_| ApiError::new(ErrorCode::InvalidRequest, "write refused"));
            return Ok(Err(refusal));
        }
        serde_json::from_slice::<AttachmentView>(&reply.bytes)
            .map(Ok)
            .map_err(|e| StoreError::Invalid(format!("corrupt attachment reply: {e}")))
    }
}

/// The operation a write is journaled under.
pub fn attachment_operation(
    command_id: &str,
    kind: &'static str,
    actor: &str,
    fingerprint: &[u8],
) -> Option<Operation> {
    if !org_proto::is_uuid_v7(command_id) {
        return None;
    }
    Some(Operation {
        id: Id::parse(command_id).ok()?,
        kind,
        actor: Id::parse(actor).map_or(Actor::System, Actor::Employee),
        fingerprint: org_store::Sha256::of_parts(&[kind.as_bytes(), b"\0", fingerprint]),
    })
}

/// One shared attachment by ID.
pub struct AttachmentById(pub Id);

impl ReadOp for AttachmentById {
    type Out = Option<AttachmentRecord>;

    fn read<S: TasksRead + AttachmentsRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        Ok(read.attachment(&self.0)?)
    }
}

/// One page of a target's shared attachments.
pub struct AttachmentsForTarget {
    pub kind: AttachmentTargetKind,
    pub target_id: Option<Id>,
    pub after: Option<Id>,
    pub limit: usize,
}

impl ReadOp for AttachmentsForTarget {
    type Out = Vec<AttachmentRecord>;

    fn read<S: TasksRead + AttachmentsRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        Ok(read.attachments_for_target(
            self.kind,
            self.target_id.as_ref(),
            self.after.as_ref(),
            self.limit,
        )?)
    }
}

/// The caller as the policy sees them: kind, owner, teams (home and above), active.
pub struct PrincipalRead {
    pub employee_id: String,
    pub owner_id: String,
}

impl ReadOp for PrincipalRead {
    type Out = Option<PrincipalContext>;

    fn read<S: TasksRead + AttachmentsRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        let Ok(id) = Id::parse(&self.employee_id) else {
            return Ok(None);
        };
        let Some(employee) = read.employee(&id)? else {
            return Ok(None);
        };
        let team_ids = read
            .team_chain(&employee.home_team_id)?
            .unwrap_or_else(|| vec![employee.home_team_id.clone()])
            .into_iter()
            .map(|team| team.to_string())
            .collect();
        Ok(Some(PrincipalContext {
            employee_id: employee.id.to_string(),
            kind: match employee.kind {
                EmployeeKind::Person => PrincipalKind::Person,
                EmployeeKind::Agent => PrincipalKind::Agent,
            },
            is_owner: employee.kind == EmployeeKind::Person
                && employee.id.as_str() == self.owner_id,
            team_ids,
            active: employee.status == EmployeeStatus::Active,
        }))
    }
}

/// What one snapshot says about a parent resource.
pub struct ResourceFactsRead(pub ResourceRef);

fn current_members<S: TasksRead>(
    read: &S,
    conversation: &Id,
) -> Result<BTreeSet<String>, StoreError> {
    let Some(conversation) = read.conversation(conversation)? else {
        return Ok(BTreeSet::new());
    };
    Ok(conversation
        .memberships
        .into_iter()
        .filter(|m| m.left_at.is_none())
        .map(|m| m.employee_id.to_string())
        .collect())
}

impl ReadOp for ResourceFactsRead {
    type Out = ParentFacts;

    fn read<S: TasksRead + AttachmentsRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        let id = |text: &str| Id::parse(text).ok();
        Ok(match self.0 {
            ResourceRef::Organization => ParentFacts::Organization,
            ResourceRef::Project(project) => match id(&project) {
                Some(project) => match read.project(&project)? {
                    Some(project) => ParentFacts::Project {
                        team_ids: std::iter::once(&project.owner_team_id)
                            .chain(&project.contributing_team_ids)
                            .map(Id::to_string)
                            .collect(),
                        archived: project.archived,
                    },
                    None => ParentFacts::Missing,
                },
                None => ParentFacts::Missing,
            },
            ResourceRef::Task(task) => match id(&task) {
                Some(task) if read.task(&task)?.is_some() => ParentFacts::Task,
                _ => ParentFacts::Missing,
            },
            ResourceRef::TaskComment {
                task_id,
                comment_id,
            } => {
                let found = match (id(&task_id), id(&comment_id)) {
                    (Some(task), Some(comment)) => read
                        .task_comments(&task)?
                        .into_iter()
                        .find(|c| c.id == comment),
                    _ => None,
                };
                found.map_or(ParentFacts::Missing, |comment| ParentFacts::TaskComment {
                    author_id: comment.author_id.to_string(),
                })
            }
            ResourceRef::Message(message) => match read.message(&message)? {
                Some(message) => ParentFacts::Message {
                    members: current_members(read, &message.conversation_id)?,
                    sender_id: message.sender_employee_id.map(|s| s.to_string()),
                },
                None => ParentFacts::Missing,
            },
            ResourceRef::AskAnswer { ask_message_id, .. } => match read.message(&ask_message_id)? {
                Some(ask) => ParentFacts::AskAnswer {
                    members: current_members(read, &ask.conversation_id)?,
                },
                None => ParentFacts::Missing,
            },
            // No review family is held yet; staged uploads, attachments and private spaces
            // are never parents the snapshot answers for.
            ResourceRef::ReviewFinding(_)
            | ResourceRef::ReviewFindingReply(_)
            | ResourceRef::StagedUpload { .. }
            | ResourceRef::Attachment { .. }
            | ResourceRef::UserPrivate { .. } => ParentFacts::Missing,
        })
    }
}
