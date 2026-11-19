//! The recovery process port over the platform's native scan.
//!
//! `org_platform::process::RecoveryScan` observes and signals processes; it has a
//! macOS adapter, a GNU/Linux adapter and a refusing one. Everything that decides is
//! here or in this module's parent: which variable carries the mark and what a valid
//! mark is, which mark evidence a target must still show before it is signalled, and
//! what proves membership in a recorded spawn's job.

use std::io;
use std::time::Instant;

use org_platform::process as platform;

use super::{
    native_mark, sealed, ProcessIdentity, ProcessObservation, RecoveryContext, RecoveryProcesses,
    RecoverySignal, SignalOutcome, VerifiedTarget,
};

/// The environment variable `PreparedSpawn` sets to the session mark.
const MARK_VARIABLE: &str = "ORG_SESSION_MARK";

/// Native recovery operations on macOS and Linux.
///
/// On Linux a snapshot retains pidfds for its live processes, and each signal is sent
/// through the matching descriptor after revalidating the target. On macOS signals
/// recheck start identity and credentials, and directly marked targets also require
/// the exact current mark; for non-child processes macOS can still recycle a PID
/// between the final check and `kill`.
pub struct NativeRecovery(platform::RecoveryScan);

impl NativeRecovery {
    /// Validate the host and open the native scan. On Linux this checks the procfs
    /// namespace and kernel pidfd support. A build target without a process adapter
    /// is refused here, before any process is observed.
    pub fn new() -> io::Result<Self> {
        platform::RecoveryScan::new(MARK_VARIABLE).map(Self)
    }
}

fn to_platform(identity: &ProcessIdentity) -> platform::ProcessIdentity {
    platform::ProcessIdentity {
        pid: identity.pid,
        started: identity.started.clone(),
        boot: identity.boot.clone(),
    }
}

fn identity(identity: platform::ProcessIdentity) -> ProcessIdentity {
    ProcessIdentity {
        pid: identity.pid,
        started: identity.started,
        boot: identity.boot,
    }
}

fn observation(row: platform::ProcessObservation) -> ProcessObservation {
    ProcessObservation {
        pid: row.pid,
        identity: row.identity.map(identity),
        parent: row.parent.map(identity),
        group: row.group,
        session: row.session,
        uid: row.uid,
        credentials_safe: row.credentials_safe,
        zombie: row.zombie,
        mark: native_mark(row.mark),
    }
}

fn job_observation(observed: platform::JobObservation) -> super::job::JobObservation {
    super::job::JobObservation {
        identity: identity(observed.identity),
        order: observed.order,
        group: observed.group,
        session: observed.session,
        safe: observed.safe,
    }
}

impl sealed::Processes for NativeRecovery {}

impl RecoveryProcesses for NativeRecovery {
    fn context(&mut self) -> io::Result<RecoveryContext> {
        let context = self.0.context();
        Ok(RecoveryContext {
            uid: context.uid,
            pid: context.pid,
            boot: context.boot.clone(),
        })
    }

    fn verify_recorded_job(
        &mut self,
        target: &ProcessIdentity,
        leader: &crate::ChildIdentity,
        deadline: Instant,
    ) -> io::Result<bool> {
        let context = self.context()?;
        super::job::verify(target, leader, &context, deadline, Instant::now, |id| {
            Ok(self
                .0
                .job_observation(&to_platform(id), deadline)?
                .map(job_observation))
        })
    }

    fn snapshot(&mut self, deadline: Instant) -> io::Result<Vec<ProcessObservation>> {
        Ok(self
            .0
            .snapshot(deadline)?
            .into_iter()
            .map(observation)
            .collect())
    }

    fn signal_checked(
        &mut self,
        target: &VerifiedTarget,
        signal: RecoverySignal,
        deadline: Instant,
    ) -> io::Result<SignalOutcome> {
        let outcome = self.0.signal_checked(
            &to_platform(target.identity()),
            &|mark| super::signal_allowed_by(target, mark),
            match signal {
                RecoverySignal::Terminate => platform::RecoverySignal::Terminate,
                RecoverySignal::Kill => platform::RecoverySignal::Kill,
            },
            deadline,
        )?;
        Ok(match outcome {
            platform::SignalOutcome::Signalled => SignalOutcome::Signalled,
            platform::SignalOutcome::Gone => SignalOutcome::Gone,
        })
    }

    fn wait_exited(&mut self, identity: &ProcessIdentity, deadline: Instant) -> io::Result<bool> {
        self.0.wait_exited(&to_platform(identity), deadline)
    }

    fn now(&mut self) -> Instant {
        Instant::now()
    }

    fn wait_until(&mut self, deadline: Instant) -> io::Result<()> {
        super::native_wait_until(deadline)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::NativeRecovery;
    use crate::recovery::{RecoveryProcesses, RecoveryReport, ScanFailure, ScanFailureKind};

    #[test]
    fn native_deadline_failure_classification_preserves_io() {
        let error = NativeRecovery::new()
            .unwrap()
            .snapshot(Instant::now() - Duration::from_nanos(1))
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert_eq!(error.to_string(), "native recovery deadline elapsed");
        assert_eq!(
            ScanFailure {
                error,
                report: RecoveryReport::default()
            }
            .kind(),
            ScanFailureKind::DeadlineElapsed
        );
    }
}
