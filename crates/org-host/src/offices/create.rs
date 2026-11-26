//! Creating an HQ office: the bounded create slots, provisioning and adding the created
//! office to the catalog.

use std::time::Instant;

use org_office_launch::OfficeFile;
use org_proto::{ApiError, ErrorCode, Organization, OrganizationCreated};

use crate::host_log;
use crate::supervisor::{CreateOutcome, CreateStartError, OfficeState};
use crate::sync::lock;

use super::Offices;

/// How many creates may run at once; more wait for a slot (within [`Timings::create_wait`](crate::Timings::create_wait)).
const MAX_CREATES_IN_FLIGHT: usize = 4;

/// The answer to a create that found no free slot within [`Timings::create_wait`](crate::Timings::create_wait).
const TOO_MANY_CREATES: &str = "too many creates in progress";

impl Offices {
    /// Takes one of the [`MAX_CREATES_IN_FLIGHT`] create slots, waiting until `deadline` for
    /// one to be freed; then `unavailable` ("too many creates in progress").
    fn create_slot(&self, deadline: Instant) -> Result<CreateSlot<'_>, ApiError> {
        let mut creates = lock(&self.creates);
        while *creates >= MAX_CREATES_IN_FLIGHT {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(ApiError::new(ErrorCode::Unavailable, TOO_MANY_CREATES));
            }
            creates = self
                .create_slot_freed
                .wait_timeout(creates, left)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
        *creates += 1;
        Ok(CreateSlot(self))
    }

    /// Actual selected create on the existing supervisor. A lost/failed attempt
    /// retains all intent, data and custody; no legacy discard or key sweep runs.
    pub(crate) fn provision_hq(
        &self,
        first: org_office_launch::CreateLine,
        recovery: org_proto::RecoveryCode,
    ) -> Result<(), ApiError> {
        let deadline = Instant::now() + self.create_wait;
        self.wait_for_startup(deadline)?;
        let p = &first.hq_creation.bootstrap;
        let office = p.office_id.clone();
        let stored = lock(&self.catalog).office(&office).cloned();
        if let Some(stored) = stored {
            if stored.org_id != p.organization_id
                || stored.org_name != p.plan.name
                || stored.version() != 2
            {
                return Err(storage_error());
            }
            // Completion is the durable identity: the worker reported `created` and the
            // host adopted the office, whatever its status is now.
            return Ok(());
        }
        lock(&self.catalog)
            .reserve_selected(p)
            .map_err(|_| already_exists())?;
        let _slot = self.create_slot(deadline)?;
        let org = first.org_id.clone();
        let name = first.org_name.clone();
        let pending = self
            .supervisor
            .begin_hq_create(first, recovery)
            .map_err(|error| {
                let phase = match error {
                    CreateStartError::NotStarted(_) => "selected HQ launch preparation refused",
                    CreateStartError::KeyStored(_) => "selected HQ admitted launch/handoff failed",
                };
                host_log::log(phase);
                selected_create_unresolved()
            })?;
        match pending.wait(self.create_timeout) {
            CreateOutcome::Created(adoptable) => {
                let file = &adoptable.office;
                if file.office_id != office
                    || file.org_id != org
                    || file.org_name != name
                    || file.role != org_proto::OfficeRole::Hq
                    || file.version() != 2
                {
                    host_log::log("selected HQ returned mismatched office metadata");
                    adoptable.stop();
                    return Err(selected_create_unresolved());
                }
                let file = file.clone();
                self.supervisor.adopt(*adoptable);
                self.add_created(&file, |_| {});
                Ok(())
            }
            // Published effects may exist after every such outcome. The caller
            // retains its original operation and reconciles; never a rejection.
            outcome => {
                let phase = match outcome {
                    CreateOutcome::Failed => "selected HQ worker reported creation failure",
                    CreateOutcome::Crashed => "selected HQ worker closed before created",
                    CreateOutcome::TimedOut => "selected HQ worker creation timed out",
                    CreateOutcome::Stopped => "selected HQ creation interrupted by host stop",
                    CreateOutcome::Created(_) => unreachable!(),
                };
                host_log::log(phase);
                Err(selected_create_unresolved())
            }
        }
    }

    /// Adds a created office to the catalog, queues the creator's answer (`respond`) and
    /// publishes `organization.created`, all in one broadcast critical section, each from the
    /// office's state at that moment. So no `office.status_changed` for the new office reaches
    /// anyone before its `organization.created` (it isn't in the catalog until then, and the
    /// next one waits for the lock), and the creator's answer is never older than an event about
    /// the office it gets after it.
    pub(super) fn add_created(&self, office: &OfficeFile, respond: impl FnOnce(&Organization)) {
        self.broadcast.critical(|held| {
            lock(&self.catalog).insert(office.clone());
            let organization = self.created(office, self.state_of(&office.office_id));
            respond(&organization);
            held.publish::<OrganizationCreated>(&organization);
        });
    }

    /// The organization a create made, with its one office in `state`.
    fn created(&self, office: &OfficeFile, state: OfficeState) -> Organization {
        Organization {
            id: office.org_id.clone(),
            name: office.org_name.clone(),
            created_at: office.created_at.clone(),
            offices: vec![self.office_info(office, state)],
        }
    }
}

/// A create slot, freed when dropped.
struct CreateSlot<'a>(&'a Offices);

impl Drop for CreateSlot<'_> {
    fn drop(&mut self) {
        *lock(&self.0.creates) -= 1;
        self.0.create_slot_freed.notify_one();
    }
}

fn already_exists() -> ApiError {
    ApiError::new(
        ErrorCode::AlreadyExists,
        "an organization with that name already exists on this Mac",
    )
}

fn storage_error() -> ApiError {
    ApiError::new(
        ErrorCode::StorageError,
        "the organization couldn't be created; try again",
    )
}

fn selected_create_unresolved() -> ApiError {
    ApiError::new(
        ErrorCode::Unavailable,
        "HQ creation is unresolved; retry the original operation",
    )
}
