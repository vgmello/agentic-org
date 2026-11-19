//! Test support for consumers' tests of the confinement contract (`test-support`):
//! what a test cannot arrange without knowing the system's mechanism.

use std::io;
use std::process::Command;

use super::native;

/// A system call of the GNU/Linux adapter that a container's own seccomp profile
/// may refuse (ADR-0017 LNX-3). A refused Landlock call leaves the web process
/// confined by its seccomp filter alone; a refused `seccomp` stops it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefusedCall {
    /// `landlock_create_ruleset`, whatever it is asked: the version query is the
    /// first to fail.
    LandlockCreateRuleset,
    /// `landlock_create_ruleset` when it is to create a ruleset: the version query
    /// is answered.
    LandlockRuleset,
    /// `landlock_restrict_self`.
    LandlockRestrictSelf,
    /// `seccomp`, and `prctl(PR_SET_SECCOMP)`: the host's own question fails.
    Seccomp,
    /// `seccomp` when it is given a filter to install, answered with "out of
    /// memory" as a kernel answers a filter it has no room for; the questions
    /// whether one can be installed are answered. No runtime's profile is known to
    /// do this: it stands for an installation that fails although the host's
    /// question passed, so that the web process's own refusal can be tested.
    SeccompInstall,
}

/// Makes the process that `command` starts run under a profile that answers `call`
/// with "operation not permitted" (or as the variant says) and allows everything
/// else, as a container runtime's profile does. The process's children inherit it.
///
/// GNU/Linux only. Every other system refuses: its adapter does not confine with
/// these calls, so there is no such profile to test, and the caller has nothing to
/// start.
pub fn refuse_call(command: &mut Command, call: RefusedCall) -> io::Result<()> {
    native::refuse_call(command, call)
}
