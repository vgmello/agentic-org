//! This Mac's offices: the catalog joined with supervision. Answers what the service needs
//! (organizations, offices, their status and endpoints), runs `organization.create`, and
//! discards unfinished creates.
//!
//! The catalog mutex guards only a create's name check and its reservation of the name and
//! handle (see `Catalog::plan_create`), never the server run that follows, so creates of
//! different names run side by side and listing never waits for a create.
//!
//! The unfinished folders found at startup are discarded in the background, a few at a time,
//! so the host answers at once; only `organization.create` waits for them (see
//! [`Offices::create_organization`]).

use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use org_office_launch::{office_dir, CreateOffice, FailCode, OfficeFile, OFFICE_FILE_NAME};
use org_proto::{ApiError, ErrorCode, OfficeInfo, OfficeStatus, Organization, OrganizationName};

use crate::app_token;
use crate::catalog::{self, Catalog, CreatePlan, OrganizationEntry, PlanError};
use crate::host_log;
use crate::ids;
use crate::supervisor::{CreateOutcome, DiscardOutcome, Supervisor};
use crate::sync::lock;
use crate::timings::Timings;

/// Where an office on this Mac runs, for display.
pub(crate) const THIS_MAC: &str = "This Mac";

/// How many startup discards run at once.
const STARTUP_DISCARDS_AT_ONCE: usize = 4;

/// The answer to a create that came before the startup discards finished, after waiting
/// [`Timings::startup_wait`].
const HOST_STARTING: &str = "host starting";

pub(crate) struct Offices {
    home: PathBuf,
    catalog: Mutex<Catalog>,
    supervisor: Supervisor,
    create_timeout: Duration,
    startup_wait: Duration,
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
        timings: &Timings,
    ) -> Arc<Self> {
        for folder in catalog.misplaced() {
            move_refused(home, folder, "its office.json is for another office");
        }
        for office in catalog.offices() {
            if let Err(e) = supervisor.start_office(&office.office_id) {
                host_log::log(&format!(
                    "couldn't supervise office {}: {e}",
                    office.office_id
                ));
            }
        }
        let offices = Arc::new(Self {
            home: home.to_path_buf(),
            catalog: Mutex::new(catalog),
            supervisor,
            create_timeout: timings.create_timeout,
            startup_wait: timings.startup_wait,
            started: Mutex::new(false),
            started_changed: Condvar::new(),
            startup: Mutex::new(None),
        });
        let background = Arc::clone(&offices);
        let spawned = thread::Builder::new()
            .name("startup-discards".into())
            .spawn(move || background.discard_at_startup(unfinished));
        match spawned {
            Ok(thread) => *lock(&offices.startup) = Some(thread),
            Err(e) => {
                // No thread for them: they wait for the next start, and creates don't wait.
                host_log::log(&format!("couldn't discard unfinished offices: {e}"));
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
                });
            }
        });
    }

    fn startup_done(&self) {
        *lock(&self.started) = true;
        self.started_changed.notify_all();
    }

    /// Waits up to [`Timings::startup_wait`] for the startup discards to finish, so a create
    /// never races the discard of an unfinished folder; then `unavailable` ("host starting").
    fn wait_for_startup(&self) -> Result<(), ApiError> {
        let deadline = Instant::now() + self.startup_wait;
        let mut started = lock(&self.started);
        while !*started {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(ApiError::new(ErrorCode::Unavailable, HOST_STARTING));
            }
            started = self
                .started_changed
                .wait_timeout(started, left)
                .unwrap_or_else(|p| p.into_inner())
                .0;
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
    /// is refused and moved aside (`move_refused`). The error never holds a path.
    pub(crate) fn create_organization(&self, raw_name: &str) -> Result<Organization, ApiError> {
        let name = OrganizationName::parse(raw_name)
            .map_err(|e| ApiError::new(ErrorCode::InvalidName, e.message()))?;
        self.wait_for_startup()?;
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
                let status = self.supervisor.adopt(*adoptable);
                lock(&self.catalog).insert(office.clone());
                Ok(self.created(&office, status))
            }
            // The folder existed before this create, so it isn't ours to discard.
            CreateOutcome::Failed(FailCode::AlreadyExists) => Err(already_exists()),
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
            CreateOutcome::Crashed | CreateOutcome::TimedOut => self.finish_unreported(&plan),
            CreateOutcome::Stopped => {
                self.clean_up(&plan.office_id);
                Err(ApiError::new(
                    ErrorCode::StorageError,
                    "the organization couldn't be created: this Mac's office service is stopping",
                ))
            }
        }
    }

    /// A create whose server ended (crashed, or was killed when it timed out) without `created`
    /// or `failed`. `office.json` is written last, so if the server stored it, the create
    /// happened: when the stored office is the one asked for, the create succeeds after all (its
    /// server is started with `--office`, status `starting`); another office is a refused create
    /// (moved to `.refused/`). With no `office.json` the create is unfinished and is discarded;
    /// one that can't be read is left alone, its name still taken (see [`Reserved`]).
    fn finish_unreported(&self, plan: &CreatePlan) -> Result<Organization, ApiError> {
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
                    "office {} was stored, but its server ended before reporting it; starting it",
                    office.office_id
                ));
                let status = match self.supervisor.start_office(&office.office_id) {
                    Ok(()) => OfficeStatus::Starting,
                    Err(e) => {
                        host_log::log(&format!(
                            "couldn't supervise office {}: {e}",
                            office.office_id
                        ));
                        OfficeStatus::Unavailable
                    }
                };
                lock(&self.catalog).insert(office.clone());
                Ok(self.created(&office, status))
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
                    "kept office {}: its server ended before reporting it, and {why}",
                    plan.office_id
                ));
                Err(storage_error())
            }
        }
    }

    /// The organization a create made, with its one office in `status`.
    fn created(&self, office: &OfficeFile, status: OfficeStatus) -> Organization {
        Organization {
            id: office.org_id.clone(),
            name: office.org_name.clone(),
            created_at: office.created_at.clone(),
            offices: vec![self.office_info(office, status)],
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
                .map(|office| {
                    let status = self
                        .supervisor
                        .status(&office.office_id)
                        // In the catalog but not supervised: supervision couldn't start.
                        .unwrap_or(OfficeStatus::Unavailable);
                    self.office_info(office, status)
                })
                .collect(),
        }
    }

    /// An office as clients see it. Its endpoint is always the host's own idea of its socket.
    fn office_info(&self, office: &OfficeFile, status: OfficeStatus) -> OfficeInfo {
        let endpoint = org_proto::location::office_socket_path(&self.home, &office.office_id)
            .and_then(|path| path.to_str().map(str::to_owned));
        OfficeInfo {
            id: office.office_id.clone(),
            organization_id: office.org_id.clone(),
            name: office.office_name.clone(),
            role: office.role,
            host: THIS_MAC.to_owned(),
            endpoint,
            status,
        }
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
