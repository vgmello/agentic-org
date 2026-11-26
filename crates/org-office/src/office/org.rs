//! The organization on an office: its snapshot, who is who, organization commands and checks,
//! projects and operation status.

use org_proto::{
    directory::{WhoParams, WhoResult},
    org::{CommandAccepted, OrgSnapshot},
    ApiError, ErrorCode, OperationOutcome,
};
use org_store::{Actor, Id, StoreError};

use crate::{
    error::OfficeError,
    organization::{directory, records},
    rules::OrgCommand,
    store::open::{
        AddPersonOp, AddProjectTeamOp, ArchiveTeamOp, CheckOp, CreateProjectOp, CreateTeamOp,
        DeactivatePersonOp, DirectorySnapshot, MovePersonOp, MoveTeamOp, ProjectCheckOp,
        ProjectInspectByRef, ProjectsList, ReceiptById, RemoveProjectTeamOp, TopTeamId,
        UpdatePersonOp, UpdateTeamOp,
    },
    structure::wire,
    time,
};

use super::{storage, Office};

impl Office {
    /// The organization's structure from one consistent read of the projection.
    pub fn org_snapshot(&self) -> Result<OrgSnapshot, OfficeError> {
        let read = DirectorySnapshot {
            owner: self.owner.clone(),
        };
        Ok(self.store.read(read).map_err(storage)?.0)
    }

    /// Returns the office's live runtime presence/status record for `member_id`, if any (`AVAIL-5`).
    pub fn member_runtime(&self, member_id: &str) -> Option<directory::MemberRuntime> {
        use directory::RuntimeView;
        self.runtime.runtime_of(member_id)
    }

    /// `who`: the directory cards matching `params`, including live runtime presence and
    /// unexpired self-reported status lines (`status.set`).
    pub fn who(&self, params: &WhoParams) -> Result<Result<WhoResult, ApiError>, OfficeError> {
        self.who_with_runtime(params, &self.runtime)
    }

    /// Like [`Self::who`], overlaying caller-provided live session presence (such as
    /// `WaitingForAnswer`, `Busy`, or `UnableToRun` from `org-office-sessions`) on top of
    /// stored `status.set` lines (`ASK-4`, `AVAIL-4`).
    pub fn who_with_runtime(
        &self,
        params: &WhoParams,
        runtime: &dyn directory::RuntimeView,
    ) -> Result<Result<WhoResult, ApiError>, OfficeError> {
        let snapshot = self.org_snapshot()?;
        let office = directory::ThisOffice {
            id: &self.info.office_id,
            name: &self.info.office_name,
        };
        let now = time::format_rfc3339_ms(self.monotonic_now_ms());
        let merged = OverlayRuntime {
            primary: runtime,
            fallback: &self.runtime,
        };
        Ok(directory::who(&snapshot, office, &now, &merged, params))
    }

    /// Runs one organization command from `member`, the member the caller's grant is bound
    /// to, dispatching all implemented organization-family operations on `org-store`. A
    /// retry with the same `command_id` gets the same answer, bypassing age/skew checks.
    pub fn submit_org(
        &self,
        member: &str,
        command: OrgCommand,
    ) -> Result<Result<CommandAccepted, ApiError>, OfficeError> {
        if member != self.owner.as_str() {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                crate::organization::features::NOT_AUTHORIZED,
            )));
        }
        let op = match wire::parse_command(member, &command, || {
            self.store.read(TopTeamId).map(|(team, _)| team)
        })
        .map_err(storage)?
        {
            Ok(op) => op,
            Err(refusal) => return Ok(Err(refusal)),
        };
        // Stored answers always replay even after the 29-day window; only unknown command IDs
        // are subject to `expired` and `clock_skew` admission rules (unrecorded).
        let op_id = op.operation_id().clone();
        let (existing, _) = self
            .store
            .read(ReceiptById(op_id.clone()))
            .map_err(storage)?;
        if existing.is_none() {
            if let Err(refusal) =
                wire::check_command_window(op_id.as_str(), self.monotonic_now_ms())
            {
                return Ok(Err(refusal));
            }
        }
        let here = &self.identity;
        let owner = &self.owner;
        let run = |op: wire::WireOp| -> Result<Result<CommandAccepted, ApiError>, StoreError> {
            match op {
                wire::WireOp::CreateTeam(r) => wire::team_create_answer(
                    self.store
                        .run(CreateTeamOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::UpdateTeam(r) => wire::changed_answer(
                    "team.update",
                    self.store
                        .run(UpdateTeamOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::MoveTeam(r) => wire::changed_answer(
                    "team.move",
                    self.store
                        .run(MoveTeamOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::ArchiveTeam(r) => wire::changed_answer(
                    "team.delete",
                    self.store
                        .run(ArchiveTeamOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::AddPerson(r) => wire::changed_answer(
                    "member.add_person",
                    self.store
                        .run(AddPersonOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::UpdatePerson(r) => wire::changed_answer(
                    "member.update_person",
                    self.store
                        .run(UpdatePersonOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::MovePerson(r) => wire::changed_answer(
                    "member.move",
                    self.store
                        .run(MovePersonOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::DeactivatePerson(r) => wire::changed_answer(
                    "member.remove",
                    self.store.run(DeactivatePersonOp::from_request(
                        r,
                        here.clone(),
                        owner.clone(),
                    )),
                ),
                wire::WireOp::CreateProject(r) => wire::project_create_answer(self.store.run(
                    CreateProjectOp::from_request(r, here.clone(), owner.clone()),
                )),
                wire::WireOp::AddProjectTeam(r) => wire::changed_answer(
                    "project.add_team",
                    self.store.run(AddProjectTeamOp::from_request(
                        r,
                        here.clone(),
                        owner.clone(),
                    )),
                ),
                wire::WireOp::RemoveProjectTeam(r) => wire::changed_answer(
                    "project.remove_team",
                    self.store.run(RemoveProjectTeamOp::from_request(
                        r,
                        here.clone(),
                        owner.clone(),
                    )),
                ),
            }
        };
        run(op).map_err(storage)
    }

    /// Runs a dry-run check of one organization command from `member` through `Store::check`:
    /// decides, frames, applies, validates, and unconditionally undoes with zero journal
    /// entries, files, receipts, or sequence advancement.
    pub fn check_org(
        &self,
        member: &str,
        command: OrgCommand,
    ) -> Result<Result<CommandAccepted, ApiError>, OfficeError> {
        if member != self.owner.as_str() {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                crate::organization::features::NOT_AUTHORIZED,
            )));
        }
        let op = match wire::parse_check_command(member, &command, || {
            self.store.read(TopTeamId).map(|(team, _)| team)
        })
        .map_err(storage)?
        {
            Ok(op) => op,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let here = &self.identity;
        let owner = &self.owner;
        let run = |op: wire::WireOp| -> Result<Result<CommandAccepted, ApiError>, StoreError> {
            match op {
                wire::WireOp::CreateTeam(r) => wire::checked_answer(
                    "team.create",
                    self.store
                        .run(CheckOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::UpdateTeam(r) => wire::checked_answer(
                    "team.update",
                    self.store
                        .run(CheckOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::MoveTeam(r) => wire::checked_answer(
                    "team.move",
                    self.store
                        .run(CheckOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::ArchiveTeam(r) => wire::checked_answer(
                    "team.delete",
                    self.store
                        .run(CheckOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::AddPerson(r) => wire::checked_answer(
                    "member.add_person",
                    self.store
                        .run(CheckOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::UpdatePerson(r) => wire::checked_answer(
                    "member.update_person",
                    self.store
                        .run(CheckOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::MovePerson(r) => wire::checked_answer(
                    "member.move",
                    self.store
                        .run(CheckOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::DeactivatePerson(r) => wire::checked_answer(
                    "member.remove",
                    self.store
                        .run(CheckOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::CreateProject(r) => wire::checked_answer(
                    "project.create",
                    self.store
                        .run(ProjectCheckOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::AddProjectTeam(r) => wire::checked_answer(
                    "project.add_team",
                    self.store
                        .run(ProjectCheckOp::from_request(r, here.clone(), owner.clone())),
                ),
                wire::WireOp::RemoveProjectTeam(r) => wire::checked_answer(
                    "project.remove_team",
                    self.store
                        .run(ProjectCheckOp::from_request(r, here.clone(), owner.clone())),
                ),
            }
        };
        run(op).map_err(storage)
    }

    /// Lists projects from a consistent snapshot of the office store (B3).
    pub fn list_projects(
        &self,
        include_archived: bool,
    ) -> Result<org_proto::projects::ProjectListResult, OfficeError> {
        let (projects, _) = self
            .store
            .read(ProjectsList { include_archived })
            .map_err(storage)?;
        Ok(org_proto::projects::ProjectListResult {
            projects: projects.iter().map(|p| p.to_wire()).collect(),
        })
    }

    /// Inspects one project by UUIDv7 ID, `P-<number>`, or canonical slug (B3).
    pub fn inspect_project(
        &self,
        reference: &str,
    ) -> Result<Result<org_proto::projects::ProjectRecord, ApiError>, OfficeError> {
        let (found, _) = self
            .store
            .read(ProjectInspectByRef(reference.to_owned()))
            .map_err(storage)?;
        Ok(found
            .map(|p| p.to_wire())
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "project not found")))
    }

    /// Reads the stored outcome of an organization command by its UUIDv7 `command_id` for
    /// `member`. Refuses another actor's receipt and non-organization receipts as `not_found`.
    pub fn operation(
        &self,
        member: &str,
        command_id: &str,
    ) -> Result<Result<OperationOutcome, ApiError>, OfficeError> {
        let not_found = || ApiError::new(ErrorCode::NotFound, "operation not found");
        if !records::is_stable_id(command_id) {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "command_id is not a UUIDv7 in lowercase text",
            )));
        }
        let Ok(op_id) = Id::parse(command_id) else {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "command_id is not a UUIDv7 in lowercase text",
            )));
        };
        if !records::is_stable_id(member) {
            return Ok(Err(not_found()));
        }
        let Ok(actor_id) = Id::parse(member) else {
            return Ok(Err(not_found()));
        };
        let (receipt, _) = self.store.read(ReceiptById(op_id)).map_err(storage)?;
        let Some(receipt) = receipt else {
            return Ok(Err(not_found()));
        };
        if !receipt.kind.starts_with("organization.")
            || receipt.kind == "organization.create"
            || receipt.actor != Actor::Employee(actor_id)
        {
            return Ok(Err(not_found()));
        }
        let outcome = wire::operation_outcome_from_receipt(&receipt).map_err(storage)?;
        Ok(Ok(outcome))
    }
}

struct OverlayRuntime<'a> {
    primary: &'a dyn directory::RuntimeView,
    fallback: &'a dyn directory::RuntimeView,
}

impl directory::RuntimeView for OverlayRuntime<'_> {
    fn runtime_of(&self, member_id: &str) -> Option<directory::MemberRuntime> {
        match (
            self.primary.runtime_of(member_id),
            self.fallback.runtime_of(member_id),
        ) {
            (Some(mut prim), Some(base)) => {
                if prim.status.is_none() {
                    prim.status = base.status;
                    prim.status_expires_at = base.status_expires_at;
                }
                if prim.current_task_id.is_none() {
                    prim.current_task_id = base.current_task_id;
                }
                if prim.last_seen_at.is_none() {
                    prim.last_seen_at = base.last_seen_at;
                }
                Some(prim)
            }
            (Some(prim), None) => Some(prim),
            (None, base) => base,
        }
    }
}
