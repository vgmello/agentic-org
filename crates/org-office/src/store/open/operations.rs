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
        records,
    },
    projects::{
        features::{
            AddProjectTeam, AttachProjectRepository, CreateProject, ProjectFeature,
            RegisterRepository, RemoveProjectTeam,
        },
        records::Project as ProjectRecord,
    },
    structure::snapshot,
    tasks::ports::TasksRead,
};

fn bind_ca_operation(
    ca: &CaMaterial,
    here: &OfficeIdentity,
    operation_id: &Id,
    op: org_store::Operation,
) -> Result<org_store::Operation, StoreError> {
    let key = ca.record(here, operation_id);
    key.check()
        .map_err(|why| StoreError::invalid(format!("the CA key record: {why}")))?;
    let sealed = records::ca_escrow_public_key(&ca.package, &here.organization_id)
        .map_err(|why| StoreError::invalid(format!("the CA escrow package: {why}")))?;
    if sealed != ca.public_key {
        return Err(StoreError::invalid(
            "the CA public key is not the escrow package's",
        ));
    }
    Ok(key.bind(op))
}

fn run_ca_bootstrap_op<E: OfficeEngine>(
    store: &Store<E>,
    operation_id: Id,
    here: OfficeIdentity,
    ca: CaMaterial,
    op: org_store::Operation,
    mut decide: impl FnMut(
            &mut org_store::UnitOfWork<'_, E>,
            &OfficeIdentity,
        ) -> Result<org_store::Change, org_store::Refusal>
        + Send
        + 'static,
) -> Result<Reply, StoreError> {
    let operation = bind_ca_operation(&ca, &here, &operation_id, op)?;
    let pkg = ca.package;
    store.submit(Some(operation), false, move |uow| {
        Ok(decide(uow, &here)?.create(records::ca_escrow_path(), pkg.clone()))
    })
}

/// Creates the organization with the HQ's CA escrow package in Entry 1 (ADR-0007, step 3).
pub struct CreateOrganizationOp {
    pub operation_id: Id,
    pub here: OfficeIdentity,
    pub command: CreateOrganization,
    pub ca: CaMaterial,
}

impl StoreOp for CreateOrganizationOp {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let op = self.command.operation(self.operation_id.clone());
        let cmd = self.command;
        run_ca_bootstrap_op(
            store,
            self.operation_id,
            self.here,
            self.ca,
            op,
            move |uow, h| create_organization::decide(uow, h, &cmd),
        )
    }
}

/// Imports an existing organization from validated Git state files as Entry 1 of a new HQ office.
pub struct ImportOrganizationOp {
    pub operation_id: Id,
    pub here: OfficeIdentity,
    pub command: crate::organization::import::ImportOrganization,
    pub ca: CaMaterial,
}

impl StoreOp for ImportOrganizationOp {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let op = self.command.operation(self.operation_id.clone());
        let cmd = self.command;
        run_ca_bootstrap_op(
            store,
            self.operation_id,
            self.here,
            self.ca,
            op,
            move |uow, h| crate::organization::import::decide_import(uow, h, &cmd),
        )
    }
}

macro_rules! feature_store_ops {
    ($op:ident, $check:ident, $trait:ident) => {
        pub struct $op<C> {
            pub operation_id: Id,
            pub here: OfficeIdentity,
            pub owner: Id,
            pub by: Actor,
            pub command: C,
        }
        impl<C> $op<C> {
            pub fn from_request(
                request: crate::structure::wire::StructureRequest<C>,
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
        impl<C: $trait> StoreOp for $op<C> {
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
        pub struct $check<C> {
            pub here: OfficeIdentity,
            pub owner: Id,
            pub by: Actor,
            pub command: C,
        }
        impl<C> $check<C> {
            pub fn from_request(
                request: crate::structure::wire::StructureRequest<C>,
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
        impl<C: $trait> StoreOp for $check<C> {
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
    };
}

feature_store_ops!(StructureOp, CheckOp, Feature);
feature_store_ops!(ProjectOp, ProjectCheckOp, ProjectFeature);

pub type CreateProjectOp = ProjectOp<CreateProject>;
pub type AddProjectTeamOp = ProjectOp<AddProjectTeam>;
pub type RemoveProjectTeamOp = ProjectOp<RemoveProjectTeam>;
pub type RegisterRepositoryOp = ProjectOp<RegisterRepository>;
pub type AttachProjectRepositoryOp = ProjectOp<AttachProjectRepository>;

pub type CreateTeamOp = StructureOp<CreateTeam>;
pub type UpdateTeamOp = StructureOp<UpdateTeam>;
pub type MoveTeamOp = StructureOp<MoveTeam>;
pub type ArchiveTeamOp = StructureOp<ArchiveTeam>;
pub type AddPersonOp = StructureOp<AddPerson>;
pub type UpdatePersonOp = StructureOp<UpdatePerson>;
pub type MovePersonOp = StructureOp<MovePerson>;
pub type DeactivatePersonOp = StructureOp<DeactivatePerson>;
pub type RegisterSatelliteOfficeOp =
    StructureOp<crate::organization::features::offices::RegisterSatelliteOffice>;

/// Where an office stands in the organization its projection holds.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Standing {
    pub organization: Option<Id>,
    pub hq: bool,
}

macro_rules! simple_read_op {
    ($(#[$m:meta])* $name:ident => $out:ty : |$read:ident| $body:expr) => {
        $(#[$m])*
        pub struct $name;
        impl ReadOp for $name {
            type Out = $out;
            fn read<S: TasksRead>(self, $read: &S) -> Result<Self::Out, StoreError> { $body }
        }
    };
    ($(#[$m:meta])* $name:ident($arg:ty) => $out:ty : |$this:ident, $read:ident| $body:expr) => {
        $(#[$m])*
        pub struct $name(pub $arg);
        impl ReadOp for $name {
            type Out = $out;
            fn read<S: TasksRead>(self, $read: &S) -> Result<Self::Out, StoreError> {
                let $this = self;
                $body
            }
        }
    };
    ($(#[$m:meta])* $name:ident { $($f:ident : $t:ty),+ $(,)? } => $out:ty : |$this:ident, $read:ident| $body:expr) => {
        $(#[$m])*
        pub struct $name { $(pub $f: $t),+ }
        impl ReadOp for $name {
            type Out = $out;
            fn read<S: TasksRead>(self, $read: &S) -> Result<Self::Out, StoreError> {
                let $this = self;
                $body
            }
        }
    };
}

simple_read_op!(/// Looks up one stored operation receipt by its UUIDv7 operation ID.
    ReceiptById(Id) => Option<Receipt> : |this, read| Ok(read.receipt(&this.0)?));
simple_read_op!(/// Resolves one project by UUIDv7 ID, `P-<number>`, or canonical slug.
    ProjectInspectByRef(String) => Option<ProjectRecord> : |this, read| Ok(read.resolve_project(&this.0)?));
simple_read_op!(/// The directory generation of the snapshot read: the organization stream's high-water.
    DirectoryGeneration => u64 : |read| Ok(read.generation()?));
simple_read_op!(/// The organization's top-level team ID (`number == 1`), when an organization exists.
    TopTeamId => Option<Id> : |read| Ok(read.teams()?.into_iter().find(|t| t.number == 1).map(|t| t.id)));
simple_read_op!(/// The organization's structure, as `org.snapshot` answers it.
    DirectorySnapshot { owner: Id } => org_proto::OrgSnapshot : |this, read| Ok(snapshot::project(read, &this.owner)?));
simple_read_op!(/// What an opening checks before it serves: the organization and the office's role.
OfficeStanding { office: Id } => Standing : |this, read| {
    let organization = read.organization()?.map(|r| r.id);
    let hq = read.office(&this.office)?.is_some_and(|o| o.role == records::OfficeRole::Hq);
    Ok(Standing { organization, hq })
});
simple_read_op!(/// Lists projects from a consistent snapshot.
ProjectsList { include_archived: bool } => Vec<ProjectRecord> : |this, read| {
    let mut projects = read.projects()?;
    if !this.include_archived {
        projects.retain(|p| !p.archived);
    }
    Ok(projects)
});
simple_read_op!(/// Lists all roles in this organization ordered by ID (`role.list`).
    RolesList => Vec<records::Role> : |read| Ok(read.roles()?));
simple_read_op!(/// Reads one hire request by UUIDv7 ID (`hr.hire.inspect`).
    HireInspectById(Id) => Option<records::HireRequest> : |this, read| Ok(read.hire_request(&this.0)?));
simple_read_op!(/// Reads one agent's execution configuration (`agent.execution.get`).
    AgentExecutionById(Id) => Option<records::AgentExecutionConfig> : |this, read| Ok(read.agent_execution(&this.0)?));
simple_read_op!(/// Lists provider connections ordered by ID (`provider.connection.list`).
    ProviderConnectionsList => Vec<records::ProviderConnection> : |read| Ok(read.provider_connections()?));
simple_read_op!(/// Lists model profiles ordered by ID (`model_profile.list`).
    ModelProfilesList => Vec<records::ModelProfile> : |read| Ok(read.model_profiles()?));
simple_read_op!(/// Reads one employee record by UUIDv7 ID.
    EmployeeById(Id) => Option<records::Employee> : |this, read| Ok(read.employee(&this.0)?));
simple_read_op!(/// Lists all employees in the organization.
    EmployeesList => Vec<records::Employee> : |read| Ok(read.employees()?));
simple_read_op!(/// Reads one role record by UUIDv7 ID.
    RoleById(Id) => Option<records::Role> : |this, read| Ok(read.role(&this.0)?));
simple_read_op!(/// Reads one provider connection by UUIDv7 ID.
    ProviderConnectionById(Id) => Option<records::ProviderConnection> : |this, read| Ok(read.provider_connection(&this.0)?));
simple_read_op!(/// Reads one model profile by UUIDv7 ID.
    ModelProfileById(Id) => Option<records::ModelProfile> : |this, read| Ok(read.model_profile(&this.0)?));
simple_read_op!(/// Reads the organization's channel watching model profile setting (`ADR-0024` d13).
    WatchingModelRead => Option<records::WatchingModelSetting> : |read| Ok(read.watching_model()?));
simple_read_op!(/// Counts registered offices in this organization (`doc-88` §6).
    OfficesCount => usize : |read| Ok(read.offices()?.len()));
simple_read_op!(/// Lists all canonical office IDs registered in this organization's `org-store`.
    OfficeIdsList => Vec<String> : |read| Ok(read.offices()?.into_iter().map(|o| o.id.as_str().to_owned()).collect()));
simple_read_op!(/// Lists all offices registered in this organization's replicated `org-store`.
    OfficesList => Vec<crate::organization::records::Office> : |read| Ok(read.offices()?));

simple_read_op!(/// Inspects one role by UUIDv7 ID, canonical key, or unique name (`role.inspect`).
RoleInspectByRef(String) => Option<records::Role> : |this, read| {
    let target = this.0.trim();
    let roles = read.roles()?;
    let direct = roles
        .iter()
        .find(|r| r.id.as_str() == target)
        .or_else(|| roles.iter().find(|r| r.key.as_deref() == Some(target)));
    if let Some(found) = direct {
        return Ok(Some(found.clone()));
    }
    let mut by_name = roles.into_iter().filter(|r| r.name.eq_ignore_ascii_case(target));
    let first = by_name.next();
    Ok(if by_name.next().is_some() { None } else { first })
});

simple_read_op!(/// Lists hire requests optionally filtered by `team_id` and `state` (`hr.hire.list`).
HiresList { team_id: Option<String>, state: Option<String> } => Vec<records::HireRequest> : |this, read| {
    let mut hires = read.hire_requests()?;
    if let Some(team) = &this.team_id {
        hires.retain(|h| h.home_team_id.as_str() == team);
    }
    if let Some(state) = &this.state {
        hires.retain(|h| h.state.as_str() == state);
    }
    Ok(hires)
});

simple_read_op!(/// Lists team limit increase requests visible to `actor`, optionally filtered (`team.limit_request.list`).
TeamLimitRequestsList {
    owner: Id,
    actor: Id,
    team_id: Option<String>,
    state: Option<String>,
    approver_employee_id: Option<String>,
} => Vec<records::TeamLimitRequest> : |this, read| {
    let Some(actor) = read.employee(&this.actor)? else { return Ok(Vec::new()); };
    let owner = crate::organization::owner::effective_owner_id(read, &this.owner)?;
    let mut out = Vec::new();
    for req in read.team_limit_requests()? {
        let excluded = this.team_id.as_deref().is_some_and(|t| req.team_id.as_str() != t)
            || this.state.as_deref().is_some_and(|s| req.state.as_str() != s)
            || this.approver_employee_id.as_deref().is_some_and(|a| req.approver_employee_id.as_str() != a);
        if !excluded && crate::organization::features::can_view_limit_request(read, &owner, &actor, &req)? {
            out.push(req);
        }
    }
    Ok(out)
});

simple_read_op!(/// Checks whether `member` may send a message to the built-in HR lead (ADR-0029 decision 3 / HIRE-2).
CheckHrMessageAccess { owner: Id, sender: Id, target: String } => bool : |this, read| {
    let employees = read.employees()?;
    let hr = employees.iter().find(|e| e.builtin_key == Some(records::BuiltinKey::Hr));
    let raw = this.target.trim_matches(|c: char| c.is_ascii_whitespace());
    let trimmed = raw.strip_prefix('@').unwrap_or(raw);
    let is_hr = trimmed.eq_ignore_ascii_case("hr")
        || hr.is_some_and(|e| e.id.as_str() == raw || e.handle.eq_ignore_ascii_case(trimmed));
    if !is_hr {
        return Ok(true);
    }
    let Some(sender) = employees.iter().find(|e| e.id == this.sender) else { return Ok(false); };
    let owner = crate::organization::owner::effective_owner_id(read, &this.owner)?;
    crate::organization::authority::may_address_hr(read, &owner, sender)
});

/// Applies an HQ-authoritative batch of `state/` files onto a satellite office's store (`TASK-52`).
pub struct ApplyReplicatedFilesOp {
    pub operation_id: Id,
    pub actor: Actor,
    pub command: crate::replication::state_files::ApplyReplicatedFiles,
}

impl StoreOp for ApplyReplicatedFilesOp {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let op = self.command.operation(self.operation_id, self.actor);
        let cmd = self.command;
        store.submit(Some(op), true, move |uow| cmd.decide(uow))
    }
}
