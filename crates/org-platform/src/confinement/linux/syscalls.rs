//! The system calls a confined web process may make, per processor architecture
//! (Linux host design, section 3.3, step 5). This list is a security boundary: a call
//! that is not here kills the process.
//!
//! Both tables are compiled on every architecture, with the kernel's numbers written
//! out (`arch/x86/entry/syscalls/syscall_64.tbl` and
//! `include/uapi/asm-generic/unistd.h`), so that the filter of each can be tested
//! anywhere. A test compares the running architecture's table with the C library's
//! `SYS_*` constants.
//!
//! The list is the design's, read narrowly: of `epoll_*` only the calls the runtime
//! makes, of `rt_sig*` only the three that act on the process itself, and of the
//! clocks only reading one. Three calls are here that the design's list does not
//! name; each says why at its entry. Four of the design's calls are decided by an
//! argument although the design does not ask for it: `clone` by its exact flags,
//! `mmap` and `mprotect` by their protection, and `fcntl` by its command.
//!
//! The aarch64 table is the x86-64 table under one rule: the same C library function
//! reaches the same kernel operation. aarch64 has no `poll`, `epoll_wait`, `open` or
//! `dup2`; glibc makes `ppoll` and `epoll_pwait` there instead. It was derived, not
//! traced: no aarch64 system runs these tests yet.

/// One allowed system call: its name and its number on one architecture.
pub(super) type Call = (&'static str, u32);

/// The filter's data for one architecture.
#[derive(Debug)]
pub(super) struct Arch {
    /// `AUDIT_ARCH_*` (`include/uapi/linux/audit.h`): what `seccomp_data.arch` holds
    /// for a native system call.
    pub(super) audit: u32,
    /// System call numbers from this value up belong to another ABI of the same
    /// architecture (x32 on x86-64, `__X32_SYSCALL_BIT`), and kill.
    pub(super) foreign_from: Option<u32>,
    /// Allowed whatever their arguments.
    pub(super) allowed: &'static [Call],
    /// Allowed only for a thread of this process (the flags glibc's `pthread_create`
    /// passes).
    pub(super) clone: u32,
    /// Answered with `ENOSYS`, so that glibc falls back to `clone`, whose flags the
    /// filter can read; `clone3` passes them in memory.
    pub(super) clone3: u32,
    /// Allowed only for the options in [`PRCTL_OPTIONS`].
    pub(super) prctl: u32,
    /// Allowed only for the requests in [`IOCTL_REQUESTS`].
    pub(super) ioctl: u32,
    /// Allowed only for the commands in [`FCNTL_COMMANDS`].
    pub(super) fcntl: u32,
    /// Allowed only without `PROT_EXEC`.
    pub(super) mmap: u32,
    /// Allowed only without `PROT_EXEC`.
    pub(super) mprotect: u32,
    /// Allowed only towards this process's own threads ("signals to itself").
    pub(super) tgkill: u32,
}

/// `PR_SET_NAME` (`include/uapi/linux/prctl.h`): a new thread names itself. The
/// design also names `PR_GET_*`; no trace of the web process shows one, so none is
/// allowed.
pub(super) const PRCTL_OPTIONS: &[u32] = &[15];

/// `FIONBIO` and `FIONREAD` (`include/uapi/asm-generic/ioctls.h`; the same values on
/// x86-64 and aarch64): make a descriptor non-blocking, and count its unread bytes.
pub(super) const IOCTL_REQUESTS: &[u32] = &[0x5421, 0x541B];

/// `F_GETFD`, `F_GETFL` and `F_DUPFD_CLOEXEC` (`include/uapi/asm-generic/fcntl.h`,
/// `include/uapi/linux/fcntl.h`; the same values on x86-64 and aarch64): the three
/// commands the web process makes. Every other command kills. With `F_SETOWN`,
/// `F_SETSIG` and `O_ASYNC` set by `F_SETFL`, a process makes the kernel send a
/// signal of its choice to any process of its user when input arrives on a socket it
/// holds; Landlock stops that only from ABI 6.
pub(super) const FCNTL_COMMANDS: &[u32] = &[1, 3, 1030];

/// The flags of glibc's `pthread_create` (`nptl/pthread_create.c`): `CLONE_VM |
/// CLONE_FS | CLONE_FILES | CLONE_SIGHAND | CLONE_THREAD | CLONE_SYSVSEM |
/// CLONE_SETTLS | CLONE_PARENT_SETTID | CLONE_CHILD_CLEARTID`. The design asks for
/// `CLONE_THREAD` in the flags; the filter asks for exactly these, so a `clone` with
/// any other flag, a new process or a new namespace among them, kills.
pub(super) const THREAD_CLONE_FLAGS: u64 = 0x003d_0f00;

/// `PROT_EXEC`: no memory becomes executable after confinement. The runtime never
/// asks for it: thread stacks and the allocator map memory readable and writable.
pub(super) const PROT_EXEC: u32 = 4;

/// `AUDIT_ARCH_X86_64`: `EM_X86_64 | __AUDIT_ARCH_64BIT | __AUDIT_ARCH_LE`.
const AUDIT_ARCH_X86_64: u32 = 0xC000_003E;
/// `AUDIT_ARCH_AARCH64`: `EM_AARCH64 | __AUDIT_ARCH_64BIT | __AUDIT_ARCH_LE`.
const AUDIT_ARCH_AARCH64: u32 = 0xC000_00B7;
/// `__X32_SYSCALL_BIT` (`arch/x86/include/uapi/asm/unistd.h`).
const X32_SYSCALL_BIT: u32 = 0x4000_0000;

pub(super) const X86_64: Arch = Arch {
    audit: AUDIT_ARCH_X86_64,
    foreign_from: Some(X32_SYSCALL_BIT),
    allowed: &[
        // Descriptors the process already holds.
        ("read", 0),
        ("write", 1),
        ("close", 3),
        ("poll", 7),
        ("writev", 20),
        // Sockets it already holds: no `socket`, `connect`, `bind` or `listen`.
        ("sendto", 44),
        ("recvfrom", 45),
        ("sendmsg", 46),
        ("recvmsg", 47),
        ("shutdown", 48),
        ("getsockname", 51),
        // Not in the design's list. The web process checks that its control
        // descriptor is a connected Unix stream by asking for its peer's address
        // (`org_socket::rights::connected_unix_stream`), after confinement.
        ("getpeername", 52),
        ("setsockopt", 54),
        ("getsockopt", 55),
        ("accept4", 288),
        // The runtime's event loop.
        ("epoll_wait", 232),
        ("epoll_ctl", 233),
        ("eventfd2", 290),
        ("epoll_create1", 291),
        // Memory; `mmap` and `mprotect` are decided by their protection.
        ("munmap", 11),
        ("brk", 12),
        ("madvise", 28),
        // Threads and their synchronization; `clone` is decided by its flags.
        ("futex", 202),
        ("sched_getaffinity", 204),
        ("set_robust_list", 273),
        ("rseq", 334),
        ("gettid", 186),
        ("exit", 60),
        ("exit_group", 231),
        // Signals inside the process; `tgkill` is decided by its target.
        ("rt_sigaction", 13),
        ("rt_sigprocmask", 14),
        ("rt_sigreturn", 15),
        ("sigaltstack", 131),
        // Not in the design's list. glibc's `raise`, and so `abort`, asks for the
        // process id before `tgkill`; without it an abort would die of the filter
        // instead of its own signal.
        ("getpid", 39),
        // Not in the design's list. The kernel makes this call itself to resume a
        // timed wait (`poll`, a `futex` with a timeout) that a stop signal
        // interrupted; without it a stopped and continued process dies.
        ("restart_syscall", 219),
        // Clocks and randomness.
        ("clock_gettime", 228),
        ("getrandom", 318),
    ],
    clone: 56,
    clone3: 435,
    prctl: 157,
    ioctl: 16,
    fcntl: 72,
    mmap: 9,
    mprotect: 10,
    tgkill: 234,
};

pub(super) const AARCH64: Arch = Arch {
    audit: AUDIT_ARCH_AARCH64,
    foreign_from: None,
    allowed: &[
        ("read", 63),
        ("write", 64),
        ("close", 57),
        // glibc's `poll` is `ppoll` here.
        ("ppoll", 73),
        ("writev", 66),
        ("sendto", 206),
        ("recvfrom", 207),
        ("sendmsg", 211),
        ("recvmsg", 212),
        ("shutdown", 210),
        ("getsockname", 204),
        ("getpeername", 205),
        ("setsockopt", 208),
        ("getsockopt", 209),
        ("accept4", 242),
        // glibc's `epoll_wait` is `epoll_pwait` here.
        ("epoll_pwait", 22),
        ("epoll_ctl", 21),
        ("eventfd2", 19),
        ("epoll_create1", 20),
        ("munmap", 215),
        ("brk", 214),
        ("madvise", 233),
        ("futex", 98),
        ("sched_getaffinity", 123),
        ("set_robust_list", 99),
        ("rseq", 293),
        ("gettid", 178),
        ("exit", 93),
        ("exit_group", 94),
        ("rt_sigaction", 134),
        ("rt_sigprocmask", 135),
        ("rt_sigreturn", 139),
        ("sigaltstack", 132),
        ("getpid", 172),
        ("restart_syscall", 128),
        ("clock_gettime", 113),
        ("getrandom", 278),
    ],
    clone: 220,
    clone3: 435,
    prctl: 167,
    ioctl: 29,
    fcntl: 25,
    mmap: 222,
    mprotect: 226,
    tgkill: 131,
};

/// The table of the architecture this build runs on, or `None` where the adapter has
/// no allowlist. Both tables are for little-endian systems with 64-bit pointers: the
/// filter reads each 64-bit argument as two 32-bit words, low word first.
pub(super) const fn native() -> Option<&'static Arch> {
    if !cfg!(all(target_endian = "little", target_pointer_width = "64")) {
        // Among others x32, whose calls the x86-64 filter kills.
        None
    } else if cfg!(target_arch = "x86_64") {
        Some(&X86_64)
    } else if cfg!(target_arch = "aarch64") {
        Some(&AARCH64)
    } else {
        None
    }
}

/// The number of `name` in `calls`. For the build-time checks below and the tests.
pub(super) const fn number(calls: &[Call], name: &str) -> u32 {
    let mut index = 0;
    while index < calls.len() {
        let (found, wanted) = (calls[index].0.as_bytes(), name.as_bytes());
        let mut same = found.len() == wanted.len();
        let mut byte = 0;
        while same && byte < found.len() {
            same = found[byte] == wanted[byte];
            byte += 1;
        }
        if same {
            return calls[index].1;
        }
        index += 1;
    }
    panic!("the system call is not in the table")
}

// The build fails when a number in the running architecture's table is not the C
// library's for that name, or when an entry of the table is left unchecked.
#[cfg(target_arch = "x86_64")]
const _: () = {
    const TABLE: &Arch = &X86_64;
    let allowed: &[(&str, libc::c_long)] = &[
        ("read", libc::SYS_read),
        ("write", libc::SYS_write),
        ("close", libc::SYS_close),
        ("poll", libc::SYS_poll),
        ("writev", libc::SYS_writev),
        ("sendto", libc::SYS_sendto),
        ("recvfrom", libc::SYS_recvfrom),
        ("sendmsg", libc::SYS_sendmsg),
        ("recvmsg", libc::SYS_recvmsg),
        ("shutdown", libc::SYS_shutdown),
        ("getsockname", libc::SYS_getsockname),
        ("getpeername", libc::SYS_getpeername),
        ("setsockopt", libc::SYS_setsockopt),
        ("getsockopt", libc::SYS_getsockopt),
        ("accept4", libc::SYS_accept4),
        ("epoll_wait", libc::SYS_epoll_wait),
        ("epoll_ctl", libc::SYS_epoll_ctl),
        ("eventfd2", libc::SYS_eventfd2),
        ("epoll_create1", libc::SYS_epoll_create1),
        ("munmap", libc::SYS_munmap),
        ("brk", libc::SYS_brk),
        ("madvise", libc::SYS_madvise),
        ("futex", libc::SYS_futex),
        ("sched_getaffinity", libc::SYS_sched_getaffinity),
        ("set_robust_list", libc::SYS_set_robust_list),
        ("rseq", libc::SYS_rseq),
        ("gettid", libc::SYS_gettid),
        ("exit", libc::SYS_exit),
        ("exit_group", libc::SYS_exit_group),
        ("rt_sigaction", libc::SYS_rt_sigaction),
        ("rt_sigprocmask", libc::SYS_rt_sigprocmask),
        ("rt_sigreturn", libc::SYS_rt_sigreturn),
        ("sigaltstack", libc::SYS_sigaltstack),
        ("getpid", libc::SYS_getpid),
        ("restart_syscall", libc::SYS_restart_syscall),
        ("clock_gettime", libc::SYS_clock_gettime),
        ("getrandom", libc::SYS_getrandom),
    ];
    assert!(allowed.len() == TABLE.allowed.len());
    let mut index = 0;
    while index < allowed.len() {
        assert!(number(TABLE.allowed, allowed[index].0) as libc::c_long == allowed[index].1);
        index += 1;
    }
    assert!(TABLE.clone as libc::c_long == libc::SYS_clone);
    assert!(TABLE.clone3 as libc::c_long == libc::SYS_clone3);
    assert!(TABLE.prctl as libc::c_long == libc::SYS_prctl);
    assert!(TABLE.ioctl as libc::c_long == libc::SYS_ioctl);
    assert!(TABLE.fcntl as libc::c_long == libc::SYS_fcntl);
    assert!(TABLE.mmap as libc::c_long == libc::SYS_mmap);
    assert!(TABLE.mprotect as libc::c_long == libc::SYS_mprotect);
    assert!(TABLE.tgkill as libc::c_long == libc::SYS_tgkill);
};
#[cfg(target_arch = "aarch64")]
const _: () = {
    const TABLE: &Arch = &AARCH64;
    let allowed: &[(&str, libc::c_long)] = &[
        ("read", libc::SYS_read),
        ("write", libc::SYS_write),
        ("close", libc::SYS_close),
        ("ppoll", libc::SYS_ppoll),
        ("writev", libc::SYS_writev),
        ("sendto", libc::SYS_sendto),
        ("recvfrom", libc::SYS_recvfrom),
        ("sendmsg", libc::SYS_sendmsg),
        ("recvmsg", libc::SYS_recvmsg),
        ("shutdown", libc::SYS_shutdown),
        ("getsockname", libc::SYS_getsockname),
        ("getpeername", libc::SYS_getpeername),
        ("setsockopt", libc::SYS_setsockopt),
        ("getsockopt", libc::SYS_getsockopt),
        ("accept4", libc::SYS_accept4),
        ("epoll_pwait", libc::SYS_epoll_pwait),
        ("epoll_ctl", libc::SYS_epoll_ctl),
        ("eventfd2", libc::SYS_eventfd2),
        ("epoll_create1", libc::SYS_epoll_create1),
        ("munmap", libc::SYS_munmap),
        ("brk", libc::SYS_brk),
        ("madvise", libc::SYS_madvise),
        ("futex", libc::SYS_futex),
        ("sched_getaffinity", libc::SYS_sched_getaffinity),
        ("set_robust_list", libc::SYS_set_robust_list),
        ("rseq", libc::SYS_rseq),
        ("gettid", libc::SYS_gettid),
        ("exit", libc::SYS_exit),
        ("exit_group", libc::SYS_exit_group),
        ("rt_sigaction", libc::SYS_rt_sigaction),
        ("rt_sigprocmask", libc::SYS_rt_sigprocmask),
        ("rt_sigreturn", libc::SYS_rt_sigreturn),
        ("sigaltstack", libc::SYS_sigaltstack),
        ("getpid", libc::SYS_getpid),
        ("restart_syscall", libc::SYS_restart_syscall),
        ("clock_gettime", libc::SYS_clock_gettime),
        ("getrandom", libc::SYS_getrandom),
    ];
    assert!(allowed.len() == TABLE.allowed.len());
    let mut index = 0;
    while index < allowed.len() {
        assert!(number(TABLE.allowed, allowed[index].0) as libc::c_long == allowed[index].1);
        index += 1;
    }
    assert!(TABLE.clone as libc::c_long == libc::SYS_clone);
    assert!(TABLE.clone3 as libc::c_long == libc::SYS_clone3);
    assert!(TABLE.prctl as libc::c_long == libc::SYS_prctl);
    assert!(TABLE.ioctl as libc::c_long == libc::SYS_ioctl);
    assert!(TABLE.fcntl as libc::c_long == libc::SYS_fcntl);
    assert!(TABLE.mmap as libc::c_long == libc::SYS_mmap);
    assert!(TABLE.mprotect as libc::c_long == libc::SYS_mprotect);
    assert!(TABLE.tgkill as libc::c_long == libc::SYS_tgkill);
};
