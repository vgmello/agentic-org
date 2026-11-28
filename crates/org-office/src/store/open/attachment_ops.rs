//! Store operations of the shared attachments family (`TASK-98.1`, ADR-0037): writing a
//! standalone file's canonical YAML through the journal, and the snapshot reads the
//! attachment facade and its authorization policy need.

use std::collections::BTreeSet;

use org_proto::attachments::{AttachmentTargetKind, AttachmentView};
use org_proto::{ApiError, ErrorCode};
use org_store::{Ack, Actor, Change, Id, Operation, Outcome, Refusal, Store, StoreError};

use crate::attachments::ports::AttachmentsRead;
use crate::attachments::records::{self, AttachmentRecord, MetaPatch};
use crate::authz::{ParentFacts, PrincipalContext, PrincipalKind, ResourceRef};
use crate::error::api_refusal as refused;
use crate::messaging::access::conversation_access;
use crate::messaging::ports::BoundMessageFile;
use crate::organization::records::{EmployeeKind, EmployeeStatus};
use crate::tasks::ports::TasksRead;

use super::{OfficeEngine, ReadOp, StoreOp};

/// What a shared write does: make a new file, or edit an existing one's metadata.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AttachmentWrite {
    Create(AttachmentRecord),
    /// Applied onto the file as the writer reads it when it decides, never onto an earlier
    /// snapshot, so concurrent edits of different fields all survive.
    Update {
        id: Id,
        patch: MetaPatch,
    },
}

/// Writes one standalone attachment file (project or organization) through the journal.
pub struct AttachmentWriteOp {
    pub operation: Operation,
    pub write: AttachmentWrite,
}

fn decide<S: TasksRead + AttachmentsRead>(
    view: &S,
    write: &AttachmentWrite,
) -> Result<Change, Refusal> {
    let (record, create) = match write {
        AttachmentWrite::Create(record) => {
            if view.attachment(&record.id)?.is_some()
                || view.message_file(record.id.as_str())?.is_some()
            {
                return Err(refused(ErrorCode::AlreadyExists, "attachment id in use"));
            }
            (record.clone(), true)
        }
        AttachmentWrite::Update { id, patch } => {
            let Some(mut existing) = view.attachment(id)? else {
                return Err(refused(ErrorCode::NotFound, "attachment not found"));
            };
            patch.apply(&mut existing);
            (existing, false)
        }
    };
    let Some(path) = records::record_path(&record) else {
        return Err(refused(
            ErrorCode::Unsupported,
            "this target has no standalone attachment file",
        ));
    };
    // The policy read a snapshot; the writer checks the project again where it decides, for
    // a new file and an edit alike.
    if let (AttachmentTargetKind::Project, Some(project)) = (record.target_kind, &record.target_id)
    {
        match view.project(project)? {
            None => return Err(refused(ErrorCode::NotFound, "project not found")),
            Some(project) if project.archived => {
                return Err(refused(ErrorCode::NotAuthorized, "the project is archived"));
            }
            Some(_) => {}
        }
    }
    let reply = serde_json::to_vec(&record.to_view()).expect("a view encodes");
    let bytes = records::encode(&record);
    let change = Change::new(Ack::Durable);
    let change = if create {
        change.create(path, bytes)
    } else {
        change.put(path, bytes)
    };
    Ok(change.reply(reply))
}

impl StoreOp for AttachmentWriteOp {
    type Out = Result<Result<AttachmentView, ApiError>, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let Self { operation, write } = self;
        let reply = match store.submit(Some(operation), false, move |uow| {
            decide(uow.view(), &write)
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

macro_rules! attachment_read {
    ($name:ident($arg:ty) => $out:ty : $method:ident) => {
        pub struct $name(pub $arg);
        impl ReadOp for $name {
            type Out = $out;
            fn read<S: TasksRead + AttachmentsRead>(
                self,
                read: &S,
            ) -> Result<Self::Out, StoreError> {
                Ok(read.$method(&self.0)?)
            }
        }
    };
}

attachment_read!(AttachmentById(Id) => Option<AttachmentRecord> : attachment);
attachment_read!(MessageFileById(String) => Option<BoundMessageFile> : message_file);

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

/// What one snapshot says about a parent resource. `owner` is the configured owner the
/// snapshot resolves (a conversation's effective members include it).
pub struct ResourceFactsRead {
    pub resource: ResourceRef,
    pub owner: Id,
}

/// A conversation's facts, shaped by `make` from its members and openness.
fn conversation_facts<S: TasksRead>(
    read: &S,
    conversation: &Id,
    owner: &Id,
    make: impl FnOnce(BTreeSet<String>, bool) -> ParentFacts,
) -> Result<ParentFacts, StoreError> {
    Ok(match conversation_access(read, conversation, owner)? {
        Some((members, open)) => make(members, open),
        None => ParentFacts::Missing,
    })
}

impl ReadOp for ResourceFactsRead {
    type Out = ParentFacts;

    fn read<S: TasksRead + AttachmentsRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        let id = |text: &str| Id::parse(text).ok();
        let owner = self.owner;
        Ok(match self.resource {
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
                let found = match id(&comment_id) {
                    Some(comment) => read
                        .task_comment_by_id(&comment)?
                        .filter(|c| task_id.is_empty() || c.task_id.as_str() == task_id.as_str()),
                    None => None,
                };
                found.map_or(ParentFacts::Missing, |comment| ParentFacts::TaskComment {
                    author_id: comment.author_id.to_string(),
                })
            }
            ResourceRef::Message(message) => match read.message(&message)? {
                Some(message) => {
                    let sender_id = message.sender_employee_id.map(|s| s.to_string());
                    conversation_facts(read, &message.conversation_id, &owner, |members, open| {
                        ParentFacts::Message {
                            members,
                            sender_id,
                            open,
                        }
                    })?
                }
                None => ParentFacts::Missing,
            },
            ResourceRef::AskAnswer { ask_message_id, .. } => match read.message(&ask_message_id)? {
                Some(ask) => {
                    conversation_facts(read, &ask.conversation_id, &owner, |members, open| {
                        ParentFacts::AskAnswer { members, open }
                    })?
                }
                None => ParentFacts::Missing,
            },
            ResourceRef::Conversation(conversation) => match id(&conversation) {
                Some(conversation) => {
                    conversation_facts(read, &conversation, &owner, |members, open| {
                        ParentFacts::Conversation { members, open }
                    })?
                }
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
