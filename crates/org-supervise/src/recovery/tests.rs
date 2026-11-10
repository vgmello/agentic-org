use std::collections::{BTreeSet, VecDeque};
use std::io;
use std::time::{Duration, Instant};

use super::scan::{fixed_point, scan_one_pass, test_support, MarkedScan, RequestIssuer};
use super::*;

const MARK: &str = "recovery-fake-session";

fn process(pid: u32, parent: u32, mark: MarkEvidence) -> ProcessObservation {
    ProcessObservation {
        pid,
        identity: Some(ProcessIdentity {
            pid,
            started: format!("start-{pid}"),
            boot: "boot".into(),
        }),
        parent,
        group: pid,
        session: pid,
        uid: 501,
        credentials_safe: true,
        zombie: false,
        mark,
    }
}
fn marked(pid: u32) -> ProcessObservation {
    process(pid, 1, MarkEvidence::Present(MARK.into()))
}
fn record(leader: bool) -> RecoveryRecord {
    RecoveryRecord::new(
        MARK.into(),
        leader.then(|| ChildIdentity {
            pid: 100,
            pgid: 100,
            started: "start-100".into(),
            boot: "boot".into(),
            mark: MARK.into(),
        }),
        vec![900],
    )
    .unwrap()
}

struct Records {
    rows: Vec<(u32, RecoveryRecord)>,
    fail: bool,
}
impl RecoveryRecords for Records {
    type Key = u32;
    fn pending(&mut self) -> io::Result<Vec<(u32, RecoveryRecord)>> {
        if self.fail {
            Err(io::Error::other("record read failed"))
        } else {
            Ok(self.rows.clone())
        }
    }
}
fn records(leader: bool) -> Records {
    Records {
        rows: vec![(7, record(leader))],
        fail: false,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Action {
    Snapshot,
    Signal(u32, RecoverySignal, Instant),
    Wait(u32, Instant),
    Pause(Instant),
}

struct Processes {
    context: RecoveryContext,
    now: Instant,
    live: Vec<ProcessObservation>,
    snapshots: VecDeque<Vec<ProcessObservation>>,
    actions: Vec<Action>,
    term_exits: bool,
    kill_exits: bool,
    signal_failure: Option<u32>,
    wait_failure: bool,
    snapshot_failure: bool,
    reused_on_signal: Option<u32>,
}
impl Processes {
    fn new(live: Vec<ProcessObservation>) -> Self {
        Self {
            context: RecoveryContext {
                uid: 501,
                pid: 9999,
                boot: "boot".into(),
            },
            now: Instant::now(),
            live,
            snapshots: VecDeque::new(),
            actions: vec![],
            term_exits: false,
            kill_exits: true,
            signal_failure: None,
            wait_failure: false,
            snapshot_failure: false,
            reused_on_signal: None,
        }
    }
    fn deadline(&self) -> Instant {
        self.now + Duration::from_secs(30)
    }
    fn signals(&self) -> Vec<(u32, RecoverySignal)> {
        self.actions
            .iter()
            .filter_map(|a| match a {
                Action::Signal(pid, signal, _) => Some((*pid, *signal)),
                _ => None,
            })
            .collect()
    }
    fn remove(&mut self, pid: u32) {
        self.live.retain(|p| p.pid != pid);
        for p in &mut self.live {
            if p.parent == pid {
                p.parent = 1;
            }
        }
    }
}
impl sealed::Processes for Processes {}
impl RecoveryProcesses for Processes {
    fn context(&mut self) -> io::Result<RecoveryContext> {
        Ok(self.context.clone())
    }
    fn snapshot(&mut self, _: Instant) -> io::Result<Vec<ProcessObservation>> {
        self.actions.push(Action::Snapshot);
        if self.snapshot_failure {
            return Err(io::Error::other("enumeration failed"));
        }
        if let Some(next) = self.snapshots.pop_front() {
            self.live = next;
        }
        Ok(self.live.clone())
    }
    fn signal_checked(
        &mut self,
        target: &VerifiedTarget,
        signal: RecoverySignal,
        _: Instant,
    ) -> io::Result<SignalOutcome> {
        let pid = target.identity().pid;
        self.actions.push(Action::Signal(pid, signal, self.now));
        assert_eq!(target.mark(), MARK);
        if self.signal_failure == Some(pid) {
            return Err(io::Error::other("checked signal failed"));
        }
        if self.reused_on_signal == Some(pid) {
            self.remove(pid);
            let mut replacement = process(pid, 1, MarkEvidence::Absent);
            replacement.identity.as_mut().unwrap().started = "reused".into();
            self.live.push(replacement);
        }
        let current = self
            .live
            .iter()
            .find(|p| p.identity.as_ref() == Some(target.identity()));
        if current.is_none() {
            return Ok(SignalOutcome::Gone);
        }
        if (signal == RecoverySignal::Terminate && self.term_exits)
            || (signal == RecoverySignal::Kill && self.kill_exits)
        {
            self.remove(pid);
        }
        Ok(SignalOutcome::Signalled)
    }
    fn wait_exited(&mut self, identity: &ProcessIdentity, deadline: Instant) -> io::Result<bool> {
        self.actions.push(Action::Wait(identity.pid, deadline));
        if self.wait_failure {
            return Err(io::Error::other("wait failed"));
        }
        let live = self
            .live
            .iter()
            .any(|p| p.identity.as_ref() == Some(identity) && !p.zombie);
        if live {
            self.now = deadline;
        }
        Ok(!live)
    }
    fn now(&mut self) -> Instant {
        self.now
    }
    fn wait_until(&mut self, deadline: Instant) -> io::Result<()> {
        self.actions.push(Action::Pause(deadline));
        self.now = deadline;
        Ok(())
    }
}

fn run(leader: bool, ops: &mut Processes) -> RecoveryBatch<u32> {
    let deadline = ops.deadline();
    recover_startup(&mut records(leader), ops, deadline).unwrap()
}

#[test]
fn recovery_mark_only_crash_gap_kills_detached_tools_and_captured_descendants() {
    let root = marked(200);
    let child = process(201, 200, MarkEvidence::Absent);
    let grandchild = process(202, 201, MarkEvidence::Absent);
    let unrelated = process(300, 1, MarkEvidence::Absent);
    let mut ops = Processes::new(vec![root, child, grandchild, unrelated]);
    let batch = run(false, &mut ops);
    assert_eq!(
        ops.signals(),
        vec![
            (200, RecoverySignal::Kill),
            (201, RecoverySignal::Kill),
            (202, RecoverySignal::Kill)
        ]
    );
    assert_eq!(ops.live.iter().map(|p| p.pid).collect::<Vec<_>>(), [300]);
    assert!(batch.unresolved.is_empty());
    assert_eq!(batch.gone[0].0, 7);
    assert_eq!(batch.gone[0].1.mark(), MARK);
    assert_eq!(batch.gone[0].1.report().passes, 2);
    assert_eq!(batch.gone[0].1.report().found_live, 3);
}

#[test]
fn recovery_recorded_leader_gets_term_then_five_second_grace_then_checked_kill() {
    let mut ops = Processes::new(vec![marked(100), marked(200)]);
    let started = ops.now;
    let batch = run(true, &mut ops);
    assert!(batch.unresolved.is_empty());
    assert_eq!(
        ops.signals(),
        vec![
            (100, RecoverySignal::Terminate),
            (100, RecoverySignal::Kill),
            (200, RecoverySignal::Kill)
        ]
    );
    assert!(ops
        .actions
        .contains(&Action::Wait(100, started + Duration::from_secs(5))));
    assert!(ops.actions.contains(&Action::Signal(
        100,
        RecoverySignal::Kill,
        started + Duration::from_secs(5)
    )));
    let snapshots = ops
        .actions
        .iter()
        .filter(|a| **a == Action::Snapshot)
        .count();
    assert_eq!(snapshots, 3); // pre-TERM, marked survivor, final empty
}

#[test]
fn recovery_term_exit_still_kills_pre_term_unmarked_descendant_after_reparenting() {
    let mut ops = Processes::new(vec![marked(100), process(101, 100, MarkEvidence::Absent)]);
    ops.term_exits = true;
    assert!(run(true, &mut ops).unresolved.is_empty());
    assert_eq!(
        ops.signals(),
        vec![
            (100, RecoverySignal::Terminate),
            (101, RecoverySignal::Kill)
        ]
    );
}

#[test]
fn recovery_reused_leader_and_session_group_ids_never_authorize_signals() {
    for reboot in [false, true] {
        let mut reused = process(100, 1, MarkEvidence::Absent);
        reused.identity.as_mut().unwrap().started = "different-start".into();
        let mut group_reuse = process(300, 1, MarkEvidence::Absent);
        group_reuse.group = 100;
        group_reuse.session = 900;
        let mut ops = Processes::new(vec![reused, group_reuse, marked(200)]);
        if reboot {
            ops.context.boot = "new-boot".into();
            for p in &mut ops.live {
                p.identity.as_mut().unwrap().boot = "new-boot".into();
            }
        }
        assert!(run(true, &mut ops).unresolved.is_empty());
        assert_eq!(ops.signals(), vec![(200, RecoverySignal::Kill)]);
    }
}

#[test]
fn recovery_gone_leader_always_runs_full_scan_beyond_tool_session_hints() {
    let mut tool = marked(200);
    tool.session = 12345;
    let mut ops = Processes::new(vec![tool]);
    assert!(run(true, &mut ops).unresolved.is_empty());
    assert_eq!(ops.signals(), vec![(200, RecoverySignal::Kill)]);
}

#[test]
fn recovery_s0_unreadable_environments_are_skipped_and_summed_as_warnings() {
    let mut ops = Processes::new(vec![process(100, 1, MarkEvidence::Unreadable), marked(200)]);
    let batch = run(false, &mut ops);
    assert!(batch.unresolved.is_empty());
    assert_eq!(batch.gone[0].1.report().unreadable_environments, 2);
    assert_eq!(ops.signals(), [(200, RecoverySignal::Kill)]);
}

#[test]
fn recovery_zombies_are_skipped_before_unreadable_environment_counting() {
    let mut zombie = process(100, 1, MarkEvidence::Unreadable);
    zombie.zombie = true;
    let mut ops = Processes::new(vec![zombie]);
    let batch = run(false, &mut ops);
    assert_eq!(batch.gone[0].1.report().zombies, 1);
    assert_eq!(batch.gone[0].1.report().unreadable_environments, 0);
    assert!(ops.signals().is_empty());
}

#[test]
fn recovery_excludes_only_exact_unreaped_leader_identity_before_environment_read() {
    let mut leader = process(100, 1, MarkEvidence::Unreadable);
    let excluded = leader.identity.clone();
    let mut ops = Processes::new(vec![leader.clone(), marked(200)]);
    let deadline = ops.deadline();
    let request = test_support::issue(MARK, excluded.clone(), deadline);
    let pass = scan_one_pass(request, &mut ops).unwrap();
    assert_eq!(pass.report().excluded, 1);
    assert_eq!(pass.report().unreadable_environments, 0);
    assert_eq!(ops.signals(), [(200, RecoverySignal::Kill)]);
    leader.identity.as_mut().unwrap().started = "replacement".into();
    leader.mark = MarkEvidence::Present(MARK.into());
    let mut ops = Processes::new(vec![leader]);
    let request = test_support::issue(MARK, excluded, ops.deadline());
    assert_eq!(
        scan_one_pass(request, &mut ops).unwrap().report().excluded,
        0
    );
    assert_eq!(ops.signals(), [(100, RecoverySignal::Kill)]);
}

#[test]
fn recovery_startup_never_excludes_the_recorded_leader_from_post_kill_scan() {
    // Leader exits on TERM; a later marked process with the same PID and a new start
    // identity is still part of the mark scan (mark reuse by a concurrent spawn is forbidden).
    let mut new = marked(100);
    new.identity.as_mut().unwrap().started = "new".into();
    let mut ops = Processes::new(vec![]);
    ops.term_exits = true;
    ops.snapshots = [vec![marked(100)], vec![new], vec![]].into();
    let batch = run(true, &mut ops);
    assert!(batch.unresolved.is_empty());
    assert_eq!(
        ops.signals(),
        [
            (100, RecoverySignal::Terminate),
            (100, RecoverySignal::Kill)
        ]
    );
}

#[test]
fn recovery_never_signals_self_root_foreign_or_set_id_processes() {
    for kind in 0..4 {
        let mut unsafe_process = marked(200);
        match kind {
            0 => unsafe_process.pid = 9999,
            1 => unsafe_process.uid = 0,
            2 => unsafe_process.uid = 502,
            _ => unsafe_process.credentials_safe = false,
        }
        unsafe_process.identity.as_mut().unwrap().pid = unsafe_process.pid;
        let mut ops = Processes::new(vec![unsafe_process]);
        let batch = run(false, &mut ops);
        assert!(ops.signals().is_empty());
        if kind == 0 || kind == 3 {
            assert_eq!(batch.unresolved.len(), 1);
        } else {
            assert_eq!(batch.gone.len(), 1);
        }
    }
}

#[test]
fn recovery_unknown_marked_identity_is_unresolved_while_unmarked_unknown_is_reported() {
    for mark in [MarkEvidence::Present(MARK.into()), MarkEvidence::Absent] {
        let known = matches!(mark, MarkEvidence::Present(_));
        let mut unknown = process(200, 1, mark);
        unknown.identity = None;
        let mut ops = Processes::new(vec![unknown]);
        let batch = run(false, &mut ops);
        assert!(ops.signals().is_empty());
        if known {
            assert_eq!(batch.unresolved[0].1.report.unreadable_identities, 1);
        } else {
            assert_eq!(batch.gone[0].1.report().unreadable_identities, 1);
        }
    }
}

#[test]
fn recovery_pid_reuse_between_snapshot_and_signal_never_hits_replacement() {
    let mut ops = Processes::new(vec![marked(200)]);
    ops.reused_on_signal = Some(200);
    assert!(run(false, &mut ops).unresolved.is_empty());
    assert_eq!(ops.live.len(), 1);
    assert_eq!(ops.live[0].identity.as_ref().unwrap().started, "reused");
}

#[test]
fn recovery_signal_and_wait_failures_never_produce_gone() {
    for wait in [false, true] {
        let mut ops = Processes::new(vec![marked(200)]);
        if wait {
            ops.wait_failure = true;
        } else {
            ops.signal_failure = Some(200);
        }
        let batch = run(false, &mut ops);
        assert!(batch.gone.is_empty());
        assert_eq!(batch.unresolved.len(), 1);
    }
}

#[test]
fn recovery_live_target_after_kill_is_unresolved() {
    let mut ops = Processes::new(vec![marked(200)]);
    ops.kill_exits = false;
    let batch = run(false, &mut ops);
    assert!(batch.gone.is_empty());
    assert_eq!(batch.unresolved[0].1.error.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn recovery_scan_failure_is_not_interpreted_as_empty() {
    let mut ops = Processes::new(vec![marked(200)]);
    ops.snapshot_failure = true;
    let batch = run(false, &mut ops);
    assert!(batch.gone.is_empty());
    assert_eq!(batch.unresolved[0].1.report.passes, 0);
}

#[test]
fn recovery_record_read_failure_happens_before_any_process_operation() {
    let mut store = records(false);
    store.fail = true;
    let mut ops = Processes::new(vec![marked(200)]);
    let deadline = ops.deadline();
    assert!(recover_startup(&mut store, &mut ops, deadline).is_err());
    assert!(ops.actions.is_empty());
    assert_eq!(store.rows.len(), 1);
}

#[test]
fn recovery_leader_failure_does_not_skip_full_scan_and_remains_unresolved() {
    let mut ops = Processes::new(vec![marked(100), marked(200)]);
    ops.signal_failure = Some(100);
    let batch = run(true, &mut ops);
    assert!(batch.gone.is_empty());
    assert_eq!(batch.unresolved.len(), 1);
    assert_eq!(
        ops.actions
            .iter()
            .filter(|a| **a == Action::Snapshot)
            .count(),
        2
    );
    assert_eq!(ops.signals()[0], (100, RecoverySignal::Terminate));
}

#[test]
fn recovery_deadline_prevents_signals_and_completion() {
    let mut ops = Processes::new(vec![marked(100)]);
    let deadline = ops.now;
    let batch = recover_startup(&mut records(true), &mut ops, deadline).unwrap();
    assert!(batch.gone.is_empty());
    assert_eq!(batch.unresolved[0].1.error.kind(), io::ErrorKind::TimedOut);
    assert!(ops.actions.is_empty());
}

#[test]
fn recovery_short_deadline_does_not_extend_five_second_grace() {
    let mut ops = Processes::new(vec![marked(100)]);
    let deadline = ops.now + Duration::from_secs(2);
    let batch = recover_startup(&mut records(true), &mut ops, deadline).unwrap();
    assert!(batch.gone.is_empty());
    assert_eq!(ops.now, deadline);
    assert_eq!(ops.signals(), [(100, RecoverySignal::Terminate)]);
}

#[test]
fn recovery_three_nonempty_passes_are_unresolved_with_fresh_requests() {
    let mut ops = Processes::new(vec![]);
    ops.snapshots = [vec![marked(200)], vec![marked(201)], vec![marked(202)]].into();
    let batch = run(false, &mut ops);
    assert!(batch.gone.is_empty());
    assert_eq!(batch.unresolved[0].1.report.passes, 3);
    assert_eq!(
        batch.unresolved[0].1.error.kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(
        ops.actions
            .iter()
            .filter(|a| matches!(a, Action::Pause(_)))
            .count(),
        2
    );
    assert_eq!(ops.signals().len(), 3);
}

struct CorruptScan {
    mark: bool,
}
impl sealed::Scan for CorruptScan {}
impl ScanPort for CorruptScan {
    fn scan_pass(
        &mut self,
        request: ScanRequest,
        ops: &mut dyn RecoveryProcesses,
    ) -> Result<ScanPass, ScanFailure> {
        let mut pass = scan_one_pass(request, ops)?;
        test_support::corrupt(&mut pass, self.mark);
        Ok(pass)
    }
}

#[test]
fn recovery_rejects_wrong_mark_and_nonce_even_on_empty_pass() {
    for mark in [false, true] {
        let mut ops = Processes::new(vec![]);
        let deadline = ops.deadline();
        let batch = startup::recover_with_scan(
            &mut records(false),
            &mut ops,
            &mut CorruptScan { mark },
            deadline,
        )
        .unwrap();
        assert!(batch.gone.is_empty());
        assert_eq!(
            batch.unresolved[0].1.error.kind(),
            io::ErrorKind::InvalidData
        );
    }
}

#[test]
fn recovery_one_pass_never_hides_a_fixed_point_loop() {
    let mut ops = Processes::new(vec![marked(200)]);
    let request = test_support::issue(MARK, None, ops.deadline());
    let pass = scan_one_pass(request, &mut ops).unwrap();
    assert_eq!(pass.found_live(), 1);
    assert_eq!(pass.report().passes, 1);
    assert_eq!(
        ops.actions
            .iter()
            .filter(|a| **a == Action::Snapshot)
            .count(),
        1
    );
    assert!(!ops.actions.iter().any(|a| matches!(a, Action::Pause(_))));
}

#[test]
fn recovery_fixed_point_waits_one_second_and_requires_final_empty_pass() {
    let mut ops = Processes::new(vec![marked(200)]);
    let start = ops.now;
    let deadline = ops.deadline();
    let report = fixed_point(MARK, None, &[], &mut ops, &mut MarkedScan, deadline).unwrap();
    assert_eq!(report.passes, 2);
    assert_eq!(ops.now, start + Duration::from_secs(1));
    assert_eq!(ops.actions.last(), Some(&Action::Snapshot));
}

#[test]
fn recovery_two_issuers_never_repeat_a_nonce_in_ten_thousand_requests() {
    let mut first = RequestIssuer;
    let mut second = RequestIssuer;
    let mut seen = BTreeSet::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    for _ in 0..10_000 {
        for issuer in [&mut first, &mut second] {
            let request = issuer.issue(MARK, None, &[], deadline).unwrap();
            assert!(seen.insert(*request.nonce()));
        }
    }
    assert_eq!(seen.len(), 20_000);
}

#[test]
fn recovery_records_validate_identity_and_marks_without_schema_knowledge() {
    assert!(RecoveryRecord::new("".into(), None, vec![]).is_err());
    for mutation in 0..6 {
        let mut leader = record(true).leader().unwrap().clone();
        match mutation {
            0 => leader.pid = 0,
            1 => leader.pgid += 1,
            2 => leader.started.clear(),
            3 => leader.boot.clear(),
            4 => leader.mark = "another".into(),
            _ => {
                leader.pid = u32::MAX;
                leader.pgid = u32::MAX;
            }
        }
        assert!(RecoveryRecord::new(MARK.into(), Some(leader), vec![]).is_err());
    }
    assert!(RecoveryRecord::new(MARK.into(), None, vec![0]).is_err());
}

#[test]
fn recovery_duplicate_snapshot_pid_is_not_a_successful_empty_scan() {
    let mut ops = Processes::new(vec![marked(200), process(200, 1, MarkEvidence::Absent)]);
    let batch = run(false, &mut ops);
    assert!(batch.gone.is_empty());
    assert_eq!(
        batch.unresolved[0].1.error.kind(),
        io::ErrorKind::InvalidData
    );
    assert!(ops.signals().is_empty());
}

#[test]
fn recovery_root_context_is_refused_before_any_signals() {
    let mut ops = Processes::new(vec![marked(200)]);
    ops.context.uid = 0;
    let deadline = ops.deadline();
    assert!(recover_startup(&mut records(false), &mut ops, deadline).is_err());
    assert!(ops.actions.is_empty());
}

#[test]
fn recovery_multiple_records_retain_unresolved_key_without_releasing_any_row() {
    let other = RecoveryRecord::new("other".into(), None, vec![]).unwrap();
    let mut store = Records {
        rows: vec![(7, record(false)), (8, other)],
        fail: false,
    };
    let mut ops = Processes::new(vec![marked(200)]);
    ops.signal_failure = Some(200);
    let deadline = ops.deadline();
    let batch = recover_startup(&mut store, &mut ops, deadline).unwrap();
    assert_eq!(batch.unresolved[0].0, 7);
    assert_eq!(batch.gone[0].0, 8);
    assert_eq!(batch.gone[0].1.mark(), "other");
    assert_eq!(store.rows.len(), 2);
}

#[test]
fn recovery_unreadable_recorded_identity_is_never_assumed_gone() {
    let mut leader = process(100, 1, MarkEvidence::Absent);
    leader.identity = None;
    let mut ops = Processes::new(vec![leader]);
    let batch = run(true, &mut ops);
    assert!(batch.gone.is_empty());
    assert_eq!(
        batch.unresolved[0].1.error.kind(),
        io::ErrorKind::PermissionDenied
    );
    assert!(batch.unresolved[0].1.report.unreadable_identities > 0);
    assert!(ops.signals().is_empty());
}

struct ReplayScan {
    previous: Option<[u8; 32]>,
}
impl sealed::Scan for ReplayScan {}
impl ScanPort for ReplayScan {
    fn scan_pass(
        &mut self,
        request: ScanRequest,
        ops: &mut dyn RecoveryProcesses,
    ) -> Result<ScanPass, ScanFailure> {
        let nonce = *request.nonce();
        let mut pass = scan_one_pass(request, ops)?;
        if let Some(previous) = self.previous {
            assert_ne!(nonce, previous);
            test_support::replace_nonce(&mut pass, previous);
        }
        self.previous = Some(nonce);
        Ok(pass)
    }
}

#[test]
fn recovery_replayed_previous_pass_nonce_cannot_authorize_final_empty_result() {
    let mut ops = Processes::new(vec![marked(200)]);
    let deadline = ops.deadline();
    let batch = startup::recover_with_scan(
        &mut records(false),
        &mut ops,
        &mut ReplayScan { previous: None },
        deadline,
    )
    .unwrap();
    assert!(batch.gone.is_empty());
    assert_eq!(
        batch.unresolved[0].1.error.kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(batch.unresolved[0].1.report.passes, 1);
}

struct LateScan;
impl sealed::Scan for LateScan {}
impl ScanPort for LateScan {
    fn scan_pass(
        &mut self,
        request: ScanRequest,
        ops: &mut dyn RecoveryProcesses,
    ) -> Result<ScanPass, ScanFailure> {
        let deadline = request.deadline();
        let pass = scan_one_pass(request, ops)?;
        ops.wait_until(deadline).unwrap();
        Ok(pass)
    }
}

#[test]
fn recovery_late_empty_result_cannot_authorize_completion() {
    let mut ops = Processes::new(vec![]);
    let deadline = ops.deadline();
    let batch =
        startup::recover_with_scan(&mut records(false), &mut ops, &mut LateScan, deadline).unwrap();
    assert!(batch.gone.is_empty());
    assert_eq!(batch.unresolved[0].1.error.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn recovery_deadline_between_passes_prevents_final_empty_receipt() {
    let mut ops = Processes::new(vec![marked(200)]);
    let deadline = ops.now + Duration::from_millis(500);
    let batch = recover_startup(&mut records(false), &mut ops, deadline).unwrap();
    assert!(batch.gone.is_empty());
    assert_eq!(batch.unresolved[0].1.error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(batch.unresolved[0].1.report.passes, 1);
    assert_eq!(ops.now, deadline);
}

#[test]
fn recovery_hints_affect_priority_but_never_membership() {
    let mut hinted = marked(300);
    hinted.session = 900;
    let unrelated = process(400, 1, MarkEvidence::Absent);
    let mut ops = Processes::new(vec![marked(200), hinted, unrelated]);
    let batch = run(false, &mut ops);
    assert!(batch.unresolved.is_empty());
    assert_eq!(
        ops.signals(),
        [(300, RecoverySignal::Kill), (200, RecoverySignal::Kill)]
    );
    assert_eq!(ops.live[0].pid, 400);
}

#[test]
fn recovery_unreadable_descendant_environment_is_s0_skipped() {
    let child = process(201, 200, MarkEvidence::Unreadable);
    let mut ops = Processes::new(vec![marked(200), child]);
    let batch = run(false, &mut ops);
    assert!(batch.unresolved.is_empty());
    assert_eq!(ops.signals(), [(200, RecoverySignal::Kill)]);
    assert_eq!(batch.gone[0].1.report().unreadable_environments, 2);
    assert_eq!(ops.live[0].pid, 201);
}
