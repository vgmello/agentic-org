//! The office's operations as [`StoreOp`]s, and its reads as [`ReadOp`]s. The composition
//! names the families; a family does not name the store's seam.

use org_store::{Actor, Id, OfficeIdentity, Receipt, Reply, Store, StoreError};

use super::{OfficeEngine, ReadOp, StoreOp};
use crate::{
    office_local::CaMaterial,
    organization::{
        features::{
            create_organization, AddPerson, ArchiveTeam, CreateOrganization, CreateTeam,
            DeactivatePerson, Feature, MovePerson, MoveTeam, UpdatePerson, UpdateTeam,
        },
        records, snapshot,
    },
    projects::{
        features::{AddProjectTeam, CreateProject, ProjectFeature, RemoveProjectTeam},
        records::Project as ProjectRecord,
    },
    tasks::ports::TasksRead,
};

/// Creates the organization, with the HQ's CA records in the same entry: the
/// organization family's public escrow package and the office-local family's wrapped CA
/// key (ADR-0007, step 3). The creator is the actor.
pub struct CreateOrganizationOp {
    /// Minted by the caller: a retry with it gets the original reply.
    pub operation_id: Id,
    /// The office deciding: the store's own identity.
    pub here: OfficeIdentity,
    pub command: CreateOrganization,
    /// Frozen before the first attempt: a retry with other material conflicts.
    pub ca: CaMaterial,
}

impl StoreOp for CreateOrganizationOp {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let Self {
            operation_id,
            here,
            command,
            ca,
        } = self;
        let key = ca.record(&here, &operation_id);
        key.check()
            .map_err(|why| StoreError::invalid(format!("the CA key record: {why}")))?;
        // The package must be this organization's and carry this public key: a mismatch
        // committed in entry 1 would leave an HQ that can never open (and nothing is
        // generated again).
        let sealed = records::ca_escrow_public_key(&ca.package, &here.organization_id)
            .map_err(|why| StoreError::invalid(format!("the CA escrow package: {why}")))?;
        if sealed != ca.public_key {
            let why = "the CA public key is not the escrow package's";
            return Err(StoreError::invalid(why));
        }
        let operation = key.bind(command.operation(operation_id));
        // TASK-10 (H-06): Entry 1 commits only public state/ records (including the public
        // escrow package). The wrapped Org CA private key record (`ca-key.v1.json`) is
        // written strictly to local security storage outside `durable/recovery/` segments.
        store.submit(Some(operation), false, move |uow| {
            let change = create_organization::decide(uow, &here, &command)?;
            Ok(change.create(records::ca_escrow_path(), ca.package.clone()))
        })
    }
}

/// Imports an existing organization from validated Git state files as Entry 1 of a new HQ office
/// (`TASK-39` / R1, `doc-61` D25), generating a fresh HQ CA escrow package in the same atomic
/// journal entry.
pub struct ImportOrganizationOp {
    pub operation_id: Id,
    pub here: OfficeIdentity,
    pub command: crate::organization::import::ImportOrganization,
    pub ca: CaMaterial,
}

impl StoreOp for ImportOrganizationOp {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let Self {
            operation_id,
            here,
            command,
            ca,
        } = self;
        let key = ca.record(&here, &operation_id);
        key.check()
            .map_err(|why| StoreError::invalid(format!("the CA key record: {why}")))?;
        let sealed = records::ca_escrow_public_key(&ca.package, &here.organization_id)
            .map_err(|why| StoreError::invalid(format!("the CA escrow package: {why}")))?;
        if sealed != ca.public_key {
            return Err(StoreError::invalid(
                "the CA public key is not the escrow package's",
            ));
        }
        let operation = key.bind(command.operation(operation_id));
        store.submit(Some(operation), false, move |uow| {
            let change = crate::organization::import::decide_import(uow, &here, &command)?;
            Ok(change.create(records::ca_escrow_path(), ca.package.clone()))
        })
    }
}

/// One structure change the organization's owner asks for: the family decides it, and
/// with it whether `by` may (only the owner, `owner`, does). The caller has authenticated
/// `by`; the organization decides what `by` may do.
pub struct StructureOp<C> {
    /// Minted by the caller: a retry with it gets the original reply.
    pub operation_id: Id,
    /// The office deciding: the store's own identity.
    pub here: OfficeIdentity,
    /// The owner's employee ID, as the creation binding names them.
    pub owner: Id,
    pub by: Actor,
    pub command: C,
}

impl<C> StructureOp<C> {
    pub fn from_request(
        request: crate::organization::wire::StructureRequest<C>,
        here: OfficeIdentity,
        owner: Id,
    ) -> Self {
        Self {
            operation_id: request.operation_id,
            here,
            owner,
            by: request.by,
            command: request.command,
        }
    }
}

impl<C: Feature> StoreOp for StructureOp<C> {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let Self {
            operation_id,
            here,
            owner,
            by,
            command,
        } = self;
        let operation = command.operation(operation_id, &by);
        store.submit(Some(operation), false, move |uow| {
            command.decide(uow, &here, &owner, &by)
        })
    }
}

/// Dry-run of one structure change on the single writer: decides, frames, applies and
/// validates, then unconditionally undoes with zero journal entries, files, or receipts.
pub struct CheckOp<C> {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub by: Actor,
    pub command: C,
}

impl<C> CheckOp<C> {
    pub fn from_request(
        request: crate::organization::wire::StructureRequest<C>,
        here: OfficeIdentity,
        owner: Id,
    ) -> Self {
        Self {
            here,
            owner,
            by: request.by,
            command: request.command,
        }
    }
}

impl<C: Feature> StoreOp for CheckOp<C> {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let Self {
            here,
            owner,
            by,
            command,
        } = self;
        store.check(move |uow| command.decide(uow, &here, &owner, &by))
    }
}

/// Looks up one stored operation receipt by its UUIDv7 operation ID.
pub struct ReceiptById(pub Id);

impl ReadOp for ReceiptById {
    type Out = Option<Receipt>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Option<Receipt>, StoreError> {
        Ok(read.receipt(&self.0)?)
    }
}

/// Runs a project-family write operation on the store.
pub struct ProjectOp<C> {
    pub operation_id: Id,
    pub here: OfficeIdentity,
    pub owner: Id,
    pub by: Actor,
    pub command: C,
}

impl<C> ProjectOp<C> {
    pub fn from_request(
        request: crate::organization::wire::StructureRequest<C>,
        here: OfficeIdentity,
        owner: Id,
    ) -> Self {
        Self {
            operation_id: request.operation_id,
            here,
            owner,
            by: request.by,
            command: request.command,
        }
    }
}

impl<C: ProjectFeature> StoreOp for ProjectOp<C> {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let Self {
            operation_id,
            here,
            owner,
            by,
            command,
        } = self;
        let operation = command.operation(operation_id, &by);
        store.submit(Some(operation), false, move |uow| {
            command.decide(uow, &here, &owner, &by)
        })
    }
}

/// Dry-run of one project-family write operation on the single writer.
pub struct ProjectCheckOp<C> {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub by: Actor,
    pub command: C,
}

impl<C> ProjectCheckOp<C> {
    pub fn from_request(
        request: crate::organization::wire::StructureRequest<C>,
        here: OfficeIdentity,
        owner: Id,
    ) -> Self {
        Self {
            here,
            owner,
            by: request.by,
            command: request.command,
        }
    }
}

impl<C: ProjectFeature> StoreOp for ProjectCheckOp<C> {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let Self {
            here,
            owner,
            by,
            command,
        } = self;
        store.check(move |uow| command.decide(uow, &here, &owner, &by))
    }
}

/// Creates a project with HQ number/slug and its accountable owning team.
pub type CreateProjectOp = ProjectOp<CreateProject>;
/// Adds a contributing team to a project.
pub type AddProjectTeamOp = ProjectOp<AddProjectTeam>;
/// Removes a contributing team from a project.
pub type RemoveProjectTeamOp = ProjectOp<RemoveProjectTeam>;

/// Lists projects from a consistent snapshot.
pub struct ProjectsList {
    pub include_archived: bool,
}

impl ReadOp for ProjectsList {
    type Out = Vec<ProjectRecord>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        let mut projects = read.projects()?;
        if !self.include_archived {
            projects.retain(|p| !p.archived);
        }
        Ok(projects)
    }
}

/// Resolves one project by UUIDv7 ID, `P-<number>`, or canonical slug.
pub struct ProjectInspectByRef(pub String);

impl ReadOp for ProjectInspectByRef {
    type Out = Option<ProjectRecord>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        Ok(read.resolve_project(&self.0)?)
    }
}

/// Creates a team, under another or at the top level.
pub type CreateTeamOp = StructureOp<CreateTeam>;
/// Renames a team, or appoints or clears its lead.
pub type UpdateTeamOp = StructureOp<UpdateTeam>;
/// Moves a team, its subtree and their profiles under another team.
pub type MoveTeamOp = StructureOp<MoveTeam>;
/// Archives an empty team.
pub type ArchiveTeamOp = StructureOp<ArchiveTeam>;
/// Adds a person to a home team.
pub type AddPersonOp = StructureOp<AddPerson>;
/// Changes a person's profile, role or manager.
pub type UpdatePersonOp = StructureOp<UpdatePerson>;
/// Moves a person to another home team.
pub type MovePersonOp = StructureOp<MovePerson>;
/// Deactivates a person.
pub type DeactivatePersonOp = StructureOp<DeactivatePerson>;

/// The directory generation of the snapshot read: the organization stream's high-water.
pub struct DirectoryGeneration;

impl ReadOp for DirectoryGeneration {
    type Out = u64;

    fn read<S: TasksRead>(self, snapshot: &S) -> Result<u64, StoreError> {
        Ok(snapshot.generation()?)
    }
}

/// The organization's top-level team ID (`number == 1`), when an organization exists.
pub struct TopTeamId;

impl ReadOp for TopTeamId {
    type Out = Option<Id>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Option<Id>, StoreError> {
        Ok(read
            .teams()?
            .into_iter()
            .find(|team| team.number == 1)
            .map(|team| team.id))
    }
}

/// The organization's structure, as `org.snapshot` answers it.
pub struct DirectorySnapshot {
    /// The member whose account owns the organization: the creator, as the creation
    /// binding names them.
    pub owner: Id,
}

impl ReadOp for DirectorySnapshot {
    type Out = org_proto::OrgSnapshot;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        Ok(snapshot::project(read, &self.owner)?)
    }
}

/// Where an office stands in the organization its projection holds.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Standing {
    /// The organization's ID, when there is an organization.
    pub organization: Option<Id>,
    /// Whether the office is on record as the organization's HQ.
    pub hq: bool,
}

/// What an opening checks before it serves: the organization and the office's role.
pub struct OfficeStanding {
    pub office: Id,
}

impl ReadOp for OfficeStanding {
    type Out = Standing;

    fn read<S: TasksRead>(self, read: &S) -> Result<Standing, StoreError> {
        let organization = read.organization()?.map(|record| record.id);
        let office = read.office(&self.office)?;
        let hq = office.is_some_and(|office| office.role == records::OfficeRole::Hq);
        Ok(Standing { organization, hq })
    }
}

/// Lists all roles in this organization ordered by ID (`role.list`).
pub struct RolesList;

impl ReadOp for RolesList {
    type Out = Vec<records::Role>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        Ok(read.roles()?)
    }
}

/// Inspects one role by UUIDv7 ID, canonical key, or unique name (`role.inspect`).
pub struct RoleInspectByRef(pub String);

impl ReadOp for RoleInspectByRef {
    type Out = Option<records::Role>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        let target = self.0.trim();
        let roles = read.roles()?;
        if let Some(found) = roles
            .iter()
            .find(|r| r.id.as_str() == target || r.key.as_deref() == Some(target))
        {
            return Ok(Some(found.clone()));
        }
        let mut by_name = roles
            .into_iter()
            .filter(|r| r.name.eq_ignore_ascii_case(target));
        let first = by_name.next();
        if by_name.next().is_some() {
            Ok(None)
        } else {
            Ok(first)
        }
    }
}

/// Lists hire requests optionally filtered by `team_id` and `state` (`hr.hire.list`).
pub struct HiresList {
    pub team_id: Option<String>,
    pub state: Option<String>,
}

impl ReadOp for HiresList {
    type Out = Vec<records::HireRequest>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        let mut hires = read.hire_requests()?;
        if let Some(team) = &self.team_id {
            hires.retain(|h| h.home_team_id.as_str() == team);
        }
        if let Some(state) = &self.state {
            hires.retain(|h| h.state.as_str() == state);
        }
        Ok(hires)
    }
}

/// Reads one hire request by UUIDv7 ID (`hr.hire.inspect`).
pub struct HireInspectById(pub Id);

impl ReadOp for HireInspectById {
    type Out = Option<records::HireRequest>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        Ok(read.hire_request(&self.0)?)
    }
}

/// Reads one agent's execution configuration (`agent.execution.get`).
pub struct AgentExecutionById(pub Id);

impl ReadOp for AgentExecutionById {
    type Out = Option<records::AgentExecutionConfig>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        Ok(read.agent_execution(&self.0)?)
    }
}

/// Lists provider connections ordered by ID (`provider.connection.list`).
pub struct ProviderConnectionsList;

impl ReadOp for ProviderConnectionsList {
    type Out = Vec<records::ProviderConnection>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        Ok(read.provider_connections()?)
    }
}

/// Lists model profiles ordered by ID (`model_profile.list`).
pub struct ModelProfilesList;

impl ReadOp for ModelProfilesList {
    type Out = Vec<records::ModelProfile>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        Ok(read.model_profiles()?)
    }
}

/// Reads one employee record by UUIDv7 ID.
pub struct EmployeeById(pub Id);

impl ReadOp for EmployeeById {
    type Out = Option<records::Employee>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        Ok(read.employee(&self.0)?)
    }
}

/// Reads one role record by UUIDv7 ID.
pub struct RoleById(pub Id);

impl ReadOp for RoleById {
    type Out = Option<records::Role>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        Ok(read.role(&self.0)?)
    }
}

/// Reads one provider connection by UUIDv7 ID.
pub struct ProviderConnectionById(pub Id);

impl ReadOp for ProviderConnectionById {
    type Out = Option<records::ProviderConnection>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        Ok(read.provider_connection(&self.0)?)
    }
}

/// Reads one model profile by UUIDv7 ID.
pub struct ModelProfileById(pub Id);

impl ReadOp for ModelProfileById {
    type Out = Option<records::ModelProfile>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        Ok(read.model_profile(&self.0)?)
    }
}

/// Reads the organization's channel watching model profile setting (`ADR-0024` d13).
pub struct WatchingModelRead;

impl ReadOp for WatchingModelRead {
    type Out = Option<records::WatchingModelSetting>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        Ok(read.watching_model()?)
    }
}

/// Checks whether `member` may send a message to the built-in HR lead (ADR-0029 decision 3 /
/// HIRE-2) when addressing `target` (`"hr"` or the HR lead's employee ID).
pub struct CheckHrMessageAccess {
    pub owner: Id,
    pub sender: Id,
    pub target: String,
}

impl ReadOp for CheckHrMessageAccess {
    type Out = bool;

    fn read<S: TasksRead>(self, read: &S) -> Result<bool, StoreError> {
        let employees = read.employees()?;
        let hr = employees
            .iter()
            .find(|e| e.builtin_key == Some(records::BuiltinKey::Hr));
        let raw = self.target.trim_matches(|c: char| c.is_ascii_whitespace());
        let trimmed = raw.strip_prefix('@').unwrap_or(raw);
        let target_is_hr = trimmed.eq_ignore_ascii_case("hr")
            || hr.is_some_and(|e| e.id.as_str() == raw || e.handle.eq_ignore_ascii_case(trimmed));
        if !target_is_hr {
            return Ok(true);
        }
        let Some(sender) = employees.iter().find(|e| e.id == self.sender) else {
            return Ok(false);
        };
        crate::organization::features::authority::may_hire_anywhere(read, &self.owner, sender)
    }
}

/// Counts registered offices in this organization so turn-permit authority selects
/// `office_local` for single-office setups and `hq_pool` for shared multi-office setups (`doc-88` §6).
pub struct OfficesCount;

impl ReadOp for OfficesCount {
    type Out = usize;

    fn read<S: TasksRead>(self, read: &S) -> Result<usize, StoreError> {
        Ok(read.offices()?.len())
    }
}

/// Lists all canonical office IDs registered in this organization's `org-store` (`TASK-52` H-13).
pub struct OfficeIdsList;

impl ReadOp for OfficeIdsList {
    type Out = Vec<String>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Vec<String>, StoreError> {
        Ok(read
            .offices()?
            .into_iter()
            .map(|o| o.id.as_str().to_owned())
            .collect())
    }
}

/// Lists all offices registered in this organization's replicated `org-store` (`TASK-120`).
pub struct OfficesList;

impl ReadOp for OfficesList {
    type Out = Vec<crate::organization::records::Office>;

    fn read<S: TasksRead>(
        self,
        read: &S,
    ) -> Result<Vec<crate::organization::records::Office>, StoreError> {
        Ok(read.offices()?)
    }
}

/// Registers (or idempotently refreshes) an enrolled satellite office at HQ (`TASK-52`).
pub type RegisterSatelliteOfficeOp =
    StructureOp<crate::organization::features::offices::RegisterSatelliteOffice>;

/// Applies an HQ-authoritative batch of `state/` files onto a satellite office's store (`TASK-52`).
pub struct ApplyReplicatedFilesOp {
    pub operation_id: Id,
    pub actor: Actor,
    pub command: crate::organization::features::offices::ApplyReplicatedFiles,
}

impl StoreOp for ApplyReplicatedFilesOp {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let op = self.command.operation(self.operation_id, self.actor);
        let cmd = self.command;
        store.submit(Some(op), true, move |uow| cmd.decide(uow))
    }
}
