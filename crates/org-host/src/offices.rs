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
    OfficeStatusReason, Organization, OrganizationCreated,
};

use crate::broadcast::Broadcast;
use crate::catalog::{self, Catalog, OrganizationEntry};
use crate::clock;
use crate::host_log;
use crate::supervisor::{
    CreateOutcome, CreateStartError, DiscardOutcome, OfficeState, RestartOutcome, StatusObserver,
    Supervisor,
};
use crate::sync::lock;
use crate::timings::Timings;

/// Where an office on this Mac runs, for display.
pub(crate) const THIS_MAC: &str = "This Mac";

/// How many startup discards run at once.
const STARTUP_DISCARDS_AT_ONCE: usize = 4;

/// The answer to a create that came before the startup discards finished, after waiting
/// [`Timings::create_wait`].
const HOST_STARTING: &str = "host starting";

/// How many creates may run at once; more wait for a slot (within [`Timings::create_wait`]).
const MAX_CREATES_IN_FLIGHT: usize = 4;

/// The answer to a create that found no free slot within [`Timings::create_wait`].
const TOO_MANY_CREATES: &str = "too many creates in progress";

pub(crate) struct Offices {
    home: PathBuf,
    catalog: Mutex<Catalog>,
    supervisor: Supervisor,
    broadcast: Arc<Broadcast>,
    /// The unfinished folders found at startup whose discard hasn't finished yet.
    discarding: Mutex<HashSet<String>>,
    create_timeout: Duration,
    create_wait: Duration,
    /// Creates running now (at most [`MAX_CREATES_IN_FLIGHT`]).
    creates: Mutex<usize>,
    create_slot_freed: Condvar,
    /// Whether the startup discards are done.
    started: Mutex<bool>,
    started_changed: Condvar,
    startup: Mutex<Option<JoinHandle<()>>>,
}

impl Offices {
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

    /// `office.restart`: starts an `offline` or `unavailable` office again now (see
    /// [`Supervisor::restart`]). Waits for the startup discards first, like a create. The
    /// caller's code signature is checked before this.
    #[cfg_attr(
        not(any(test, all(feature = "test-support", debug_assertions))),
        expect(dead_code, reason = "activated by 2b.3 CLI")
    )]
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

    /// Every office on this Mac, in the order of [`Offices::organizations`].
    pub(crate) fn offices(&self) -> Vec<OfficeInfo> {
        self.organizations()
            .into_iter()
            .flat_map(|org| org.offices)
            .collect()
    }

    /// Actual selected create on the existing supervisor. A lost/failed attempt
    /// retains all intent, data and custody; no legacy discard or key sweep runs.
    pub(crate) fn provision_hq(
        &self,
        first: org_office_launch::CreateLine,
        recovery: org_proto::RecoveryCode,
    ) -> Result<bool, ApiError> {
        let deadline = Instant::now() + self.create_wait;
        self.wait_for_startup(deadline)?;
        let p = first
            .hq_creation
            .as_ref()
            .and_then(|c| c.bootstrap.as_ref())
            .ok_or_else(storage_error)?;
        let office = p.office_id.clone();
        let stored = lock(&self.catalog).office(&office).cloned();
        if let Some(stored) = stored {
            if stored.org_id != p.organization_id
                || stored.org_name != p.plan.name
                || stored.version() != 2
            {
                return Err(storage_error());
            }
            return Ok(self.state_of(&office).status == OfficeStatus::Active);
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
                Ok(self.state_of(&office).status == OfficeStatus::Active)
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
    fn add_created(&self, office: &OfficeFile, respond: impl FnOnce(&Organization)) {
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

    /// Stops every office server, and the startup discards still running (the rest wait for
    /// the next start).
    pub(crate) fn shutdown(&self) {
        self.supervisor.shutdown();
        if let Some(startup) = lock(&self.startup).take() {
            let _ = startup.join();
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
/// only a server can open its databases to check that no organization was committed. Its key
/// is deleted only after `discarded` (see `Supervisor::discard`). A folder holding
/// `office.json` is never discarded. A failure is logged, not retried: the next host start
/// tries again, once, with the key it kept. `already_exists` (an organization committed in the
/// folder's databases) and `storage_error` (databases the server can't open or check) mean the
/// folder may be a real office that lost its `office.json`: it is left for a person, with its key.
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
                 its databases may hold an organization, so it is left for a person"
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::memory::MemoryKeys;
    use crate::supervisor::SupervisorConfig;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn a_stored_folder_is_never_handed_to_discard() {
        // Under /tmp, with `run/`: the launch rendezvous path must fit a socket address.
        let dir = tempfile::Builder::new()
            .prefix("of")
            .tempdir_in("/tmp")
            .unwrap();
        let home = dir.path();
        fs::create_dir(home.join("run")).unwrap();
        // A server that never refuses: it records each run and exits. Only the host's own check
        // can keep a stored folder from it. It is a `#!` script, started by its path (the test
        // admission): a system that executes verified bytes starts no script.
        let script = home.join("server");
        fs::write(
            &script,
            "#!/bin/sh\necho \"$@\" >> \"$ORG_OFFICE_HOME/ran\"\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        let supervisor = Supervisor::new(
            SupervisorConfig {
                program: script,
                home: home.to_path_buf(),
                runtime_dir: None,
                logs_dir: home.join("logs"),
                log_max_bytes: 1024,
                timings: Timings::default(),
                keys: Arc::new(MemoryKeys::default()),
                server_check: Arc::new(crate::server_check::ServerCheck::ByPath),
                sweep_orphans: false,
            },
            INSTANCE,
        );
        for (id, stored) in [("STORED", true), ("UNFINISHED", false)] {
            let folder = home.join("offices").join(id);
            fs::create_dir_all(&folder).unwrap();
            if stored {
                fs::write(folder.join(org_office_launch::OFFICE_FILE_NAME), b"{}").unwrap();
            }
            discard_unfinished(home, &supervisor, id);
        }
        let ran = fs::read_to_string(home.join("ran")).unwrap_or_default();
        assert_eq!(ran, "--discard UNFINISHED\n");
    }

    const OFFICE: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
    const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c0";

    fn hq() -> OfficeFile {
        OfficeFile::new(
            OFFICE,
            "HQ",
            org_proto::OfficeRole::Hq,
            "01ORGA",
            "Acme",
            "2026-11-08T00:00:00.000Z",
        )
    }

    /// Offices over `catalog` in `home`, whose server program doesn't exist (nothing here needs
    /// it to run). With `stopped`, the supervisor is shut down first, so it can't start anything.
    fn offices(home: &Path, catalog: Catalog, stopped: bool) -> Arc<Offices> {
        let supervisor = Supervisor::new(
            SupervisorConfig {
                program: home.join("no-such-server"),
                home: home.to_path_buf(),
                runtime_dir: None,
                logs_dir: home.join("logs"),
                log_max_bytes: 1024,
                timings: Timings::default(),
                keys: Arc::new(MemoryKeys::default()),
                server_check: Arc::new(crate::server_check::ServerCheck::Skip),
                sweep_orphans: false,
            },
            INSTANCE,
        );
        if stopped {
            supervisor.shutdown();
        }
        let broadcast = Arc::new(Broadcast::default());
        Offices::start(
            home,
            catalog,
            Vec::new(),
            supervisor,
            broadcast,
            &Timings::default(),
        )
    }

    /// At startup, after the discards, the keys of offices with no folder are swept, but only
    /// when the host may (`sweep_orphans`): one marked 7 days or more ago is deleted, a new one
    /// is only marked, and a key whose folder exists is always kept.
    #[test]
    fn the_startup_sweeps_orphan_keys_only_when_allowed() {
        use crate::keys::OfficeSecretsKeys;
        use org_proto::OfficeSecretsKey;
        const ORPHAN: &str = "01ORPHANORPHANORPHANORPHAN";
        const NEW_ORPHAN: &str = "01NEWORPHANNEWORPHANNEWORP";
        for sweep in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path();
            // Two orphans beside three live offices (more than half orphaned isn't swept).
            const LIVE: [&str; 3] = [
                OFFICE,
                "01LIVEBLIVEBLIVEBLIVEBLIVE",
                "01LIVECLIVECLIVECLIVECLIVE",
            ];
            let keys = Arc::new(MemoryKeys::default());
            for id in LIVE {
                fs::create_dir_all(home.join("offices").join(id)).unwrap();
            }
            for id in LIVE.into_iter().chain([ORPHAN, NEW_ORPHAN]) {
                keys.create(id, &OfficeSecretsKey::from_bytes([1; 32]))
                    .unwrap();
            }
            // ORPHAN was marked long ago; NEW_ORPHAN never was.
            let record = home.join("orphan-keys.json");
            let marked = format!("{{\"v\":1,\"first_seen_ms\":{{\"{ORPHAN}\":1000}}}}");
            fs::write(&record, &marked).unwrap();
            let supervisor = Supervisor::new(
                SupervisorConfig {
                    program: home.join("no-such-server"),
                    home: home.to_path_buf(),
                    runtime_dir: None,
                    logs_dir: home.join("logs"),
                    log_max_bytes: 1024,
                    timings: Timings::default(),
                    keys: Arc::clone(&keys) as Arc<dyn OfficeSecretsKeys>,
                    server_check: Arc::new(crate::server_check::ServerCheck::Skip),
                    sweep_orphans: sweep,
                },
                INSTANCE,
            );
            let offices = Offices::start(
                home,
                Catalog::default(),
                Vec::new(),
                supervisor,
                Arc::new(Broadcast::default()),
                &Timings::default(),
            );
            offices
                .wait_for_startup(Instant::now() + Duration::from_secs(10))
                .unwrap();
            assert_eq!(keys.has(ORPHAN), !sweep, "sweep {sweep}");
            // Never deleted on first sight, only marked.
            assert!(keys.has(NEW_ORPHAN) && keys.has(OFFICE), "sweep {sweep}");
            let now_marked = fs::read_to_string(&record).unwrap();
            if sweep {
                assert!(now_marked.contains(NEW_ORPHAN) && !now_marked.contains(ORPHAN));
            } else {
                assert_eq!(now_marked, marked);
            }
            offices.shutdown();
        }
    }

    /// `office.restart` on an office the host couldn't supervise, when it still can't: listed
    /// `unavailable`, and the restart answers `unavailable` too.
    #[test]
    fn restarting_an_office_the_host_still_cant_supervise_is_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let mut catalog = Catalog::default();
        catalog.insert(hq());
        let offices = offices(dir.path(), catalog, true);
        assert_eq!(offices.state_of(OFFICE).status, OfficeStatus::Unavailable);
        let err = offices.restart_office(OFFICE).unwrap_err();
        assert_eq!(err.code, ErrorCode::Unavailable);
        assert!(offices.supervisor.state(OFFICE).is_none());
        offices.shutdown();
    }

    /// `office.restart` on an office the host doesn't supervise, when it now can: supervision
    /// starts. (The office is put in the catalog after the start, so supervision never began for
    /// it: the state an office is left in when the host couldn't supervise it.)
    #[test]
    fn restarting_an_office_the_host_couldnt_supervise_starts_it() {
        let dir = tempfile::tempdir().unwrap();
        let offices = offices(dir.path(), Catalog::default(), false);
        lock(&offices.catalog).insert(hq());
        assert!(offices.supervisor.state(OFFICE).is_none());
        assert_eq!(offices.state_of(OFFICE).status, OfficeStatus::Unavailable);
        offices.restart_office(OFFICE).unwrap();
        assert!(offices.supervisor.state(OFFICE).is_some());
        offices.shutdown();
    }
}
