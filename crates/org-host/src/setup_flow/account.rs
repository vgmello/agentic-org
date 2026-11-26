//! Account lookups for the service's user and running a step as that account.

use std::time::{Duration, Instant};

use super::output::fail;

/// This process's user, by its account name, which `admin reset --user` takes: from the
/// account database, else from `USER` when that is a plain name, else the number (X5
/// review F12: a container's service user has no `USER`).
pub(super) fn user_name() -> String {
    let uid = crate::home::euid();
    plain_account_name(uid)
        .or_else(|| std::env::var("USER").ok().filter(|user| plain_name(user)))
        .unwrap_or_else(|| format!("uid {uid}"))
}

/// The account name of `uid` for a line of output: letters, digits, `_`, `-` and `.` only,
/// or the number where the account database has no such name.
pub(super) fn account_name(uid: u32) -> String {
    plain_account_name(uid).unwrap_or_else(|| format!("uid {uid}"))
}

/// Letters, digits, `_`, `-` and `.` only, and not empty.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// The account database's name of `uid`, when it has one and it is a [`plain_name`].
pub(super) fn plain_account_name(uid: u32) -> Option<String> {
    let mut buffer = vec![0 as libc::c_char; 16 * 1024];
    // SAFETY: an all-zero `passwd` is a valid value for the call to fill in.
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: an entry, buffer and result valid for the call.
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            &mut entry,
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut found,
        )
    };
    if status == 0 && !found.is_null() && !entry.pw_name.is_null() {
        // SAFETY: on success `pw_name` is a NUL-terminated string inside `buffer`.
        let name = unsafe { std::ffi::CStr::from_ptr(entry.pw_name) }.to_string_lossy();
        if plain_name(&name) {
            return Some(name.into_owned());
        }
    }
    None
}

/// An account of this machine: its user and primary group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Account {
    pub uid: u32,
    pub gid: u32,
}

/// Whose instance `admin reset` acts on, pure. `me` is this process's effective user,
/// `named` the `--user` option and `sudo_uid` the `SUDO_UID` variable. `None`: this
/// user's own, as itself. Root acts for the account it names and never for itself (the
/// service does not run as root); anyone else acts only for itself.
pub(crate) fn service_account(
    me: u32,
    named_by_user: bool,
    named: Option<&str>,
    sudo_uid: Option<&str>,
    by_name: impl Fn(&str) -> Option<Account>,
    by_id: impl Fn(u32) -> Option<Account>,
) -> Result<Option<Account>, String> {
    let unknown = |account: &str| format!("this machine has no account {account:?}");
    if me != 0 {
        return match named {
            None => Ok(None),
            Some(account) => match by_name(account) {
                Some(found) if found.uid == me => Ok(None),
                Some(_) => Err(format!(
                    "--user {account} names another account: only root acts for another user"
                )),
                None => Err(unknown(account)),
            },
        };
    }
    if !named_by_user {
        return Err("Run agentic-org admin reset as the service user.".into());
    }
    let account = match (named, sudo_uid) {
        (Some(account), _) => by_name(account).ok_or_else(|| unknown(account))?,
        (None, Some(uid)) => uid
            .parse()
            .ok()
            .and_then(by_id)
            .ok_or_else(|| unknown(uid))?,
        (None, None) => {
            return Err(
                "as root, name the user the service runs as: agentic-org admin reset --user \
                 <service user>"
                    .into(),
            )
        }
    };
    if account.uid == 0 {
        return Err("the service does not run as root; name the user it runs as".into());
    }
    Ok(Some(account))
}

/// One entry of the account database, by name or by number.
fn account_entry(
    look: impl Fn(&mut libc::passwd, &mut [libc::c_char], &mut *mut libc::passwd) -> libc::c_int,
) -> Option<Account> {
    let mut buffer = vec![0 as libc::c_char; 16 * 1024];
    // SAFETY: an all-zero `passwd` is a valid value for the call to fill in.
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    if look(&mut entry, &mut buffer, &mut found) != 0 || found.is_null() {
        return None;
    }
    Some(Account {
        uid: entry.pw_uid,
        gid: entry.pw_gid,
    })
}

pub(super) fn account_by_name(name: &str) -> Option<Account> {
    let name = std::ffi::CString::new(name).ok()?;
    account_entry(|entry, buffer, found| {
        // SAFETY: a NUL-terminated name, and an entry, buffer and result valid for the call.
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

pub(super) fn account_by_id(uid: u32) -> Option<Account> {
    account_entry(|entry, buffer, found| {
        // SAFETY: an entry, buffer and result valid for the call.
        unsafe { libc::getpwuid_r(uid, entry, buffer.as_mut_ptr(), buffer.len(), found) }
    })
}

/// How long root waits for the lookup of the service user's instance ([`as_account`]).
pub(super) const LOOKUP_WITHIN: Duration = Duration::from_secs(10);

/// The most a lookup answers with: a setup record, an instance ID and a socket path.
const LOOKUP_BYTES: usize = 256 * 1024;

/// Runs `work` in a child process with `account`'s whole identity, and returns what it
/// returned (X5 review, fourth round, P2-9 and F4). The child takes the account's
/// supplementary groups (`initgroups`), its group and its user, real, effective and saved
/// alike, and checks that it can never be root again, before `work` runs: every ownership
/// and group check of the instance's folders holds as for that user (a folder only a group
/// of the user may search included), a file `work` makes (a lock) belongs to that user,
/// and what that user's files say is read and parsed in a process without root's
/// privileges. The answer comes back as JSON over a pipe, at most [`LOOKUP_BYTES`], within
/// [`LOOKUP_WITHIN`], after which the child is killed. A process that is not root already
/// is the account it acts for, and its child changes nothing.
///
/// The process has one thread when a command calls this, so the child may run ordinary code;
/// it never returns into the caller, whatever `work` does.
pub(super) fn as_account<T: serde::Serialize + serde::de::DeserializeOwned>(
    account: Account,
    work: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    in_child(|| become_account(account), work, LOOKUP_WITHIN)
}

/// `account`'s identity, for good, in this (child) process: nothing when it is not root.
pub(super) fn become_account(account: Account) -> Result<(), String> {
    let error = |what: &str| fail(what, std::io::Error::last_os_error());
    if crate::home::euid() != 0 {
        return Ok(());
    }
    let mut buffer = vec![0 as libc::c_char; 16 * 1024];
    // SAFETY: an all-zero `passwd` is a valid value for the call to fill in.
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: an entry, buffer and result valid for the call.
    let status = unsafe {
        libc::getpwuid_r(
            account.uid,
            &mut entry,
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut found,
        )
    };
    // The account's groups: all of them where the database names the account, else its
    // primary group alone.
    let grouped = if status == 0 && !found.is_null() && !entry.pw_name.is_null() {
        // SAFETY: a NUL-terminated name inside `buffer`, and the account's group.
        unsafe { libc::initgroups(entry.pw_name, account.gid as _) }
    } else {
        // SAFETY: one group, read from a valid array.
        unsafe { libc::setgroups(1, &account.gid) }
    };
    if grouped != 0 {
        return Err(error("acting as the service user"));
    }
    // Root sets the real, effective and saved IDs alike. SAFETY: plain identity changes.
    if unsafe { libc::setgid(account.gid) } != 0 || unsafe { libc::setuid(account.uid) } != 0 {
        return Err(error("acting as the service user"));
    }
    // SAFETY: plain identity queries and one change that must fail.
    let dropped = unsafe {
        libc::getuid() == account.uid
            && libc::geteuid() == account.uid
            && libc::getgid() == account.gid
            && libc::getegid() == account.gid
            && libc::setuid(0) != 0
    };
    if !dropped {
        return Err("acting as the service user: root's identity was not given up".into());
    }
    Ok(())
}

/// Forks; the child runs `become` and then `work`, writes the answer to a pipe and ends;
/// this process reads it within `limit`. See [`as_account`].
pub(super) fn in_child<T: serde::Serialize + serde::de::DeserializeOwned>(
    become_child: impl FnOnce() -> Result<(), String>,
    work: impl FnOnce() -> Result<T, String>,
    limit: Duration,
) -> Result<T, String> {
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    let lookup = |why: std::io::Error| fail("finding the service user's instance", why);
    let mut ends = [0 as libc::c_int; 2];
    // SAFETY: a writable array of two descriptors.
    if unsafe { libc::pipe(ends.as_mut_ptr()) } != 0 {
        return Err(lookup(std::io::Error::last_os_error()));
    }
    // SAFETY: two new descriptors owned only here.
    let (read, write) = unsafe { (OwnedFd::from_raw_fd(ends[0]), OwnedFd::from_raw_fd(ends[1])) };
    for end in [&read, &write] {
        // SAFETY: an open descriptor owned here. One thread: no spawn races this.
        if unsafe { libc::fcntl(end.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
            return Err(lookup(std::io::Error::last_os_error()));
        }
    }
    // SAFETY: the process has one thread (see `as_account`); the child never returns.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(lookup(std::io::Error::last_os_error()));
    }
    if pid == 0 {
        drop(read);
        // The child keeps no descriptor of this process but its end of the pipe: nothing
        // root holds (its terminal, a listener, a lock) is the account's process's, or stays
        // held while it runs. Its standard three are the null device.
        // SAFETY: a NUL-terminated path; each standard descriptor is replaced by a copy.
        unsafe {
            let null = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
            if null >= 0 {
                for fd in 0..3 {
                    libc::dup2(null, fd);
                }
                if null > 2 {
                    libc::close(null);
                }
            }
        }
        let keep = write.as_raw_fd();
        // SAFETY: sysconf has no preconditions.
        let most = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) }.clamp(3, 65_536) as libc::c_int;
        for fd in 3..most {
            if fd != keep {
                // SAFETY: closing a descriptor this child does not use; a closed one fails.
                unsafe { libc::close(fd) };
            }
        }
        let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            become_child().and_then(|()| work())
        }))
        .unwrap_or_else(|_| Err("the lookup failed".into()));
        let bytes = serde_json::to_vec(&answer).unwrap_or_default();
        let mut file = std::fs::File::from(write);
        let _ = std::io::Write::write_all(&mut file, &bytes);
        drop(file);
        // SAFETY: the end of the child, without the parent's exit handlers.
        unsafe { libc::_exit(0) };
    }
    drop(write);
    let deadline = Instant::now() + limit;
    let mut file = std::fs::File::from(read);
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 8192];
    let read = loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let mut poll = libc::pollfd {
            fd: file.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let millis = libc::c_int::try_from(left.as_millis()).unwrap_or(libc::c_int::MAX);
        // SAFETY: one valid pollfd.
        match unsafe { libc::poll(&mut poll, 1, millis) } {
            0 => {
                break Err(format!(
                    "the lookup did not end within {}s",
                    limit.as_secs()
                ))
            }
            n if n < 0 => {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                break Err(lookup(error));
            }
            _ => match file.read(&mut chunk) {
                Ok(0) => break Ok(()),
                Ok(n) if bytes.len() + n <= LOOKUP_BYTES => bytes.extend_from_slice(&chunk[..n]),
                Ok(_) => break Err("the lookup answered with too much".into()),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => break Err(lookup(e)),
            },
        }
    };
    if read.is_err() {
        // SAFETY: the child this call made.
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
    // Reaped within a bound too: a child the kill cannot end at once (stuck in a filesystem
    // that does not answer) is left to the system, and the command goes on.
    let reap = Instant::now() + Duration::from_secs(1);
    let mut status = 0;
    // SAFETY: the child this call made, reaped once, without waiting.
    while unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) } == 0 && Instant::now() < reap {
        std::thread::sleep(Duration::from_millis(10));
    }
    read?;
    serde_json::from_slice::<Result<T, String>>(&bytes)
        .map_err(|_| "the lookup did not answer".to_owned())?
}
