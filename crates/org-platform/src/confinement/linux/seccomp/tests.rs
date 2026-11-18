//! The filter program, interpreted: which system calls it allows, on which
//! architecture, with which arguments. Nothing here installs a filter; the
//! child-process tests of the adapter do.

use std::collections::BTreeSet;

use super::super::syscalls::{Arch, Call, AARCH64, X86_64};
use super::*;

/// The process id the test programs are built for.
const PID: u32 = 4242;

/// One system call as the filter sees it (`struct seccomp_data`).
#[derive(Clone, Copy)]
struct Entry {
    number: u32,
    arch: u32,
    arguments: [u64; 6],
}

impl Entry {
    fn bytes(&self) -> [u8; 64] {
        let mut bytes = [0u8; 64];
        bytes[0..4].copy_from_slice(&self.number.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.arch.to_le_bytes());
        for (index, argument) in self.arguments.iter().enumerate() {
            bytes[16 + 8 * index..24 + 8 * index].copy_from_slice(&argument.to_le_bytes());
        }
        bytes
    }
}

/// Runs a filter over one system call and returns its action. It knows exactly the
/// instructions the builder uses, and panics on any other, on a load outside the
/// data and on a jump outside the program: a filter the kernel would refuse.
fn run(program: &[Instruction], entry: &Entry) -> u32 {
    let data = entry.bytes();
    let mut accumulator = 0u32;
    let mut at = 0usize;
    loop {
        let instruction = program[at];
        let branch = |taken: bool| {
            at + 1
                + usize::from(if taken {
                    instruction.jt
                } else {
                    instruction.jf
                })
        };
        at = match instruction.code {
            LOAD => {
                let offset = instruction.k as usize;
                assert!(offset.is_multiple_of(4) && offset + 4 <= data.len());
                accumulator = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
                at + 1
            }
            JUMP_EQUAL => branch(accumulator == instruction.k),
            JUMP_AT_LEAST => branch(accumulator >= instruction.k),
            JUMP_ANY_BIT => branch(accumulator & instruction.k != 0),
            RETURN => return instruction.k,
            other => panic!("an instruction the builder does not use: {other:#x}"),
        };
    }
}

fn decide(arch: &Arch, number: u32, arguments: [u64; 6]) -> u32 {
    run(
        &program(arch, PID),
        &Entry {
            number,
            arch: arch.audit,
            arguments,
        },
    )
}

const NONE: [u64; 6] = [0; 6];
const ERRNO_ENOSYS: u32 = RET_ERRNO | 38;

fn tables() -> [(&'static str, &'static Arch); 2] {
    [("x86_64", &X86_64), ("aarch64", &AARCH64)]
}

#[test]
fn the_program_checks_the_architecture_before_anything_else() {
    for (name, arch) in tables() {
        let program = program(arch, PID);
        let first: Vec<_> = program[..4]
            .iter()
            .map(|i| (i.code, i.jt, i.jf, i.k))
            .collect();
        assert_eq!(
            first,
            [
                (LOAD, 0, 0, OFFSET_ARCH),
                (JUMP_EQUAL, 1, 0, arch.audit),
                (RETURN, 0, 0, RET_KILL_PROCESS),
                (LOAD, 0, 0, OFFSET_NR),
            ],
            "{name}"
        );
        let last = program.last().unwrap();
        assert_eq!((last.code, last.k), (RETURN, RET_KILL_PROCESS), "{name}");
        assert!(program.len() < 4096);
    }
}

#[test]
fn a_foreign_architecture_kills_whatever_the_call() {
    // AUDIT_ARCH_I386, AUDIT_ARCH_ARM, AUDIT_ARCH_RISCV64 and no architecture.
    let others = [0x4000_0003, 0x4000_0028, 0xC000_00F3, 0];
    for (name, arch) in tables() {
        let program = program(arch, PID);
        let foreign = tables()
            .into_iter()
            .map(|(_, other)| other.audit)
            .filter(|audit| *audit != arch.audit)
            .chain(others);
        for audit in foreign {
            for number in 0..600 {
                let entry = Entry {
                    number,
                    arch: audit,
                    arguments: NONE,
                };
                assert_eq!(run(&program, &entry), RET_KILL_PROCESS, "{name} {number}");
            }
        }
    }
}

#[test]
fn x32_calls_kill_on_x86_64() {
    // `__X32_SYSCALL_BIT`: the same architecture value, another system call table.
    for (_, number) in X86_64.allowed {
        assert_eq!(
            decide(&X86_64, number | 0x4000_0000, NONE),
            RET_KILL_PROCESS
        );
    }
    assert_eq!(decide(&X86_64, 0x4000_0000, NONE), RET_KILL_PROCESS);
    assert_eq!(decide(&X86_64, u32::MAX, NONE), RET_KILL_PROCESS);
    assert_eq!(decide(&AARCH64, u32::MAX, NONE), RET_KILL_PROCESS);
}

#[test]
fn every_listed_number_is_allowed_and_no_other() {
    for (name, arch) in tables() {
        let allowed: BTreeSet<u32> = arch.allowed.iter().map(|(_, number)| *number).collect();
        assert_eq!(allowed.len(), arch.allowed.len(), "{name}: a number twice");
        let by_argument = [
            arch.clone,
            arch.clone3,
            arch.prctl,
            arch.ioctl,
            arch.fcntl,
            arch.mmap,
            arch.mprotect,
            arch.tgkill,
        ];
        for number in by_argument {
            assert!(!allowed.contains(&number), "{name} {number}");
        }
        let program = program(arch, PID);
        for number in 0..=1024 {
            let action = run(
                &program,
                &Entry {
                    number,
                    arch: arch.audit,
                    arguments: NONE,
                },
            );
            let expected = if allowed.contains(&number) {
                RET_ALLOW
            } else if number == arch.clone3 {
                ERRNO_ENOSYS
            } else if number == arch.mmap || number == arch.mprotect {
                // No protection asked for: not executable.
                RET_ALLOW
            } else {
                // Every other call, and with these arguments `clone`, `prctl`,
                // `ioctl`, `fcntl` and `tgkill` too.
                RET_KILL_PROCESS
            };
            assert_eq!(action, expected, "{name} {number}");
        }
    }
}

#[test]
fn the_allowed_names_are_the_designs_list_and_three_more() {
    // Linux host design, section 3.3, step 5, in its order, with `epoll_*`,
    // `rt_sig*` and "clocks" read as the calls the runtime makes. The design's
    // `mmap`, `mprotect`, `fcntl`, `clone`, `prctl`, `ioctl` and signals to itself
    // are not here: their arguments decide.
    let design = [
        "munmap",
        "madvise",
        "brk",
        "futex",
        "epoll_create1",
        "epoll_ctl",
        "epoll_wait",
        "eventfd2",
        "read",
        "write",
        "writev",
        "recvfrom",
        "recvmsg",
        "sendmsg",
        "sendto",
        "accept4",
        "getsockname",
        "getsockopt",
        "setsockopt",
        "shutdown",
        "close",
        "poll",
        "clock_gettime",
        "getrandom",
        "rt_sigaction",
        "rt_sigprocmask",
        "rt_sigreturn",
        "sigaltstack",
        "rseq",
        "set_robust_list",
        "gettid",
        "sched_getaffinity",
        "exit",
        "exit_group",
    ];
    let beyond = ["getpeername", "getpid", "restart_syscall"];
    let names =
        |calls: &[Call]| -> BTreeSet<&str> { calls.iter().map(|(name, _)| *name).collect() };
    let expected: BTreeSet<&str> = design.into_iter().chain(beyond).collect();
    assert_eq!(names(X86_64.allowed), expected);
    // aarch64 has no `poll` and no `epoll_wait`: glibc makes these two instead.
    let aarch64: BTreeSet<&str> = expected
        .iter()
        .map(|name| match *name {
            "poll" => "ppoll",
            "epoll_wait" => "epoll_pwait",
            other => other,
        })
        .collect();
    assert_eq!(names(AARCH64.allowed), aarch64);
}

/// Calls the design names as killed, and others a confined process must not make,
/// with their numbers (`syscall_64.tbl`; `asm-generic/unistd.h`).
const KILLED_X86_64: &[Call] = &[
    ("open", 2),
    ("stat", 4),
    ("fstat", 5),
    ("pipe", 22),
    ("mremap", 25),
    ("dup", 32),
    ("dup2", 33),
    ("sendfile", 40),
    ("socket", 41),
    ("connect", 42),
    ("accept", 43),
    ("bind", 49),
    ("listen", 50),
    ("socketpair", 53),
    ("fork", 57),
    ("vfork", 58),
    ("execve", 59),
    ("kill", 62),
    ("truncate", 76),
    ("ftruncate", 77),
    ("chdir", 80),
    ("rename", 82),
    ("mkdir", 83),
    ("creat", 85),
    ("link", 86),
    ("unlink", 87),
    ("symlink", 88),
    ("chmod", 90),
    ("fchmod", 91),
    ("chown", 92),
    ("ptrace", 101),
    ("setuid", 105),
    ("rt_sigqueueinfo", 129),
    ("personality", 135),
    ("chroot", 161),
    ("mount", 165),
    ("init_module", 175),
    ("tkill", 200),
    ("getdents64", 217),
    ("add_key", 248),
    ("keyctl", 250),
    ("openat", 257),
    ("newfstatat", 262),
    ("unlinkat", 263),
    ("renameat", 264),
    ("unshare", 272),
    ("epoll_pwait", 281),
    ("dup3", 292),
    ("pipe2", 293),
    ("rt_tgsigqueueinfo", 297),
    ("perf_event_open", 298),
    ("setns", 308),
    ("process_vm_readv", 310),
    ("process_vm_writev", 311),
    ("finit_module", 313),
    ("renameat2", 316),
    ("seccomp", 317),
    ("memfd_create", 319),
    ("bpf", 321),
    ("execveat", 322),
    ("userfaultfd", 323),
    ("pkey_mprotect", 329),
    ("statx", 332),
    ("pidfd_send_signal", 424),
    ("io_uring_setup", 425),
    ("pidfd_open", 434),
    ("openat2", 437),
    ("landlock_create_ruleset", 444),
    ("landlock_add_rule", 445),
    ("landlock_restrict_self", 446),
];

const KILLED_AARCH64: &[Call] = &[
    ("dup", 23),
    ("dup3", 24),
    ("mkdirat", 34),
    ("unlinkat", 35),
    ("symlinkat", 36),
    ("linkat", 37),
    ("renameat", 38),
    ("mount", 40),
    ("truncate", 45),
    ("ftruncate", 46),
    ("chdir", 49),
    ("chroot", 51),
    ("fchmod", 52),
    ("fchmodat", 53),
    ("fchownat", 54),
    ("openat", 56),
    ("pipe2", 59),
    ("getdents64", 61),
    ("sendfile", 71),
    ("newfstatat", 79),
    ("fstat", 80),
    ("personality", 92),
    ("unshare", 97),
    ("init_module", 105),
    ("ptrace", 117),
    ("kill", 129),
    ("tkill", 130),
    ("rt_sigqueueinfo", 138),
    ("setuid", 146),
    ("socket", 198),
    ("socketpair", 199),
    ("bind", 200),
    ("listen", 201),
    ("accept", 202),
    ("connect", 203),
    ("mremap", 216),
    ("add_key", 217),
    ("keyctl", 219),
    ("execve", 221),
    ("rt_tgsigqueueinfo", 240),
    ("perf_event_open", 241),
    ("setns", 268),
    ("process_vm_readv", 270),
    ("process_vm_writev", 271),
    ("finit_module", 273),
    ("renameat2", 276),
    ("seccomp", 277),
    ("memfd_create", 279),
    ("bpf", 280),
    ("execveat", 281),
    ("userfaultfd", 282),
    ("pkey_mprotect", 288),
    ("statx", 291),
    ("pidfd_send_signal", 424),
    ("io_uring_setup", 425),
    ("pidfd_open", 434),
    ("openat2", 437),
    ("landlock_create_ruleset", 444),
    ("landlock_add_rule", 445),
    ("landlock_restrict_self", 446),
];

#[test]
fn the_calls_the_design_names_as_killed_are_killed() {
    let named = [
        "socket",
        "connect",
        "bind",
        "listen",
        "openat",
        "openat2",
        "execve",
        "execveat",
        "ptrace",
        "process_vm_readv",
        "process_vm_writev",
        "unshare",
        "setns",
        "mount",
        "bpf",
        "io_uring_setup",
        "memfd_create",
    ];
    for (name, arch, killed) in [
        ("x86_64", &X86_64, KILLED_X86_64),
        ("aarch64", &AARCH64, KILLED_AARCH64),
    ] {
        for call in named {
            assert!(
                killed.iter().any(|(listed, _)| *listed == call),
                "{name} {call}"
            );
        }
        for (call, number) in killed {
            for arguments in [NONE, [u64::MAX; 6], [3, 0x5421, 0, 0, 0, 0]] {
                assert_eq!(
                    decide(arch, *number, arguments),
                    RET_KILL_PROCESS,
                    "{name} {call}"
                );
            }
        }
    }
    // x86-64 alone has the older calls that take a path.
    for call in ["open", "creat", "rename", "fork", "vfork"] {
        assert!(KILLED_X86_64.iter().any(|(listed, _)| *listed == call));
    }
}

#[test]
fn clone3_is_answered_with_enosys_whatever_its_arguments() {
    for (_, arch) in tables() {
        for arguments in [NONE, [u64::MAX; 6], [0x003d_0f00, 0, 0, 0, 0, 0]] {
            assert_eq!(decide(arch, arch.clone3, arguments), ERRNO_ENOSYS);
        }
    }
    assert_eq!(CLONE3_ERRNO, 38);
}

#[test]
fn clone_is_allowed_only_with_the_flags_of_a_thread() {
    let thread = (libc::CLONE_VM
        | libc::CLONE_FS
        | libc::CLONE_FILES
        | libc::CLONE_SIGHAND
        | libc::CLONE_THREAD
        | libc::CLONE_SYSVSEM
        | libc::CLONE_SETTLS
        | libc::CLONE_PARENT_SETTID
        | libc::CLONE_CHILD_CLEARTID) as u64;
    assert_eq!(thread, 0x003d_0f00);
    for (name, arch) in tables() {
        let flags = |flags: u64| decide(arch, arch.clone, [flags, 0x7000_0000, 1, 2, 3, 0]);
        assert_eq!(flags(thread), RET_ALLOW, "{name}");
        for other in [
            0,
            libc::SIGCHLD as u64,
            // `fork` and `vfork` as glibc makes them.
            (libc::CLONE_CHILD_SETTID | libc::CLONE_CHILD_CLEARTID | libc::SIGCHLD) as u64,
            (libc::CLONE_VM | libc::CLONE_VFORK | libc::SIGCHLD) as u64,
            // A thread flag missing, and each namespace flag added.
            thread & !(libc::CLONE_THREAD as u64),
            thread & !(libc::CLONE_FILES as u64),
            thread | libc::CLONE_NEWUSER as u64,
            thread | libc::CLONE_NEWNS as u64,
            thread | libc::CLONE_NEWNET as u64,
            thread | libc::CLONE_NEWPID as u64,
            thread | libc::CLONE_PIDFD as u64,
            thread | libc::CLONE_PTRACE as u64,
            thread | libc::CLONE_UNTRACED as u64,
            // The same low word with a high word.
            thread | 1 << 32,
            thread | 1 << 63,
            u64::MAX,
        ] {
            assert_eq!(flags(other), RET_KILL_PROCESS, "{name} {other:#x}");
        }
    }
}

#[test]
fn prctl_is_allowed_only_to_name_a_thread() {
    assert_eq!(libc::PR_SET_NAME, 15);
    for (name, arch) in tables() {
        let option = |option: u64| decide(arch, arch.prctl, [option, 0x7000_0000, 0, 0, 0, 0]);
        assert_eq!(option(libc::PR_SET_NAME as u64), RET_ALLOW, "{name}");
        // The kernel reads the option as a 32-bit integer.
        assert_eq!(option(libc::PR_SET_NAME as u64 | 7 << 32), RET_ALLOW);
        for other in [
            0,
            libc::PR_SET_DUMPABLE,
            libc::PR_GET_DUMPABLE,
            libc::PR_GET_NAME,
            libc::PR_SET_SECCOMP,
            libc::PR_GET_SECCOMP,
            libc::PR_SET_NO_NEW_PRIVS,
            libc::PR_SET_MM,
            libc::PR_CAP_AMBIENT,
            libc::PR_SET_PTRACER,
            libc::PR_SET_PDEATHSIG,
            libc::PR_SET_KEEPCAPS,
            64,
            -1,
        ] {
            assert_eq!(option(other as u64), RET_KILL_PROCESS, "{name} {other}");
        }
    }
}

#[test]
fn ioctl_is_allowed_only_for_the_two_socket_requests() {
    assert_eq!((libc::FIONBIO, libc::FIONREAD), (0x5421, 0x541B));
    for (name, arch) in tables() {
        let request = |request: u64| decide(arch, arch.ioctl, [5, request, 0x7000_0000, 0, 0, 0]);
        assert_eq!(request(libc::FIONBIO), RET_ALLOW, "{name}");
        assert_eq!(request(libc::FIONREAD), RET_ALLOW, "{name}");
        for other in [
            0,
            libc::TIOCSTI,
            libc::TCGETS,
            libc::TIOCGWINSZ,
            libc::FIOCLEX,
            libc::FIONCLEX,
            libc::FIOASYNC,
            libc::SIOCGIFCONF,
            libc::TIOCSCTTY,
            u64::from(u32::MAX),
        ] {
            assert_eq!(request(other), RET_KILL_PROCESS, "{name} {other:#x}");
        }
        // The request is the second argument, not the first.
        assert_eq!(
            decide(arch, arch.ioctl, [libc::FIONBIO, 0, 0, 0, 0, 0]),
            RET_KILL_PROCESS
        );
    }
}

#[test]
fn fcntl_is_allowed_only_for_the_three_commands_the_runtime_makes() {
    assert_eq!(
        (libc::F_GETFD, libc::F_GETFL, libc::F_DUPFD_CLOEXEC),
        (1, 3, 1030)
    );
    for (name, arch) in tables() {
        let command = |command: i32| decide(arch, arch.fcntl, [5, command as u64, 0x800, 0, 0, 0]);
        for allowed in [libc::F_GETFD, libc::F_GETFL, libc::F_DUPFD_CLOEXEC] {
            assert_eq!(command(allowed), RET_ALLOW, "{name} {allowed}");
        }
        for other in [
            libc::F_DUPFD,
            libc::F_SETFD,
            libc::F_SETFL,
            libc::F_SETOWN,
            libc::F_GETOWN,
            // F_SETSIG and F_SETOWN_EX (`include/uapi/asm-generic/fcntl.h`).
            10,
            15,
            libc::F_SETLK,
            libc::F_SETLEASE,
            libc::F_NOTIFY,
            libc::F_SETPIPE_SZ,
            libc::F_ADD_SEALS,
            -1,
        ] {
            assert_eq!(command(other), RET_KILL_PROCESS, "{name} {other}");
        }
        // The command is the second argument, not the first.
        assert_eq!(
            decide(arch, arch.fcntl, [libc::F_GETFD as u64, 8, 0, 0, 0, 0]),
            RET_KILL_PROCESS
        );
    }
}

#[test]
fn no_memory_is_mapped_or_made_executable() {
    assert_eq!(libc::PROT_EXEC, 4);
    for (name, arch) in tables() {
        for number in [arch.mmap, arch.mprotect] {
            let protection = |protection: i32| {
                decide(
                    arch,
                    number,
                    [0x7000_0000, 4096, protection as u64, 0x22, u64::MAX, 0],
                )
            };
            for allowed in [
                libc::PROT_NONE,
                libc::PROT_READ,
                libc::PROT_WRITE,
                libc::PROT_READ | libc::PROT_WRITE,
            ] {
                assert_eq!(protection(allowed), RET_ALLOW, "{name} {allowed}");
            }
            for executable in [
                libc::PROT_EXEC,
                libc::PROT_READ | libc::PROT_EXEC,
                libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
                -1,
            ] {
                assert_eq!(
                    protection(executable),
                    RET_KILL_PROCESS,
                    "{name} {executable}"
                );
            }
        }
    }
}

#[test]
fn a_signal_goes_only_to_the_processs_own_threads() {
    for (name, arch) in tables() {
        let target = |group: u64| {
            decide(
                arch,
                arch.tgkill,
                [group, 77, libc::SIGABRT as u64, 0, 0, 0],
            )
        };
        assert_eq!(target(u64::from(PID)), RET_ALLOW, "{name}");
        for other in [
            0,
            1,
            u64::from(PID) + 1,
            u64::from(PID) - 1,
            u64::from(u32::MAX),
        ] {
            assert_eq!(target(other), RET_KILL_PROCESS, "{name} {other}");
        }
        // The filter is built for one process: another's id is not its own.
        let program = program(arch, PID + 1);
        let entry = Entry {
            number: arch.tgkill,
            arch: arch.audit,
            arguments: [u64::from(PID), 77, 6, 0, 0, 0],
        };
        assert_eq!(run(&program, &entry), RET_KILL_PROCESS, "{name}");
    }
}

#[test]
fn every_jump_lands_inside_the_program() {
    for (name, arch) in tables() {
        let program = program(arch, PID);
        for (at, instruction) in program.iter().enumerate() {
            match instruction.code {
                JUMP_EQUAL | JUMP_AT_LEAST | JUMP_ANY_BIT => {
                    for offset in [instruction.jt, instruction.jf] {
                        assert!(at + 1 + usize::from(offset) < program.len(), "{name} {at}");
                    }
                }
                LOAD | RETURN => (),
                other => panic!("{name}: an unknown instruction {other:#x} at {at}"),
            }
        }
    }
}

#[test]
fn the_running_architecture_has_a_list() {
    let native = super::super::syscalls::native().expect("x86_64 and aarch64 have a list");
    let name = tables()
        .into_iter()
        .find(|(_, arch)| arch.audit == native.audit)
        .map(|(name, _)| name);
    assert_eq!(name, Some(std::env::consts::ARCH));
}

#[test]
fn this_kernel_can_kill_a_process_from_a_filter() {
    assert_eq!(kill_process_available(), Ok(()));
}

// The numbers of the killed calls are the C library's, checked when this
// architecture's tests are built.
#[cfg(target_arch = "x86_64")]
const _: () = {
    let killed: &[(&str, libc::c_long)] = &[
        ("open", libc::SYS_open),
        ("stat", libc::SYS_stat),
        ("fstat", libc::SYS_fstat),
        ("pipe", libc::SYS_pipe),
        ("mremap", libc::SYS_mremap),
        ("dup", libc::SYS_dup),
        ("dup2", libc::SYS_dup2),
        ("sendfile", libc::SYS_sendfile),
        ("socket", libc::SYS_socket),
        ("connect", libc::SYS_connect),
        ("accept", libc::SYS_accept),
        ("bind", libc::SYS_bind),
        ("listen", libc::SYS_listen),
        ("socketpair", libc::SYS_socketpair),
        ("fork", libc::SYS_fork),
        ("vfork", libc::SYS_vfork),
        ("execve", libc::SYS_execve),
        ("kill", libc::SYS_kill),
        ("truncate", libc::SYS_truncate),
        ("ftruncate", libc::SYS_ftruncate),
        ("chdir", libc::SYS_chdir),
        ("rename", libc::SYS_rename),
        ("mkdir", libc::SYS_mkdir),
        ("creat", libc::SYS_creat),
        ("link", libc::SYS_link),
        ("unlink", libc::SYS_unlink),
        ("symlink", libc::SYS_symlink),
        ("chmod", libc::SYS_chmod),
        ("fchmod", libc::SYS_fchmod),
        ("chown", libc::SYS_chown),
        ("ptrace", libc::SYS_ptrace),
        ("setuid", libc::SYS_setuid),
        ("rt_sigqueueinfo", libc::SYS_rt_sigqueueinfo),
        ("personality", libc::SYS_personality),
        ("chroot", libc::SYS_chroot),
        ("mount", libc::SYS_mount),
        ("init_module", libc::SYS_init_module),
        ("tkill", libc::SYS_tkill),
        ("getdents64", libc::SYS_getdents64),
        ("add_key", libc::SYS_add_key),
        ("keyctl", libc::SYS_keyctl),
        ("openat", libc::SYS_openat),
        ("newfstatat", libc::SYS_newfstatat),
        ("unlinkat", libc::SYS_unlinkat),
        ("renameat", libc::SYS_renameat),
        ("unshare", libc::SYS_unshare),
        ("epoll_pwait", libc::SYS_epoll_pwait),
        ("dup3", libc::SYS_dup3),
        ("pipe2", libc::SYS_pipe2),
        ("rt_tgsigqueueinfo", libc::SYS_rt_tgsigqueueinfo),
        ("perf_event_open", libc::SYS_perf_event_open),
        ("setns", libc::SYS_setns),
        ("process_vm_readv", libc::SYS_process_vm_readv),
        ("process_vm_writev", libc::SYS_process_vm_writev),
        ("finit_module", libc::SYS_finit_module),
        ("renameat2", libc::SYS_renameat2),
        ("seccomp", libc::SYS_seccomp),
        ("memfd_create", libc::SYS_memfd_create),
        ("bpf", libc::SYS_bpf),
        ("execveat", libc::SYS_execveat),
        ("userfaultfd", libc::SYS_userfaultfd),
        ("pkey_mprotect", libc::SYS_pkey_mprotect),
        ("statx", libc::SYS_statx),
        ("pidfd_send_signal", libc::SYS_pidfd_send_signal),
        ("io_uring_setup", libc::SYS_io_uring_setup),
        ("pidfd_open", libc::SYS_pidfd_open),
        ("openat2", libc::SYS_openat2),
        ("landlock_create_ruleset", libc::SYS_landlock_create_ruleset),
        ("landlock_add_rule", libc::SYS_landlock_add_rule),
        ("landlock_restrict_self", libc::SYS_landlock_restrict_self),
    ];
    same_numbers(KILLED_X86_64, killed);
};

#[cfg(target_arch = "aarch64")]
const _: () = {
    let killed: &[(&str, libc::c_long)] = &[
        ("dup", libc::SYS_dup),
        ("dup3", libc::SYS_dup3),
        ("mkdirat", libc::SYS_mkdirat),
        ("unlinkat", libc::SYS_unlinkat),
        ("symlinkat", libc::SYS_symlinkat),
        ("linkat", libc::SYS_linkat),
        ("renameat", libc::SYS_renameat),
        ("mount", libc::SYS_mount),
        ("truncate", libc::SYS_truncate),
        ("ftruncate", libc::SYS_ftruncate),
        ("chdir", libc::SYS_chdir),
        ("chroot", libc::SYS_chroot),
        ("fchmod", libc::SYS_fchmod),
        ("fchmodat", libc::SYS_fchmodat),
        ("fchownat", libc::SYS_fchownat),
        ("openat", libc::SYS_openat),
        ("pipe2", libc::SYS_pipe2),
        ("getdents64", libc::SYS_getdents64),
        ("sendfile", libc::SYS_sendfile),
        ("newfstatat", libc::SYS_newfstatat),
        ("fstat", libc::SYS_fstat),
        ("personality", libc::SYS_personality),
        ("unshare", libc::SYS_unshare),
        ("init_module", libc::SYS_init_module),
        ("ptrace", libc::SYS_ptrace),
        ("kill", libc::SYS_kill),
        ("tkill", libc::SYS_tkill),
        ("rt_sigqueueinfo", libc::SYS_rt_sigqueueinfo),
        ("setuid", libc::SYS_setuid),
        ("socket", libc::SYS_socket),
        ("socketpair", libc::SYS_socketpair),
        ("bind", libc::SYS_bind),
        ("listen", libc::SYS_listen),
        ("accept", libc::SYS_accept),
        ("connect", libc::SYS_connect),
        ("mremap", libc::SYS_mremap),
        ("add_key", libc::SYS_add_key),
        ("keyctl", libc::SYS_keyctl),
        ("execve", libc::SYS_execve),
        ("rt_tgsigqueueinfo", libc::SYS_rt_tgsigqueueinfo),
        ("perf_event_open", libc::SYS_perf_event_open),
        ("setns", libc::SYS_setns),
        ("process_vm_readv", libc::SYS_process_vm_readv),
        ("process_vm_writev", libc::SYS_process_vm_writev),
        ("finit_module", libc::SYS_finit_module),
        ("renameat2", libc::SYS_renameat2),
        ("seccomp", libc::SYS_seccomp),
        ("memfd_create", libc::SYS_memfd_create),
        ("bpf", libc::SYS_bpf),
        ("execveat", libc::SYS_execveat),
        ("userfaultfd", libc::SYS_userfaultfd),
        ("pkey_mprotect", libc::SYS_pkey_mprotect),
        ("statx", libc::SYS_statx),
        ("pidfd_send_signal", libc::SYS_pidfd_send_signal),
        ("io_uring_setup", libc::SYS_io_uring_setup),
        ("pidfd_open", libc::SYS_pidfd_open),
        ("openat2", libc::SYS_openat2),
        ("landlock_create_ruleset", libc::SYS_landlock_create_ruleset),
        ("landlock_add_rule", libc::SYS_landlock_add_rule),
        ("landlock_restrict_self", libc::SYS_landlock_restrict_self),
    ];
    same_numbers(KILLED_AARCH64, killed);
};

/// Every entry of `table` is in `library` with the same number.
const fn same_numbers(table: &[Call], library: &[(&str, libc::c_long)]) {
    assert!(table.len() == library.len());
    let mut index = 0;
    while index < library.len() {
        assert!(
            super::super::syscalls::number(table, library[index].0) as libc::c_long
                == library[index].1
        );
        index += 1;
    }
}
