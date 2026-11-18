//! Test support for consumers' tests of the confinement contract (`test-support`):
//! what a test cannot arrange without knowing the system's mechanism.

use std::io;
use std::process::Command;

use super::native;

/// A system call of the GNU/Linux adapter that a container's own seccomp profile
/// may refuse (ADR-0017 LNX-3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefusedCall {
    /// `landlock_create_ruleset`: the version query and the ruleset.
    LandlockCreateRuleset,
    /// `landlock_restrict_self`.
    LandlockRestrictSelf,
    /// `seccomp`, and `prctl(PR_SET_SECCOMP)`.
    Seccomp,
}

/// Makes the process that `command` starts run under a profile that answers `call`
/// with "operation not permitted" and allows everything else, as a container
/// runtime's profile does. A process so started must refuse to confine itself and
/// say which call failed.
///
/// GNU/Linux only. Every other system refuses: its adapter does not confine with
/// these calls, so there is no such profile to test, and the caller has nothing to
/// start.
pub fn refuse_call(command: &mut Command, call: RefusedCall) -> io::Result<()> {
    native::refuse_call(command, call)
}
