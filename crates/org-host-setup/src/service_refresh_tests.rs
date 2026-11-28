//! TASK-155: `release install` refreshes the services setup registered (fake service
//! manager, setup records in a temporary folder; no real launchd or systemd unit).

use super::*;
use crate::{loopback, release_pin};
use org_host::instance::setup::Input;
use org_platform::service::Registration;
use org_platform::testing::{Call, FakeServiceManager};
use std::cell::Cell;
use std::os::unix::fs::PermissionsExt;

const DEFAULT_ID: &str = "01965d2e-8b1c-7c3a-9f00-0123456789ab";
const LAB_ID: &str = "01965d2e-8b1c-7c3a-9f00-0123456789ac";

/// An account folder with setup records, and a release store holding releases `r1`, `r2`.
struct Account {
    _temp: tempfile::TempDir,
    base: PathBuf,
    store: PathBuf,
}

impl Account {
    fn new() -> Self {
        let temp =
            tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
        let root = temp.path().canonicalize().unwrap();
        let base = root.join("base");
        std::fs::create_dir(&base).unwrap();
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700)).unwrap();
        let store = root.join("store");
        for release in ["r1", "r2"] {
            let folder = store.join("releases").join(release);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("agentic-org"), b"#!/bin/sh\n").unwrap();
        }
        Self {
            _temp: temp,
            base,
            store,
        }
    }

    fn program(&self, release: &str) -> PathBuf {
        self.store
            .join("releases")
            .join(release)
            .join("agentic-org")
    }

    /// Setup's record of instance `name`, reserved as `id`, with its service registered.
    fn set_up(&self, name: &str, id: &str, service: Service, port: u16) {
        let journal = Journal::open(&self.base, Some(name)).unwrap();
        let input = Input {
            version: 1,
            service,
            root: self.base.join("instances").join(name),
            credentials: None,
            listen: loopback(port).unwrap(),
            federation: None,
        };
        let mut record = journal
            .freeze(input, false, release_pin().unwrap())
            .unwrap();
        record.instance_id = Some(id.into());
        record.service_registered = true;
        journal.write(&record).unwrap();
    }

    /// The unit setup registers for instance `name` with `program`.
    fn unit(name: &str, id: &str, program: &Path) -> Unit {
        Unit {
            instance_id: id.into(),
            program: program.to_owned(),
            args: vec!["serve".into(), "--name".into(), name.into()],
            env: vec![("HOME".into(), "/Users/a".into())],
        }
    }

    fn inspect(&self, manager: &FakeServiceManager) -> ServiceRefresh {
        ServiceRefresh::inspect_at(manager, &self.base, &self.store)
    }
}

/// The install stand-in: the store is busy while any of `holders` runs (a release host
/// holds it), then the install succeeds, or fails with `fails`. Counts its attempts.
fn installer<'a>(
    manager: &'a FakeServiceManager,
    holders: &'a [&'a str],
    fails: Option<&'a str>,
    attempts: &'a Cell<u32>,
) -> impl FnMut() -> Result<&'static str, InstallAttempt> + 'a {
    move || {
        attempts.set(attempts.get() + 1);
        let held = holders
            .iter()
            .any(|id| matches!(manager.status(id), Ok(ServiceStatus::Running { .. })));
        if held {
            return Err(InstallAttempt::Busy("opening release store: busy".into()));
        }
        match fails {
            Some(why) => Err(InstallAttempt::Failed(why.into())),
            None => Ok("r2"),
        }
    }
}

#[test]
fn a_running_managed_service_is_stopped_refreshed_and_started_on_the_new_release() {
    let account = Account::new();
    account.set_up("default", DEFAULT_ID, Service::Registered, 7420);
    let manager = FakeServiceManager::default();
    let old = Account::unit("default", DEFAULT_ID, &account.program("r1"));
    assert_eq!(manager.register(&old).unwrap(), Registration::Loaded);

    let mut refresh = account.inspect(&manager);
    assert_eq!(
        refresh.planned(),
        vec![("default".to_owned(), "refresh", true)]
    );
    let attempts = Cell::new(0);
    let installed = refresh
        .install(
            &manager,
            installer(&manager, &[DEFAULT_ID], None, &attempts),
        )
        .unwrap();
    assert_eq!(installed, "r2");
    assert_eq!(
        attempts.get(),
        2,
        "busy once, then installed after the stop"
    );
    let reports = refresh.refresh(&manager, &account.program("r2"), true);

    assert_eq!(reports.len(), 1);
    let report = &reports[0];
    assert_eq!(report.outcome, RefreshOutcome::Refreshed, "{report:?}");
    assert!(report.was_running && report.running, "{report:?}");
    assert_eq!(
        report.program.as_deref(),
        Some(account.program("r2").to_str().unwrap())
    );
    assert!(report.note.contains("runs") && report.note.contains("again"));
    // Only the program changed: the arguments and environment are what setup wrote.
    let now = manager.unit(DEFAULT_ID).unwrap();
    assert_eq!(
        now,
        Unit {
            program: account.program("r2"),
            ..old
        }
    );
    assert_eq!(
        manager.status(DEFAULT_ID).unwrap(),
        ServiceStatus::Running { pid: 1 }
    );
    let calls = manager.calls();
    let stop = calls
        .iter()
        .position(|c| *c == Call::Stop(DEFAULT_ID.into()));
    let redefine = calls.iter().position(|c| matches!(c, Call::Redefine(_)));
    let start = calls
        .iter()
        .position(|c| *c == Call::Start(DEFAULT_ID.into()));
    assert!(stop < redefine && redefine < start, "{calls:?}");
}

#[test]
fn a_stopped_managed_service_is_refreshed_and_left_stopped() {
    let account = Account::new();
    account.set_up("lab", LAB_ID, Service::Registered, 7421);
    let manager = FakeServiceManager::default();
    manager
        .register(&Account::unit("lab", LAB_ID, &account.program("r1")))
        .unwrap();
    manager.stop(LAB_ID).unwrap();

    let mut refresh = account.inspect(&manager);
    let attempts = Cell::new(0);
    refresh
        .install(&manager, installer(&manager, &[LAB_ID], None, &attempts))
        .unwrap();
    assert_eq!(attempts.get(), 1);
    let reports = refresh.refresh(&manager, &account.program("r2"), true);

    let report = &reports[0];
    assert_eq!(report.outcome, RefreshOutcome::Refreshed, "{report:?}");
    assert!(!report.was_running && !report.running, "{report:?}");
    assert!(
        report.note.contains("stays stopped")
            && report.note.contains("agentic-org service start --name lab"),
        "{}",
        report.note
    );
    assert_eq!(manager.unit(LAB_ID).unwrap().program, account.program("r2"));
    assert_eq!(manager.status(LAB_ID).unwrap(), ServiceStatus::Stopped);
    assert!(
        !manager.calls().iter().any(|c| matches!(c, Call::Start(_))),
        "a stopped service is never started"
    );
}

#[test]
fn no_restart_refreshes_a_running_service_and_leaves_it_stopped() {
    let account = Account::new();
    account.set_up("default", DEFAULT_ID, Service::Registered, 7420);
    let manager = FakeServiceManager::default();
    manager
        .register(&Account::unit(
            "default",
            DEFAULT_ID,
            &account.program("r1"),
        ))
        .unwrap();
    let mut refresh = account.inspect(&manager);
    let attempts = Cell::new(0);
    refresh
        .install(
            &manager,
            installer(&manager, &[DEFAULT_ID], None, &attempts),
        )
        .unwrap();
    let reports = refresh.refresh(&manager, &account.program("r2"), false);
    let report = &reports[0];
    assert_eq!(report.outcome, RefreshOutcome::Refreshed);
    assert!(report.was_running && !report.running, "{report:?}");
    assert!(report.note.contains("--no-restart"), "{}", report.note);
    assert_eq!(manager.status(DEFAULT_ID).unwrap(), ServiceStatus::Stopped);
    assert!(!manager.calls().contains(&Call::Start(DEFAULT_ID.into())));
}

/// A definition changed by hand is never stopped, replaced or started: the install that
/// needs its store fails with what to do, and an install that does not names the steps.
#[test]
fn a_hand_edited_service_definition_is_refused_with_manual_instructions() {
    let account = Account::new();
    account.set_up("default", DEFAULT_ID, Service::Registered, 7420);
    let manager = FakeServiceManager::default();
    let old = Account::unit("default", DEFAULT_ID, &account.program("r1"));
    manager.register(&old).unwrap();
    manager.edit_by_hand(DEFAULT_ID);

    let mut refresh = account.inspect(&manager);
    assert_eq!(
        refresh.planned(),
        vec![("default".to_owned(), "refused", false)]
    );
    // Running, it holds the store: the install is refused, with the instance named.
    let attempts = Cell::new(0);
    let error = refresh
        .install(
            &manager,
            installer(&manager, &[DEFAULT_ID], None, &attempts),
        )
        .unwrap_err();
    assert!(
        error.contains("busy")
            && error.contains("instance \"default\"")
            && error.contains("changed by hand")
            && error.contains("agentic-org service stop --yes"),
        "{error}"
    );
    assert_eq!(
        attempts.get(),
        1,
        "nothing was stopped, so nothing is tried again"
    );

    // Stopped by the operator, the install goes ahead and the definition stays untouched.
    manager.stop(DEFAULT_ID).unwrap();
    let mut refresh = account.inspect(&manager);
    refresh
        .install(
            &manager,
            installer(&manager, &[DEFAULT_ID], None, &attempts),
        )
        .unwrap();
    let reports = refresh.refresh(&manager, &account.program("r2"), true);
    let report = &reports[0];
    assert_eq!(report.outcome, RefreshOutcome::Refused, "{report:?}");
    assert!(!report.running);
    let program = account.program("r2").display().to_string();
    assert!(
        report.note.contains("changed by hand")
            && report
                .note
                .contains(&format!("change its program to {program}"))
            && report.note.contains("agentic-org setup"),
        "{}",
        report.note
    );
    assert_eq!(manager.unit(DEFAULT_ID).unwrap(), old);
    let touched = manager
        .calls()
        .into_iter()
        .filter(|c| matches!(c, Call::Redefine(_) | Call::Start(_) | Call::Register(_)))
        .count();
    assert_eq!(touched, 1, "only the test's own registration");
}

/// What setup did not register from this store is left as it is: a development build's
/// program, other arguments, foreground instances and records without a registration.
#[test]
fn services_that_do_not_run_a_release_of_this_store_are_left_alone() {
    let account = Account::new();
    account.set_up("default", DEFAULT_ID, Service::Registered, 7420);
    account.set_up("lab", LAB_ID, Service::Registered, 7421);
    account.set_up(
        "box",
        "01965d2e-8b1c-7c3a-9f00-0123456789ad",
        Service::Foreground,
        7422,
    );
    let manager = FakeServiceManager::default();
    let dev = Account::unit(
        "default",
        DEFAULT_ID,
        Path::new("/opt/dev/target/agentic-org"),
    );
    manager.register(&dev).unwrap();
    let other = Unit {
        args: vec!["serve".into(), "--name".into(), "elsewhere".into()],
        ..Account::unit("lab", LAB_ID, &account.program("r1"))
    };
    manager.register(&other).unwrap();
    manager.stop(LAB_ID).unwrap();
    let before = manager.calls().len();

    // What runs is reported as running, though the install leaves it alone (review nit:
    // a running development build was reported as not running).
    let refresh = account.inspect(&manager);
    assert_eq!(
        refresh.planned(),
        vec![
            ("default".to_owned(), "skipped", true),
            ("lab".to_owned(), "refused", false),
        ]
    );
    let reports = refresh.refresh(&manager, &account.program("r2"), true);
    assert_eq!(reports[0].outcome, RefreshOutcome::Skipped);
    assert!(
        reports[0].was_running && reports[0].running,
        "{:?}",
        reports[0]
    );
    assert!(
        reports[0].note.contains("not a release in this store"),
        "{}",
        reports[0].note
    );
    assert_eq!(reports[1].outcome, RefreshOutcome::Refused);
    assert!(
        !reports[1].was_running && !reports[1].running,
        "{:?}",
        reports[1]
    );
    assert_eq!(manager.unit(DEFAULT_ID).unwrap(), dev);
    assert_eq!(manager.unit(LAB_ID).unwrap(), other);
    assert!(!manager.calls()[before..]
        .iter()
        .any(|c| matches!(c, Call::Stop(_) | Call::Redefine(_) | Call::Start(_))));
}

/// A release's `agentic-org` in this store is only `<releases>/<release-id>/agentic-org`
/// written plainly: a `.`, `..` or empty component, or a folder that is no release ID, is
/// not one of the store's releases, whatever it resolves to.
#[test]
fn only_a_plainly_written_release_program_is_in_the_store() {
    let account = Account::new();
    let releases = account.store.join("releases").canonicalize().unwrap();
    let r = releases.display();
    assert!(in_store(&account.program("r1"), &releases));
    for odd in [
        format!("{r}/./agentic-org"),
        format!("{r}/r1/./agentic-org"),
        format!("{r}/./r1/agentic-org"),
        format!("{r}//r1/agentic-org"),
        format!("{r}/r1/agentic-org/"),
        format!("{r}/../releases/r1/agentic-org"),
        format!("{r}/r1/../r1/agentic-org"),
        format!("{r}/.hidden/agentic-org"),
        format!("{r}/r 1/agentic-org"),
        format!("{r}/agentic-org"),
        format!("{r}/r1/agentic-org-web"),
        "releases/r1/agentic-org".to_owned(),
    ] {
        assert!(!in_store(Path::new(&odd), &releases), "{odd}");
    }
}

/// A failed install starts again what it stopped, on the definition it had.
#[test]
fn a_failed_install_starts_the_stopped_services_again_unchanged() {
    let account = Account::new();
    account.set_up("default", DEFAULT_ID, Service::Registered, 7420);
    let manager = FakeServiceManager::default();
    let old = Account::unit("default", DEFAULT_ID, &account.program("r1"));
    manager.register(&old).unwrap();
    let mut refresh = account.inspect(&manager);
    let attempts = Cell::new(0);
    let error = refresh
        .install(
            &manager,
            installer(
                &manager,
                &[DEFAULT_ID],
                Some("verifying: bad signature"),
                &attempts,
            ),
        )
        .unwrap_err();
    assert_eq!(error, "verifying: bad signature");
    assert_eq!(manager.unit(DEFAULT_ID).unwrap(), old);
    assert_eq!(
        manager.status(DEFAULT_ID).unwrap(),
        ServiceStatus::Running { pid: 1 }
    );
    assert!(!manager
        .calls()
        .iter()
        .any(|c| matches!(c, Call::Redefine(_))));
}

/// The same release again: a running service already on it is not restarted, and nothing
/// is redefined.
#[test]
fn a_service_already_on_the_release_is_left_running() {
    let account = Account::new();
    account.set_up("default", DEFAULT_ID, Service::Registered, 7420);
    let manager = FakeServiceManager::default();
    let current = Account::unit("default", DEFAULT_ID, &account.program("r2"));
    manager.register(&current).unwrap();
    let mut refresh = account.inspect(&manager);
    let attempts = Cell::new(0);
    refresh
        .install(&manager, installer(&manager, &[], None, &attempts))
        .unwrap();
    let reports = refresh.refresh(&manager, &account.program("r2"), true);
    assert_eq!(reports[0].outcome, RefreshOutcome::Unchanged);
    assert!(reports[0].was_running && reports[0].running);
    assert!(!manager
        .calls()
        .iter()
        .any(|c| matches!(c, Call::Stop(_) | Call::Redefine(_) | Call::Start(_))));
}

#[test]
fn no_account_records_means_nothing_is_asked() {
    let account = Account::new();
    let manager = FakeServiceManager::default();
    let mut refresh = account.inspect(&manager);
    assert!(refresh.planned().is_empty());
    let attempts = Cell::new(0);
    let error = refresh
        .install(&manager, installer(&manager, &[], Some("nope"), &attempts))
        .unwrap_err();
    assert_eq!(error, "nope");
    assert!(refresh
        .refresh(&manager, &account.program("r2"), true)
        .is_empty());
    assert!(manager.calls().is_empty());
}

/// P1-1: On Linux `systemd`, a crash-looping unit reports `ActiveState=activating` with no
/// running PID (`ServiceStatus::Unloaded`, so `was_running == false`), and `redefine` rejects it
/// with `ServiceError::Conflict` unless `stop` is called first. `rollback_to_program` must always
/// call `stop_and_wait` whenever `want_running || was_running` and `!stopped`.
#[test]
fn rollback_to_program_stops_activating_crash_loop_before_redefine() {
    use std::sync::Mutex;

    struct ActivatingCrashLoopManager {
        state: Mutex<ActivatingState>,
    }

    struct ActivatingState {
        unit: Unit,
        activating: bool,
        running: bool,
        events: Vec<String>,
    }

    impl ServiceManager for ActivatingCrashLoopManager {
        fn register(&self, unit: &Unit) -> Result<Registration, ServiceError> {
            let mut st = self.state.lock().unwrap();
            st.unit = unit.clone();
            st.events.push("register".into());
            Ok(Registration::Loaded)
        }

        fn start(&self, id: &str) -> Result<(), ServiceError> {
            let mut st = self.state.lock().unwrap();
            assert_eq!(st.unit.instance_id, id);
            st.activating = false;
            st.running = true;
            st.events.push("start".into());
            Ok(())
        }

        fn stop(&self, id: &str) -> Result<(), ServiceError> {
            let mut st = self.state.lock().unwrap();
            assert_eq!(st.unit.instance_id, id);
            st.activating = false;
            st.running = false;
            st.events.push("stop".into());
            Ok(())
        }

        fn status(&self, id: &str) -> Result<ServiceStatus, ServiceError> {
            let st = self.state.lock().unwrap();
            if st.unit.instance_id != id {
                return Ok(ServiceStatus::NotRegistered);
            }
            if st.running {
                Ok(ServiceStatus::Running { pid: 99 })
            } else if st.activating {
                // Mirrors `systemd.rs:429-430`: `shown.running() == None && !shown.settled()`.
                Ok(ServiceStatus::Unloaded)
            } else {
                Ok(ServiceStatus::Stopped)
            }
        }

        fn unregister(&self, _id: &str) -> Result<(), ServiceError> {
            Ok(())
        }

        fn definition(&self, id: &str) -> Result<Option<Unit>, ServiceError> {
            let st = self.state.lock().unwrap();
            Ok((st.unit.instance_id == id).then(|| st.unit.clone()))
        }

        fn redefine(&self, unit: &Unit) -> Result<Registration, ServiceError> {
            let mut st = self.state.lock().unwrap();
            if st.activating || st.running {
                return Err(ServiceError::Conflict(
                    "unit is activating; stop it before its definition is replaced".into(),
                ));
            }
            st.unit = unit.clone();
            st.events
                .push(format!("redefine:{}", unit.program.display()));
            Ok(Registration::Loaded)
        }
    }

    let account = Account::new();
    account.set_up("default", DEFAULT_ID, Service::Registered, 7420);
    let mgr = ActivatingCrashLoopManager {
        state: Mutex::new(ActivatingState {
            unit: Account::unit("default", DEFAULT_ID, &account.program("r2")),
            activating: true,
            running: false,
            events: Vec::new(),
        }),
    };

    let refresh = ServiceRefresh::inspect_at(&mgr, &account.base, &account.store);
    let managed = refresh.managed_targets();
    assert_eq!(managed.len(), 1);
    assert!(
        !managed[0].was_running,
        "activating crash-loop inspects as not running"
    );

    let reports =
        refresh.rollback_to_program(&mgr, &account.program("r1"), &["default".into()], true);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].outcome, RefreshOutcome::Refreshed, "{reports:?}");
    assert!(reports[0].running, "{reports:?}");
    let st = mgr.state.lock().unwrap();
    assert_eq!(st.unit.program, account.program("r1"));
    assert!(st.running);
    let stop_pos = st
        .events
        .iter()
        .position(|e| e == "stop")
        .expect("rollback must call stop before redefine");
    let redefine_pos = st
        .events
        .iter()
        .position(|e| e.starts_with("redefine:"))
        .expect("rollback must redefine to previous program");
    let start_pos = st
        .events
        .iter()
        .position(|e| e == "start")
        .expect("rollback must restart service");
    assert!(
        stop_pos < redefine_pos && redefine_pos < start_pos,
        "{:?}",
        st.events
    );
}

/// P1-2: An activating/crash-looping unit inspects as `was_running == false` (`Unloaded`) and
/// holds `store.lock` until `stop` is called. `stop_for_rollback` must stop it BEFORE `install`
/// acquires `store.lock`, and subsequent `rollback_to_program` must be idempotent (no second stop).
#[test]
fn p1_2_stop_for_rollback_stops_activating_lock_holder_before_install_and_is_idempotent() {
    use std::sync::Mutex;

    struct ActivatingLockHolderManager {
        state: Mutex<LockHolderState>,
    }

    struct LockHolderState {
        unit: Unit,
        activating: bool,
        running: bool,
        holds_store_lock: bool,
        events: Vec<String>,
    }

    impl ServiceManager for ActivatingLockHolderManager {
        fn register(&self, unit: &Unit) -> Result<Registration, ServiceError> {
            let mut st = self.state.lock().unwrap();
            st.unit = unit.clone();
            st.events.push("register".into());
            Ok(Registration::Loaded)
        }

        fn start(&self, id: &str) -> Result<(), ServiceError> {
            let mut st = self.state.lock().unwrap();
            assert_eq!(st.unit.instance_id, id);
            st.activating = false;
            st.running = true;
            st.events.push("start".into());
            Ok(())
        }

        fn stop(&self, id: &str) -> Result<(), ServiceError> {
            let mut st = self.state.lock().unwrap();
            assert_eq!(st.unit.instance_id, id);
            st.activating = false;
            st.running = false;
            st.holds_store_lock = false;
            st.events.push("stop".into());
            Ok(())
        }

        fn status(&self, id: &str) -> Result<ServiceStatus, ServiceError> {
            let st = self.state.lock().unwrap();
            if st.unit.instance_id != id {
                return Ok(ServiceStatus::NotRegistered);
            }
            if st.running {
                Ok(ServiceStatus::Running { pid: 77 })
            } else if st.activating {
                Ok(ServiceStatus::Unloaded)
            } else {
                Ok(ServiceStatus::Stopped)
            }
        }

        fn unregister(&self, _id: &str) -> Result<(), ServiceError> {
            Ok(())
        }

        fn definition(&self, id: &str) -> Result<Option<Unit>, ServiceError> {
            let st = self.state.lock().unwrap();
            Ok((st.unit.instance_id == id).then(|| st.unit.clone()))
        }

        fn redefine(&self, unit: &Unit) -> Result<Registration, ServiceError> {
            let mut st = self.state.lock().unwrap();
            if st.activating || st.running {
                return Err(ServiceError::Conflict("unit is still activating".into()));
            }
            st.unit = unit.clone();
            st.events
                .push(format!("redefine:{}", unit.program.display()));
            Ok(Registration::Loaded)
        }
    }

    let account = Account::new();
    account.set_up("default", DEFAULT_ID, Service::Registered, 7420);
    let mgr = ActivatingLockHolderManager {
        state: Mutex::new(LockHolderState {
            unit: Account::unit("default", DEFAULT_ID, &account.program("r2")),
            activating: true,
            running: false,
            holds_store_lock: true,
            events: Vec::new(),
        }),
    };

    let mut refresh = ServiceRefresh::inspect_at(&mgr, &account.base, &account.store);
    assert!(!refresh.managed_targets()[0].was_running);

    let should_start = vec!["default".to_owned()];
    refresh.stop_for_rollback(&mgr, &should_start);

    let attempts = Cell::new(0);
    let restored = refresh
        .install(&mgr, || {
            attempts.set(attempts.get() + 1);
            let mut st = mgr.state.lock().unwrap();
            if st.holds_store_lock {
                return Err(InstallAttempt::Busy("store.lock held by crash-loop".into()));
            }
            st.events.push("install:ok".into());
            Ok("r1")
        })
        .expect("stop_for_rollback must release store.lock before install");
    assert_eq!(restored, "r1");
    assert_eq!(attempts.get(), 1);

    let reports = refresh.rollback_to_program(&mgr, &account.program("r1"), &should_start, true);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].outcome, RefreshOutcome::Refreshed, "{reports:?}");
    assert!(reports[0].running);

    let st = mgr.state.lock().unwrap();
    let stop_count = st.events.iter().filter(|e| e.as_str() == "stop").count();
    assert_eq!(
        stop_count, 1,
        "rollback_to_program must be idempotent when already stopped: {:?}",
        st.events
    );
    let stop_pos = st.events.iter().position(|e| e == "stop").unwrap();
    let install_pos = st.events.iter().position(|e| e == "install:ok").unwrap();
    let redefine_pos = st
        .events
        .iter()
        .position(|e| e.starts_with("redefine:"))
        .unwrap();
    let start_pos = st.events.iter().position(|e| e == "start").unwrap();
    assert!(
        stop_pos < install_pos && install_pos < redefine_pos && redefine_pos < start_pos,
        "{:?}",
        st.events
    );
}
