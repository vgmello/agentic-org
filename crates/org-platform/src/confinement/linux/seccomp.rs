//! Step 5 of the Linux adapter: the seccomp filter (Linux host design, section 3.3).
//!
//! A classic BPF program over `struct seccomp_data` (`include/uapi/linux/seccomp.h`).
//! It kills the process on a foreign architecture, allows the calls of
//! [`super::syscalls`], answers `clone3` with `ENOSYS`, and kills the process on every
//! other call: a forbidden call never returns an error.

use std::io;
use std::mem;

use super::syscalls::{
    Arch, FCNTL_COMMANDS, IOCTL_REQUESTS, MREMAP_FLAGS, PRCTL_OPTIONS, PROT_EXEC, SENDTO_FLAGS,
    THREAD_CLONE_FLAGS,
};

/// One instruction (`struct sock_filter`, `include/uapi/linux/filter.h`).
pub(super) type Instruction = libc::sock_filter;

const _: () = {
    assert!(mem::size_of::<libc::sock_filter>() == 8);
    assert!(mem::offset_of!(libc::sock_filter, code) == 0);
    assert!(mem::offset_of!(libc::sock_filter, jt) == 2);
    assert!(mem::offset_of!(libc::sock_filter, jf) == 3);
    assert!(mem::offset_of!(libc::sock_filter, k) == 4);
    assert!(mem::size_of::<libc::sock_fprog>() == 16);
    assert!(mem::offset_of!(libc::sock_fprog, len) == 0);
    assert!(mem::offset_of!(libc::sock_fprog, filter) == 8);
    assert!(mem::size_of::<libc::seccomp_data>() == 64);
    assert!(mem::offset_of!(libc::seccomp_data, nr) == OFFSET_NR as usize);
    assert!(mem::offset_of!(libc::seccomp_data, arch) == OFFSET_ARCH as usize);
    assert!(mem::offset_of!(libc::seccomp_data, args) == OFFSET_ARGS as usize);
};

// Offsets in `struct seccomp_data`.
pub(super) const OFFSET_NR: u32 = 0;
pub(super) const OFFSET_ARCH: u32 = 4;
pub(super) const OFFSET_ARGS: u32 = 16;

// `include/uapi/linux/seccomp.h`.
pub(super) const RET_KILL_PROCESS: u32 = 0x8000_0000;
pub(super) const RET_ERRNO: u32 = 0x0005_0000;
pub(super) const RET_ALLOW: u32 = 0x7fff_0000;
const SET_MODE_FILTER: libc::c_uint = 1;
const GET_ACTION_AVAIL: libc::c_uint = 2;
const _: () = {
    assert!(RET_KILL_PROCESS == libc::SECCOMP_RET_KILL_PROCESS);
    assert!(RET_ERRNO == libc::SECCOMP_RET_ERRNO);
    assert!(RET_ALLOW == libc::SECCOMP_RET_ALLOW);
    assert!(SET_MODE_FILTER == libc::SECCOMP_SET_MODE_FILTER);
    assert!(GET_ACTION_AVAIL == libc::SECCOMP_GET_ACTION_AVAIL);
};

/// What `clone3` returns: glibc then calls `clone`.
pub(super) const CLONE3_ERRNO: u32 = libc::ENOSYS as u32;

// Instruction codes (`include/uapi/linux/bpf_common.h`).
pub(super) const LOAD: u16 = (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16;
pub(super) const JUMP_EQUAL: u16 = (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16;
pub(super) const JUMP_AT_LEAST: u16 = (libc::BPF_JMP | libc::BPF_JGE | libc::BPF_K) as u16;
pub(super) const JUMP_ANY_BIT: u16 = (libc::BPF_JMP | libc::BPF_JSET | libc::BPF_K) as u16;
pub(super) const RETURN: u16 = (libc::BPF_RET | libc::BPF_K) as u16;

/// The kernel's limit on a filter's length (`BPF_MAXINSNS`).
const MOST_INSTRUCTIONS: usize = 4096;

const fn statement(code: u16, k: u32) -> Instruction {
    libc::sock_filter {
        code,
        jt: 0,
        jf: 0,
        k,
    }
}

const fn jump(code: u16, k: u32, jt: u8, jf: u8) -> Instruction {
    libc::sock_filter { code, jt, jf, k }
}

/// The low 32-bit word of argument `index`. Little-endian: the low word comes first.
const fn low(index: u32) -> u32 {
    OFFSET_ARGS + 8 * index
}

/// The high 32-bit word of argument `index`.
const fn high(index: u32) -> u32 {
    OFFSET_ARGS + 8 * index + 4
}

/// Where a test inside an argument check goes.
#[derive(Clone, Copy)]
enum To {
    /// The next instruction of the check.
    Next,
    Allow,
    Kill,
}

/// One step of an argument check: a load, or a test with its two destinations.
enum Step {
    Load(u32),
    Test { code: u16, k: u32, yes: To, no: To },
}

/// Appends the check of one system call that is decided by its arguments. The check
/// is entered only for `number`; every path through it ends in allow or kill, never
/// in the instructions after it, because the accumulator no longer holds the number.
fn decided_by_arguments(program: &mut Vec<Instruction>, number: u32, steps: &[Step]) {
    assert!(
        matches!(
            steps.last(),
            Some(Step::Test {
                yes: To::Allow | To::Kill,
                no: To::Allow | To::Kill,
                ..
            })
        ),
        "the last step of an argument check must decide"
    );
    let count = steps.len();
    // The check's steps, then its `allow`, then its `kill`.
    let skip = u8::try_from(count + 2).expect("an argument check is short");
    program.push(jump(JUMP_EQUAL, number, 0, skip));
    for (position, step) in steps.iter().enumerate() {
        let offset = |to: To| {
            let target = match to {
                To::Next => position + 1,
                To::Allow => count,
                To::Kill => count + 1,
            };
            (target - position - 1) as u8
        };
        program.push(match *step {
            Step::Load(offset) => statement(LOAD, offset),
            Step::Test { code, k, yes, no } => jump(code, k, offset(yes), offset(no)),
        });
    }
    program.push(statement(RETURN, RET_ALLOW));
    program.push(statement(RETURN, RET_KILL_PROCESS));
}

/// Steps that allow the loaded word when it is one of `values`, and kill otherwise.
fn one_of(values: &[u32]) -> Vec<Step> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| Step::Test {
            code: JUMP_EQUAL,
            k: *value,
            yes: To::Allow,
            no: if index + 1 == values.len() {
                To::Kill
            } else {
                To::Next
            },
        })
        .collect()
}

/// The filter of one architecture for the process `pid`.
pub(super) fn program(arch: &Arch, pid: u32) -> Vec<Instruction> {
    let mut program = vec![
        // A system call of another architecture kills: its numbers mean other calls.
        statement(LOAD, OFFSET_ARCH),
        jump(JUMP_EQUAL, arch.audit, 1, 0),
        statement(RETURN, RET_KILL_PROCESS),
        statement(LOAD, OFFSET_NR),
    ];
    if let Some(foreign) = arch.foreign_from {
        program.push(jump(JUMP_AT_LEAST, foreign, 0, 1));
        program.push(statement(RETURN, RET_KILL_PROCESS));
    }
    for (_, number) in arch.allowed {
        program.push(jump(JUMP_EQUAL, *number, 0, 1));
        program.push(statement(RETURN, RET_ALLOW));
    }
    program.push(jump(JUMP_EQUAL, arch.clone3, 0, 1));
    program.push(statement(RETURN, RET_ERRNO | CLONE3_ERRNO));
    // clone(flags, ...): a thread of this process, with exactly glibc's flags.
    decided_by_arguments(
        &mut program,
        arch.clone,
        &[
            Step::Load(high(0)),
            Step::Test {
                code: JUMP_EQUAL,
                k: (THREAD_CLONE_FLAGS >> 32) as u32,
                yes: To::Next,
                no: To::Kill,
            },
            Step::Load(low(0)),
            Step::Test {
                code: JUMP_EQUAL,
                k: THREAD_CLONE_FLAGS as u32,
                yes: To::Allow,
                no: To::Kill,
            },
        ],
    );
    // prctl(option, ...): the kernel reads the option as a 32-bit integer.
    let mut prctl = vec![Step::Load(low(0))];
    prctl.extend(one_of(PRCTL_OPTIONS));
    decided_by_arguments(&mut program, arch.prctl, &prctl);
    // ioctl(fd, request, ...): the kernel reads the request as a 32-bit integer.
    let mut ioctl = vec![Step::Load(low(1))];
    ioctl.extend(one_of(IOCTL_REQUESTS));
    decided_by_arguments(&mut program, arch.ioctl, &ioctl);
    // fcntl(fd, command, ...): the kernel reads the command as a 32-bit integer.
    let mut fcntl = vec![Step::Load(low(1))];
    fcntl.extend(one_of(FCNTL_COMMANDS));
    decided_by_arguments(&mut program, arch.fcntl, &fcntl);
    // sendto(fd, buffer, length, flags, destination, destination length): a send on
    // a connection the process holds. No flag but the two (the kernel reads the
    // flags as a 32-bit integer), no destination pointer and no destination length:
    // a destination can connect the socket, or address another one.
    decided_by_arguments(
        &mut program,
        arch.sendto,
        &[
            Step::Load(low(3)),
            Step::Test {
                code: JUMP_ANY_BIT,
                k: !SENDTO_FLAGS,
                yes: To::Kill,
                no: To::Next,
            },
            Step::Load(low(4)),
            Step::Test {
                code: JUMP_EQUAL,
                k: 0,
                yes: To::Next,
                no: To::Kill,
            },
            Step::Load(high(4)),
            Step::Test {
                code: JUMP_EQUAL,
                k: 0,
                yes: To::Next,
                no: To::Kill,
            },
            Step::Load(low(5)),
            Step::Test {
                code: JUMP_EQUAL,
                k: 0,
                yes: To::Allow,
                no: To::Kill,
            },
        ],
    );
    // mmap(addr, length, prot, ...) and mprotect(addr, length, prot): never
    // executable. `PROT_EXEC` is in the low word.
    for number in [arch.mmap, arch.mprotect] {
        decided_by_arguments(
            &mut program,
            number,
            &[
                Step::Load(low(2)),
                Step::Test {
                    code: JUMP_ANY_BIT,
                    k: PROT_EXEC,
                    yes: To::Kill,
                    no: To::Allow,
                },
            ],
        );
    }
    // mremap(old address, old size, new size, flags, ...): resize a block in place
    // or let the kernel move it. The kernel reads the flags as a 64-bit integer.
    let mut mremap = vec![
        Step::Load(high(3)),
        Step::Test {
            code: JUMP_EQUAL,
            k: 0,
            yes: To::Next,
            no: To::Kill,
        },
        Step::Load(low(3)),
    ];
    mremap.extend(one_of(MREMAP_FLAGS));
    decided_by_arguments(&mut program, arch.mremap, &mremap);
    // tgkill(tgid, tid, signal): a signal to one of this process's own threads. The
    // kernel reads the thread group as a 32-bit integer.
    decided_by_arguments(
        &mut program,
        arch.tgkill,
        &[
            Step::Load(low(0)),
            Step::Test {
                code: JUMP_EQUAL,
                k: pid,
                yes: To::Allow,
                no: To::Kill,
            },
        ],
    );
    program.push(statement(RETURN, RET_KILL_PROCESS));
    assert!(program.len() <= MOST_INSTRUCTIONS);
    program
}

/// Whether the kernel can kill a process from a filter. A query: it installs nothing.
/// The error is the call's error number.
pub(super) fn kill_process_available() -> Result<(), i32> {
    let action = RET_KILL_PROCESS;
    // SAFETY: the query reads one initialized 32-bit action through the pointer and
    // changes nothing.
    let result = unsafe {
        libc::syscall(
            libc::SYS_seccomp,
            GET_ACTION_AVAIL,
            0usize,
            &action as *const u32,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error().raw_os_error().unwrap_or(0));
    }
    Ok(())
}

/// Installs the filter on the calling thread, which must be the process's only one
/// and must have no-new-privileges set. It cannot be removed, and every thread
/// created later inherits it.
pub(super) fn install(arch: &Arch) -> io::Result<()> {
    let program = program(arch, std::process::id());
    let description = libc::sock_fprog {
        len: program.len() as libc::c_ushort,
        filter: program.as_ptr().cast_mut(),
    };
    // SAFETY: `description` points to `len` initialized instructions that live
    // through the call; the kernel copies and verifies them. Flags are zero.
    let result = unsafe {
        libc::syscall(
            libc::SYS_seccomp,
            SET_MODE_FILTER,
            0usize,
            &description as *const libc::sock_fprog,
        )
    };
    if result != 0 {
        return Err(super::failed_call("seccomp"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
