use std::fs;
use std::os::unix::fs::symlink;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::drain::{
    wait_for_idle_drain, ActiveTurnSnapshot, DiskActiveTurnDrainProbe, DrainConfig, DrainPhase,
    DrainProbe,
};

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
fn disk_drain_probe_detects_active_turns_ignores_idle_and_refuses_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let office_dir = root.join("office-state/offices/off1");
    let emp_active = office_dir.join("durable/local/offices/off1/sessions/emp-active");
    let emp_idle = office_dir.join("durable/local/offices/off1/sessions/emp-idle");
    fs::create_dir_all(&emp_active).unwrap();
    fs::create_dir_all(&emp_idle).unwrap();
    fs::write(
        office_dir.join("office.json"),
        br#"{"office_id":"off1","org_id":"org1"}"#,
    )
    .unwrap();
    fs::write(
        emp_active.join("active-turn.v1.json"),
        br#"{"employee_id":"emp-active","run_id":"r-1","active":true,"stall_locked":false,"recovery_required":false}"#,
    )
    .unwrap();
    fs::write(
        emp_idle.join("active-turn.v1.json"),
        br#"{"employee_id":"emp-idle","run_id":"r-0","active":false}"#,
    )
    .unwrap();

    let probe = DiskActiveTurnDrainProbe::new(vec![("prod".into(), root.clone())]);
    let found = probe.active_turns().unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].employee_id, "emp-active");
    assert_eq!(found[0].run_id, "r-1");

    // Once active becomes false, probe reports idle.
    fs::write(
        emp_active.join("active-turn.v1.json"),
        br#"{"employee_id":"emp-active","run_id":"r-1","active":false}"#,
    )
    .unwrap();
    assert!(probe.active_turns().unwrap().is_empty());

    // Symlink in session state fails closed.
    let outside = root.join("outside.json");
    fs::write(&outside, br#"{}"#).unwrap();
    fs::remove_file(emp_active.join("active-turn.v1.json")).unwrap();
    symlink(&outside, emp_active.join("active-turn.v1.json")).unwrap();
    assert!(probe.active_turns().is_err());
}
