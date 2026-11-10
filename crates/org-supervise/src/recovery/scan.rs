use std::fmt;
use std::io;
use std::time::{Duration, Instant};

use super::capture::{capture_targets, CaptureScope, ValidatedSnapshot};
use super::checkpoint::TargetCheckpoint;

use super::{
    check_deadline, checked_context, ProcessIdentity, RecoveryProcesses, RecoveryReport,
    RecoverySignal,
};

/// A fresh, crate-issued pass request. Only the supervisor can issue requests, after
/// the appropriate leader/group kill stage. Callers cannot invent completion evidence.
///
/// ```compile_fail
/// use org_supervise::recovery::ScanRequest;
/// let request = ScanRequest { mark: "forged".into(), nonce: [0; 32],
///     deadline: std::time::Instant::now(), excluded: None, tool_sessions: vec![] };
/// ```
pub struct ScanRequest<'a> {
    mark: String,
    nonce: [u8; 32],
    deadline: Instant,
    excluded: Option<ProcessIdentity>,
    tool_sessions: Vec<u32>,
    inventory: &'a mut dyn TargetCheckpoint,
}

impl ScanRequest<'_> {
    /// Non-secret mark to scan for.
    pub fn mark(&self) -> &str {
        &self.mark
    }
    /// Fresh pass nonce, to be echoed only by the pass consuming this request.
    pub fn nonce(&self) -> &[u8; 32] {
        &self.nonce
    }
    /// Absolute deadline for this pass.
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    /// Exact owned unreaped leader to skip, if supplied by probe cleanup.
    pub fn excluded(&self) -> Option<&ProcessIdentity> {
        self.excluded.as_ref()
    }
    /// Hints only; full mark enumeration is mandatory even when these are present.
    pub fn tool_sessions(&self) -> &[u32] {
        &self.tool_sessions
    }
}

pub(super) struct RequestIssuer;

impl RequestIssuer {
    pub(super) fn issue<'a>(
        &mut self,
        mark: &str,
        excluded: Option<ProcessIdentity>,
        tool_sessions: &[u32],
        deadline: Instant,
        inventory: &'a mut dyn TargetCheckpoint,
    ) -> io::Result<ScanRequest<'a>> {
        let mut nonce = [0; 32];
        entropy(&mut nonce)?;
        Ok(ScanRequest {
            mark: mark.into(),
            nonce,
            deadline,
            excluded,
            tool_sessions: tool_sessions.into(),
            inventory,
        })
    }
}

fn entropy(bytes: &mut [u8; 32]) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        loop {
            // SAFETY: getentropy accepts this writable buffer, which is below its 256-byte limit.
            if unsafe { libc::getentropy(bytes.as_mut_ptr().cast(), bytes.len()) } == 0 {
                return Ok(());
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        let mut offset = 0;
        while offset < bytes.len() {
            // SAFETY: this is the remaining initialized writable slice. Refuse an
            // uninitialized entropy pool instead of blocking past the recovery deadline.
            let count = unsafe {
                libc::getrandom(
                    bytes[offset..].as_mut_ptr().cast(),
                    bytes.len() - offset,
                    libc::GRND_NONBLOCK,
                )
            };
            if count < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if count == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "empty entropy read",
                ));
            }
            offset += count as usize;
        }
        Ok(())
    }
}

/// Evidence from exactly one pass, bound to its request's mark and nonce.
///
/// ```compile_fail
/// use org_supervise::recovery::{ScanPass, RecoveryReport};
/// let pass = ScanPass { mark: "forged".into(), nonce: [0; 32],
///     report: RecoveryReport::default() };
/// ```
#[derive(Debug)]
pub struct ScanPass {
    mark: String,
    nonce: [u8; 32],
    report: RecoveryReport,
}

impl ScanPass {
    /// Echo of the consumed request's mark.
    pub fn mark(&self) -> &str {
        &self.mark
    }
    /// Echo of the consumed request's nonce.
    pub fn nonce(&self) -> &[u8; 32] {
        &self.nonce
    }
    /// Live eligible processes found by this pass, before signals.
    pub fn found_live(&self) -> usize {
        self.report.found_live
    }
    /// Includes unreadable-environment warnings, without environment bodies.
    pub fn report(&self) -> &RecoveryReport {
        &self.report
    }
}

/// Incomplete scan/recovery, retaining partial counts for diagnostics and retries.
#[derive(Debug)]
pub struct ScanFailure {
    /// Underlying refusal, enumeration, signal, wait or deadline error.
    pub error: io::Error,
    /// Work/warnings recorded before failure; never a completion proof.
    pub report: RecoveryReport,
}

impl fmt::Display for ScanFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}
impl std::error::Error for ScanFailure {}

/// One pass per call. The coordinator owns retries and issues fresh requests.
/// Process operations are an explicit parameter so scan and leader recovery share
/// one adapter and its pinned identity state, without shared mutable global state.
///
/// ```compile_fail,E0277
/// use org_supervise::recovery::*;
/// struct Forged;
/// impl ScanPort for Forged {
///     fn scan_pass(&mut self, _: ScanRequest, _: &mut dyn RecoveryProcesses)
///         -> Result<ScanPass, ScanFailure> { unimplemented!() }
/// }
/// ```
pub trait ScanPort: super::sealed::Scan {
    /// Consume exactly one request, returning its evidence or an incomplete failure.
    fn scan_pass(
        &mut self,
        request: ScanRequest,
        processes: &mut dyn RecoveryProcesses,
    ) -> Result<ScanPass, ScanFailure>;
}

/// The policy scanner, used with fake operations now and native adapters in slice 4b.
pub(super) struct MarkedScan;
impl super::sealed::Scan for MarkedScan {}
impl ScanPort for MarkedScan {
    fn scan_pass(
        &mut self,
        request: ScanRequest,
        processes: &mut dyn RecoveryProcesses,
    ) -> Result<ScanPass, ScanFailure> {
        scan_one_pass(request, processes)
    }
}

/// Scan once: snapshot all eligible marked roots and their descendants before any
/// signals, then kill/wait each verified identity individually. Unreadable environments
/// are counted/skipped. Zombies and the exact excluded leader are skipped first.
pub(crate) fn scan_one_pass(
    request: ScanRequest,
    ops: &mut dyn RecoveryProcesses,
) -> Result<ScanPass, ScanFailure> {
    let mut report = RecoveryReport::default();
    let result = (|| {
        if request.inventory.mark() != request.mark {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "scan checkpoint mark mismatch",
            ));
        }
        check_deadline(ops, request.deadline)?;
        let context = checked_context(ops)?;
        let snapshot = ValidatedSnapshot::capture(ops, request.deadline)?;
        check_deadline(ops, request.deadline)?;
        let mut targets = capture_targets(
            CaptureScope {
                mark: &request.mark,
                excluded: request.excluded.as_ref(),
                saved: request.inventory.saved(),
            },
            &snapshot,
            &context,
            ops,
            request.deadline,
            &mut report,
        )?;
        // Tool sessions only affect priority. Unhinted/detached marked tools remain included.
        targets.sort_by_key(|t| {
            !snapshot
                .get(t.identity.pid)
                .is_some_and(|p| request.tool_sessions.contains(&p.session))
        });
        // This write commits the entire cumulative inventory before the first kill.
        check_deadline(ops, request.deadline)?;
        request.inventory.save(&targets, request.deadline)?;
        for target in targets {
            check_deadline(ops, request.deadline)?;
            report.signals += 1;
            ops.signal_checked(&target, RecoverySignal::Kill, request.deadline)?;
            check_deadline(ops, request.deadline)?;
            let until = (ops.now() + Duration::from_secs(1)).min(request.deadline);
            if !ops.wait_exited(&target.identity, until)? {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "scan target still live after kill",
                ));
            }
        }
        check_deadline(ops, request.deadline)?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            report.passes = 1;
            Ok(ScanPass {
                mark: request.mark,
                nonce: request.nonce,
                report,
            })
        }
        Err(error) => Err(ScanFailure { error, report }),
    }
}

/// Shared by startup now and probe cleanup later. The caller has completed the
/// required kill stage before entering; only this coordinator issues each fresh request.
pub(crate) fn fixed_point(
    mark: &str,
    excluded: Option<ProcessIdentity>,
    tool_sessions: &[u32],
    ops: &mut dyn RecoveryProcesses,
    scan: &mut dyn ScanPort,
    deadline: Instant,
    inventory: &mut dyn TargetCheckpoint,
) -> Result<RecoveryReport, ScanFailure> {
    let mut report = RecoveryReport::default();
    let mut issuer = RequestIssuer;
    for _ in 0..3 {
        check_deadline(ops, deadline).map_err(|error| ScanFailure {
            error,
            report: std::mem::take(&mut report),
        })?;
        let request = issuer
            .issue(mark, excluded.clone(), tool_sessions, deadline, inventory)
            .map_err(|error| ScanFailure {
                error,
                report: std::mem::take(&mut report),
            })?;
        let nonce = request.nonce;
        let pass = match scan.scan_pass(request, ops) {
            Ok(pass) => pass,
            Err(failure) => {
                report.append(failure.report);
                return Err(ScanFailure {
                    error: failure.error,
                    report,
                });
            }
        };
        if pass.mark != mark || pass.nonce != nonce || pass.report.passes != 1 {
            return Err(ScanFailure {
                error: io::Error::new(
                    io::ErrorKind::InvalidData,
                    "scan pass does not match current request",
                ),
                report,
            });
        }
        let empty = pass.found_live() == 0;
        report.append(pass.report);
        check_deadline(ops, deadline).map_err(|error| ScanFailure {
            error,
            report: std::mem::take(&mut report),
        })?;
        if empty {
            return Ok(report);
        }
        if report.passes < 3 {
            let until = (ops.now() + Duration::from_secs(1)).min(deadline);
            ops.wait_until(until).map_err(|error| ScanFailure {
                error,
                report: std::mem::take(&mut report),
            })?;
        }
    }
    Err(ScanFailure {
        error: io::Error::new(
            io::ErrorKind::WouldBlock,
            "recovery remained nonempty after three scan passes",
        ),
        report,
    })
}

#[cfg(test)]
pub(super) mod test_support {
    use super::*;
    pub(in crate::recovery) fn issue<'a>(
        mark: &str,
        excluded: Option<ProcessIdentity>,
        deadline: Instant,
        inventory: &'a mut dyn TargetCheckpoint,
    ) -> ScanRequest<'a> {
        RequestIssuer
            .issue(mark, excluded, &[], deadline, inventory)
            .unwrap()
    }
    pub(in crate::recovery) fn corrupt(pass: &mut ScanPass, mark: bool) {
        if mark {
            pass.mark.push('x');
        } else {
            pass.nonce[0] ^= 1;
        }
    }
    pub(in crate::recovery) fn replace_nonce(pass: &mut ScanPass, nonce: [u8; 32]) {
        pass.nonce = nonce;
    }
}
