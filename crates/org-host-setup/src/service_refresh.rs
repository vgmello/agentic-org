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

use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use org_platform::service::{ServiceError, ServiceManager, ServiceStatus, Unit};
use serde::Serialize;

use org_host::instance::setup::{Journal, Service};

use super::report::service_id;

/// How long a service asked to stop may take to end (the host stops within 2.5 s).
const STOP_WAIT: Duration = Duration::from_secs(30);
/// How long the store may stay busy after the services that held it stopped.
const STORE_WAIT: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(100);

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

/// The account's registered services, as an install found them and has changed them.
#[derive(Debug, Default)]
pub struct ServiceRefresh {
    entries: Vec<Entry>,
}

#[derive(Debug)]
struct Entry {
    instance: String,
    id: String,
    found: Found,
}

#[derive(Debug)]
enum Found {
    /// What setup registered, running a release in this store.
    Managed {
        unit: Unit,
        was_running: bool,
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
    pub(crate) fn inspect_at(manager: &dyn ServiceManager, base: &Path, store: &Path) -> Self {
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
                id,
                found,
            });
        }
        Self { entries }
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
        let mut deadline: Option<Instant> = None;
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
            if let Found::Managed { stopped, .. } = &mut entry.found {
                if !*stopped {
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

/// One service setup registered from this store, on its way to the new release.
struct Move<'a> {
    manager: &'a dyn ServiceManager,
    instance: &'a str,
    id: &'a str,
    /// ` --name <instance>`, or nothing for `default`.
    flag: String,
    was_running: bool,
}

impl Move<'_> {
    /// Points the definition at `program` unless it names it already (`stopped`: the
    /// install stopped the service), then starts it again or says how it is left.
    fn run(
        &self,
        unit: Unit,
        stopped: bool,
        program: &Path,
        restart: bool,
    ) -> ServiceRefreshReport {
        let target = Unit {
            program: program.to_owned(),
            ..unit.clone()
        };
        let (outcome, stopped) = if unit.program == target.program {
            (RefreshOutcome::Unchanged, stopped)
        } else {
            let stop = self.was_running && !stopped;
            match redefine(self.manager, self.id, &target, stop) {
                Ok(()) => (RefreshOutcome::Refreshed, stopped || stop),
                Err(refused) => return self.refused(refused, &unit.program, program, stopped),
            }
        };
        if stopped && restart {
            return self.restart(outcome, &target);
        }
        self.left(outcome, &target.program, stopped)
    }

    fn report(
        &self,
        outcome: RefreshOutcome,
        program: &Path,
        note: String,
        running: bool,
    ) -> ServiceRefreshReport {
        report(self.instance, outcome, Some(program), note).ran(self.was_running, running)
    }

    /// The definition could not be pointed at `program`: it still names `previous`.
    fn refused(
        &self,
        (outcome, why, now_stopped): (RefreshOutcome, String, bool),
        previous: &Path,
        program: &Path,
        stopped: bool,
    ) -> ServiceRefreshReport {
        let stopped = stopped || now_stopped;
        let mut note = untouched_note(outcome, &why, program, &self.flag);
        if stopped {
            note.push_str(
                "; it was stopped for the install and its previous release no longer starts",
            );
        }
        self.report(outcome, previous, note, self.was_running && !stopped)
    }

    /// Loads the definition again where the manager needs it (launchd) and starts it.
    fn restart(&self, outcome: RefreshOutcome, target: &Unit) -> ServiceRefreshReport {
        let shown = target.program.display();
        let started = self
            .manager
            .register(target)
            .and_then(|_| self.manager.start(self.id));
        match started {
            Ok(()) => {
                let note = format!("its service runs {shown} again");
                self.report(outcome, &target.program, note, true)
            }
            Err(e) => {
                let note = format!(
                    "its service now names {shown} but could not be started again ({e}): run \
                     `agentic-org setup{}`",
                    self.flag
                );
                self.report(RefreshOutcome::Failed, &target.program, note, false)
            }
        }
    }

    /// A service not started again: one that still runs, one that was stopped and stays
    /// so, or one `--no-restart` leaves stopped.
    fn left(&self, outcome: RefreshOutcome, program: &Path, stopped: bool) -> ServiceRefreshReport {
        let shown = program.display();
        if self.was_running && !stopped {
            let note = format!("its service already runs {shown}");
            return self.report(outcome, program, note, true);
        }
        let flag = &self.flag;
        let how = match self.manager.status(self.id) {
            // launchd loads an agent only to start it: the redefined one waits for a login.
            Ok(ServiceStatus::Unloaded) => {
                format!("it starts at the next login, or now with `agentic-org setup{flag}`")
            }
            _ => format!("start it with `agentic-org service start{flag}`"),
        };
        let why = if self.was_running {
            "it was stopped for the install and not started again (--no-restart)"
        } else {
            "it stays stopped"
        };
        let note = format!("its service now names {shown}; {why}: {how}");
        self.report(outcome, program, note, false)
    }
}

/// Stops the service first when `stop` (it still runs), then points its definition at
/// `target`. A refusal is the outcome, why, and whether this call stopped the service.
fn redefine(
    manager: &dyn ServiceManager,
    id: &str,
    target: &Unit,
    stop: bool,
) -> Result<(), (RefreshOutcome, String, bool)> {
    if stop {
        stop_and_wait(manager, id).map_err(|e| {
            let why = format!("it could not be stopped: {e}");
            (RefreshOutcome::Failed, why, false)
        })?;
    }
    match manager.redefine(target) {
        Ok(_) => Ok(()),
        Err(ServiceError::Conflict(why)) => Err((RefreshOutcome::Refused, why, stop)),
        Err(e) => Err((RefreshOutcome::Failed, e.to_string(), stop)),
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

fn report(
    instance: &str,
    outcome: RefreshOutcome,
    program: Option<&Path>,
    note: String,
) -> ServiceRefreshReport {
    ServiceRefreshReport {
        instance: instance.to_owned(),
        outcome,
        was_running: false,
        running: false,
        program: program.map(|p| p.display().to_string()),
        note: format!("Instance \"{instance}\": {note}."),
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

/// What the operator does about a service an install left untouched.
fn untouched_note(outcome: RefreshOutcome, why: &str, program: &Path, flag: &str) -> String {
    let program = program.display();
    match outcome {
        RefreshOutcome::Refused => format!(
            "its service definition was left untouched because it is not what setup wrote \
             ({why}); to move it to {program}: stop it (`agentic-org service stop{flag} \
             --yes`), change its program to {program} and reload it in the service manager, \
             then start it (`agentic-org service start{flag}`); or undo the changes and run \
             `agentic-org setup{flag}`"
        ),
        RefreshOutcome::Skipped => format!(
            "its service was left as it is ({why}); run `agentic-org setup{flag}` with the \
             release it should run"
        ),
        _ => format!(
            "its service was left as it is ({why}); when it is stopped, run `agentic-org \
             setup{flag}` to point it at {program}"
        ),
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

/// Asks the service to stop and waits until the manager no longer reports it running.
fn stop_and_wait(manager: &dyn ServiceManager, id: &str) -> Result<(), ServiceError> {
    manager.stop(id)?;
    let deadline = Instant::now() + STOP_WAIT;
    while matches!(manager.status(id)?, ServiceStatus::Running { .. }) {
        if Instant::now() >= deadline {
            return Err(ServiceError::System(format!(
                "it still runs {} s after it was asked to stop",
                STOP_WAIT.as_secs()
            )));
        }
        std::thread::sleep(POLL);
    }
    Ok(())
}

/// ` --name <instance>`, nothing for `default`.
fn name_flag(instance: &str) -> String {
    if instance == "default" {
        String::new()
    } else {
        format!(" --name {instance}")
    }
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
