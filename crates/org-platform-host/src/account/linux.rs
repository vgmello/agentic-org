//! GNU/Linux: the service user's private group, from the account database.
//!
//! The two entries are read with `getpwuid_r` and `getgrgid_r`, the reentrant calls:
//! the host is threaded, and the plain calls return storage that another thread's call
//! overwrites. The answer is whatever the system's name service gives (files, or a
//! directory service where one is configured).

use std::ffi::CStr;
use std::io;

use super::decision::{is_private_group, GroupEntry, UserEntry};

/// The first size of the buffer an entry's strings are read into, in bytes.
const FIRST_BUFFER: usize = 1024;

/// The largest such buffer. An entry that needs more is not read.
const LARGEST_BUFFER: usize = 1 << 20;

/// The private group of the user `user`, or `None` if the user has none: no account
/// entry, no group entry for its primary group, or a primary group that is not private
/// (the module documentation has the rule).
///
/// An error means the account database could not be read. A caller that grants
/// something to the private group grants nothing then.
pub(crate) fn private_group_of(user: libc::uid_t) -> io::Result<Option<libc::gid_t>> {
    let Some(account) = user_entry(user)? else {
        return Ok(None);
    };
    let Some(group) = group_entry(account.primary_group)? else {
        return Ok(None);
    };
    Ok(is_private_group(&account, &group).then_some(group.id))
}

/// A larger buffer for an entry that did not fit, or the error that it never will.
fn grown(buffer: &mut Vec<libc::c_char>) -> io::Result<()> {
    if buffer.len() >= LARGEST_BUFFER {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "an account database entry is larger than this host reads",
        ));
    }
    buffer.resize(buffer.len() * 2, 0);
    Ok(())
}

/// The account entry of `user`, or `None` if there is none.
fn user_entry(user: libc::uid_t) -> io::Result<Option<UserEntry>> {
    let mut buffer: Vec<libc::c_char> = vec![0; FIRST_BUFFER];
    loop {
        // SAFETY: an all-zero `passwd` (null pointers, zero ids) is a valid value for
        // the call to overwrite.
        let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
        let mut found: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: `entry`, `buffer` and `found` are valid for the call, which writes
        // the entry into `entry`, its strings into `buffer` (of the length passed) and
        // a pointer to `entry`, or null, into `found`. It keeps nothing.
        let status = unsafe {
            libc::getpwuid_r(
                user,
                &mut entry,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut found,
            )
        };
        match status {
            0 if found.is_null() || entry.pw_name.is_null() => return Ok(None),
            0 => {
                // SAFETY: on success `pw_name` points to a NUL-terminated string in
                // `buffer`, which is alive and unchanged since the call.
                let name = unsafe { CStr::from_ptr(entry.pw_name) }.to_bytes().to_vec();
                return Ok(Some(UserEntry {
                    name,
                    primary_group: entry.pw_gid,
                }));
            }
            libc::ERANGE => grown(&mut buffer)?,
            libc::EINTR => {}
            // The C library reports "no such entry" this way in some configurations.
            libc::ENOENT | libc::ESRCH => return Ok(None),
            errno => return Err(io::Error::from_raw_os_error(errno)),
        }
    }
}

/// The group entry of `group`, or `None` if there is none.
fn group_entry(group: libc::gid_t) -> io::Result<Option<GroupEntry>> {
    let mut buffer: Vec<libc::c_char> = vec![0; FIRST_BUFFER];
    loop {
        // SAFETY: an all-zero `group` (null pointers, a zero id) is a valid value for
        // the call to overwrite.
        let mut entry: libc::group = unsafe { std::mem::zeroed() };
        let mut found: *mut libc::group = std::ptr::null_mut();
        // SAFETY: as for `getpwuid_r` above: valid storage of the length passed, and
        // nothing is kept.
        let status = unsafe {
            libc::getgrgid_r(
                group,
                &mut entry,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut found,
            )
        };
        match status {
            0 if found.is_null() || entry.gr_name.is_null() => return Ok(None),
            0 => {
                // SAFETY: on success `gr_name` points to a NUL-terminated string in
                // `buffer`, which is alive and unchanged since the call.
                let name = unsafe { CStr::from_ptr(entry.gr_name) }.to_bytes().to_vec();
                let mut members = Vec::new();
                let mut member = entry.gr_mem;
                // SAFETY: on success `gr_mem` is null or points to an array of
                // pointers to NUL-terminated strings, all in `buffer`, that ends with
                // a null pointer. Each step reads one element inside that array.
                while !member.is_null() && !unsafe { *member }.is_null() {
                    // SAFETY: a non-null element of the array, as above.
                    members.push(unsafe { CStr::from_ptr(*member) }.to_bytes().to_vec());
                    // SAFETY: the array ends with a null pointer, which the loop has
                    // not passed yet.
                    member = unsafe { member.add(1) };
                }
                return Ok(Some(GroupEntry {
                    id: entry.gr_gid,
                    name,
                    members,
                }));
            }
            libc::ERANGE => grown(&mut buffer)?,
            libc::EINTR => {}
            libc::ENOENT | libc::ESRCH => return Ok(None),
            errno => return Err(io::Error::from_raw_os_error(errno)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An id no account database of a test machine assigns.
    const NOBODY_HAS_THIS: u32 = 0x7fff_fff1;

    #[test]
    fn an_id_without_an_entry_has_no_private_group() {
        assert_eq!(user_entry(NOBODY_HAS_THIS).unwrap(), None);
        assert_eq!(group_entry(NOBODY_HAS_THIS).unwrap(), None);
        assert_eq!(private_group_of(NOBODY_HAS_THIS).unwrap(), None);
    }

    #[test]
    fn roots_entries_are_read_and_judged_by_the_rule() {
        let root = user_entry(0).unwrap().expect("the root account");
        assert_eq!(root.name, b"root");
        let group = group_entry(root.primary_group)
            .unwrap()
            .expect("root's primary group");
        assert_eq!(group.id, root.primary_group);
        assert_eq!(
            private_group_of(0).unwrap(),
            is_private_group(&root, &group).then_some(group.id)
        );
    }

    /// The probe on the account these tests run as, whatever it is: the answer is the
    /// rule's over the two entries the system gives, and a private group is this
    /// user's primary group. It prints what it found, for the record of a run.
    #[test]
    fn this_users_private_group_is_the_rules_answer_over_its_own_entries() {
        // SAFETY: no arguments and no side effects; they cannot fail.
        let (user, primary) = unsafe { (libc::geteuid(), libc::getegid()) };
        let found = private_group_of(user).unwrap();
        let account = user_entry(user).unwrap();
        let expected = account.as_ref().and_then(|account| {
            let group = group_entry(account.primary_group).unwrap()?;
            is_private_group(account, &group).then_some(group.id)
        });
        assert_eq!(found, expected);
        if let (Some(group), Some(account)) = (found, &account) {
            assert_eq!(group, account.primary_group);
        }
        println!(
            "uid {user} (process group {primary}): account entry {}, private group {found:?}",
            if account.is_some() {
                "found"
            } else {
                "missing"
            }
        );
    }

    #[test]
    fn an_entry_that_does_not_fit_gets_a_larger_buffer_up_to_the_limit() {
        let mut buffer = vec![0; FIRST_BUFFER];
        grown(&mut buffer).unwrap();
        assert_eq!(buffer.len(), 2 * FIRST_BUFFER);
        let mut largest = vec![0; LARGEST_BUFFER];
        let error = grown(&mut largest).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(largest.len(), LARGEST_BUFFER);
    }
}
