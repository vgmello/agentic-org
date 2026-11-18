//! The refusing process adapter: nothing is checked, started, watched or signalled.
//!
//! Every function returns the `process` refusal. The two types have no value, so the
//! methods that would act on one cannot be reached. The only item that is not a
//! refusal is [`is_exited_group_denial`], which answers `false`: no error is ever
//! taken to mean that a process group is empty.

use std::ffi::CStr;
use std::io;
use std::os::fd::OwnedFd;
use std::time::{Duration, Instant};

use super::{
    ExitFields, JobObservation, MarkEvidence, ProcessIdentity, ProcessObservation, RecoveryContext,
    RecoverySignal, SignalOutcome, Uids,
};
use crate::Refused;

fn refused<T>() -> io::Result<T> {
    Err(Refused::unsupported("process").into())
}

pub(super) fn uids() -> io::Result<Uids> {
    refused()
}

pub(super) fn check_version() -> io::Result<()> {
    refused()
}

pub(super) enum Watch {}

impl Watch {
    pub(super) fn open(_pid: libc::pid_t) -> io::Result<Self> {
        refused()
    }

    pub(super) fn poll(&self, _remaining: Duration) -> libc::c_int {
        match *self {}
    }
}

pub(super) fn identity(_pid: libc::pid_t) -> io::Result<(u32, String, String)> {
    refused()
}

pub(super) fn exit_fields() -> io::Result<ExitFields> {
    refused()
}

pub(super) fn release_spawned(
    _pid: libc::pid_t,
    _exited: impl FnOnce() -> io::Result<bool>,
) -> io::Result<()> {
    refused()
}

pub(super) fn is_exited_group_denial(_error: &io::Error) -> bool {
    false
}

pub(super) fn group_contains_only_leader(_pid: libc::pid_t) -> io::Result<bool> {
    refused()
}

pub(super) const PIPE_IS_CLOEXEC: bool = false;

pub(super) fn pipe() -> io::Result<[OwnedFd; 2]> {
    refused()
}

/// # Safety
///
/// None: `actions` is not touched.
pub(super) unsafe fn add_chdir(
    _actions: *mut libc::posix_spawn_file_actions_t,
    _cwd: &CStr,
) -> io::Result<()> {
    refused()
}

/// # Safety
///
/// None: `actions` is not touched.
pub(super) unsafe fn session_flags(
    _actions: *mut libc::posix_spawn_file_actions_t,
) -> io::Result<libc::c_int> {
    refused()
}

pub(super) fn entropy(_bytes: &mut [u8; 32]) -> io::Result<()> {
    refused()
}

pub(super) enum RecoveryScan {}

impl RecoveryScan {
    pub(super) fn new(_mark_prefix: Vec<u8>) -> io::Result<Self> {
        refused()
    }

    pub(super) fn context(&self) -> &RecoveryContext {
        match *self {}
    }

    pub(super) fn snapshot(&mut self, _deadline: Instant) -> io::Result<Vec<ProcessObservation>> {
        match *self {}
    }

    pub(super) fn job_observation(
        &self,
        _identity: &ProcessIdentity,
        _deadline: Instant,
    ) -> io::Result<Option<JobObservation>> {
        match *self {}
    }

    pub(super) fn signal_checked(
        &mut self,
        _target: &ProcessIdentity,
        _mark_accepted: &dyn Fn(&MarkEvidence) -> bool,
        _signal: RecoverySignal,
        _deadline: Instant,
    ) -> io::Result<SignalOutcome> {
        match *self {}
    }

    pub(super) fn wait_exited(
        &mut self,
        _identity: &ProcessIdentity,
        _deadline: Instant,
    ) -> io::Result<bool> {
        match *self {}
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use crate::Refused;

    fn assert_refused<T>(result: io::Result<T>) {
        let Err(error) = result else {
            panic!("the refusing adapter succeeded");
        };
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert_eq!(
            error
                .get_ref()
                .and_then(|source| source.downcast_ref::<Refused>()),
            Some(&Refused::unsupported("process"))
        );
    }

    #[test]
    fn the_preparation_check_refuses_before_reading_anything() {
        assert_refused(super::uids());
        assert_refused(super::check_version());
    }

    #[test]
    fn nothing_is_watched_identified_observed_or_released() {
        // SAFETY: getpid has no arguments or side effects.
        let own = unsafe { libc::getpid() };
        assert_refused(super::Watch::open(own));
        assert_refused(super::identity(own));
        let mut asked = false;
        assert_refused(super::release_spawned(own, || {
            asked = true;
            Ok(false)
        }));
        assert!(!asked, "the refusing adapter asked about a child");
        assert_refused(super::exit_fields());
    }

    #[test]
    fn no_signal_error_means_an_empty_group() {
        for code in [libc::EPERM, libc::ESRCH] {
            assert!(!super::is_exited_group_denial(
                &io::Error::from_raw_os_error(code)
            ));
        }
        // SAFETY: getpid has no arguments or side effects.
        assert_refused(super::group_contains_only_leader(unsafe { libc::getpid() }));
    }

    #[test]
    fn nothing_is_piped_or_prepared_for_a_spawn() {
        assert_refused(super::pipe());
        // SAFETY: the refusing adapter touches neither pointer.
        unsafe {
            assert_refused(super::add_chdir(std::ptr::null_mut(), c"/"));
            assert_refused(super::session_flags(std::ptr::null_mut()));
        }
    }

    #[test]
    fn no_entropy_is_read() {
        let mut bytes = [0x5a; 32];
        assert_refused(super::entropy(&mut bytes));
        assert_eq!(bytes, [0x5a; 32]);
    }

    #[test]
    fn no_recovery_scan_opens() {
        assert_refused(super::RecoveryScan::new(b"MARK=".to_vec()));
    }
}
