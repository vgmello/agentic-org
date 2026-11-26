//! The one Phase-1 authorization choke-point for attachments and the resources they bind to
//! (`TASK-98.1`, ADR-0037, `.agents/rules` §2): every attachment read, write and binding asks
//! [`AuthorizationPolicy`], and nothing else decides.
//!
//! - **Files are addressed by `attachment_id`, never by content.** Two bindings of the same
//!   bytes are two attachments with their own ACL; `content_sha256` is never a lookup key.
//! - **A staged upload is its uploader's alone.** Anyone else, whoever they are, gets
//!   `NotFound`: the policy never confirms that an upload exists.
//! - **An attached file inherits its parent's read rule** (task, task comment, message, ask
//!   answer, project, organization). A message or an ask is readable only by the members of
//!   its conversation.
//! - **Phase 1: any active employee may publish organization-wide files** (`doc-103` §1.B,
//!   §1.C, §4.D; `TASK-98.4`, `TASK-98.7`). Phase 2 (`TASK-176`) narrows that to the owner and
//!   admins.
//! - **A person's private space is that person's alone.** Read and write need
//!   `kind == Person && id == owner`: no agent ever (not even one the owner manages), no other
//!   person, no exception. Anyone asking for another's space gets `NotFound`, so no space's
//!   existence is ever confirmed; an agent asking for a space of its own is told it has none
//!   and to send the file in a 1:1 direct message instead (`doc-103`).
//! - **Binding needs both sides.** Binding a staged upload needs the uploader; re-attaching an
//!   existing attachment needs `can_read(source)`; either way the target needs
//!   `can_read(target) && can_write(target)`.
//!
//! The policy is pure: what it needs to know about a parent comes from [`ResourceFacts`],
//! which the office answers from one consistent snapshot.

use std::collections::BTreeSet;

use org_proto::{ApiError, ErrorCode};

use crate::error::OfficeError;

/// Whether the caller is a person or an agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrincipalKind {
    Person,
    Agent,
}

/// Who is calling, as the office's directory says now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrincipalContext {
    pub employee_id: String,
    pub kind: PrincipalKind,
    /// Whether the caller is the organization's live owner.
    pub is_owner: bool,
    /// The caller's home team and every team above it.
    pub team_ids: BTreeSet<String>,
    /// Only an active employee is authorized for anything.
    pub active: bool,
}

impl PrincipalContext {
    fn is(&self, employee_id: &str) -> bool {
        self.employee_id == employee_id
    }

    fn is_person(&self) -> bool {
        self.kind == PrincipalKind::Person
    }
}

/// What a call reads, writes or binds to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceRef {
    /// An upload still in the uploader's staging area.
    StagedUpload {
        id: String,
        uploaded_by: String,
    },
    /// A bound attachment, with what it is bound to.
    Attachment {
        id: String,
        target: Box<ResourceRef>,
        uploaded_by: String,
    },
    Task(String),
    TaskComment {
        task_id: String,
        comment_id: String,
    },
    Message(String),
    AskAnswer {
        ask_message_id: String,
        answerer_id: String,
    },
    ReviewFinding(String),
    ReviewFindingReply(String),
    Project(String),
    Organization,
    /// A person's own HQ-only private space.
    UserPrivate {
        owner_employee_id: String,
    },
}

/// What one snapshot says about a parent resource, as far as the policy needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParentFacts {
    /// Not there (or a kind this office does not hold yet).
    Missing,
    Task,
    TaskComment {
        author_id: String,
    },
    Message {
        /// The conversation's current members.
        members: BTreeSet<String>,
        sender_id: Option<String>,
    },
    AskAnswer {
        /// The ask's conversation's current members.
        members: BTreeSet<String>,
    },
    Project {
        /// The owning team and every contributing team.
        team_ids: BTreeSet<String>,
        archived: bool,
    },
    Organization,
}

/// Answers [`ParentFacts`] for a parent resource. The office reads them from one snapshot.
pub trait ResourceFacts {
    fn facts(&self, resource: &ResourceRef) -> Result<ParentFacts, OfficeError>;
}

/// Why the policy refused, or that it could not decide.
#[derive(Debug)]
pub enum Denied {
    /// The caller may not learn that the resource exists.
    NotFound,
    /// The caller may see the resource but not do this to it.
    NotAuthorized(&'static str),
    /// The facts could not be read: the call fails, it is never allowed.
    Storage(OfficeError),
}

impl Denied {
    /// The refusal a caller sees; a storage failure stays an office error.
    pub fn into_api(self) -> Result<ApiError, OfficeError> {
        match self {
            Self::NotFound => Ok(ApiError::new(ErrorCode::NotFound, "attachment not found")),
            Self::NotAuthorized(why) => Ok(ApiError::new(ErrorCode::NotAuthorized, why)),
            Self::Storage(error) => Err(error),
        }
    }
}

impl From<OfficeError> for Denied {
    fn from(error: OfficeError) -> Self {
        Self::Storage(error)
    }
}

/// The policy over one source of [`ResourceFacts`].
pub struct AuthorizationPolicy<'a, F: ResourceFacts + ?Sized> {
    facts: &'a F,
}

impl<'a, F: ResourceFacts + ?Sized> AuthorizationPolicy<'a, F> {
    pub fn new(facts: &'a F) -> Self {
        Self { facts }
    }

    /// Whether `caller` may read `resource` (an attachment's metadata and bytes, or a parent).
    pub fn can_read(
        &self,
        caller: &PrincipalContext,
        resource: &ResourceRef,
    ) -> Result<(), Denied> {
        if !caller.active {
            return Err(Denied::NotFound);
        }
        match resource {
            ResourceRef::StagedUpload { uploaded_by, .. } => only(caller.is(uploaded_by)),
            ResourceRef::UserPrivate { owner_employee_id } => {
                if !caller.is(owner_employee_id) {
                    return Err(Denied::NotFound);
                }
                if !caller.is_person() {
                    return Err(Denied::NotAuthorized(AGENT_HAS_NO_PRIVATE_SPACE));
                }
                Ok(())
            }
            // An attachment the caller cannot read through its parent does not exist for them.
            ResourceRef::Attachment { target, .. } => {
                self.can_read(caller, target)
                    .map_err(|denied| match denied {
                        Denied::Storage(error) => Denied::Storage(error),
                        _ => Denied::NotFound,
                    })
            }
            parent => match self.facts.facts(parent)? {
                ParentFacts::Missing => Err(Denied::NotFound),
                ParentFacts::Message { members, .. } | ParentFacts::AskAnswer { members } => {
                    only(members.contains(&caller.employee_id))
                }
                ParentFacts::Task
                | ParentFacts::TaskComment { .. }
                | ParentFacts::Project { .. }
                | ParentFacts::Organization => Ok(()),
            },
        }
    }

    /// Whether `caller` may change `resource`: add to it, or edit an attachment's metadata.
    pub fn can_write(
        &self,
        caller: &PrincipalContext,
        resource: &ResourceRef,
    ) -> Result<(), Denied> {
        self.can_read(caller, resource)?;
        match resource {
            // Read already required the uploader or the private space's owner.
            ResourceRef::StagedUpload { .. } | ResourceRef::UserPrivate { .. } => Ok(()),
            ResourceRef::Attachment {
                target,
                uploaded_by,
                ..
            } => {
                if !caller.is(uploaded_by) && !caller.is_owner {
                    return Err(Denied::NotAuthorized(
                        "only the uploader or the organization owner may change this attachment",
                    ));
                }
                self.can_write(caller, target)
            }
            parent => self.can_write_parent(caller, parent),
        }
    }

    /// Whether `caller` may bind `source` (a staged upload, or an attachment to re-attach) to
    /// `target`.
    pub fn can_attach(
        &self,
        caller: &PrincipalContext,
        source: &ResourceRef,
        target: &ResourceRef,
    ) -> Result<(), Denied> {
        match source {
            ResourceRef::StagedUpload { .. } => self.can_write(caller, source)?,
            ResourceRef::Attachment { .. } => self.can_read(caller, source)?,
            _ => return Err(Denied::NotAuthorized("only a file can be attached")),
        }
        if matches!(
            target,
            ResourceRef::StagedUpload { .. } | ResourceRef::Attachment { .. }
        ) {
            return Err(Denied::NotAuthorized(
                "a file is attached to a resource, not to a file",
            ));
        }
        self.can_write(caller, target)
    }

    fn can_write_parent(
        &self,
        caller: &PrincipalContext,
        parent: &ResourceRef,
    ) -> Result<(), Denied> {
        match self.facts.facts(parent)? {
            ParentFacts::Missing => Err(Denied::NotFound),
            // Phase 1: every active employee (`can_read` checked that) publishes org-wide.
            ParentFacts::Organization => Ok(()),
            ParentFacts::Project { archived: true, .. } => {
                Err(Denied::NotAuthorized("the project is archived"))
            }
            ParentFacts::Project { team_ids, .. } => {
                if caller.is_owner || !caller.team_ids.is_disjoint(&team_ids) {
                    Ok(())
                } else {
                    Err(Denied::NotAuthorized(
                        "only the project's teams may add files to it",
                    ))
                }
            }
            // Anyone who may read a task may add to it, as they may comment on it.
            ParentFacts::Task => Ok(()),
            ParentFacts::TaskComment { author_id } => author_only(caller, &author_id),
            ParentFacts::Message { sender_id, .. } => match sender_id {
                Some(sender) => author_only(caller, &sender),
                None => Err(Denied::NotAuthorized(
                    "only the sender may add to a message",
                )),
            },
            ParentFacts::AskAnswer { .. } => match parent {
                ResourceRef::AskAnswer { answerer_id, .. } => author_only(caller, answerer_id),
                _ => Err(Denied::NotFound),
            },
        }
    }
}

/// What an agent asking for a private space of its own is told.
pub const AGENT_HAS_NO_PRIVATE_SPACE: &str = "agents have no private file space; to hand a file \
     to one person, send it in a 1:1 direct message with `--attach`";

fn only(allowed: bool) -> Result<(), Denied> {
    if allowed {
        Ok(())
    } else {
        Err(Denied::NotFound)
    }
}

fn author_only(caller: &PrincipalContext, author: &str) -> Result<(), Denied> {
    if caller.is(author) {
        Ok(())
    } else {
        Err(Denied::NotAuthorized("only the author may add files here"))
    }
}

#[cfg(test)]
mod tests;
