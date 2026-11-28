//! Moving one registered service unit to a target release executable, stopping it safely
//! beforehand when needed, and rendering human-readable refresh reports.

use std::path::Path;
use std::time::{Duration, Instant};

use org_platform::service::{ServiceError, ServiceManager, ServiceStatus, Unit};

use super::{RefreshOutcome, ServiceRefreshReport};

/// How long a service asked to stop may take to end (the host stops within 2.5 s).
const STOP_WAIT: Duration = Duration::from_secs(30);
pub(super) const POLL: Duration = Duration::from_millis(100);

/// One service setup registered from this store, on its way to the new release.
pub(super) struct Move<'a> {
    pub(super) manager: &'a dyn ServiceManager,
    pub(super) instance: &'a str,
    pub(super) id: &'a str,
    /// ` --name <instance>`, or nothing for `default`.
    pub(super) flag: String,
    pub(super) was_running: bool,
}

impl Move<'_> {
    /// Points the definition at `program` unless it names it already (`stopped`: the
    /// install or rollback pre-stop already stopped the service), then starts it again or says
    /// how it is left.
    pub(super) fn run(
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
        if stopped && self.was_running && restart {
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
        if stopped && self.was_running {
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

pub(super) fn report(
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

/// What the operator does about a service an install left untouched.
pub(super) fn untouched_note(
    outcome: RefreshOutcome,
    why: &str,
    program: &Path,
    flag: &str,
) -> String {
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

/// Asks the service to stop and waits until the manager no longer reports it running.
pub(super) fn stop_and_wait(manager: &dyn ServiceManager, id: &str) -> Result<(), ServiceError> {
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
pub(super) fn name_flag(instance: &str) -> String {
    if instance == "default" {
        String::new()
    } else {
        format!(" --name {instance}")
    }
}
