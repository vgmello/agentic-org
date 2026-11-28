use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::drain::{
    wait_for_idle_drain, ActiveTurnSnapshot, DiskActiveTurnDrainProbe, DrainConfig, DrainPhase,
    DrainProbe,
};
use super::health::{evaluate_post_restart_health, HealthProbe, HealthTarget};
use super::read_store_head_readonly;
use crate::service_refresh::{ManagedServiceInfo, RefreshOutcome, ServiceRefreshReport};

struct CountdownDrainProbe {
    remaining_busy_polls: AtomicUsize,
}

impl DrainProbe for CountdownDrainProbe {
    fn active_turns(&self) -> Result<Vec<ActiveTurnSnapshot>, String> {
        let prev = self.remaining_busy_polls.load(Ordering::Acquire);
        if prev == 0 {
            return Ok(Vec::new());
        }
        self.remaining_busy_polls.fetch_sub(1, Ordering::AcqRel);
        Ok(vec![ActiveTurnSnapshot {
            instance: "default".into(),
            office_id: "office-1".into(),
            employee_id: "emp-1".into(),
            run_id: "run-1".into(),
        }])
    }
}

struct AlwaysHealthyProbe;

impl HealthProbe for AlwaysHealthyProbe {
    fn check_healthy(&self, _target: &HealthTarget, _timeout: Duration) -> Result<String, String> {
        Ok("healthy".into())
    }
}

#[test]
fn idle_drain_waits_for_active_turn_to_finish_then_becomes_ready() {
    let probe = CountdownDrainProbe {
        remaining_busy_polls: AtomicUsize::new(2),
    };
    let receipt = wait_for_idle_drain(
        &probe,
        DrainConfig {
            timeout: Duration::from_secs(2),
            poll_interval: Duration::from_millis(10),
            force: false,
        },
    )
    .unwrap();
    assert_eq!(receipt.phase, DrainPhase::Ready);
    assert!(!receipt.forced);
    assert!(receipt.remaining_turns.is_empty());
}

#[test]
fn idle_drain_times_out_cleanly_unless_forced() {
    let busy = CountdownDrainProbe {
        remaining_busy_polls: AtomicUsize::new(100),
    };
    let timeout_receipt = wait_for_idle_drain(
        &busy,
        DrainConfig {
            timeout: Duration::from_millis(40),
            poll_interval: Duration::from_millis(10),
            force: false,
        },
    )
    .unwrap();
    assert_eq!(timeout_receipt.phase, DrainPhase::BusyTimeout);
    assert!(!timeout_receipt.forced);
    assert_eq!(timeout_receipt.remaining_turns.len(), 1);

    let forced_receipt = wait_for_idle_drain(
        &busy,
        DrainConfig {
            timeout: Duration::from_millis(40),
            poll_interval: Duration::from_millis(10),
            force: true,
        },
    )
    .unwrap();
    assert_eq!(forced_receipt.phase, DrainPhase::Ready);
    assert!(forced_receipt.forced);
    assert_eq!(forced_receipt.remaining_turns.len(), 1);
}

#[test]
fn disk_drain_probe_detects_hq_and_satellite_layouts_and_fails_closed_on_corruption_and_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();

    // 1. Real HQ durable layout: `<instance_root>/<org_id>/durable/local/offices/<office_id>/sessions/<emp>`
    let hq_office_dir = root.join("office-state/offices/off-hq");
    fs::create_dir_all(&hq_office_dir).unwrap();
    fs::write(
        hq_office_dir.join("office.json"),
        br#"{"office_id":"off-hq","org_id":"org-hq"}"#,
    )
    .unwrap();
    let hq_emp = root.join("org-hq/durable/local/offices/off-hq/sessions/emp-hq");
    fs::create_dir_all(&hq_emp).unwrap();
    fs::write(
        hq_emp.join("active-turn.v1.json"),
        br#"{"employee_id":"emp-hq","run_id":"run-hq-1","active":true,"stall_locked":false,"recovery_required":false}"#,
    )
    .unwrap();

    // 2. Real Satellite durable layout (matching `org-host::writer::satellite`):
    //    `<instance_root>/<org_id>/offices/<office_id>/durable/local/offices/<office_id>/sessions/<emp>`
    let sat_office_dir = root.join("office-state/offices/off-sat");
    fs::create_dir_all(&sat_office_dir).unwrap();
    fs::write(
        sat_office_dir.join("office.json"),
        br#"{"office_id":"off-sat","org_id":"org-sat"}"#,
    )
    .unwrap();
    let sat_emp =
        root.join("org-sat/offices/off-sat/durable/local/offices/off-sat/sessions/emp-sat");
    fs::create_dir_all(&sat_emp).unwrap();
    fs::write(
        sat_emp.join("pending-start.v1.json"),
        br#"{"employee_id":"emp-sat","run_id":"run-sat-1"}"#,
    )
    .unwrap();

    let probe = DiskActiveTurnDrainProbe::new(vec![("prod".into(), root.clone())]);
    let found = probe.active_turns().unwrap();
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[0].office_id, "off-hq");
    assert_eq!(found[0].employee_id, "emp-hq");
    assert_eq!(found[1].office_id, "off-sat");
    assert_eq!(found[1].employee_id, "emp-sat");

    // Mark both idle -> probe reports empty.
    fs::write(
        hq_emp.join("active-turn.v1.json"),
        br#"{"employee_id":"emp-hq","run_id":"run-hq-1","active":false}"#,
    )
    .unwrap();
    fs::remove_file(sat_emp.join("pending-start.v1.json")).unwrap();
    assert!(probe.active_turns().unwrap().is_empty());

    // 3. Corrupt `active-turn.v1.json` fails closed (never treated as idle).
    fs::write(hq_emp.join("active-turn.v1.json"), b"{not-json").unwrap();
    assert!(
        probe.active_turns().is_err(),
        "corrupt active-turn.v1.json must fail closed"
    );
    fs::write(
        hq_emp.join("active-turn.v1.json"),
        br#"{"employee_id":"emp-hq","run_id":"run-hq-1","active":false}"#,
    )
    .unwrap();

    // 4. Corrupt `pending-start.v1.json` fails closed.
    fs::write(sat_emp.join("pending-start.v1.json"), b"broken").unwrap();
    assert!(
        probe.active_turns().is_err(),
        "corrupt pending-start.v1.json must fail closed"
    );
    fs::remove_file(sat_emp.join("pending-start.v1.json")).unwrap();

    // 5. Corrupt `office.json` fails closed.
    fs::write(sat_office_dir.join("office.json"), b"{bad").unwrap();
    assert!(
        probe.active_turns().is_err(),
        "corrupt office.json must fail closed"
    );
    fs::write(
        sat_office_dir.join("office.json"),
        br#"{"office_id":"off-sat","org_id":"org-sat"}"#,
    )
    .unwrap();
    assert!(probe.active_turns().unwrap().is_empty());

    // 6. Intermediate symlink (`durable/`, `local/`, `offices/`, `sessions/`) fails closed.
    let decoy = root.join("decoy-empty");
    fs::create_dir_all(&decoy).unwrap();
    let sat_sessions = root.join("org-sat/offices/off-sat/durable/local/offices/off-sat/sessions");
    fs::remove_dir_all(&sat_sessions).unwrap();
    symlink(&decoy, &sat_sessions).unwrap();
    assert!(
        probe.active_turns().is_err(),
        "symlinked sessions/ directory must fail closed"
    );
    fs::remove_file(&sat_sessions).unwrap();

    let sat_durable = root.join("org-sat/offices/off-sat/durable");
    fs::remove_dir_all(&sat_durable).unwrap();
    symlink(&decoy, &sat_durable).unwrap();
    assert!(
        probe.active_turns().is_err(),
        "symlinked durable/ directory must fail closed"
    );
    fs::remove_file(&sat_durable).unwrap();

    // 7. Leaf symlink on `active-turn.v1.json` fails closed.
    let outside = root.join("outside.json");
    fs::write(&outside, br#"{}"#).unwrap();
    fs::remove_file(hq_emp.join("active-turn.v1.json")).unwrap();
    symlink(&outside, hq_emp.join("active-turn.v1.json")).unwrap();
    assert!(probe.active_turns().is_err());
}

#[test]
fn read_store_head_readonly_rejects_symlinked_store_dir_even_with_trailing_slash() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let real = root.join("real-store");
    fs::create_dir_all(real.join("pins")).unwrap();
    let link = root.join("link-store");
    symlink(&real, &link).unwrap();

    assert!(
        read_store_head_readonly(&link).is_err(),
        "symlinked store_dir must be rejected"
    );
    let with_slash = PathBuf::from(format!("{}/", link.display()));
    assert!(
        read_store_head_readonly(&with_slash).is_err(),
        "symlinked store_dir with trailing slash must be rejected"
    );
}

#[test]
fn evaluate_post_restart_health_fails_closed_on_missing_or_partial_service_failures() {
    let managed = vec![
        ManagedServiceInfo {
            instance: "svc-a".into(),
            instance_id: "id-a".into(),
            service_id: "id-a".into(),
            root: PathBuf::from("/tmp/a"),
            previous_program: PathBuf::from("/store/v1/agentic-org"),
            was_running: true,
        },
        ManagedServiceInfo {
            instance: "svc-b".into(),
            instance_id: "id-b".into(),
            service_id: "id-b".into(),
            root: PathBuf::from("/tmp/b"),
            previous_program: PathBuf::from("/store/v1/agentic-org"),
            was_running: true,
        },
    ];
    let probe = AlwaysHealthyProbe;

    // 1. Empty reports when services were running must fail closed (never treated as healthy).
    let (ok, reports) =
        evaluate_post_restart_health(&probe, &managed, &[], true, Duration::from_secs(1));
    assert!(!ok);
    assert_eq!(reports.len(), 2);

    // 2. Partial missing report (only svc-a present, svc-b vanished) fails closed.
    let only_a = vec![ServiceRefreshReport {
        instance: "svc-a".into(),
        outcome: RefreshOutcome::Refreshed,
        was_running: true,
        running: true,
        program: Some("/store/v2/agentic-org".into()),
        note: "ok".into(),
    }];
    let (ok, reports) =
        evaluate_post_restart_health(&probe, &managed, &only_a, true, Duration::from_secs(1));
    assert!(!ok);
    assert!(reports.iter().any(|r| r.instance == "svc-b" && !r.healthy));

    // 3. Partial redefine failure (even if running: true on old program) fails closed.
    let partial_fail = vec![
        only_a[0].clone(),
        ServiceRefreshReport {
            instance: "svc-b".into(),
            outcome: RefreshOutcome::Failed,
            was_running: true,
            running: true,
            program: Some("/store/v1/agentic-org".into()),
            note: "redefine conflict".into(),
        },
    ];
    let (ok, reports) = evaluate_post_restart_health(
        &probe,
        &managed,
        &partial_fail,
        true,
        Duration::from_secs(1),
    );
    assert!(!ok);
    assert!(reports.iter().any(|r| r.instance == "svc-b" && !r.healthy));
}

#[test]
fn p2_1_clear_pending_only_when_services_resumed_and_store_unmodified() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

    use super::clear_pending_if_install_unmodified;
    use crate::service_refresh::{InstallAttempt, ServiceRefresh};
    use crate::{loopback, release_pin};
    use org_host::instance::setup::{Input, Journal, Service};
    use org_host::release::{ReleasePin, UpdateStateStore};
    use org_platform::service::{Registration, ServiceError, ServiceManager, ServiceStatus, Unit};

    struct ResumeFailManager {
        unit: Unit,
        running: Mutex<bool>,
        fail_start: Mutex<bool>,
    }

    impl ServiceManager for ResumeFailManager {
        fn register(&self, _unit: &Unit) -> Result<Registration, ServiceError> {
            Ok(Registration::Loaded)
        }
        fn start(&self, _id: &str) -> Result<(), ServiceError> {
            if *self.fail_start.lock().unwrap() {
                return Err(ServiceError::System("cannot restart".into()));
            }
            *self.running.lock().unwrap() = true;
            Ok(())
        }
        fn stop(&self, _id: &str) -> Result<(), ServiceError> {
            *self.running.lock().unwrap() = false;
            Ok(())
        }
        fn status(&self, id: &str) -> Result<ServiceStatus, ServiceError> {
            if self.unit.instance_id != id {
                return Ok(ServiceStatus::NotRegistered);
            }
            if *self.running.lock().unwrap() {
                Ok(ServiceStatus::Running { pid: 10 })
            } else {
                Ok(ServiceStatus::Stopped)
            }
        }
        fn unregister(&self, _id: &str) -> Result<(), ServiceError> {
            Ok(())
        }
        fn definition(&self, id: &str) -> Result<Option<Unit>, ServiceError> {
            Ok((self.unit.instance_id == id).then(|| self.unit.clone()))
        }
        fn redefine(&self, _unit: &Unit) -> Result<Registration, ServiceError> {
            Ok(Registration::Loaded)
        }
    }

    let temp =
        tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
    let root = temp.path().canonicalize().unwrap();
    let base = root.join("base");
    let store_dir = root.join("store");
    let state_dir = root.join("state");
    fs::create_dir_all(&base).unwrap();
    fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
    fs::create_dir_all(store_dir.join("releases/v0.1.0")).unwrap();
    fs::create_dir_all(store_dir.join("pins")).unwrap();
    fs::create_dir_all(store_dir.join("intents")).unwrap();
    fs::write(
        store_dir.join("releases/v0.1.0/agentic-org"),
        b"#!/bin/sh\n",
    )
    .unwrap();

    let v1_pin = ReleasePin {
        release_id: "v0.1.0".into(),
        manifest_sha256: "1".repeat(64),
        sequence: 1,
    };
    let v2_pin = ReleasePin {
        release_id: "v0.2.0".into(),
        manifest_sha256: "2".repeat(64),
        sequence: 2,
    };
    let write_pin = |ord: u64, pin: &ReleasePin| {
        let rec = serde_json::json!({
            "version": 1,
            "ordinal": ord,
            "previous": null,
            "highest_sequence": pin.sequence,
            "pin": pin,
        });
        fs::write(
            store_dir.join(format!("pins/{ord:020}.json")),
            serde_json::to_vec(&rec).unwrap(),
        )
        .unwrap();
    };
    let write_intent = |ord: u64| {
        fs::write(store_dir.join(format!("intents/{ord:020}.json")), b"{}").unwrap();
    };
    write_pin(1, &v1_pin);
    write_intent(1);

    let instance_id = "01965d2e-8b1c-7c3a-9f00-0123456789ab";
    let journal = Journal::open(&base, Some("default")).unwrap();
    let input = Input {
        version: 1,
        service: Service::Registered,
        root: base.join("instances/default"),
        credentials: None,
        listen: loopback(7420).unwrap(),
        federation: None,
    };
    let mut rec = journal
        .freeze(input, false, release_pin().unwrap())
        .unwrap();
    rec.instance_id = Some(instance_id.into());
    rec.service_registered = true;
    journal.write(&rec).unwrap();

    let state_store = UpdateStateStore::open(&state_dir).unwrap();
    let seed_pending = || {
        state_store
            .record_pending_with_services(
                Some(v1_pin.clone()),
                v2_pin.clone(),
                None,
                vec!["default".into()],
            )
            .unwrap();
    };

    let mgr = ResumeFailManager {
        unit: Unit {
            instance_id: instance_id.into(),
            program: store_dir.join("releases/v0.1.0/agentic-org"),
            args: vec![
                OsString::from("serve"),
                OsString::from("--name"),
                OsString::from("default"),
            ],
            env: Vec::new(),
        },
        running: Mutex::new(true),
        fail_start: Mutex::new(false),
    };

    // 1. Clean pre-commit abort: service resumed + store pin/intents unchanged -> clears pending.
    seed_pending();
    let mut refresh = ServiceRefresh::inspect_at(&mgr, &base, &store_dir);
    let _ = refresh
        .install(&mgr, || -> Result<(), InstallAttempt> {
            if *mgr.running.lock().unwrap() {
                Err(InstallAttempt::Busy("busy".into()))
            } else {
                Err(InstallAttempt::Failed("pre-commit error".into()))
            }
        })
        .unwrap_err();
    assert!(!refresh.has_stopped_managed());
    let cleared =
        clear_pending_if_install_unmodified(&state_store, &store_dir, Some(&v1_pin), &refresh)
            .unwrap();
    assert!(cleared);
    assert!(state_store.load().unwrap().pending_health_check.is_none());

    // 2. Service stopped and `resume` failed to restart it -> MUST keep pending intact!
    seed_pending();
    *mgr.running.lock().unwrap() = true;
    *mgr.fail_start.lock().unwrap() = true;
    let mut refresh = ServiceRefresh::inspect_at(&mgr, &base, &store_dir);
    let _ = refresh
        .install(&mgr, || -> Result<(), InstallAttempt> {
            if *mgr.running.lock().unwrap() {
                Err(InstallAttempt::Busy("busy".into()))
            } else {
                Err(InstallAttempt::Failed("boom".into()))
            }
        })
        .unwrap_err();
    assert!(refresh.has_stopped_managed());
    let cleared =
        clear_pending_if_install_unmodified(&state_store, &store_dir, Some(&v1_pin), &refresh)
            .unwrap();
    assert!(!cleared);
    assert!(state_store.load().unwrap().pending_health_check.is_some());

    // 3. No service stopped, but uncommitted intent written (`intents/2 > pins/1`) -> keeps pending!
    *mgr.running.lock().unwrap() = true;
    *mgr.fail_start.lock().unwrap() = false;
    write_intent(2);
    let refresh = ServiceRefresh::inspect_at(&mgr, &base, &store_dir);
    let cleared =
        clear_pending_if_install_unmodified(&state_store, &store_dir, Some(&v1_pin), &refresh)
            .unwrap();
    assert!(!cleared);
    assert!(state_store.load().unwrap().pending_health_check.is_some());

    // 4. Store active pin already advanced (`v2` != `v1`) -> keeps pending!
    write_pin(2, &v2_pin);
    let cleared =
        clear_pending_if_install_unmodified(&state_store, &store_dir, Some(&v1_pin), &refresh)
            .unwrap();
    assert!(!cleared);
    assert!(state_store.load().unwrap().pending_health_check.is_some());
}
