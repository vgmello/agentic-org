//! The account database (`getpwuid_r`, `getpwnam_r`) and taking an account's identity
//! (`initgroups`/`setgroups`, `setgid`, `setuid`).

use std::ffi::{CStr, CString, OsStr};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use super::check;

/// The largest buffer a lookup grows to on `ERANGE`.
const MOST_BUFFER: usize = 1 << 20;

/// One entry of the account database.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountEntry {
    /// The account's name (`pw_name`), when the entry has one.
    pub name: Option<CString>,
    /// Its user ID (`pw_uid`).
    pub uid: u32,
    /// Its primary group ID (`pw_gid`).
    pub gid: u32,
    /// Its home directory (`pw_dir`), when the entry has one.
    pub home: Option<PathBuf>,
}

/// The entry of the user `uid`; `Ok(None)` when the database has none.
pub fn account_by_uid(uid: u32) -> io::Result<Option<AccountEntry>> {
    lookup(|entry, buffer, found| {
        // SAFETY: `entry`, `buffer` (of the length passed) and `found` are live and
        // exclusively borrowed for the call, which writes the strings `entry` points to
        // into `buffer`.
        unsafe { libc::getpwuid_r(uid, entry, buffer.as_mut_ptr(), buffer.len(), found) }
    })
}

/// The entry of the account named `name`; `Ok(None)` when the database has none.
pub fn account_by_name(name: &CStr) -> io::Result<Option<AccountEntry>> {
    lookup(|entry, buffer, found| {
        // SAFETY: `name` is NUL-terminated, and `entry`, `buffer` (of the length passed)
        // and `found` are live and exclusively borrowed for the call.
        unsafe {
            libc::getpwnam_r(
                name.as_ptr(),
                entry,
                buffer.as_mut_ptr(),
                buffer.len(),
                found,
            )
        }
    })
}

fn lookup(
    call: impl Fn(&mut libc::passwd, &mut [libc::c_char], &mut *mut libc::passwd) -> libc::c_int,
) -> io::Result<Option<AccountEntry>> {
    let mut buffer = vec![0 as libc::c_char; 16 * 1024];
    loop {
        // SAFETY: passwd is plain data for which all-zero bytes are a valid value.
        let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
        let mut found: *mut libc::passwd = std::ptr::null_mut();
        let status = call(&mut entry, &mut buffer, &mut found);
        if status == libc::ERANGE && buffer.len() < MOST_BUFFER {
            buffer.resize(buffer.len() * 2, 0);
            continue;
        }
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status));
        }
        if found.is_null() {
            return Ok(None);
        }
        let text = |pointer: *const libc::c_char| {
            // SAFETY: on success each non-null string field of `entry` points at a
            // NUL-terminated string inside `buffer`, which is still live and unchanged.
            (!pointer.is_null()).then(|| unsafe { CStr::from_ptr(pointer) }.to_owned())
        };
        return Ok(Some(AccountEntry {
            name: text(entry.pw_name),
            uid: entry.pw_uid,
            gid: entry.pw_gid,
            home: text(entry.pw_dir).map(|dir| PathBuf::from(OsStr::from_bytes(dir.as_bytes()))),
        }));
    }
}

/// Give this process the supplementary groups of the account `name` and `gid`
/// (`initgroups`). Needs privilege.
pub fn initialize_groups(name: &CStr, gid: u32) -> io::Result<()> {
    // SAFETY: `name` is NUL-terminated and live for the call; the group is a plain number
    // in the C library's own type.
    check(unsafe { libc::initgroups(name.as_ptr(), gid as _) }).map(drop)
}

/// Make `groups` this process's supplementary groups (`setgroups`). Needs privilege. More
/// groups than an `int` holds (the narrowest count any supported system takes: macOS's;
/// GNU/Linux takes a `size_t`) is [`io::ErrorKind::InvalidInput`].
pub fn set_groups(groups: &[u32]) -> io::Result<()> {
    let count = libc::c_int::try_from(groups.len()).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "too many supplementary groups")
    })?;
    // SAFETY: the pointer and count describe the live `groups` slice, which is only read;
    // the count is not negative, so it is the same number in the call's own count type.
    check(unsafe { libc::setgroups(count as _, groups.as_ptr()) }).map(drop)
}

/// Set this process's group ID (`setgid`): as root, the real, effective and saved IDs.
pub fn set_gid(gid: u32) -> io::Result<()> {
    // SAFETY: setgid takes a plain number and touches no memory.
    check(unsafe { libc::setgid(gid) }).map(drop)
}

/// Set this process's user ID (`setuid`): as root, the real, effective and saved IDs, for
/// good.
pub fn set_uid(uid: u32) -> io::Result<()> {
    // SAFETY: setuid takes a plain number and touches no memory.
    check(unsafe { libc::setuid(uid) }).map(drop)
}
