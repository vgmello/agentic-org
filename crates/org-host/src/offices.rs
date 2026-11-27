//! This Mac's offices: the catalog joined with supervision. Answers what the service needs
//! (organizations, offices, their status and endpoints), runs `organization.create`, and
//! discards unfinished creates.
//!
//! The catalog mutex guards only a create's name check and its reservation of the name and
//! handle (see `Catalog::plan_create`), never the server run that follows, so creates of
//! different names run side by side and listing never waits for a create.
//!
//! The unfinished folders found at startup are discarded in the background, a few at a time,
//! so the host answers at once; only the calls that need a settled catalog wait for them
//! (`organization.create` and `office.restart`).
//!
//! Every change of an office's state is announced as `office.status_changed` through the
//! [`Broadcast`], in order with the list answers.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use org_office_launch::{FailCode, OfficeFile};
use org_proto::{
    ApiError, ErrorCode, OfficeInfo, OfficeStatus, OfficeStatusChanged, OfficeStatusChangedData,
    OfficeStatusReason, Organization,
};

use crate::broadcast::Broadcast;
use crate::catalog::{self, Catalog, OrganizationEntry};
use crate::clock;
use crate::host_log;
use crate::supervisor::{DiscardOutcome, OfficeState, RestartOutcome, StatusObserver, Supervisor};
use crate::sync::lock;
use crate::timings::Timings;

mod create;
mod join;
pub(crate) use join::{has_active_pending_join, stop_pending_join_for_office, SatelliteJoinSpec};

/// Where an office on this Mac runs, for display.
pub(crate) const THIS_MAC: &str = "This Mac";

/// How many startup discards run at once.
const STARTUP_DISCARDS_AT_ONCE: usize = 4;

/// The answer to a create that came before the startup discards finished, after waiting
/// [`Timings::create_wait`].
const HOST_STARTING: &str = "host starting";

pub(crate) struct Offices {
    home: PathBuf,
    catalog: Mutex<Catalog>,
    supervisor: Supervisor,
    broadcast: Arc<Broadcast>,
    /// The unfinished folders found at startup whose discard hasn't finished yet.
    discarding: Mutex<HashSet<String>>,
    create_timeout: Duration,
    create_wait: Duration,
    /// Creates running now (at most `create::MAX_CREATES_IN_FLIGHT`).
    creates: Mutex<usize>,
    create_slot_freed: Condvar,
    /// Whether the startup discards are done.
    started: Mutex<bool>,
    started_changed: Condvar,
    startup: Mutex<Option<JoinHandle<()>>>,
}

impl Offices {
    pub(crate) fn home(&self) -> &Path {
        &self.home
    }

    pub(crate) fn endpoint(
        &self,
        office: &str,
    ) -> Option<Arc<crate::supervisor::channels::Endpoint>> {
        self.supervisor.endpoint(office)
    }
    /// Moves the folders holding another office's `office.json` to `offices/.refused/`, starts
    /// supervising every office in `catalog`, and discards the `unfinished` office folders in
    /// the background, [`STARTUP_DISCARDS_AT_ONCE`] at a time.
    pub(crate) fn start(
        home: &Path,
        catalog: Catalog,
        unfinished: Vec<String>,
        supervisor: Supervisor,
        broadcast: Arc<Broadcast>,
        timings: &Timings,
    ) -> Arc<Self> {
        for folder in catalog.misplaced() {
            move_refused(home, folder, "its office.json is for another office");
        }
        let to_start: Vec<String> = catalog
            .offices()
            .iter()
            .map(|office| office.office_id.clone())
            .collect();
        let offices = Arc::new(Self {
            home: home.to_path_buf(),
            catalog: Mutex::new(catalog),
            supervisor,
            broadcast,
            discarding: Mutex::new(unfinished.iter().cloned().collect()),
            create_timeout: timings.create_timeout,
            create_wait: timings.create_wait,
            creates: Mutex::new(0),
            create_slot_freed: Condvar::new(),
            started: Mutex::new(false),
            started_changed: Condvar::new(),
            startup: Mutex::new(None),
        });
        let observer: Weak<dyn StatusObserver> = Arc::downgrade(&offices) as Weak<Self>;
        offices.supervisor.observe(observer);
        for office_id in to_start {
            if let Err(e) = offices.supervisor.start_office(&office_id) {
                host_log::log(&format!("couldn't supervise office {office_id}: {e}"));
            }
        }
        let background = Arc::clone(&offices);
        let spawned = thread::Builder::new()
            .name("startup-discards".into())
            .spawn(move || background.discard_at_startup(unfinished));
        match spawned {
            Ok(thread) => *lock(&offices.startup) = Some(thread),
            Err(e) => {
                // No thread for them: they wait for the next start, and creates don't wait.
                host_log::log(&format!("couldn't discard unfinished offices: {e}"));
                lock(&offices.discarding).clear();
                offices.startup_done();
            }
        }
        offices
    }

    /// Discards the unfinished folders found at startup, a few at a time, sweeps the keys of
    /// offices that have no folder, then lets creates in.
    fn discard_at_startup(&self, unfinished: Vec<String>) {
        // Creates are let in however this ends, a panic included, so they never wait forever.
        struct LetCreatesIn<'a>(&'a Offices);
        impl Drop for LetCreatesIn<'_> {
            fn drop(&mut self) {
                self.0.startup_done();
            }
        }
        let _let_creates_in = LetCreatesIn(self);
        let queue = Mutex::new(VecDeque::from(unfinished));
        thread::scope(|scope| {
            for _ in 0..STARTUP_DISCARDS_AT_ONCE {
                scope.spawn(|| loop {
                    let Some(office_id) = lock(&queue).pop_front() else {
                        return;
                    };
                    discard_unfinished(&self.home, &self.supervisor, &office_id);
                    lock(&self.discarding).remove(&office_id);
                });
            }
        });
        // After the discards (which need their keys), before creates are let in (a create
        // stores its key before its folder exists).
        self.supervisor
            .sweep_orphan_keys(|ids| catalog::orphans(&self.home, ids));
    }

    fn startup_done(&self) {
        *lock(&self.started) = true;
        self.started_changed.notify_all();
    }

    /// Waits until `deadline` for the startup discards to finish, so a create never races the
    /// discard of an unfinished folder; then `not_ready` ("host starting"), so a client retries.
    fn wait_for_startup(&self, deadline: Instant) -> Result<(), ApiError> {
        let mut started = lock(&self.started);
        while !*started {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(ApiError::new(ErrorCode::NotReady, HOST_STARTING));
            }
            started = self
                .started_changed
                .wait_timeout(started, left)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
        Ok(())
    }

    /// `office.restart`: starts an `offline` or `unavailable` office again now (see
    /// [`Supervisor::restart`]). Waits for the startup discards first, like a create. The
    /// caller's code signature is checked before this.
    pub(crate) fn restart_office(&self, office_id: &str) -> Result<(), ApiError> {
        self.wait_for_startup(Instant::now() + self.create_wait)?;
        self.settled_office(office_id)?;
        match self.supervisor.restart(office_id) {
            RestartOutcome::Restarting => Ok(()),
            RestartOutcome::Conflict => Err(ApiError::new(
                ErrorCode::Conflict,
                "the office isn't offline or unavailable",
            )),
            RestartOutcome::CantFix(reason) => Err(ApiError::new(
                ErrorCode::Unavailable,
                match reason {
                    OfficeStatusReason::UnsupportedVersion => {
                        concat!(
                            "a restart can't fix this office: its server doesn't speak this ",
                            "host's version (install an updated app, then restart the host)"
                        )
                    }
                    _ => {
                        concat!(
                            "a restart can't fix this office: the host started its server in a ",
                            "way it didn't understand (install an updated app, then restart the host)"
                        )
                    }
                },
            )),
            // Supervision never started for it: start it now.
            RestartOutcome::NotSupervised => match self.supervisor.start_office(office_id) {
                Ok(()) => {
                    // From `unavailable` (unsupervised) to `starting`: say so.
                    self.status_changed(office_id);
                    Ok(())
                }
                Err(e) => {
                    host_log::log(&format!("couldn't supervise office {office_id}: {e}"));
                    Err(ApiError::new(
                        ErrorCode::Unavailable,
                        "the host can't start this office's server",
                    ))
                }
            },
        }
    }

    /// Updates every office in `org_id` to the new owner and gracefully reloads running workers
    /// so the transferred owner takes effect immediately (`TASK-38` / `doc-78`). If any office
    /// custody file fails to update, `restore_db` runs FIRST so `HostDb::org_owners` returns to
    /// `(previous_owner_account, previous_owner_member)`, and then previously updated custody
    /// files are rolled back against that restored database state before returning an error.
    pub(crate) fn reload_org_offices(
        &self,
        org_id: &str,
        previous_owner_account: &str,
        previous_owner_member: &str,
        new_owner_account: &str,
        new_owner_member: &str,
        restore_db: impl FnOnce() -> Result<(), crate::hostdb::Error>,
    ) -> Result<(), ApiError> {
        let office_ids: Vec<String> = self
            .offices()
            .into_iter()
            .filter(|o| o.organization_id == org_id)
            .map(|o| o.id)
            .collect();
        let mut updated: Vec<String> = Vec::with_capacity(office_ids.len());
        for office_id in &office_ids {
            if let Err(err) =
                self.supervisor
                    .update_owner(office_id, org_id, new_owner_account, new_owner_member)
            {
                host_log::log(&format!(
                    "failed to update writer ownership for office {office_id}: {err}; rolling back"
                ));
                self.rollback_owner_updates(
                    updated,
                    org_id,
                    previous_owner_account,
                    previous_owner_member,
                    restore_db,
                );
                return Err(ApiError::new(
                    ErrorCode::StorageError,
                    "could not update office ownership binding",
                ));
            }
            updated.push(office_id.clone());
        }
        for office_id in office_ids {
            self.supervisor.reload_office(&office_id);
        }
        Ok(())
    }

    fn rollback_owner_updates(
        &self,
        updated: Vec<String>,
        org_id: &str,
        previous_owner_account: &str,
        previous_owner_member: &str,
        restore_db: impl FnOnce() -> Result<(), crate::hostdb::Error>,
    ) {
        if let Err(db_err) = restore_db() {
            host_log::log(&format!(
                "failed to restore org_owners for {org_id} during handover rollback: {db_err:?}"
            ));
            return;
        }
        for done in updated.into_iter().rev() {
            if let Err(revert_err) = self.supervisor.update_owner(
                &done,
                org_id,
                previous_owner_account,
                previous_owner_member,
            ) {
                host_log::log(&format!(
                    "failed to roll back writer ownership for office {done}: {revert_err}"
                ));
            }
        }
    }

    pub(crate) fn inspect_import(
        &self,
        repository_remote_url: &str,
        state_branch: &str,
    ) -> Result<org_office_launch::InspectImportResponse, ApiError> {
        self.supervisor
            .inspect_import(repository_remote_url, state_branch)
            .map_err(|_| {
                ApiError::new(ErrorCode::Unavailable, "organization inspector unavailable")
            })
    }

    /// `not_ready` if `office_id` is still being created, `not_found` if it isn't an office here.
    fn settled_office(&self, office_id: &str) -> Result<(), ApiError> {
        let catalog = lock(&self.catalog);
        if catalog.is_reserved(office_id) {
            return Err(ApiError::new(
                ErrorCode::NotReady,
                "the office is still being created",
            ));
        }
        if !catalog.has_office(office_id) {
            return Err(ApiError::new(ErrorCode::NotFound, "no such office here"));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn insert_catalog_for_test(&self, office: org_office_launch::OfficeFile) {
        lock(&self.catalog).insert(office);
    }

    #[cfg(all(feature = "test-support", debug_assertions))]
    pub(crate) fn kill_office_for_test(&self, office_id: &str) {
        self.supervisor.kill_office_for_test(office_id);
    }

    #[cfg(all(feature = "test-support", debug_assertions))]
    pub(crate) fn broadcast_for_test(&self) -> Arc<Broadcast> {
        Arc::clone(&self.broadcast)
    }

    /// Every organization with an office on this Mac (see [`Catalog::organizations`]).
    pub(crate) fn organizations(&self) -> Vec<Organization> {
        let catalog = lock(&self.catalog);
        catalog
            .organizations()
            .into_iter()
            .map(|org| self.organization(org))
            .collect()
    }

    /// One organization with its offices on this Mac (HQ first), if the catalog holds it.
    pub(crate) fn organization_by_id(&self, id: &str) -> Option<Organization> {
        self.organizations().into_iter().find(|o| o.id == id)
    }

    /// Every office on this Mac, in the order of [`Offices::organizations`].
    pub(crate) fn offices(&self) -> Vec<OfficeInfo> {
        self.organizations()
            .into_iter()
            .flat_map(|org| org.offices)
            .collect()
    }

    /// Stops every office server, and the startup discards still running (the rest wait for
    /// the next start).
    pub(crate) fn shutdown(&self) {
        self.supervisor.shutdown();
        if let Some(startup) = lock(&self.startup).take() {
            let _ = startup.join();
        }
    }

    /// [`Offices::shutdown`] with the servers' grace period ending at `deadline`
    /// ([`Supervisor::shutdown_by`]), waiting for the startup discards until `until` at the
    /// latest: the supervisor stopped their runs and starts no other, and one left still
    /// running (its key store slow to answer, say) is logged.
    pub(crate) fn shutdown_by(&self, deadline: Instant, until: Instant) {
        self.supervisor.shutdown_by(deadline);
        if let Some(startup) = lock(&self.startup).take() {
            if !crate::sync::join_by(startup, until) {
                host_log::log("the startup discards did not end with the host");
            }
        }
    }

    fn organization(&self, org: OrganizationEntry<'_>) -> Organization {
        Organization {
            id: org.id.to_owned(),
            name: org.name.to_owned(),
            created_at: org.created_at.to_owned(),
            offices: org
                .offices
                .into_iter()
                .map(|office| self.office_info(office, self.state_of(&office.office_id)))
                .collect(),
        }
    }

    /// A catalog office's state; one in the catalog but not supervised (supervision couldn't
    /// start) is `unavailable`.
    fn state_of(&self, office_id: &str) -> OfficeState {
        self.supervisor
            .state(office_id)
            .unwrap_or_else(unsupervised)
    }

    /// An office as clients see it. Its endpoint is always the host's own idea of its socket.
    /// The reason goes only with `offline` and `unavailable`, and the next restart only with
    /// `offline`, as the contract says.
    fn office_info(&self, office: &OfficeFile, state: OfficeState) -> OfficeInfo {
        let endpoint = self
            .supervisor
            .socket_path(&office.office_id)
            .and_then(|path| path.to_str().map(str::to_owned));
        let down = matches!(
            state.status,
            OfficeStatus::Offline | OfficeStatus::Unavailable
        );
        OfficeInfo {
            id: office.office_id.clone(),
            organization_id: office.org_id.clone(),
            name: office.office_name.clone(),
            role: office.role,
            host: THIS_MAC.to_owned(),
            endpoint,
            status: state.status,
            status_reason: state.reason.filter(|_| down),
            next_restart_at: state
                .next_restart_at
                .filter(|_| state.status == OfficeStatus::Offline)
                .map(clock::rfc3339_millis),
        }
    }
}

/// Announces an office's changed state as `office.status_changed`, built under the broadcast
/// lock from its state now (an office not in the catalog yet, being adopted, is announced by
/// `organization.created` instead). Level-triggered, as the design says: each event carries the
/// state at that moment, so an intermediate state may be skipped and an event may repeat one.
impl StatusObserver for Offices {
    fn status_changed(&self, office_id: &str) {
        self.broadcast.publish::<OfficeStatusChanged>(|| {
            let catalog = lock(&self.catalog);
            let office = catalog.office(office_id)?;
            Some(OfficeStatusChangedData {
                office: self.office_info(office, self.state_of(office_id)),
            })
        });
    }
}

/// The state of an office the host couldn't supervise.
fn unsupervised() -> OfficeState {
    OfficeState {
        status: OfficeStatus::Unavailable,
        reason: None,
        next_restart_at: None,
    }
}

/// Moves an office folder the host refuses to `offices/.refused/`, out of the catalog's sight,
/// and never discards it, so its key stays in the host's key store (accepted; logged). `why`
/// is for the log. Used at startup for a folder holding another office's `office.json`, and for
/// a refused create (its server already stopped): the requested office's folder. Nothing of
/// another office id that server reported is touched:
/// not its folder (from the next start on it is listed if it holds a valid `office.json`, and
/// discarded if it holds none), and not its socket, which the host can't protect from a server
/// that binds it. If the move fails (rare: a rename within `offices/`) the folder stays where it
/// is, holding the refused `office.json`: the next start lists it if its `office_id` names the
/// folder, and otherwise skips it.
fn move_refused(home: &Path, office_id: &str, why: &str) {
    match catalog::move_refused(home, office_id) {
        Ok(moved) => host_log::log(&format!(
            "moved office folder {office_id} to offices/{}/{} ({why}); its key stays in the \
             host's key store",
            catalog::REFUSED_DIR_NAME,
            moved.file_name().unwrap_or_default().to_string_lossy()
        )),
        Err(e) => host_log::log(&format!(
            "couldn't move office folder {office_id} aside ({why}): {e}; it stays in offices/"
        )),
    }
}

/// The one cleanup of an unfinished create, at startup and after a failed create: a server
/// discards it (`--discard`, with the office's key, or `null` when the host has none), because
/// only the server's discard removes the folder under the office lock, after checking that it
/// holds nothing an office makes. Its key is deleted only after `discarded` (see
/// `Supervisor::discard`). A folder holding `office.json` is never discarded. A failure is
/// logged, not retried: the next host start tries again, once, with the key it kept.
/// `already_exists` (the folder holds a creation intent, `office.json` or the host's custody
/// marker) and `storage_error` (a folder the server can't read or check) mean the folder may be
/// a real office: it is left for a person, with its key.
pub(crate) fn discard_unfinished(home: &Path, supervisor: &Supervisor, office_id: &str) {
    if catalog::is_stored(home, office_id) {
        host_log::log(&format!("kept office {office_id}: it is stored"));
        return;
    }
    match supervisor.discard(office_id) {
        Ok(DiscardOutcome::Discarded) => {
            host_log::log(&format!("discarded unfinished office {office_id}"));
        }
        Ok(DiscardOutcome::KeyUnreadable) => host_log::log(&format!(
            "kept unfinished office {office_id}: its key couldn't be read; the next start tries \
             again"
        )),
        Ok(DiscardOutcome::Failed(code @ (FailCode::AlreadyExists | FailCode::StorageError))) => {
            host_log::log(&format!(
                "kept unfinished office {office_id}: its server refused to discard it ({code:?}): \
                 it may be an office, so it is left for a person"
            ));
        }
        Ok(outcome) => host_log::log(&format!(
            "couldn't discard unfinished office {office_id} ({outcome:?}); the next start tries again"
        )),
        Err(e) => host_log::log(&format!(
            "couldn't discard unfinished office {office_id}: {e}; the next start tries again"
        )),
    }
}

#[cfg(test)]
mod tests;
