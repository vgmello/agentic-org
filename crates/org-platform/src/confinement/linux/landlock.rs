//! Step 4 of the Linux adapter: Landlock, where the kernel offers it (Linux host
//! design, section 3.3). It is a second layer under the seccomp filter, which alone
//! holds every guarantee: a kernel without Landlock, or a profile that refuses its
//! calls, is not a reason to refuse, and [`apply`] then says why it did nothing.
//!
//! A ruleset that handles every access right the running kernel's Landlock ABI
//! knows, with no rule: nothing it handles is allowed. The constants and the
//! structure are the kernel's `include/uapi/linux/landlock.h`; this crate links the
//! C library only, so they are written out here and the three system calls are made
//! through `libc::syscall`.

use std::fmt;
use std::io;
use std::mem;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::ptr;

// `LANDLOCK_ACCESS_FS_*`, `include/uapi/linux/landlock.h`.
pub(super) const ACCESS_FS_EXECUTE: u64 = 1 << 0;
pub(super) const ACCESS_FS_WRITE_FILE: u64 = 1 << 1;
pub(super) const ACCESS_FS_READ_FILE: u64 = 1 << 2;
pub(super) const ACCESS_FS_READ_DIR: u64 = 1 << 3;
pub(super) const ACCESS_FS_REMOVE_DIR: u64 = 1 << 4;
pub(super) const ACCESS_FS_REMOVE_FILE: u64 = 1 << 5;
pub(super) const ACCESS_FS_MAKE_CHAR: u64 = 1 << 6;
pub(super) const ACCESS_FS_MAKE_DIR: u64 = 1 << 7;
pub(super) const ACCESS_FS_MAKE_REG: u64 = 1 << 8;
pub(super) const ACCESS_FS_MAKE_SOCK: u64 = 1 << 9;
pub(super) const ACCESS_FS_MAKE_FIFO: u64 = 1 << 10;
pub(super) const ACCESS_FS_MAKE_BLOCK: u64 = 1 << 11;
pub(super) const ACCESS_FS_MAKE_SYM: u64 = 1 << 12;
/// ABI 2, Linux 5.19.
pub(super) const ACCESS_FS_REFER: u64 = 1 << 13;
/// ABI 3, Linux 6.2.
pub(super) const ACCESS_FS_TRUNCATE: u64 = 1 << 14;
/// ABI 5, Linux 6.10.
pub(super) const ACCESS_FS_IOCTL_DEV: u64 = 1 << 15;
// `LANDLOCK_ACCESS_NET_*`: ABI 4, Linux 6.7.
pub(super) const ACCESS_NET_BIND_TCP: u64 = 1 << 0;
pub(super) const ACCESS_NET_CONNECT_TCP: u64 = 1 << 1;
// `LANDLOCK_SCOPE_*`: ABI 6, Linux 6.12.
pub(super) const SCOPE_ABSTRACT_UNIX_SOCKET: u64 = 1 << 0;
pub(super) const SCOPE_SIGNAL: u64 = 1 << 1;

/// `LANDLOCK_CREATE_RULESET_VERSION`: with a null attribute and size 0, the call
/// returns the ABI version and creates nothing.
const CREATE_RULESET_VERSION: u32 = 1 << 0;

/// The first ABI there is (Linux 5.13). Each ABI is applied with the rights it
/// knows: ABI 1 and 2 do not cover truncating a file by path, which the seccomp
/// filter stops as it stops every other path operation.
pub(super) const MINIMUM_ABI: u32 = 1;
/// The newest ABI whose rights this table lists. ABI 7 (Linux 6.15) added logging
/// flags to `landlock_restrict_self` and no access right.
pub(super) const HIGHEST_KNOWN_ABI: u32 = 7;

/// `struct landlock_ruleset_attr`. The kernel reads as many bytes as the caller says
/// the structure has: 8 until ABI 3, 16 from ABI 4, 24 from ABI 6.
#[repr(C)]
struct RulesetAttr {
    handled_access_fs: u64,
    handled_access_net: u64,
    scoped: u64,
}
const _: () = {
    assert!(mem::size_of::<RulesetAttr>() == 24);
    assert!(mem::offset_of!(RulesetAttr, handled_access_fs) == 0);
    assert!(mem::offset_of!(RulesetAttr, handled_access_net) == 8);
    assert!(mem::offset_of!(RulesetAttr, scoped) == 16);
};

/// What a ruleset handles under one ABI, and how many bytes of the attribute that
/// ABI's kernel knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Handled {
    pub(super) fs: u64,
    pub(super) net: u64,
    pub(super) scoped: u64,
    pub(super) size: usize,
}

/// Every right the ABI knows, or `None` below [`MINIMUM_ABI`]. An ABI newer than
/// [`HIGHEST_KNOWN_ABI`] gets the rights of the highest known one, never fewer: a
/// newer kernel accepts them, and what it added is simply not handled.
pub(super) const fn handled(abi: u32) -> Option<Handled> {
    if abi < MINIMUM_ABI {
        return None;
    }
    // ABI 1, Linux 5.13.
    let mut fs = ACCESS_FS_EXECUTE
        | ACCESS_FS_WRITE_FILE
        | ACCESS_FS_READ_FILE
        | ACCESS_FS_READ_DIR
        | ACCESS_FS_REMOVE_DIR
        | ACCESS_FS_REMOVE_FILE
        | ACCESS_FS_MAKE_CHAR
        | ACCESS_FS_MAKE_DIR
        | ACCESS_FS_MAKE_REG
        | ACCESS_FS_MAKE_SOCK
        | ACCESS_FS_MAKE_FIFO
        | ACCESS_FS_MAKE_BLOCK
        | ACCESS_FS_MAKE_SYM;
    if abi >= 2 {
        fs |= ACCESS_FS_REFER;
    }
    if abi >= 3 {
        fs |= ACCESS_FS_TRUNCATE;
    }
    let (mut net, mut scoped, mut size) = (0, 0, 8);
    if abi >= 4 {
        net = ACCESS_NET_BIND_TCP | ACCESS_NET_CONNECT_TCP;
        size = 16;
    }
    if abi >= 5 {
        fs |= ACCESS_FS_IOCTL_DEV;
    }
    if abi >= 6 {
        scoped = SCOPE_ABSTRACT_UNIX_SOCKET | SCOPE_SIGNAL;
        size = 24;
    }
    Some(Handled {
        fs,
        net,
        scoped,
        size,
    })
}

/// The ABI the adapter enforces in full, and reports, on a kernel with ABI `kernel`.
pub(super) const fn enforced_abi(kernel: u32) -> u32 {
    if kernel > HIGHEST_KNOWN_ABI {
        HIGHEST_KNOWN_ABI
    } else {
        kernel
    }
}

/// Why Landlock cannot confine a process here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Unavailable {
    /// `ENOSYS`: the kernel has no Landlock system calls (before Linux 5.13, or built
    /// without it).
    Missing,
    /// `EOPNOTSUPP`: Landlock is built in but is not among the enabled security
    /// modules.
    Disabled,
    /// The version query answered a number that is no ABI: zero or less.
    NoAbi(i64),
    /// Any other error of the version query, by its number: `EPERM` under a profile
    /// that refuses the call.
    Failed(i32),
}

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str(
                "landlock_create_ruleset failed with ENOSYS: this kernel has no Landlock",
            ),
            Self::Disabled => f.write_str(
                "landlock_create_ruleset failed with EOPNOTSUPP: Landlock is built into \
                 this kernel but is not an enabled security module",
            ),
            Self::NoAbi(abi) => write!(
                f,
                "the Landlock version query answered {abi}, which is no ABI"
            ),
            Self::Failed(errno) => write!(
                f,
                "landlock_create_ruleset failed with {}: the Landlock version query was \
                 refused",
                super::errno_text(*errno)
            ),
        }
    }
}

/// The decision over the version query's outcome: its return value, or its error
/// number. Pure.
pub(super) fn decide(query: Result<i64, i32>) -> Result<u32, Unavailable> {
    match query {
        Ok(abi) => u32::try_from(abi)
            .ok()
            .filter(|abi| *abi >= MINIMUM_ABI)
            .ok_or(Unavailable::NoAbi(abi)),
        Err(libc::ENOSYS) => Err(Unavailable::Missing),
        Err(libc::EOPNOTSUPP) => Err(Unavailable::Disabled),
        Err(errno) => Err(Unavailable::Failed(errno)),
    }
}

/// The running kernel's Landlock ABI, when it has one this process may use. The
/// query creates nothing and restricts nothing.
pub(super) fn abi() -> Result<u32, Unavailable> {
    // SAFETY: the version query takes a null attribute and size 0 and writes nowhere.
    let version = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            ptr::null::<RulesetAttr>(),
            0usize,
            CREATE_RULESET_VERSION,
        )
    };
    decide(if version < 0 {
        Err(io::Error::last_os_error().raw_os_error().unwrap_or(0))
    } else {
        Ok(version)
    })
}

/// Why no Landlock domain was applied. The process is then confined by the seccomp
/// filter alone, and this is what it tells its host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum NotApplied {
    /// The version query did not answer an ABI.
    Unavailable(Unavailable),
    /// `landlock_create_ruleset` or `landlock_restrict_self` failed, with this error
    /// number.
    Failed(&'static str, i32),
}

impl fmt::Display for NotApplied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(reason) => reason.fmt(f),
            Self::Failed(call, errno) => write!(
                f,
                "{call} failed with {} ({})",
                super::errno_text(*errno),
                io::Error::from_raw_os_error(*errno)
            ),
        }
    }
}

/// Restricts the calling thread, which must be the process's only one and must have
/// no-new-privileges set: every right `kernel`'s ABI knows is handled and no rule
/// allows anything. Returns the ABI enforced in full. A failure restricts nothing.
pub(super) fn restrict(kernel: u32) -> Result<u32, NotApplied> {
    let errno = || io::Error::last_os_error().raw_os_error().unwrap_or(0);
    let abi = enforced_abi(kernel);
    let handled = handled(abi).ok_or(NotApplied::Unavailable(Unavailable::NoAbi(i64::from(
        kernel,
    ))))?;
    let attr = RulesetAttr {
        handled_access_fs: handled.fs,
        handled_access_net: handled.net,
        scoped: handled.scoped,
    };
    // SAFETY: `attr` is a live, initialized structure of at least `handled.size`
    // bytes, which is the size this ABI's kernel reads; flags are zero.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            &attr as *const RulesetAttr,
            handled.size,
            0u32,
        )
    };
    if fd < 0 {
        return Err(NotApplied::Failed("landlock_create_ruleset", errno()));
    }
    // SAFETY: a successful landlock_create_ruleset returns a new descriptor that
    // nothing else owns; it is closed when this function returns.
    let ruleset = unsafe { OwnedFd::from_raw_fd(fd as i32) };
    // SAFETY: a live ruleset descriptor and zero flags; the call changes only the
    // calling thread's credentials.
    if unsafe { libc::syscall(libc::SYS_landlock_restrict_self, ruleset.as_raw_fd(), 0u32) } != 0 {
        return Err(NotApplied::Failed("landlock_restrict_self", errno()));
    }
    Ok(abi)
}

/// Applies a Landlock domain at the kernel's ABI, if the kernel has one and lets
/// this process use it. Any failure, of the version query or of either call, leaves
/// the process as it was and is returned as the reason.
pub(super) fn apply() -> Result<u32, NotApplied> {
    restrict(abi().map_err(NotApplied::Unavailable)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `LANDLOCK_ACCESS_FS_*` of ABI 1: bits 0 to 12.
    const ABI_1_FS: u64 = (1 << 13) - 1;

    #[test]
    fn each_abi_handles_every_right_the_kernel_added_up_to_it() {
        // include/uapi/linux/landlock.h: ABI 1 has bits 0 to 12 of the filesystem
        // rights; REFER is bit 13 (ABI 2), TRUNCATE bit 14 (ABI 3), IOCTL_DEV bit 15
        // (ABI 5); BIND_TCP and CONNECT_TCP are bits 0 and 1 of the network rights
        // (ABI 4); ABSTRACT_UNIX_SOCKET and SIGNAL bits 0 and 1 of the scopes (ABI 6).
        // ABI 7 adds no right.
        let abi_1 = Handled {
            fs: ABI_1_FS,
            net: 0,
            scoped: 0,
            size: 8,
        };
        let abi_2 = Handled {
            fs: ABI_1_FS | 1 << 13,
            ..abi_1
        };
        let abi_3 = Handled {
            fs: abi_2.fs | 1 << 14,
            ..abi_2
        };
        let abi_4 = Handled {
            net: 0b11,
            size: 16,
            ..abi_3
        };
        let abi_5 = Handled {
            fs: abi_4.fs | 1 << 15,
            ..abi_4
        };
        let abi_6 = Handled {
            scoped: 0b11,
            size: 24,
            ..abi_5
        };
        assert_eq!(handled(1), Some(abi_1));
        assert_eq!(handled(2), Some(abi_2));
        assert_eq!(handled(3), Some(abi_3));
        assert_eq!(handled(4), Some(abi_4));
        assert_eq!(handled(5), Some(abi_5));
        assert_eq!(handled(6), Some(abi_6));
        assert_eq!(handled(7), Some(abi_6));
        assert_eq!(abi_1.fs, 0x1fff);
        assert_eq!(abi_3.fs, 0x7fff);
        assert_eq!(abi_6.fs, 0xffff);
    }

    #[test]
    fn zero_is_no_abi() {
        assert_eq!(handled(0), None);
        assert_eq!(MINIMUM_ABI, 1);
    }

    #[test]
    fn an_unknown_newer_abi_handles_the_rights_of_the_highest_known_one() {
        for abi in [8, 9, 100, u32::MAX] {
            assert_eq!(handled(abi), handled(HIGHEST_KNOWN_ABI));
            assert_eq!(enforced_abi(abi), HIGHEST_KNOWN_ABI);
        }
        for abi in 1..=7 {
            assert_eq!(enforced_abi(abi), abi);
        }
    }

    #[test]
    fn a_newer_abi_never_handles_fewer_rights() {
        let mut previous = handled(1).unwrap();
        for abi in 2..=12 {
            let next = handled(abi).unwrap();
            assert_eq!(next.fs & previous.fs, previous.fs);
            assert_eq!(next.net & previous.net, previous.net);
            assert_eq!(next.scoped & previous.scoped, previous.scoped);
            assert!(next.size >= previous.size);
            previous = next;
        }
    }

    #[test]
    fn the_version_query_is_decided_by_its_abi_or_its_error() {
        for abi in [1, 2, 3, 4, 7, 9] {
            assert_eq!(decide(Ok(abi)), Ok(abi as u32));
        }
        for abi in [0, -5] {
            assert_eq!(decide(Ok(abi)), Err(Unavailable::NoAbi(abi)));
        }
        assert_eq!(decide(Err(libc::ENOSYS)), Err(Unavailable::Missing));
        assert_eq!(decide(Err(libc::EOPNOTSUPP)), Err(Unavailable::Disabled));
        assert_eq!(
            decide(Err(libc::EPERM)),
            Err(Unavailable::Failed(libc::EPERM))
        );
    }

    #[test]
    fn each_reason_names_the_call_and_the_cause() {
        let text = |query| decide(query).unwrap_err().to_string();
        assert!(text(Err(libc::ENOSYS)).contains("ENOSYS: this kernel has no Landlock"));
        assert!(text(Err(libc::EOPNOTSUPP)).contains("EOPNOTSUPP"));
        assert!(text(Err(libc::EOPNOTSUPP)).contains("not an enabled security module"));
        assert!(text(Err(libc::EPERM)).contains("landlock_create_ruleset failed with EPERM"));
        assert_eq!(
            text(Ok(0)),
            "the Landlock version query answered 0, which is no ABI"
        );
        assert_eq!(
            NotApplied::Failed("landlock_restrict_self", libc::EPERM).to_string(),
            "landlock_restrict_self failed with EPERM (Operation not permitted (os error 1))"
        );
        assert_eq!(
            NotApplied::Unavailable(Unavailable::Missing).to_string(),
            text(Err(libc::ENOSYS))
        );
    }

    #[test]
    fn the_version_query_answers_an_abi_or_names_the_call_that_failed() {
        // Both are a kernel the web process runs on.
        match abi() {
            Ok(abi) => assert!(abi >= MINIMUM_ABI),
            Err(reason) => assert!(
                reason
                    .to_string()
                    .starts_with("landlock_create_ruleset failed with "),
                "{reason}"
            ),
        }
    }
}
