//! `agentic-org release install` and the instances' registered services (TASK-155).
//!
//! A running release host holds the release store it was admitted from, so an upgrade
//! cannot install while its service runs, and a service whose definition still names the
//! previous release cannot start once the pin has moved. An install therefore:
//!
//! 1. reads, for every instance of this account set up with a registered service, the
//!    definition the service manager holds ([`ServiceManager::definition`]); nothing changes;
//! 2. only when the store is busy, stops the services that run from it and waits until the
//!    store is free ([`ServiceRefresh::install`]); a failed install starts them again;
//! 3. points each definition at the newly pinned release ([`ServiceManager::redefine`], which
//!    keeps its arguments and environment and never starts it), and starts again the
//!    services that were running ([`ServiceRefresh::refresh`]). A service that was stopped
//!    stays stopped.
//!
//! Only what setup wrote and nobody changed since is touched: a definition the adapter does
//! not read back as its own exact rendering (changed by hand), or that runs something other
//! than `agentic-org serve --name <instance>`, is left untouched with what the operator does
//! instead; one that runs a program outside this store (a development build) is left too.

mod move_unit;

use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use org_platform::service::{ServiceError, ServiceManager, ServiceStatus, Unit};
use serde::Serialize;

use org_host::instance::setup::{Journal, Service};

use super::report::service_id;
use move_unit::{name_flag, report, stop_and_wait, untouched_note, Move, POLL};

/// How long the store may stay busy after the services that held it stopped.
const STORE_WAIT: Duration = Duration::from_secs(15);

/// What `release install` did to one instance's registered service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServiceRefreshReport {
    /// The instance's name (`default` when unnamed).
    pub instance: String,
    pub outcome: RefreshOutcome,
    /// Whether the service was running when the install began.
    pub was_running: bool,
    /// Whether it runs when the install ends.
    pub running: bool,
    /// The program its definition names now, when it was read.
    pub program: Option<String>,
    /// One sentence for a person: what happened, and what to do when something is left.
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshOutcome {
    /// The definition now runs the new release.
    Refreshed,
    /// It already did; at most a restart after a stop the install needed.
    Unchanged,
    /// Its definition is not what setup wrote (changed by hand): left untouched.
    Refused,
    /// It runs a program that is not a release in this store: left untouched.
    Skipped,
    /// The service manager failed; the note says what to do.
    Failed,
}

/// What [`ServiceRefresh::install`]'s attempt reports when it did not install.
#[derive(Debug)]
pub enum InstallAttempt {
    /// The store is held by another process (a running host); trying again may work.
    Busy(String),
    /// Anything else; nothing is tried again.
    Failed(String),
}

/// Snapshot of one managed service inspected before an install or auto-update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedServiceInfo {
    pub instance: String,
    pub instance_id: String,
    pub service_id: String,
    pub root: PathBuf,
    pub previous_program: PathBuf,
    pub was_running: bool,
}

/// The account's registered services, as an install found them and has changed them.
#[derive(Debug, Default)]
pub struct ServiceRefresh {
    entries: Vec<Entry>,
}

#[derive(Debug)]
struct Entry {
    instance: String,
    instance_id: String,
    id: String,
    root: PathBuf,
    found: Found,
}

#[derive(Debug)]
enum Found {
    /// What setup registered, running a release in this store.
    Managed {
        unit: Unit,
        was_running: bool,
        unloaded: bool,
        stopped: bool,
    },
    /// Not touched: changed by hand (`Refused`), elsewhere (`Skipped`) or unreadable
    /// (`Failed`), with why and the program, when it was read, and whether the manager
    /// reported it running (asked only when its definition was read).
    Untouched {
        outcome: RefreshOutcome,
        why: String,
        program: Option<PathBuf>,
        running: bool,
    },
}

impl ServiceRefresh {
    /// Reads every instance of this account set up with a registered service, as `manager`
    /// holds it, for an install into `store`. Nothing is made or changed; what cannot be read
    /// of one instance is that instance's report. Only an account whose setup records cannot
    /// be located at all is an error.
    pub fn inspect(manager: &dyn ServiceManager, store: &Path) -> Result<Self, String> {
        Ok(match setup_base()? {
            Some(base) => Self::inspect_at(manager, &base, store),
            None => Self::default(),
        })
    }

    /// [`Self::inspect`] with the setup records under `base`.
    pub fn inspect_at(manager: &dyn ServiceManager, base: &Path, store: &Path) -> Self {
        let releases = std::fs::canonicalize(store.join("releases")).ok();
        let mut names: Vec<String> = std::fs::read_dir(base.join("setup"))
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .collect();
        names.sort();
        let mut entries = Vec::new();
        for name in names {
            let Ok(record) = Journal::snapshot(base, Some(&name)) else {
                continue;
            };
            let Some(instance_id) = record.instance_id.as_deref() else {
                continue;
            };
            if record.input.service != Service::Registered || !record.service_registered {
                continue;
            }
            let id = service_id(instance_id);
            let found = inspect_one(manager, &id, &name, releases.as_deref());
            entries.push(Entry {
                instance: name,
                instance_id: instance_id.to_owned(),
                id,
                root: record.input.root,
                found,
            });
        }
        Self { entries }
    }

    /// Managed registered services found in this store, for idle-drain and rollback coordination.
    pub fn managed_targets(&self) -> Vec<ManagedServiceInfo> {
        self.entries
            .iter()
            .filter_map(|entry| match &entry.found {
                Found::Managed {
                    unit, was_running, ..
                } => Some(ManagedServiceInfo {
                    instance: entry.instance.clone(),
                    instance_id: entry.instance_id.clone(),
                    service_id: entry.id.clone(),
                    root: entry.root.clone(),
                    previous_program: unit.program.clone(),
                    was_running: *was_running,
                }),
                Found::Untouched { .. } => None,
            })
            .collect()
    }

    /// Registered services whose definition or status could not be read from the service manager
    /// (for example when `systemd --user` is unavailable).
    pub fn inspection_failures(&self) -> Vec<(String, String)> {
        self.entries
            .iter()
            .filter_map(|entry| match &entry.found {
                Found::Untouched {
                    outcome: RefreshOutcome::Failed,
                    why,
                    ..
                } => Some((entry.instance.clone(), why.clone())),
                _ => None,
            })
            .collect()
    }

    /// Names of account instances recorded with a registered system service (`Service::Registered`
    /// and `service_registered == true`), read without invoking any service manager.
    pub fn registered_setup_instances(base: Option<&Path>) -> Vec<String> {
        let Some(base) = base else {
            return Vec::new();
        };
        let mut names: Vec<String> = std::fs::read_dir(base.join("setup"))
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .collect();
        names.sort();
        names
            .into_iter()
            .filter(|name| {
                Journal::snapshot(base, Some(name)).is_ok_and(|record| {
                    record.instance_id.is_some()
                        && record.input.service == Service::Registered
                        && record.service_registered
                })
            })
            .collect()
    }

    /// Stops every managed service that is running, crash-looping (`Unloaded`, e.g. `systemd`
    /// `ActiveState=activating`), or named in `should_start` BEFORE `install` acquires
    /// `store.lock`, so crash-looping units release `store.lock` and never cause `redefine`
    /// conflicts. Idempotent when entries are already marked `stopped = true`.
    pub(crate) fn stop_for_rollback(
        &mut self,
        manager: &dyn ServiceManager,
        should_start: &[String],
    ) {
        for entry in &mut self.entries {
            let want_running = should_start.iter().any(|name| name == &entry.instance);
            entry.prepare_for_rollback(manager, want_running);
        }
    }

    /// Re-points managed services to `program` and restarts every instance named in `should_start`
    /// (even if its process crashed or entered a `systemd` `activating` crash-loop before rollback).
    pub(crate) fn rollback_to_program(
        mut self,
        manager: &dyn ServiceManager,
        program: &Path,
        should_start: &[String],
        restart: bool,
    ) -> Vec<ServiceRefreshReport> {
        self.stop_for_rollback(manager, should_start);
        self.refresh(manager, program, restart)
    }

    /// Whether any managed service stopped by this refresh remains stopped (for example when
    /// `install` failed and `resume` could not start it again).
    pub(crate) fn has_stopped_managed(&self) -> bool {
        self.entries
            .iter()
            .any(|e| matches!(e.found, Found::Managed { stopped: true, .. }))
    }

    pub(crate) fn account_setup_base() -> Result<Option<PathBuf>, String> {
        setup_base()
    }

    /// What an install would do to each service, for `--dry-run`: `refresh` or the outcome
    /// that leaves it untouched, with whether it runs.
    pub fn planned(&self) -> Vec<(String, &'static str, bool)> {
        self.entries
            .iter()
            .map(|entry| match &entry.found {
                Found::Managed { was_running, .. } => {
                    (entry.instance.clone(), "refresh", *was_running)
                }
                Found::Untouched {
                    outcome, running, ..
                } => (entry.instance.clone(), outcome.wire(), *running),
            })
            .collect()
    }

    /// Runs `attempt` (open the release store and install into it). When it finds the store
    /// busy and this account runs services from the store, stops them first and tries again
    /// until the store opens or [`STORE_WAIT`] passes. When the install fails, the services
    /// it stopped are started again (their definitions are unchanged).
    pub fn install<T>(
        &mut self,
        manager: &dyn ServiceManager,
        mut attempt: impl FnMut() -> Result<T, InstallAttempt>,
    ) -> Result<T, String> {
        let mut deadline: Option<Instant> = self
            .has_stopped_managed()
            .then(|| Instant::now() + STORE_WAIT);
        loop {
            match attempt() {
                Ok(done) => return Ok(done),
                Err(InstallAttempt::Busy(why)) if deadline.is_none() && self.runs_any() => {
                    if let Err(stop) = self.stop_running(manager) {
                        return Err(self.resume(manager, format!("{why}; {stop}")));
                    }
                    deadline = Some(Instant::now() + STORE_WAIT);
                }
                Err(InstallAttempt::Busy(why)) => match deadline {
                    Some(at) if Instant::now() < at => std::thread::sleep(POLL),
                    _ => {
                        let why = self.held_elsewhere(why);
                        return Err(self.resume(manager, why));
                    }
                },
                Err(InstallAttempt::Failed(why)) => return Err(self.resume(manager, why)),
            }
        }
    }

    /// Points every service setup registered from this store at `program`, the newly pinned
    /// release's `agentic-org`, and starts again the ones that were running unless `restart`
    /// is off. One report per instance, in name order.
    pub fn refresh(
        self,
        manager: &dyn ServiceManager,
        program: &Path,
        restart: bool,
    ) -> Vec<ServiceRefreshReport> {
        self.entries
            .into_iter()
            .map(|entry| entry.refresh(manager, program, restart))
            .collect()
    }

    fn runs_any(&self) -> bool {
        self.entries.iter().any(|e| {
            matches!(
                e.found,
                Found::Managed {
                    was_running: true,
                    stopped: false,
                    ..
                }
            )
        })
    }

    /// Stops every running service setup registered from this store and waits until each
    /// has ended. The first that does not stop ends it, with why.
    fn stop_running(&mut self, manager: &dyn ServiceManager) -> Result<(), String> {
        for entry in &mut self.entries {
            if let Found::Managed {
                was_running: true,
                stopped,
                ..
            } = &mut entry.found
            {
                if !*stopped {
                    stop_and_wait(manager, &entry.id).map_err(|e| {
                        format!(
                            "the service of instance \"{}\" did not stop for the install: {e}",
                            entry.instance
                        )
                    })?;
                    *stopped = true;
                }
            }
        }
        Ok(())
    }

    /// `why`, after the services this install stopped were started again on their unchanged
    /// definitions, with any that could not be.
    fn resume(&mut self, manager: &dyn ServiceManager, why: String) -> String {
        let mut said = why;
        for entry in &mut self.entries {
            if let Found::Managed {
                was_running,
                stopped,
                ..
            } = &mut entry.found
            {
                if !*stopped || !*was_running {
                    continue;
                }
                match manager.start(&entry.id) {
                    Ok(()) => *stopped = false,
                    Err(e) => said.push_str(&format!(
                        "; the service of instance \"{}\" was stopped for the install and \
                         could not be started again ({e}): start it with `agentic-org service \
                         start{}`",
                        entry.instance,
                        name_flag(&entry.instance)
                    )),
                }
            }
        }
        said
    }

    /// A busy store's error, with the services this install left running and why.
    fn held_elsewhere(&self, why: String) -> String {
        let mut said = why;
        for entry in &self.entries {
            if let Found::Untouched {
                outcome: RefreshOutcome::Refused | RefreshOutcome::Failed,
                why,
                ..
            } = &entry.found
            {
                said.push_str(&format!(
                    "; the install did not stop the service of instance \"{}\" ({why}): stop it \
                     with `agentic-org service stop{} --yes` and install again",
                    entry.instance,
                    name_flag(&entry.instance)
                ));
            }
        }
        said
    }
}

impl Entry {
    /// Stops any running or crash-looping service (`want_running || *was_running || *unloaded`)
    /// BEFORE setting `*stopped = true`, cancelling `systemd` `ActiveState=activating` restarts
    /// and macOS `launchd` `KeepAlive` loops so `install` never hits `store.lock` contention and
    /// `manager.redefine` never fails with `ServiceError::Conflict`.
    fn prepare_for_rollback(&mut self, manager: &dyn ServiceManager, want_running: bool) {
        let Found::Managed {
            unit,
            was_running,
            unloaded,
            stopped,
        } = &mut self.found
        else {
            return;
        };
        if (want_running || *was_running || *unloaded) && !*stopped {
            match stop_and_wait(manager, &self.id) {
                Ok(()) => *stopped = true,
                Err(err) => {
                    let program = Some(unit.program.clone());
                    self.found = Found::Untouched {
                        outcome: RefreshOutcome::Failed,
                        why: format!("it could not be stopped before rollback: {err}"),
                        program,
                        running: false,
                    };
                    return;
                }
            }
        }
        if want_running {
            *was_running = true;
        }
    }

    fn refresh(
        self,
        manager: &dyn ServiceManager,
        program: &Path,
        restart: bool,
    ) -> ServiceRefreshReport {
        let flag = name_flag(&self.instance);
        match self.found {
            Found::Managed {
                unit,
                was_running,
                stopped,
                ..
            } => Move {
                manager,
                instance: &self.instance,
                id: &self.id,
                flag,
                was_running,
            }
            .run(unit, stopped, program, restart),
            Found::Untouched {
                outcome,
                why,
                program: found,
                running,
            } => {
                let note = untouched_note(outcome, &why, program, &flag);
                report(&self.instance, outcome, found.as_deref(), note).ran(running, running)
            }
        }
    }
}

impl ServiceRefreshReport {
    fn ran(mut self, was_running: bool, running: bool) -> Self {
        self.was_running = was_running;
        self.running = running;
        self
    }
}

impl RefreshOutcome {
    /// The JSON name, as `serde` writes it.
    pub fn wire(self) -> &'static str {
        match self {
            Self::Refreshed => "refreshed",
            Self::Unchanged => "unchanged",
            Self::Refused => "refused",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
        }
    }
}

fn untouched(outcome: RefreshOutcome, why: String, program: Option<PathBuf>) -> Found {
    Found::Untouched {
        outcome,
        why,
        program,
        running: false,
    }
}

/// An untouched service whose definition was read: whether it runs is asked too, so the
/// report says so (a service an install leaves alone keeps running or stopped as it was).
fn left_alone(
    manager: &dyn ServiceManager,
    id: &str,
    outcome: RefreshOutcome,
    why: String,
    program: Option<PathBuf>,
) -> Found {
    Found::Untouched {
        outcome,
        why,
        program,
        running: matches!(manager.status(id), Ok(ServiceStatus::Running { .. })),
    }
}

/// One instance's registered service, read without changing anything.
fn inspect_one(
    manager: &dyn ServiceManager,
    id: &str,
    name: &str,
    releases: Option<&Path>,
) -> Found {
    let unit = match manager.definition(id) {
        Ok(Some(unit)) => unit,
        // Setup recorded a registration the manager no longer has: nothing to point anywhere.
        Ok(None) => {
            let why = "no definition is registered".into();
            return untouched(RefreshOutcome::Skipped, why, None);
        }
        Err(ServiceError::Conflict(why)) => return untouched(RefreshOutcome::Refused, why, None),
        Err(e) => return untouched(RefreshOutcome::Failed, e.to_string(), None),
    };
    let program = Some(unit.program.clone());
    if unit.args != ["serve", "--name", name] {
        let why = "it runs other arguments than agentic-org serve --name <instance>".into();
        return left_alone(manager, id, RefreshOutcome::Refused, why, program);
    }
    if !releases.is_some_and(|releases| in_store(&unit.program, releases)) {
        let why = format!(
            "it runs {}, which is not a release in this store",
            unit.program.display()
        );
        return left_alone(manager, id, RefreshOutcome::Skipped, why, program);
    }
    match manager.status(id) {
        Ok(status) => Found::Managed {
            unit,
            was_running: matches!(status, ServiceStatus::Running { .. }),
            unloaded: matches!(status, ServiceStatus::Unloaded),
            stopped: false,
        },
        Err(ServiceError::Conflict(why)) => untouched(RefreshOutcome::Refused, why, program),
        Err(e) => untouched(RefreshOutcome::Failed, e.to_string(), program),
    }
}

/// Whether `program` is a release's `agentic-org` in the store whose `releases` folder is
/// `releases` (canonical): `<releases>/<release-id>/agentic-org`, written plainly (absolute,
/// with no `.`, `..` or empty component, and a release ID the store accepts).
fn in_store(program: &Path, releases: &Path) -> bool {
    let plain = program.is_absolute()
        && program
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
        && program.components().collect::<PathBuf>().as_os_str() == program.as_os_str();
    if !plain || program.file_name().and_then(|n| n.to_str()) != Some("agentic-org") {
        return false;
    }
    let Some(release) = program.parent() else {
        return false;
    };
    let named = release.file_name().and_then(|n| n.to_str());
    if !named.is_some_and(org_release::valid_release_id) {
        return false;
    }
    let Some(folder) = release.parent() else {
        return false;
    };
    folder == releases || std::fs::canonicalize(folder).is_ok_and(|f| f == releases)
}

/// This account's application base, where setup keeps one record per instance
/// (`setup/<name>`), found without making anything. A development build may name it
/// (`ORG_DEV_ACCOUNT_BASE`), which a release build refuses.
fn setup_base() -> Result<Option<PathBuf>, String> {
    let variable = org_host::human::DEV_ACCOUNT_BASE;
    if let Some(base) = std::env::var_os(variable).filter(|b| !b.is_empty()) {
        if !cfg!(debug_assertions) {
            return Err(format!(
                "{variable} is a development setting; this release build refuses it"
            ));
        }
        let base = PathBuf::from(base);
        return Ok(base.is_absolute().then_some(base));
    }
    let Some(account) = org_host::home::account_dir() else {
        return Ok(None);
    };
    Ok(org_platform::dirs::application_base(&account).ok())
}

#[cfg(test)]
#[path = "service_refresh_tests.rs"]
mod tests;
