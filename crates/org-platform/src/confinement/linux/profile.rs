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

/// A filter that answers `calls` with `errno` and allows everything else. `prctl` is
/// answered so only for `PR_SET_SECCOMP`, the older way to install a filter.
pub(super) fn refusing(calls: &[libc::c_long], errno: i32) -> Vec<Instruction> {
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
    let refuse = statement(RETURN, RET_ERRNO | errno as u32);
    let mut program = vec![statement(LOAD, OFFSET_NR)];
    for call in calls {
        if *call == libc::SYS_prctl {
            program.extend([
                jump(*call as u32, 0, 4),
                statement(LOAD, OFFSET_ARGS),
                jump(libc::PR_SET_SECCOMP as u32, 0, 1),
                refuse,
                statement(RETURN, RET_ALLOW),
            ]);
        } else {
            program.extend([jump(*call as u32, 0, 1), refuse]);
        }
    }
    program.push(statement(RETURN, RET_ALLOW));
    program
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
    let calls: &[libc::c_long] = match call {
        RefusedCall::LandlockCreateRuleset => &[libc::SYS_landlock_create_ruleset],
        RefusedCall::LandlockRestrictSelf => &[libc::SYS_landlock_restrict_self],
        RefusedCall::Seccomp => &[libc::SYS_seccomp, libc::SYS_prctl],
    };
    let program = refusing(calls, libc::EPERM);
    // SAFETY: the hook runs in the new process before `exec` and makes only the two
    // system calls of `enter`, on instructions built before the fork.
    unsafe { command.pre_exec(move || enter(&program)) };
    Ok(())
}
