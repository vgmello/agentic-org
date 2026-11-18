//! Step 4 of the Linux adapter: Landlock (Linux host design, section 3.3).
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

/// The first ABI that covers truncating a file by path (Linux 6.2). Below it Landlock
/// alone does not deny every file change, so the adapter refuses.
pub(super) const MINIMUM_ABI: u32 = 3;
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
    fs |= ACCESS_FS_REFER; // ABI 2
    fs |= ACCESS_FS_TRUNCATE; // ABI 3
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
    /// The version query answered an ABI below [`MINIMUM_ABI`].
    TooOld(i64),
    /// Any other error of the version query, by its number: `EPERM` under a profile
    /// that refuses the call.
    Failed(i32),
}

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str(
                "landlock_create_ruleset failed with ENOSYS: this kernel has no Landlock \
                 (Linux 6.2 or later with Landlock is required)",
            ),
            Self::Disabled => f.write_str(
                "landlock_create_ruleset failed with EOPNOTSUPP: Landlock is built into \
                 this kernel but is not an enabled security module",
            ),
            Self::TooOld(abi) => write!(
                f,
                "this kernel's Landlock ABI is {abi}, below {MINIMUM_ABI} (Linux 6.2 or \
                 later is required)"
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
            .ok_or(Unavailable::TooOld(abi)),
        Err(libc::ENOSYS) => Err(Unavailable::Missing),
        Err(libc::EOPNOTSUPP) => Err(Unavailable::Disabled),
        Err(errno) => Err(Unavailable::Failed(errno)),
    }
}

/// The running kernel's Landlock ABI, when it is high enough. The query creates
/// nothing and restricts nothing, so the host may ask it of itself.
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

/// Restricts the calling thread, which must be the process's only one and must have
/// no-new-privileges set: every right `kernel`'s ABI knows is handled and no rule
/// allows anything. Returns the ABI enforced in full.
pub(super) fn restrict(kernel: u32) -> io::Result<u32> {
    let abi = enforced_abi(kernel);
    let handled =
        handled(abi).ok_or_else(|| super::refuse(Unavailable::TooOld(i64::from(kernel))))?;
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
        return Err(super::failed_call("landlock_create_ruleset"));
    }
    // SAFETY: a successful landlock_create_ruleset returns a new descriptor that
    // nothing else owns; it is closed when this function returns.
    let ruleset = unsafe { OwnedFd::from_raw_fd(fd as i32) };
    // SAFETY: a live ruleset descriptor and zero flags; the call changes only the
    // calling thread's credentials.
    if unsafe { libc::syscall(libc::SYS_landlock_restrict_self, ruleset.as_raw_fd(), 0u32) } != 0 {
        return Err(super::failed_call("landlock_restrict_self"));
    }
    Ok(abi)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `LANDLOCK_ACCESS_FS_*` of ABI 1: bits 0 to 12.
    const ABI_1_FS: u64 = (1 << 13) - 1;

    #[test]
    fn each_abi_handles_every_right_the_kernel_added_up_to_it() {
        // include/uapi/linux/landlock.h: REFER is bit 13 (ABI 2), TRUNCATE bit 14
        // (ABI 3), IOCTL_DEV bit 15 (ABI 5); BIND_TCP and CONNECT_TCP are bits 0 and
        // 1 of the network rights (ABI 4); ABSTRACT_UNIX_SOCKET and SIGNAL bits 0 and
        // 1 of the scopes (ABI 6). ABI 7 adds no right.
        let abi_3 = Handled {
            fs: ABI_1_FS | 1 << 13 | 1 << 14,
            net: 0,
            scoped: 0,
            size: 8,
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
        assert_eq!(handled(3), Some(abi_3));
        assert_eq!(handled(4), Some(abi_4));
        assert_eq!(handled(5), Some(abi_5));
        assert_eq!(handled(6), Some(abi_6));
        assert_eq!(handled(7), Some(abi_6));
        assert_eq!(abi_3.fs, 0x7fff);
        assert_eq!(abi_6.fs, 0xffff);
    }

    #[test]
    fn an_abi_below_three_has_no_ruleset() {
        for abi in [0, 1, 2] {
            assert_eq!(handled(abi), None);
        }
    }

    #[test]
    fn an_unknown_newer_abi_handles_the_rights_of_the_highest_known_one() {
        for abi in [8, 9, 100, u32::MAX] {
            assert_eq!(handled(abi), handled(HIGHEST_KNOWN_ABI));
            assert_eq!(enforced_abi(abi), HIGHEST_KNOWN_ABI);
        }
        for abi in 3..=7 {
            assert_eq!(enforced_abi(abi), abi);
        }
    }

    #[test]
    fn a_newer_abi_never_handles_fewer_rights() {
        let mut previous = handled(3).unwrap();
        for abi in 4..=12 {
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
        for abi in [3, 4, 7, 9] {
            assert_eq!(decide(Ok(abi)), Ok(abi as u32));
        }
        for abi in [0, 1, 2, -5] {
            assert_eq!(decide(Ok(abi)), Err(Unavailable::TooOld(abi)));
        }
        assert_eq!(decide(Err(libc::ENOSYS)), Err(Unavailable::Missing));
        assert_eq!(decide(Err(libc::EOPNOTSUPP)), Err(Unavailable::Disabled));
        assert_eq!(
            decide(Err(libc::EPERM)),
            Err(Unavailable::Failed(libc::EPERM))
        );
    }

    #[test]
    fn each_refusal_names_the_call_and_the_cause() {
        let text = |query| decide(query).unwrap_err().to_string();
        assert!(text(Err(libc::ENOSYS)).contains("ENOSYS: this kernel has no Landlock"));
        assert!(text(Err(libc::EOPNOTSUPP)).contains("EOPNOTSUPP"));
        assert!(text(Err(libc::EOPNOTSUPP)).contains("not an enabled security module"));
        assert!(text(Err(libc::EPERM)).contains("landlock_create_ruleset failed with EPERM"));
        for abi in [0, 1, 2] {
            assert!(text(Ok(abi)).contains(&format!("Landlock ABI is {abi}, below 3")));
        }
    }

    #[test]
    fn this_kernel_has_an_abi_the_adapter_accepts() {
        assert!(abi().unwrap() >= MINIMUM_ABI);
    }
}
