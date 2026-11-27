//! The built-in Chief of Staff (Tess, `TASK-80.1`) at the office: the idempotent bootstrap
//! the HQ runs when it opens, and the live authority of the member she acts for, which the
//! server intersects with hers on every call she makes while acting for them (`caller ∩ tess`).

use org_proto::chat::ConversationKind;
use org_store::{Actor, Id, Outcome, StoreError};

use super::{storage, Office};
use crate::error::OfficeError;
use crate::organization::{
    authority,
    chief_of_staff::ensure_operation_id,
    features::{ChiefOfStaffEnsured, EnsureChiefOfStaff},
    records::{BuiltinKey, Employee, EmployeeKind, EmployeeStatus},
};
use crate::store::open::{ReadOp, StructureOp};
use crate::tasks::ports::TasksRead;

/// What the member Tess acts for may do right now, read live from the organization on every
/// call (never cached in the session): an offboarded, deactivated or demoted caller narrows
/// her at once.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CallerAuthority {
    /// The caller is an employee of the organization and active.
    pub active: bool,
    /// The caller is a person (members ask Tess; agents never act through her).
    pub is_person: bool,
    /// The caller is the organization's live owner.
    pub is_owner: bool,
    /// The caller may hire into at least one team (`ADR-0029`).
    pub may_hire: bool,
}

/// Reads [`CallerAuthority`] for `caller` against `owner`.
struct ReadCallerAuthority {
    owner: Id,
    caller: Id,
}

impl ReadOp for ReadCallerAuthority {
    type Out = CallerAuthority;

    fn read<S: TasksRead>(self, read: &S) -> Result<CallerAuthority, StoreError> {
        let Some(caller) = read.employee(&self.caller)? else {
            return Ok(CallerAuthority::default());
        };
        let owner = crate::organization::owner::effective_owner_id(read, &self.owner)?;
        let active = caller.status == EmployeeStatus::Active;
        Ok(CallerAuthority {
            active,
            is_person: caller.kind == EmployeeKind::Person,
            is_owner: active && authority::is_owner(&owner, &caller),
            may_hire: active && authority::may_hire_anywhere(read, &owner, &caller)?,
        })
    }
}

/// Reads whether `caller` may hire into `team` (`ADR-0029` decision 2), for the hire calls the
/// Chief of Staff makes while acting for `caller`.
struct ReadCallerMayHireInto {
    owner: Id,
    caller: Id,
    team: Id,
}

impl ReadOp for ReadCallerMayHireInto {
    type Out = bool;

    fn read<S: TasksRead>(self, read: &S) -> Result<bool, StoreError> {
        let Some(caller) = read.employee(&self.caller)? else {
            return Ok(false);
        };
        let owner = crate::organization::owner::effective_owner_id(read, &self.owner)?;
        authority::may_hire_into(read, &owner, &caller, &self.team)
    }
}

/// A hire request as the `caller ∩ tess` gate weighs it (`TASK-80.1`): the team it hires into
/// and who filed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelegatedHireScope {
    pub home_team_id: String,
    pub requested_by_employee_id: String,
}

/// Reads [`DelegatedHireScope`] of one hire request.
struct ReadHireScope {
    request: Id,
}

impl ReadOp for ReadHireScope {
    type Out = Option<DelegatedHireScope>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        Ok(read
            .hire_request(&self.request)?
            .map(|request| DelegatedHireScope {
                home_team_id: request.home_team_id.as_str().to_owned(),
                requested_by_employee_id: request.requested_by_employee_id.as_str().to_owned(),
            }))
    }
}

/// Reads the conversation one message (a chat or mailbox message, or an ask) is in.
struct ReadMessageConversation {
    message: String,
}

impl ReadOp for ReadMessageConversation {
    type Out = Option<String>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        Ok(read
            .message(&self.message)?
            .map(|message| message.conversation_id.as_str().to_owned()))
    }
}

/// Reads the kind of one conversation.
struct ReadConversationKind {
    conversation: Id,
}

impl ReadOp for ReadConversationKind {
    type Out = Option<ConversationKind>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        Ok(read.conversation(&self.conversation)?.map(|c| c.kind))
    }
}

/// Resolves a `dm.open` participant reference (an ID, handle, name or `owner`) to the member it
/// names, exactly as `dm.open` resolves it.
struct ReadMemberReference {
    owner: Id,
    reference: String,
}

impl ReadOp for ReadMemberReference {
    type Out = Option<String>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        let owner = crate::organization::owner::effective_owner_id(read, &self.owner)?;
        let employees = read.employees()?;
        Ok(
            crate::messaging::access::resolve_employee(&employees, &owner, &self.reference)
                .ok()
                .map(|employee| employee.id.as_str().to_owned()),
        )
    }
}

impl Office {
    /// The organization's built-in Chief of Staff, if she is on record.
    pub fn chief_of_staff(&self) -> Result<Option<Employee>, OfficeError> {
        Ok(self
            .list_employees()?
            .into_iter()
            .find(|e| e.builtin_key == Some(BuiltinKey::ChiefOfStaff)))
    }

    /// Whether `employee_id` is the organization's built-in Chief of Staff.
    pub fn is_chief_of_staff(&self, employee_id: &str) -> bool {
        self.load_employee(employee_id)
            .ok()
            .flatten()
            .is_some_and(|e| e.builtin_key == Some(BuiltinKey::ChiefOfStaff))
    }

    /// Provisions the Chief of Staff in an organization created before she existed, as the
    /// system: answers `Ok(None)` when she is already on record (the usual case, including
    /// after a projection is deleted and rebuilt from the durable files) or this office is not
    /// the HQ, and who she now is otherwise. The operation's ID is derived from the
    /// organization, so a retry after a crash is answered from its receipt.
    pub fn ensure_chief_of_staff(&self) -> Result<Option<ChiefOfStaffEnsured>, OfficeError> {
        if self.info.role != crate::model::OfficeRole::Hq || self.chief_of_staff()?.is_some() {
            return Ok(None);
        }
        let organization = Id::parse(&self.info.org_id)
            .map_err(|_| OfficeError::Invalid("organization id is not a UUIDv7"))?;
        let op = StructureOp {
            operation_id: ensure_operation_id(&organization),
            here: self.identity.clone(),
            owner: self.effective_owner_for_ops(),
            by: Actor::System,
            command: EnsureChiefOfStaff::default(),
        };
        let reply = match self.store.run(op) {
            Ok(reply) => reply,
            // Refused before admission (no organization yet, not HQ): nothing to provision.
            Err(StoreError::Invalid(_)) => return Ok(None),
            Err(err) => return Err(storage(err)),
        };
        if reply.outcome != Outcome::Completed {
            // A recorded refusal: she appeared meanwhile, or the organization has no team
            // to place her in. Nothing was written.
            return Ok(None);
        }
        serde_json::from_slice(&reply.bytes)
            .map(Some)
            .map_err(|_| OfficeError::Invalid("unreadable Chief of Staff bootstrap reply"))
    }

    /// Whether `caller_id`, the member the Chief of Staff acts for, may hire into `team_id`
    /// right now (`ADR-0029` decision 2): her organization-wide reach never widens theirs. An
    /// unknown or malformed ID may hire nowhere.
    pub fn caller_may_hire_into(
        &self,
        caller_id: &str,
        team_id: &str,
    ) -> Result<bool, OfficeError> {
        let (Ok(caller), Ok(team)) = (Id::parse(caller_id), Id::parse(team_id)) else {
            return Ok(false);
        };
        let op = ReadCallerMayHireInto {
            owner: self.effective_owner_for_ops(),
            caller,
            team,
        };
        Ok(self.store.read(op).map_err(storage)?.0)
    }

    /// The team a hire request hires into and who filed it; `None` for an unknown request.
    pub fn delegated_hire_scope(
        &self,
        request_id: &str,
    ) -> Result<Option<DelegatedHireScope>, OfficeError> {
        let Ok(request) = Id::parse(request_id) else {
            return Ok(None);
        };
        Ok(self
            .store
            .read(ReadHireScope { request })
            .map_err(storage)?
            .0)
    }

    /// The conversation message `message_id` (a chat or mailbox message, or an ask) is in, so
    /// the `caller ∩ tess` gate can check a call that acts on it against the member she acts
    /// for; `None` for an unknown message.
    pub fn message_conversation_id(&self, message_id: &str) -> Result<Option<String>, OfficeError> {
        let op = ReadMessageConversation {
            message: message_id.to_owned(),
        };
        Ok(self.store.read(op).map_err(storage)?.0)
    }

    /// The kind of conversation `conversation_id`, so the Chief of Staff's wakes from a DM never
    /// reach her shared wake partition (`TASK-80.1`); `None` for an unknown or malformed ID.
    pub fn conversation_kind(
        &self,
        conversation_id: &str,
    ) -> Result<Option<ConversationKind>, OfficeError> {
        let Ok(conversation) = Id::parse(conversation_id) else {
            return Ok(None);
        };
        let op = ReadConversationKind { conversation };
        Ok(self.store.read(op).map_err(storage)?.0)
    }

    /// The member a `dm.open` participant reference names, resolved exactly as `dm.open`
    /// resolves it and without writing anything, so the `caller ∩ tess` gate can refuse a DM
    /// the caller would not be in before it is opened; `None` when it names nobody.
    pub fn resolve_member_id(&self, reference: &str) -> Result<Option<String>, OfficeError> {
        let op = ReadMemberReference {
            owner: self.effective_owner_for_ops(),
            reference: reference.to_owned(),
        };
        Ok(self.store.read(op).map_err(storage)?.0)
    }

    /// The live authority of `caller_id`, the member the Chief of Staff acts for. An unknown
    /// or malformed ID has no authority at all.
    pub fn caller_authority(&self, caller_id: &str) -> Result<CallerAuthority, OfficeError> {
        let Ok(caller) = Id::parse(caller_id) else {
            return Ok(CallerAuthority::default());
        };
        let op = ReadCallerAuthority {
            owner: self.effective_owner_for_ops(),
            caller,
        };
        Ok(self.store.read(op).map_err(storage)?.0)
    }
}
