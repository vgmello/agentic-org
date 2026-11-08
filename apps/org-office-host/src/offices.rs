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
//! (`organization.create`, `office.restart`, and `office.app_token` for an office still being
//! discarded).
//!
//! Every change of an office's state is announced as `office.status_changed` through the
//! [`Broadcast`], in order with the list answers.

use std::collections::{HashSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use org_office_launch::{office_dir, CreateOffice, FailCode, OfficeFile, OFFICE_FILE_NAME};
use org_proto::{
    ApiError, AppToken, ErrorCode, OfficeInfo, OfficeStatus, OfficeStatusChanged,
    OfficeStatusChangedData, OfficeStatusReason, Organization, OrganizationCreated,
    OrganizationName,
};

use crate::app_token;
use crate::broadcast::Broadcast;
use crate::catalog::{self, Catalog, CreatePlan, OrganizationEntry, PlanError};
use crate::clock;
use crate::host_log;
use crate::ids;
use crate::supervisor::{
    CreateOutcome, DiscardOutcome, OfficeState, RestartOutcome, StatusObserver, Supervisor,
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

    /// Discards the unfinished folders found at startup, a few at a time, then lets creates in.
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

    /// `office.app_token`: runs `use_token` with a running office's app token (it's written
    /// straight into the response, never copied). Errors: `not_found` (no such office here),
    /// `not_ready` (the office is still being created, or still being discarded at startup
    /// after waiting [`Timings::create_wait`]), `unavailable` (the office isn't running). The
    /// caller's code signature is checked before this.
    pub(crate) fn app_token<R>(
        &self,
        office_id: &str,
        use_token: impl FnOnce(&AppToken) -> R,
    ) -> Result<R, ApiError> {
        if lock(&self.discarding).contains(office_id) {
            self.wait_for_startup(Instant::now() + self.create_wait)?;
        }
        self.settled_office(office_id)?;
        let running = self.supervisor.state(office_id).is_some_and(|state| {
            matches!(state.status, OfficeStatus::Starting | OfficeStatus::Active)
        });
        if !running {
            return Err(ApiError::new(
                ErrorCode::Unavailable,
                "the office isn't running, so it has no app token",
            ));
        }
        self.supervisor
            .with_app_token(office_id, use_token)
            .ok_or_else(|| {
                ApiError::new(
                    ErrorCode::Unavailable,
                    "the office isn't running, so it has no app token",
                )
            })
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

    /// `organization.create`: makes the organization and its HQ office, and leaves the HQ's
    /// server running under supervision. On failure the unfinished office is discarded (except
    /// after `already_exists`, which isn't ours); a created office other than the one asked for
    /// is refused and moved aside (`move_refused`). The error never holds a path. On success,
    /// `respond` queues the answer, inside the critical section that adds the organization (see
    /// [`Offices::add_created`]).
    pub(crate) fn create_organization(
        &self,
        raw_name: &str,
        respond: impl FnOnce(&Organization),
    ) -> Result<(), ApiError> {
        let name = OrganizationName::parse(raw_name)
            .map_err(|e| ApiError::new(ErrorCode::InvalidName, e.message()))?;
        // One budget for both waits.
        let deadline = Instant::now() + self.create_wait;
        self.wait_for_startup(deadline)?;
        // A taken name is answered at once, not after waiting for a slot. (`plan_create` checks
        // again, under the same lock as the reservation.)
        if lock(&self.catalog).has_organization_named(&name) {
            return Err(already_exists());
        }
        let _slot = self.create_slot(deadline)?;
        // The catalog mutex is held only for this statement.
        let plan = lock(&self.catalog)
            .plan_create(&name, ids::generate)
            .map_err(|e| match e {
                PlanError::NameTaken => already_exists(),
                PlanError::Ids(e) => {
                    host_log::log(&format!("couldn't make ids: {e}"));
                    storage_error()
                }
            })?;
        let _reserved = Reserved {
            catalog: &self.catalog,
            home: &self.home,
            office_id: &plan.office_id,
        };
        let pending = create_message(&plan)
            .and_then(|create| self.supervisor.begin_create(create))
            .map_err(|e| {
                host_log::log(&format!("couldn't start an office server: {e}"));
                storage_error()
            })?;
        match pending.wait(self.create_timeout) {
            CreateOutcome::Created(adoptable) => {
                if !matches_plan(&adoptable.office, &plan) {
                    let reported = &adoptable.office.office_id;
                    if *reported == plan.office_id {
                        host_log::log(
                            "an office server created an office other than the one asked for",
                        );
                    } else {
                        // A plain id: the contract refuses any other in `created`.
                        host_log::log(&format!(
                            "an office server created office {reported}, not the one asked for"
                        ));
                    }
                    adoptable.stop();
                    move_refused(&self.home, &plan.office_id, "a refused create");
                    return Err(storage_error());
                }
                let office = adoptable.office.clone();
                self.supervisor.adopt(*adoptable);
                self.add_created(&office, respond);
                Ok(())
            }
            // The folder existed before this create, so it isn't ours to discard.
            CreateOutcome::Failed(FailCode::AlreadyExists) => Err(already_exists()),
            // A server that failed after storing the office still made it: the same as one that
            // ended without reporting.
            CreateOutcome::Failed(_) if catalog::is_stored(&self.home, &plan.office_id) => {
                self.finish_unreported(&plan, respond)
            }
            CreateOutcome::Failed(code) => {
                self.clean_up(&plan.office_id);
                Err(match code {
                    FailCode::InvalidName => ApiError::new(
                        ErrorCode::InvalidName,
                        "the organization name isn't allowed",
                    ),
                    _ => storage_error(),
                })
            }
            CreateOutcome::Crashed | CreateOutcome::TimedOut => {
                self.finish_unreported(&plan, respond)
            }
            CreateOutcome::Stopped => {
                self.clean_up(&plan.office_id);
                Err(ApiError::new(
                    ErrorCode::StorageError,
                    "the organization couldn't be created: this Mac's office service is stopping",
                ))
            }
        }
    }

    /// A create whose server ended without `created`: it crashed, was killed when it timed out,
    /// or reported `failed` after storing the office. `office.json` is written last, so if the
    /// server stored it, the create happened: when the stored office is the one asked for, the
    /// create succeeds after all (its server is started with `--office`, status `starting`);
    /// another office is a refused create (moved to `.refused/`). With no `office.json` the create is unfinished and is discarded;
    /// one that can't be read is left alone, its name still taken (see [`Reserved`]).
    fn finish_unreported(
        &self,
        plan: &CreatePlan,
        respond: impl FnOnce(&Organization),
    ) -> Result<(), ApiError> {
        let stored = office_dir(&self.home, &plan.office_id)
            .ok_or_else(storage_error)
            .map(|dir| catalog::read_office_file(&dir.join(OFFICE_FILE_NAME)))?;
        match stored {
            Ok(None) => {
                self.clean_up(&plan.office_id);
                Err(storage_error())
            }
            Ok(Some(office)) if matches_plan(&office, plan) => {
                host_log::log(&format!(
                    "office {} was stored, but its server ended without reporting it created; \
                     starting it",
                    office.office_id
                ));
                if let Err(e) = self.supervisor.start_office(&office.office_id) {
                    host_log::log(&format!(
                        "couldn't supervise office {}: {e}",
                        office.office_id
                    ));
                }
                self.add_created(&office, respond);
                Ok(())
            }
            Ok(Some(_)) => {
                host_log::log(&format!(
                    "office {} was stored as an office other than the one asked for",
                    plan.office_id
                ));
                move_refused(&self.home, &plan.office_id, "a refused create");
                Err(storage_error())
            }
            Err(why) => {
                host_log::log(&format!(
                    "kept office {}: its server ended without reporting it created, and {why}",
                    plan.office_id
                ));
                Err(storage_error())
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

    fn clean_up(&self, office_id: &str) {
        discard_unfinished(&self.home, &self.supervisor, office_id);
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
        let endpoint = org_proto::location::office_socket_path(&self.home, &office.office_id)
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
/// and never discards it, so any Keychain key its office made stays (accepted; logged). `why`
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
            "moved office folder {office_id} to offices/{}/{} ({why}); any Keychain key its \
             office made stays",
            catalog::REFUSED_DIR_NAME,
            moved.file_name().unwrap_or_default().to_string_lossy()
        )),
        Err(e) => host_log::log(&format!(
            "couldn't move office folder {office_id} aside ({why}): {e}; it stays in offices/"
        )),
    }
}

/// The one cleanup of an unfinished create, at startup and after a failed create: the server
/// itself discards it (`--discard`), because only it can delete a Keychain key the create may
/// have left. A folder holding `office.json` is never discarded. A failure is logged, not
/// retried: the next host start tries again, once. `already_exists` (an organization committed
/// in the folder's databases) and `storage_error` (databases the server can't open or check)
/// mean the folder may be a real office that lost its `office.json`: it is left for a person.
pub(crate) fn discard_unfinished(home: &Path, supervisor: &Supervisor, office_id: &str) {
    if catalog::is_stored(home, office_id) {
        host_log::log(&format!("kept office {office_id}: it is stored"));
        return;
    }
    match supervisor.discard(office_id) {
        Ok(DiscardOutcome::Discarded) => {
            host_log::log(&format!("discarded unfinished office {office_id}"));
        }
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

/// The create message for `plan`, with a fresh app token (the office's for this host run).
fn create_message(plan: &CreatePlan) -> io::Result<CreateOffice> {
    Ok(CreateOffice {
        org_id: plan.org_id.clone(),
        org_name: plan.org_name.clone(),
        office_id: plan.office_id.clone(),
        office_name: plan.office_name.clone(),
        role: plan.role,
        app_token: app_token::generate()?,
    })
}

/// A create slot, freed when dropped.
struct CreateSlot<'a>(&'a Offices);

impl Drop for CreateSlot<'_> {
    fn drop(&mut self) {
        *lock(&self.0.creates) -= 1;
        self.0.create_slot_freed.notify_one();
    }
}

/// A create's reservation of its name and handle in the catalog. Dropping it ends the
/// reservation after a failed create (once its cleanup is done), unless the failed create left
/// a stored office outside the catalog (an `office.json` that couldn't be read, or a create
/// stopped by the shutdown): that office is listed from the next start on, so its name and
/// handle stay reserved for the rest of this run. After a successful create there is nothing left to end: inserting the office into
/// the catalog already did.
struct Reserved<'a> {
    catalog: &'a Mutex<Catalog>,
    home: &'a Path,
    office_id: &'a str,
}

impl Drop for Reserved<'_> {
    fn drop(&mut self) {
        let mut catalog = lock(self.catalog);
        if !catalog::is_stored(self.home, self.office_id) {
            catalog.release(self.office_id);
        } else if !catalog
            .offices()
            .iter()
            .any(|o| o.office_id == self.office_id)
        {
            host_log::log(&format!(
                "office {} was stored but never reported; its name stays taken until the next \
                 start lists it",
                self.office_id
            ));
        }
    }
}

/// Whether the server created exactly the office it was asked for: the same ids, the names as
/// they were sent (already trimmed), and the role (HQ).
fn matches_plan(office: &OfficeFile, plan: &CreatePlan) -> bool {
    office.office_id == plan.office_id
        && office.org_id == plan.org_id
        && office.org_name == plan.org_name
        && office.office_name == plan.office_name
        && office.role == plan.role
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supervisor::SupervisorConfig;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn a_stored_folder_is_never_handed_to_discard() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        // A server that never refuses: it records each run and exits. Only the host's own check
        // can keep a stored folder from it.
        let script = home.join("server");
        fs::write(
            &script,
            "#!/bin/sh\necho \"$@\" >> \"$ORG_OFFICE_HOME/ran\"\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        let supervisor = Supervisor::new(SupervisorConfig {
            program: script,
            home: home.to_path_buf(),
            logs_dir: home.join("logs"),
            log_max_bytes: 1024,
            timings: Timings::default(),
        });
        for (id, stored) in [("STORED", true), ("UNFINISHED", false)] {
            let folder = home.join("offices").join(id);
            fs::create_dir_all(&folder).unwrap();
            if stored {
                fs::write(folder.join(OFFICE_FILE_NAME), b"{}").unwrap();
            }
            discard_unfinished(home, &supervisor, id);
        }
        let ran = fs::read_to_string(home.join("ran")).unwrap_or_default();
        assert_eq!(ran, "--discard UNFINISHED\n");
    }

    const OFFICE: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";

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
        let supervisor = Supervisor::new(SupervisorConfig {
            program: home.join("no-such-server"),
            home: home.to_path_buf(),
            logs_dir: home.join("logs"),
            log_max_bytes: 1024,
            timings: Timings::default(),
        });
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
