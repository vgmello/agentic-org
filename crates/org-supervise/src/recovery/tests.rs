use std::collections::{BTreeSet, VecDeque};
use std::io;
use std::time::{Duration, Instant};
use std::{cell::RefCell, rc::Rc};

use super::scan::{
    fixed_point, scan_one_pass, test_support, MarkedScan, RequestIssuer, ScanPass, ScanRequest,
};
use super::*;

const MARK: &str = "recovery-fake-session";

mod failure;

#[test]
fn native_mark_identical_duplicates_count_once_on_each_platform() {
    for bytes in [
        b"ORG_SESSION_MARK=a\0ORG_SESSION_MARK=a\0".as_slice(),
        b"ORG_SESSION_MARK=a\0OTHER=value\0ORG_SESSION_MARK=a\0ORG_SESSION_MARK=a\0",
    ] {
        assert_eq!(native_mark(bytes), MarkEvidence::Present("a".into()));
    }
    assert_eq!(
        native_mark(b"ORG_SESSION_MARK=a\0ORG_SESSION_MARK=a\0ORG_SESSION_MARK=b\0"),
        MarkEvidence::Unreadable
    );
}

#[test]
fn native_guards_recheck_the_exact_mark_before_direct_signals() {
    for evidence in [
        TargetEvidence::Marked,
        TargetEvidence::RecordedLeader,
        TargetEvidence::Descendant(vec![]),
        TargetEvidence::CheckpointDescendant(vec![]),
        TargetEvidence::Checkpoint(vec![]),
    ] {
        let direct = matches!(
            evidence,
            TargetEvidence::Marked | TargetEvidence::RecordedLeader
        );
        let target = VerifiedTarget {
            identity: ProcessIdentity {
                pid: 42,
                started: "start".into(),
                boot: "boot".into(),
            },
            mark: MARK.into(),
            evidence,
        };
        assert!(signal_mark_matches(
            &target,
            &MarkEvidence::Present(MARK.into())
        ));
        assert_eq!(
            signal_mark_matches(&target, &MarkEvidence::Unreadable),
            matches!(target.evidence(), TargetEvidence::Checkpoint(_))
        );
        for changed in [MarkEvidence::Absent, MarkEvidence::Present("other".into())] {
            assert_eq!(signal_mark_matches(&target, &changed), !direct);
        }
    }
}

#[test]
fn native_guards_quarantine_conflicting_duplicate_marks() {
    assert_eq!(
        native_mark(b"ORG_SESSION_MARK=a\0ORG_SESSION_MARK=b\0"),
        MarkEvidence::Unreadable
    );
    assert_eq!(
        native_mark(b"ORG_SESSION_MARK=a\0"),
        MarkEvidence::Present("a".into())
    );
}

fn process(pid: u32, parent: u32, mark: MarkEvidence) -> ProcessObservation {
    ProcessObservation {
        pid,
        identity: Some(ProcessIdentity {
            pid,
            started: format!("start-{pid}"),
            boot: "boot".into(),
        }),
        parent: (parent > 1).then(|| ProcessIdentity {
            pid: parent,
            started: format!("start-{parent}"),
            boot: "boot".into(),
        }),
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

#[derive(Default)]
struct Records {
    rows: Vec<(u32, RecoveryRecord)>,
    fail: bool,
    load_failure: bool,
    loaded: Vec<u32>,
    checkpoint_failure: bool,
    checkpoint_failure_at: Option<usize>,
    checkpoints: usize,
    committed: Rc<RefCell<Vec<CheckpointTarget>>>,
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
    fn load(&mut self, key: &u32, _: Instant) -> io::Result<Option<RecoveryRecord>> {
        self.loaded.push(*key);
        if self.load_failure {
            return Err(io::Error::other("single-record read failed"));
        }
        Ok(self
            .rows
            .iter()
            .find(|(id, _)| id == key)
            .map(|(_, row)| row.clone()))
    }
}
impl RecoveryCheckpoint for Records {
    fn checkpoint(
        &mut self,
        key: &u32,
        mark: &str,
        inventory: &[CheckpointTarget],
        _: Instant,
    ) -> io::Result<()> {
        self.checkpoints += 1;
        if self.checkpoint_failure || self.checkpoint_failure_at == Some(self.checkpoints) {
            return Err(io::Error::other("checkpoint failed"));
        }
        validate_inventory(inventory)?;
        let row = self
            .rows
            .iter_mut()
            .find(|(id, row)| id == key && row.mark() == mark)
            .ok_or_else(|| io::Error::other("stale recovery key/mark"))?;
        if !row.1.inventory.iter().all(|old| inventory.contains(old)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "stale recovery inventory",
            ));
        }
        row.1.inventory = inventory.to_vec();
        *self.committed.borrow_mut() = inventory.to_vec();
        Ok(())
    }
}
fn journal(store: &mut Records) -> checkpoint::Journal<'_, Records> {
    checkpoint::Journal::new(store, &7, &record(false))
}
fn records(leader: bool) -> Records {
    Records {
        rows: vec![(7, record(leader))],
        ..Records::default()
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
    signal_evidence: Vec<(u32, TargetEvidence)>,
    term_exits: bool,
    term_removes: Option<u32>,
    spawn_during_grace: Option<ProcessObservation>,
    kill_exits: bool,
    signal_failure: Option<u32>,
    wait_failure: bool,
    snapshot_failure: bool,
    snapshot_deadline: bool,
    hidden_from_snapshot: Option<u32>,
    reused_on_signal: Option<u32>,
    job_reuse: bool,
    job_error: bool,
    job_late: bool,
    expected_at_term: Vec<ProcessIdentity>,
    checkpoint_checks: Option<Rc<RefCell<Vec<CheckpointTarget>>>>,
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
            signal_evidence: vec![],
            term_exits: false,
            term_removes: None,
            spawn_during_grace: None,
            kill_exits: true,
            signal_failure: None,
            wait_failure: false,
            snapshot_failure: false,
            snapshot_deadline: false,
            hidden_from_snapshot: None,
            reused_on_signal: None,
            job_reuse: false,
            job_error: false,
            job_late: false,
            expected_at_term: vec![],
            checkpoint_checks: None,
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
            if p.parent.as_ref().is_some_and(|parent| parent.pid == pid) {
                p.parent = None;
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
        if self.snapshot_deadline {
            return Err(super::failure::tagged(
                io::ErrorKind::TimedOut,
                ScanFailureKind::DeadlineElapsed,
                "native recovery deadline elapsed",
            ));
        }
        if self.snapshot_failure {
            return Err(io::Error::other("enumeration failed"));
        }
        if let Some(next) = self.snapshots.pop_front() {
            self.live = next;
        }
        Ok(self
            .live
            .iter()
            .filter(|p| self.hidden_from_snapshot != Some(p.pid))
            .cloned()
            .collect())
    }
    fn verify_recorded_job(
        &mut self,
        target: &ProcessIdentity,
        leader: &ChildIdentity,
        deadline: Instant,
    ) -> io::Result<bool> {
        if self.job_error {
            if self.job_late {
                self.now = deadline;
            }
            return Err(io::Error::other("job observation failed"));
        }
        if self.job_reuse {
            self.job_reuse = false;
            if let Some(current) = self.live.iter_mut().find(|p| p.pid == leader.pid) {
                current.identity.as_mut().unwrap().started = "start-101".into();
                current.mark = MarkEvidence::Absent;
            }
            for current in &mut self.live {
                current.parent = None;
            }
        }
        let now = self.now;
        let result = job::verify(
            target,
            leader,
            &self.context,
            deadline,
            || now,
            |id| {
                let Some(p) = self.live.iter().find(|p| p.pid == id.pid) else {
                    return Ok(None);
                };
                let Some(identity) = &p.identity else {
                    return Ok(None);
                };
                let order = identity
                    .started
                    .strip_prefix("start-")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                Ok(Some(job::JobObservation {
                    identity: identity.clone(),
                    order: (order, 0),
                    group: p.group,
                    session: p.session,
                    safe: safe_identity(p, &self.context).is_some() && !p.zombie,
                }))
            },
        );
        if self.job_late {
            self.now = deadline;
        }
        result
    }
    fn signal_checked(
        &mut self,
        target: &VerifiedTarget,
        signal: RecoverySignal,
        _: Instant,
    ) -> io::Result<SignalOutcome> {
        let pid = target.identity().pid;
        if let Some(checkpoint) = &self.checkpoint_checks {
            if signal == RecoverySignal::Terminate {
                for expected in &self.expected_at_term {
                    assert!(
                        checkpoint
                            .borrow()
                            .iter()
                            .any(|saved| &saved.identity == expected),
                        "job member must be saved before leader TERM"
                    );
                }
            }
            assert!(
                checkpoint
                    .borrow()
                    .iter()
                    .any(|saved| &saved.identity == target.identity()),
                "signal issued before its durable checkpoint"
            );
        }
        self.actions.push(Action::Signal(pid, signal, self.now));
        self.signal_evidence.push((pid, target.evidence().clone()));
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
        if signal == RecoverySignal::Terminate {
            if let Some(tool) = self.term_removes {
                self.remove(tool);
            }
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
            if self
                .spawn_during_grace
                .as_ref()
                .is_some_and(|child| child.parent.as_ref() == Some(identity))
            {
                self.live.push(self.spawn_during_grace.take().unwrap());
            }
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
    run_store(&mut records(leader), ops)
}

fn run_store(store: &mut Records, ops: &mut Processes) -> RecoveryBatch<u32> {
    let deadline = ops.deadline();
    ops.checkpoint_checks = Some(store.committed.clone());
    recover_startup(store, ops, deadline).unwrap()
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
    assert_eq!(snapshots, 4); // pre-TERM, refreshed pre-KILL, marked survivor, final empty
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
    let mut store = records(false);
    let mut inventory = journal(&mut store);
    let request = test_support::issue(MARK, excluded.clone(), deadline, &mut inventory);
    let pass = scan_one_pass(request, &mut ops).unwrap();
    assert_eq!(pass.report().excluded, 1);
    assert_eq!(pass.report().unreadable_environments, 0);
    assert_eq!(ops.signals(), [(200, RecoverySignal::Kill)]);
    leader.identity.as_mut().unwrap().started = "replacement".into();
    leader.mark = MarkEvidence::Present(MARK.into());
    let mut ops = Processes::new(vec![leader]);
    let request = test_support::issue(MARK, excluded, ops.deadline(), &mut inventory);
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
    let mut store = records(false);
    let mut inventory = journal(&mut store);
    let request = test_support::issue(MARK, None, ops.deadline(), &mut inventory);
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
    let mut store = records(false);
    let mut inventory = journal(&mut store);
    let report = fixed_point(
        MARK,
        None,
        &[],
        &mut ops,
        &mut MarkedScan,
        deadline,
        &mut inventory,
    )
    .unwrap();
    assert_eq!(report.passes, 2);
    assert_eq!(ops.now, start + Duration::from_secs(1));
    assert_eq!(
        ops.actions
            .iter()
            .filter(|a| **a == Action::Snapshot)
            .count(),
        2
    );
}

#[test]
fn recovery_two_issuers_never_repeat_a_nonce_in_ten_thousand_requests() {
    let mut first = RequestIssuer;
    let mut second = RequestIssuer;
    let mut seen = BTreeSet::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut store = records(false);
    let mut inventory = journal(&mut store);
    for _ in 0..10_000 {
        for issuer in [&mut first, &mut second] {
            let request = issuer
                .issue(MARK, None, &[], deadline, &mut inventory)
                .unwrap();
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
        ..Records::default()
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

#[test]
fn recovery_failed_descendant_cleanup_cannot_allow_false_gone_on_retry() {
    let child = process(201, 200, MarkEvidence::Absent);
    let mut ops = Processes::new(vec![marked(200), child]);
    ops.signal_failure = Some(201);
    let mut store = records(false);
    let deadline = ops.deadline();
    let first = recover_startup(&mut store, &mut ops, deadline).unwrap();
    assert!(first.gone.is_empty());
    assert_eq!(first.unresolved.len(), 1);
    assert_eq!(ops.live[0].pid, 201);
    assert!(ops.live[0].parent.is_none());
    ops.signal_failure = None;
    let deadline = ops.deadline();
    let second = recover_startup(&mut store, &mut ops, deadline).unwrap();
    assert!(
        second.gone.is_empty() || ops.live.iter().all(|p| p.pid != 201),
        "retry must not issue Gone while captured descendant remains live"
    );
}

#[test]
fn recovery_crash_reload_preserves_targets_before_first_signal_and_after_term() {
    for leader in [false, true] {
        let pid = if leader { 100 } else { 200 };
        let mut store = records(leader);
        let mut ops = Processes::new(vec![
            marked(pid),
            process(pid + 1, pid, MarkEvidence::Absent),
        ]);
        ops.term_exits = leader;
        // Mark-only: crash after write-ahead commit before the first signal took effect.
        // Recorded leader: TERM exits/reparents; the subsequent child kill fails.
        ops.signal_failure = Some(if leader { pid + 1 } else { pid });
        assert!(run_store(&mut store, &mut ops).gone.is_empty());
        assert_eq!(store.rows[0].1.inventory().len(), 2);
        assert_eq!(
            ops.signals()[0].1,
            if leader {
                RecoverySignal::Terminate
            } else {
                RecoverySignal::Kill
            }
        );
        let mut loaded = Records {
            rows: store.rows.clone(),
            ..Records::default()
        };
        // No previous in-memory adapter state. The parent is gone; the saved child
        // now has an unmarked descendant that must also be captured before any kill.
        let mut next = Processes::new(vec![
            process(pid + 1, 1, MarkEvidence::Absent),
            process(pid + 2, pid + 1, MarkEvidence::Absent),
            process(400, 1, MarkEvidence::Absent),
        ]);
        assert!(run_store(&mut loaded, &mut next).unresolved.is_empty());
        assert_eq!(
            next.signals(),
            [
                (pid + 1, RecoverySignal::Kill),
                (pid + 2, RecoverySignal::Kill)
            ]
        );
        assert_eq!(next.live[0].pid, 400);
        assert_eq!(loaded.rows.len(), 1);
        assert_eq!(loaded.rows[0].1.inventory().len(), 3); // append, never discard resolved root
        let grandchild = loaded.rows[0]
            .1
            .inventory()
            .iter()
            .find(|t| t.identity.pid == pid + 2)
            .unwrap();
        assert_eq!(
            grandchild
                .ancestry
                .iter()
                .map(|p| p.pid)
                .collect::<Vec<_>>(),
            [pid + 1],
            "new snapshot ancestry must start at the live saved anchor, not its historical parent"
        );
        assert!(next
            .signal_evidence
            .iter()
            .any(|(target, evidence)| *target == pid + 2
                && matches!(evidence, TargetEvidence::CheckpointDescendant(chain)
                if chain.iter().map(|p| p.pid).eq([pid+1]))));
    }
}

#[test]
fn recovery_wait_failure_after_parent_exit_cannot_erase_child_inventory() {
    let mut store = records(false);
    let mut ops = Processes::new(vec![marked(200), process(201, 200, MarkEvidence::Absent)]);
    ops.wait_failure = true;
    assert!(run_store(&mut store, &mut ops).gone.is_empty());
    assert_eq!(ops.live[0].pid, 201);
    ops.wait_failure = false;
    assert!(run_store(&mut store, &mut ops).unresolved.is_empty());
    assert!(ops.live.is_empty());
    assert_eq!(store.rows[0].1.inventory().len(), 2);
}

#[test]
fn recovery_checkpoint_failure_prevents_every_signal_and_gone() {
    for leader in [false, true] {
        let pid = if leader { 100 } else { 200 };
        let mut store = records(leader);
        store.checkpoint_failure = true;
        let mut ops = Processes::new(vec![
            marked(pid),
            process(pid + 1, pid, MarkEvidence::Absent),
        ]);
        let batch = run_store(&mut store, &mut ops);
        assert!(batch.gone.is_empty());
        assert_eq!(batch.unresolved.len(), 1);
        assert!(ops.signals().is_empty());
        assert_eq!(ops.live.len(), 2);
        assert!(store.rows[0].1.inventory().is_empty());
        assert!(store.checkpoints > 0);
    }
}

#[test]
fn recovery_257_captured_identities_refuse_before_signal_or_partial_write() {
    for leader in [false, true] {
        let mut store = records(leader);
        let live = if leader {
            std::iter::once(marked(100))
                .chain((101..357).map(|pid| process(pid, 100, MarkEvidence::Absent)))
                .collect()
        } else {
            (200..457).map(marked).collect()
        };
        let mut ops = Processes::new(live);
        assert!(run_store(&mut store, &mut ops).gone.is_empty());
        assert!(ops.signals().is_empty());
        assert_eq!(ops.live.len(), 257);
        assert_eq!(store.checkpoints, 0);
        assert!(store.rows[0].1.inventory().is_empty());
    }
}

#[test]
fn recovery_inventory_cap_includes_resolved_targets_and_ancestry_only_entries() {
    for ancestry_only in [false, true] {
        let inventory = if ancestry_only {
            vec![CheckpointTarget {
                identity: marked(1255).identity.unwrap(),
                ancestry: (1000..1255)
                    .map(|pid| marked(pid).identity.unwrap())
                    .collect(),
            }]
        } else {
            (1000..1256)
                .map(|pid| CheckpointTarget {
                    identity: marked(pid).identity.unwrap(),
                    ancestry: vec![],
                })
                .collect()
        };
        let count = inventory.len();
        let mut store = records(false);
        store.rows[0].1 = record(false).with_inventory(inventory).unwrap();
        let mut ops = Processes::new(vec![]);
        assert!(run_store(&mut store, &mut ops).unresolved.is_empty()); // exactly 256 identities
        let writes = store.checkpoints;
        ops.live.push(marked(200));
        assert!(run_store(&mut store, &mut ops).gone.is_empty());
        assert!(ops.signals().is_empty());
        assert_eq!(store.checkpoints, writes);
        assert_eq!(store.rows[0].1.inventory().len(), count);
    }
}

fn saved_parent_and_child() -> Vec<CheckpointTarget> {
    let parent = marked(200).identity.unwrap();
    vec![
        CheckpointTarget {
            identity: parent.clone(),
            ancestry: vec![],
        },
        CheckpointTarget {
            identity: marked(201).identity.unwrap(),
            ancestry: vec![parent],
        },
    ]
}

#[test]
fn recovery_saved_target_requires_exact_pid_start_and_boot_match() {
    for change in 0..4 {
        let mut store = records(false);
        store.rows[0].1 = record(false)
            .with_inventory(saved_parent_and_child())
            .unwrap();
        let mut child = process(201, 1, MarkEvidence::Absent);
        match change {
            1 => child.identity.as_mut().unwrap().started = "reused".into(),
            2 => child.identity.as_mut().unwrap().boot = "reboot".into(),
            3 => {
                child.pid = 202;
                child.identity.as_mut().unwrap().pid = 202;
            }
            _ => {}
        }
        let mut ops = Processes::new(vec![child]);
        if change == 2 {
            ops.context.boot = "reboot".into();
        }
        assert!(run_store(&mut store, &mut ops).unresolved.is_empty());
        if change == 0 {
            assert_eq!(ops.signals(), [(201, RecoverySignal::Kill)]);
        } else {
            assert!(ops.signals().is_empty());
            assert_eq!(ops.live.len(), 1);
        }
        assert_eq!(store.rows[0].1.inventory().len(), 2);
    }
}

#[test]
fn recovery_checkpointed_unreadable_leader_gets_term_after_checkpoint() {
    let leader = process(100, 1, MarkEvidence::Unreadable);
    let inventory = vec![CheckpointTarget {
        identity: leader.identity.clone().unwrap(),
        ancestry: vec![],
    }];
    let mut store = records(true);
    store.rows[0].1 = record(true).with_inventory(inventory.clone()).unwrap();
    let mut ops = Processes::new(vec![leader]);
    ops.term_exits = true;
    ops.checkpoint_checks = Some(Rc::clone(&store.committed));
    let batch = run_store(&mut store, &mut ops);
    assert!(batch.unresolved.is_empty());
    assert_eq!(batch.gone.len(), 1);
    assert_eq!(ops.signals(), [(100, RecoverySignal::Terminate)]);
    assert!(batch.gone[0].1.report().unreadable_environments > 0);
    assert!(store.checkpoints > 0);
    assert_eq!(store.rows[0].1.inventory(), inventory);
}

#[test]
fn recovery_checkpointed_unreadable_target_is_cleaned_after_parent_exit() {
    let mut store = records(false);
    store.rows[0].1 = record(false)
        .with_inventory(saved_parent_and_child())
        .unwrap();
    let mut ops = Processes::new(vec![process(201, 1, MarkEvidence::Unreadable)]);
    ops.checkpoint_checks = Some(Rc::clone(&store.committed));
    let batch = run_store(&mut store, &mut ops);
    assert!(batch.unresolved.is_empty());
    assert_eq!(batch.gone.len(), 1);
    assert_eq!(ops.signals(), [(201, RecoverySignal::Kill)]);
    assert!(ops.live.is_empty());
    assert_eq!(batch.gone[0].1.report().unreadable_environments, 1);
    assert_eq!(store.rows[0].1.inventory().len(), 2);
}

#[test]
fn recovery_job_capture_enforces_its_own_cap_and_late_optional_proof() {
    use super::capture::{capture_targets, CaptureScope, ValidatedSnapshot};
    let record = record(true);
    for late in [false, true] {
        let mut ops = Processes::new(vec![process(100, 1, MarkEvidence::Absent)]);
        if late {
            ops.live.push(unreadable_job_member(200, 100, 100));
            ops.job_error = true;
            ops.job_late = true;
        } else {
            ops.live
                .extend((200..457).map(|pid| unreadable_job_member(pid, 100, 100)));
        }
        let deadline = ops.deadline();
        let snapshot = ValidatedSnapshot::capture(&mut ops, deadline).unwrap();
        let context = ops.context.clone();
        let mut report = RecoveryReport::default();
        let error = capture_targets(
            CaptureScope {
                mark: MARK,
                excluded: None,
                saved: &[],
                leader: record.leader(),
            },
            &snapshot,
            &context,
            &mut ops,
            deadline,
            &mut report,
        )
        .expect_err("capture itself must refuse before any journal or signal");
        assert_eq!(
            error.kind(),
            if late {
                io::ErrorKind::TimedOut
            } else {
                io::ErrorKind::InvalidInput
            }
        );
        if !late {
            assert!(error.to_string().contains("256 identities"));
        }
        assert!(ops.signals().is_empty());
    }
}

#[test]
fn recovery_one_pass_uses_fresh_recorded_job_proof_and_checkpoints_all_members() {
    let mut store = records(true);
    let record = store.rows[0].1.clone();
    let committed = store.committed.clone();
    let mut inventory = checkpoint::Journal::new(&mut store, &7, &record);
    let mut ops = Processes::new(vec![marked(100), unreadable_job_member(200, 100, 100)]);
    ops.checkpoint_checks = Some(committed);
    let request = test_support::issue(MARK, None, ops.deadline(), &mut inventory);
    let result = scan_one_pass(request, &mut ops).unwrap();
    assert_eq!(result.found_live(), 2);
    assert_eq!(
        ops.signals(),
        [(100, RecoverySignal::Kill), (200, RecoverySignal::Kill)]
    );
    assert!(ops.live.is_empty());
}

#[test]
fn recovery_job_s0_checkpoint_survives_a_failed_kill_and_reload_after_escape() {
    let mut store = records(true);
    let child = unreadable_job_member(200, 100, 100);
    let mut ops = Processes::new(vec![marked(100), child.clone()]);
    ops.term_exits = true;
    ops.expected_at_term.push(child.identity.clone().unwrap());
    ops.signal_failure = Some(200);
    let failed = run_store(&mut store, &mut ops);
    assert_eq!(failed.unresolved.len(), 1);
    assert_eq!(store.rows[0].1.inventory().len(), 2);
    assert!(ops.live.iter().any(|p| p.pid == 200));
    let mut escaped = child;
    escaped.group = 200;
    escaped.session = 200;
    let mut retry = Processes::new(vec![escaped]);
    let done = run_store(&mut store, &mut retry);
    assert_eq!(done.gone.len(), 1);
    assert!(done.unresolved.is_empty());
    assert_eq!(retry.signals(), [(200, RecoverySignal::Kill)]);
    assert!(retry.live.is_empty());
    assert_eq!(store.rows[0].1.inventory().len(), 2);
}

fn unreadable_job_member(pid: u32, group: u32, session: u32) -> ProcessObservation {
    let mut p = process(pid, 1, MarkEvidence::Unreadable);
    p.group = group;
    p.session = session;
    p
}

#[test]
fn recovery_live_recorded_job_is_saved_before_term_and_survives_anchor_exit() {
    for (group, session) in [(100, 200), (200, 100), (100, 100)] {
        let mut store = records(true);
        let mut ops = Processes::new(vec![
            marked(100),
            unreadable_job_member(200, group, session),
        ]);
        ops.term_exits = true;
        ops.expected_at_term
            .push(ops.live[1].identity.clone().unwrap());
        let batch = run_store(&mut store, &mut ops);
        assert!(batch.unresolved.is_empty());
        assert_eq!(batch.gone.len(), 1);
        assert_eq!(
            ops.signals(),
            [
                (100, RecoverySignal::Terminate),
                (200, RecoverySignal::Kill)
            ]
        );
        assert!(ops.live.is_empty());
        assert_eq!(store.rows[0].1.inventory().len(), 2);
        assert!(ops.signal_evidence.iter().any(
            |(pid, evidence)| *pid == 200 && matches!(evidence, TargetEvidence::Checkpoint(_))
        ));
        assert!(batch.gone[0].1.report().unreadable_environments > 0);
    }
}

#[test]
fn recovery_s0_job_proof_failure_never_saves_or_signals_the_child() {
    for variant in 0..16 {
        let mut leader = marked(100);
        let mut child = unreadable_job_member(200, 100, 100);
        let mut store = records(true);
        let mut ops = Processes::new(vec![]);
        match variant {
            0 => leader.identity.as_mut().unwrap().started = "start-101".into(),
            1 => child.identity.as_mut().unwrap().started = "start-99".into(),
            2 => child.identity.as_mut().unwrap().boot = "other-boot".into(),
            3 => child.uid = 0,
            4 => child.uid = 502,
            5 => child.credentials_safe = false,
            6 => child.zombie = true,
            7 => ops.context.pid = 200,
            8 => store.rows[0].1.leader.as_mut().unwrap().pgid = 101,
            9 => ops.job_error = true,
            10 => ops.job_reuse = true,
            11 => {
                child.group = 200;
                child.session = 200;
            }
            12 => leader.group = 101,
            13 => child.mark = MarkEvidence::Absent,
            14 => child.mark = MarkEvidence::Present("other-mark".into()),
            15 => child.identity = None,
            _ => unreachable!(),
        }
        ops.live = vec![leader, child];
        ops.term_exits = true;
        let batch = run_store(&mut store, &mut ops);
        assert!(
            !ops.signals().iter().any(|(pid, _)| *pid == 200),
            "variant {variant}"
        );
        assert!(
            !store.rows[0]
                .1
                .inventory()
                .iter()
                .any(|t| t.identity.pid == 200),
            "variant {variant}"
        );
        assert!(ops.live.iter().any(|p| p.pid == 200), "variant {variant}");
        if variant == 10 {
            assert!(ops
                .live
                .iter()
                .any(|p| p.pid == 100 && p.identity.as_ref().unwrap().started == "start-101"));
        }
        assert!(batch.gone.len() + batch.unresolved.len() == 1);
    }
    // The same numeric job identifiers never authorize when its recorded leader is absent.
    let mut ops = Processes::new(vec![unreadable_job_member(200, 100, 100)]);
    let batch = run(true, &mut ops);
    assert_eq!(batch.gone.len(), 1);
    assert!(batch.gone[0].1.report().unreadable_environments > 0);
    assert!(ops.signals().is_empty());
}

#[test]
fn recovery_saved_s0_retains_all_identity_and_credential_refusals() {
    for variant in 0..9 {
        let mut store = records(false);
        store.rows[0].1 = record(false)
            .with_inventory(saved_parent_and_child())
            .unwrap();
        let mut child = process(201, 1, MarkEvidence::Unreadable);
        let mut ops = Processes::new(vec![]);
        match variant {
            0 => child.identity.as_mut().unwrap().started = "reused".into(),
            1 => child.identity.as_mut().unwrap().boot = "other-boot".into(),
            2 => child.uid = 0,
            3 => child.uid = 502,
            4 => child.credentials_safe = false,
            5 => child.zombie = true,
            6 => ops.context.pid = 201,
            7 => child.identity = None,
            8 => ops.context.uid = 0,
            _ => unreachable!(),
        }
        ops.live = vec![child];
        if variant == 8 {
            let deadline = ops.deadline();
            assert_eq!(
                recover_startup(&mut store, &mut ops, deadline)
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidInput
            );
        } else {
            let _batch = run_store(&mut store, &mut ops);
        }
        assert!(ops.signals().is_empty(), "variant {variant}");
        assert_eq!(ops.live.len(), 1);
        assert_eq!(store.rows[0].1.inventory().len(), 2);
    }
}

#[test]
fn recovery_s0_checkpoint_failure_deadline_or_cap_prevents_every_signal() {
    for variant in 0..3 {
        let mut store = records(true);
        let mut ops = Processes::new(vec![marked(100), unreadable_job_member(200, 100, 100)]);
        match variant {
            0 => store.checkpoint_failure = true,
            1 => ops.job_late = true,
            2 => ops
                .live
                .extend((201..456).map(|pid| unreadable_job_member(pid, 100, 100))),
            _ => unreachable!(),
        }
        let batch = run_store(&mut store, &mut ops);
        assert_eq!(batch.unresolved.len(), 1, "variant {variant}");
        assert!(batch.gone.is_empty());
        assert!(ops.signals().is_empty());
        assert!(store.rows[0].1.inventory().is_empty());
        assert!(store.committed.borrow().is_empty());
    }
}

#[test]
fn recovery_pass_rejects_checkpoint_for_different_mark_even_when_empty() {
    let mut store = records(false);
    let mut inventory = journal(&mut store);
    let mut ops = Processes::new(vec![]);
    let request = test_support::issue("another-mark", None, ops.deadline(), &mut inventory);
    assert_eq!(
        scan_one_pass(request, &mut ops).unwrap_err().error.kind(),
        io::ErrorKind::InvalidData
    );
    assert!(ops.actions.is_empty());
    assert_eq!(store.checkpoints, 0);
}

#[test]
fn recovery_term_checkpoint_includes_detached_marked_tool_tree() {
    let mut store = records(true);
    let mut ops = Processes::new(vec![
        marked(100),
        marked(200),
        process(201, 200, MarkEvidence::Absent),
    ]);
    ops.term_exits = true;
    ops.term_removes = Some(200);
    let batch = run_store(&mut store, &mut ops);
    assert!(batch.unresolved.is_empty());
    assert!(
        ops.live.is_empty(),
        "TERM orphaned an unmarked detached tool descendant"
    );
    assert_eq!(
        ops.signals(),
        [
            (100, RecoverySignal::Terminate),
            (201, RecoverySignal::Kill)
        ]
    );
    assert_eq!(store.rows[0].1.inventory().len(), 3);
}

#[test]
fn recovery_missing_snapshot_entry_is_not_proof_a_known_process_exited() {
    for leader in [false, true] {
        let mut store = records(leader);
        let pid = if leader { 100 } else { 201 };
        if !leader {
            store.rows[0].1 = record(false)
                .with_inventory(saved_parent_and_child())
                .unwrap();
        }
        let mut ops = Processes::new(vec![process(pid, 1, MarkEvidence::Absent)]);
        ops.hidden_from_snapshot = Some(pid);
        let batch = run_store(&mut store, &mut ops);
        assert!(
            batch.gone.is_empty(),
            "known live identity was filtered from user enumeration"
        );
        assert!(ops.signals().is_empty());
        assert_eq!(ops.live[0].pid, pid);
    }
}

#[test]
fn recovery_checkpoints_children_created_during_term_grace_before_final_kill() {
    for fail_refresh in [false, true] {
        let mut store = records(true);
        if fail_refresh {
            store.checkpoint_failure_at = Some(2);
        }
        let mut ops = Processes::new(vec![marked(100)]);
        ops.spawn_during_grace = Some(process(101, 100, MarkEvidence::Absent));
        let batch = run_store(&mut store, &mut ops);
        if fail_refresh {
            assert!(batch.gone.is_empty());
            assert_eq!(ops.signals(), [(100, RecoverySignal::Terminate)]);
            assert_eq!(ops.live.len(), 2);
            assert_eq!(store.rows[0].1.inventory().len(), 1);
        } else {
            assert!(batch.unresolved.is_empty());
            assert!(
                ops.live.is_empty(),
                "a child born during TERM grace survived Gone"
            );
            assert_eq!(
                ops.signals(),
                [
                    (100, RecoverySignal::Terminate),
                    (100, RecoverySignal::Kill),
                    (101, RecoverySignal::Kill)
                ]
            );
            assert_eq!(store.rows[0].1.inventory().len(), 2);
        }
    }
}

#[test]
fn recovery_walks_verified_unreadable_intermediary_without_signalling_it() {
    for identity_readable in [true, false] {
        let mut store = records(false);
        let mut intermediary = process(201, 200, MarkEvidence::Unreadable);
        if !identity_readable {
            intermediary.identity = None;
        }
        let mut ops = Processes::new(vec![
            marked(200),
            intermediary,
            process(202, 201, MarkEvidence::Absent),
        ]);
        let batch = run_store(&mut store, &mut ops);
        assert!(batch.unresolved.is_empty());
        assert!(!ops.signals().iter().any(|(pid, _)| *pid == 201));
        if identity_readable {
            assert_eq!(
                ops.signals(),
                [(200, RecoverySignal::Kill), (202, RecoverySignal::Kill)]
            );
            let inventory = store.rows[0].1.inventory();
            assert_eq!(inventory.len(), 2); // bridge is ancestry evidence, not a saved signal target
            let saved = inventory.iter().find(|t| t.identity.pid == 202).unwrap();
            assert!(saved.ancestry.iter().map(|p| p.pid).eq([200, 201]));
        } else {
            assert_eq!(ops.signals(), [(200, RecoverySignal::Kill)]);
            assert!(ops.live.iter().any(|p| p.pid == 202));
        }
    }
}

#[test]
fn recovery_torn_snapshot_cannot_attach_old_ppid_to_reused_intermediary() {
    let mut bridge = process(201, 200, MarkEvidence::Unreadable);
    bridge.identity.as_mut().unwrap().started = "replacement".into();
    let child = process(202, 201, MarkEvidence::Absent);
    let mut ops = Processes::new(vec![marked(200), bridge, child]);
    let mut store = records(false);
    let batch = run_store(&mut store, &mut ops);
    assert!(batch.gone.is_empty());
    assert!(
        ops.signals().is_empty(),
        "a reused parent PID cannot establish ancestry"
    );
    assert_eq!(store.checkpoints, 0);
}

#[test]
fn recovery_cyclic_snapshot_cannot_authorize_descendant_signal() {
    let root = process(200, 201, MarkEvidence::Present(MARK.into()));
    let child = process(201, 200, MarkEvidence::Absent);
    let mut ops = Processes::new(vec![root, child]);
    let mut store = records(false);
    let batch = run_store(&mut store, &mut ops);
    assert!(batch.gone.is_empty());
    assert!(
        ops.signals().is_empty(),
        "cyclic parent links cannot establish ancestry"
    );
    assert_eq!(store.checkpoints, 0);
}

#[test]
fn recovery_detects_torn_parent_identity_even_if_replacement_is_a_zombie() {
    let mut bridge = process(201, 200, MarkEvidence::Unreadable);
    bridge.identity.as_mut().unwrap().started = "zombie-replacement".into();
    bridge.zombie = true;
    let child = process(202, 201, MarkEvidence::Absent);
    let mut ops = Processes::new(vec![marked(200), bridge, child]);
    let mut store = records(false);
    let batch = run_store(&mut store, &mut ops);
    assert!(
        batch.gone.is_empty(),
        "filtering must not hide an inconsistent parent identity"
    );
    assert!(ops.signals().is_empty());
    assert_eq!(store.checkpoints, 0);
}

#[test]
fn recovery_startup_cannot_discard_torn_snapshot_when_recorded_leader_is_reused() {
    let mut replacement = process(100, 1, MarkEvidence::Unreadable);
    replacement.identity.as_mut().unwrap().started = "zombie-replacement".into();
    replacement.zombie = true;
    let mut ops = Processes::new(vec![]);
    ops.snapshots = [
        vec![replacement, process(101, 100, MarkEvidence::Absent)],
        vec![marked(200)],
        vec![],
    ]
    .into();
    let mut store = records(true);
    let batch = run_store(&mut store, &mut ops);
    assert!(batch.gone.is_empty());
    assert!(ops.signals().is_empty());
    assert_eq!(store.checkpoints, 0);
    assert_eq!(
        ops.actions
            .iter()
            .filter(|a| **a == Action::Snapshot)
            .count(),
        2,
        "failure must still observe a full mark scan without authorizing signals"
    );
}

#[test]
fn recovery_unreadable_identity_cannot_hide_reported_cycle() {
    let mut leader = marked(100);
    leader.zombie = true;
    let mut child = process(101, 101, MarkEvidence::Unreadable);
    child.identity = None;
    let mut ops = Processes::new(vec![]);
    ops.snapshots = [vec![leader, child], vec![marked(200)], vec![]].into();
    let mut store = records(true);
    let batch = run_store(&mut store, &mut ops);
    assert!(batch.gone.is_empty());
    assert!(ops.signals().is_empty());
    assert_eq!(store.checkpoints, 0);
}

#[test]
fn recovery_live_worker_reloads_only_one_record_across_retries() {
    let mut store = records(false);
    store.fail = true; // the startup pending sweep is unavailable to a live cleanup worker
    store.rows.push((
        8,
        RecoveryRecord::new("other".into(), None, vec![]).unwrap(),
    ));
    let mut ops = Processes::new(vec![
        marked(200),
        process(201, 200, MarkEvidence::Absent),
        process(300, 1, MarkEvidence::Present("other".into())),
    ]);
    ops.checkpoint_checks = Some(store.committed.clone());
    ops.signal_failure = Some(201);
    let deadline = ops.deadline();
    assert!(recover_record(&mut store, &7, &mut ops, deadline).is_err());
    assert_eq!(store.rows[0].1.inventory().len(), 2);
    ops.signal_failure = None;
    let deadline = ops.deadline();
    let gone = recover_record(&mut store, &7, &mut ops, deadline).unwrap();
    assert_eq!(gone.mark(), MARK);
    assert_eq!(store.loaded, [7, 7]);
    assert_eq!(ops.live.iter().map(|p| p.pid).collect::<Vec<_>>(), [300]);
    assert_eq!(store.rows.len(), 2);
    assert!(store.rows[1].1.inventory().is_empty());
}

#[test]
fn recovery_live_worker_missing_or_unreadable_record_cannot_authorize_signals() {
    for read_failure in [false, true] {
        let mut store = records(false);
        store.load_failure = read_failure;
        let mut ops = Processes::new(vec![marked(200)]);
        let deadline = ops.deadline();
        let key = if read_failure { 7 } else { 8 };
        let failure = recover_record(&mut store, &key, &mut ops, deadline).unwrap_err();
        assert_eq!(
            failure.error.kind(),
            if read_failure {
                io::ErrorKind::Other
            } else {
                io::ErrorKind::NotFound
            }
        );
        assert_eq!(store.loaded, [key]);
        assert_eq!(store.checkpoints, 0);
        assert!(ops.actions.is_empty());
    }
}

#[test]
fn recovery_native_sleep_recomputes_the_budget_after_interrupts() {
    use std::cell::Cell;
    let start = Instant::now();
    let deadline = start + Duration::from_millis(100);
    let clock = Cell::new(start);
    let mut calls = 0;
    wait_until_with(
        deadline,
        || clock.get(),
        |remaining| {
            calls += 1;
            match calls {
                1 => {
                    assert_eq!(remaining, Duration::from_millis(100));
                    clock.set(start + Duration::from_millis(70));
                    Err(io::Error::from(io::ErrorKind::Interrupted))
                }
                2 => {
                    assert_eq!(remaining, Duration::from_millis(30));
                    clock.set(deadline);
                    Ok(())
                }
                _ => panic!("wait retried after its absolute deadline"),
            }
        },
    )
    .unwrap();
    assert_eq!(calls, 2);

    clock.set(start);
    calls = 0;
    wait_until_with(
        deadline,
        || clock.get(),
        |_| {
            calls += 1;
            clock.set(deadline + Duration::from_millis(1));
            Err(io::Error::from(io::ErrorKind::Interrupted))
        },
    )
    .unwrap();
    assert_eq!(calls, 1);
}
