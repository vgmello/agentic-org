//! Test support: a seccomp profile that refuses some of the adapter's own calls, as
//! a container runtime's profile may (ADR-0017 LNX-3). Never part of a production
//! build.

use std::io;
use std::os::unix::process::CommandExt;
use std::process::Command;

use super::seccomp::{
    Instruction, JUMP_EQUAL, LOAD, OFFSET_ARGS, OFFSET_NR, RETURN, RET_ALLOW, RET_ERRNO,
};
use crate::confinement::testing::RefusedCall;

/// Which uses of a system call a profile refuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum When {
    /// Every use.
    Always,
    /// The low half of the argument with this index has this value.
    Argument(u32, u32),
    /// Argument 0 is `SECCOMP_SET_MODE_FILTER` and argument 2, the filter, is not
    /// null: an installation, not the question whether one is possible.
    Installation,
}

/// A filter that answers each of `calls`, when its condition holds, with `errno`,
/// and allows everything else.
pub(super) fn refusing_when(calls: &[(libc::c_long, When)], errno: i32) -> Vec<Instruction> {
    let statement = |code, k| libc::sock_filter {
        code,
        jt: 0,
        jf: 0,
        k,
    };
    let jump = |k, jt, jf| libc::sock_filter {
        code: JUMP_EQUAL,
        jt,
        jf,
        k,
    };
    let argument = |index: u32| statement(LOAD, OFFSET_ARGS + 8 * index);
    // The high half of an argument: these targets are little-endian.
    let high = |index: u32| statement(LOAD, OFFSET_ARGS + 8 * index + 4);
    let refuse = statement(RETURN, RET_ERRNO | errno as u32);
    let allow = statement(RETURN, RET_ALLOW);
    let mut program = Vec::new();
    for (call, when) in calls {
        // Each block loads the call number again: a block that looked at an
        // argument has replaced it.
        program.push(statement(LOAD, OFFSET_NR));
        match *when {
            When::Always => program.extend([jump(*call as u32, 0, 1), refuse]),
            When::Argument(index, value) => program.extend([
                jump(*call as u32, 0, 4),
                argument(index),
                jump(value, 0, 1),
                refuse,
                allow,
            ]),
            When::Installation => program.extend([
                jump(*call as u32, 0, 8),
                argument(0),
                jump(libc::SECCOMP_SET_MODE_FILTER, 0, 5),
                argument(2),
                jump(0, 0, 2),
                high(2),
                jump(0, 1, 0),
                refuse,
                allow,
            ]),
        }
    }
    program.push(allow);
    program
}

/// A filter that answers `calls` with `errno` and allows everything else. `prctl` is
/// answered so only for `PR_SET_SECCOMP`, the older way to install a filter.
pub(super) fn refusing(calls: &[libc::c_long], errno: i32) -> Vec<Instruction> {
    let calls: Vec<_> = calls
        .iter()
        .map(|call| match *call {
            libc::SYS_prctl => (*call, When::Argument(0, libc::PR_SET_SECCOMP as u32)),
            _ => (*call, When::Always),
        })
        .collect();
    refusing_when(&calls, errno)
}

/// The profile of each [`RefusedCall`].
pub(super) fn profile(call: RefusedCall) -> Vec<Instruction> {
    match call {
        RefusedCall::LandlockCreateRuleset => {
            refusing(&[libc::SYS_landlock_create_ruleset], libc::EPERM)
        }
        // The version query passes flag 1 and a ruleset is created with flags 0.
        RefusedCall::LandlockRuleset => refusing_when(
            &[(libc::SYS_landlock_create_ruleset, When::Argument(2, 0))],
            libc::EPERM,
        ),
        RefusedCall::LandlockRestrictSelf => {
            refusing(&[libc::SYS_landlock_restrict_self], libc::EPERM)
        }
        RefusedCall::Seccomp => refusing(&[libc::SYS_seccomp, libc::SYS_prctl], libc::EPERM),
        RefusedCall::SeccompInstall => {
            refusing_when(&[(libc::SYS_seccomp, When::Installation)], libc::ENOMEM)
        }
    }
}

/// Puts the calling thread under `program`, which then holds for the process it
/// becomes and for its children. Only `prctl` and `seccomp`: safe between `fork` and
/// `exec`.
pub(super) fn enter(program: &[Instruction]) -> io::Result<()> {
    let description = libc::sock_fprog {
        len: program.len() as libc::c_ushort,
        filter: program.as_ptr().cast_mut(),
    };
    // SAFETY: no-new-privileges takes integers; the filter description points to
    // live instructions, which the kernel copies.
    unsafe {
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
            || libc::syscall(
                libc::SYS_seccomp,
                libc::SECCOMP_SET_MODE_FILTER,
                0,
                &description as *const libc::sock_fprog,
            ) != 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub(in crate::confinement) fn refuse_call(
    command: &mut Command,
    call: RefusedCall,
) -> io::Result<()> {
    let program = profile(call);
    // SAFETY: the hook runs in the new process before `exec` and makes only the two
    // system calls of `enter`, on instructions built before the fork.
    unsafe { command.pre_exec(move || enter(&program)) };
    Ok(())
}
