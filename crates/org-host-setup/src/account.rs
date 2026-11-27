//! Account lookups for the service's user and running a step as that account.

use std::io::{self, Write};
use std::time::Duration;

use org_platform::posix::{self, ChildAnswerError};

use super::output::fail;

/// This process's user, by its account name, which `admin reset --user` takes: from the
/// account database, else from `USER` when that is a plain name, else the number (X5
/// review F12: a container's service user has no `USER`).
pub(super) fn user_name() -> String {
    let uid = org_host::home::euid();
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
    let name = posix::account_by_uid(uid).ok()??.name?;
    let name = name.to_string_lossy();
    plain_name(&name).then(|| name.into_owned())
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

/// One entry of the account database, as an [`Account`].
fn account_entry(entry: io::Result<Option<posix::AccountEntry>>) -> Option<Account> {
    let entry = entry.ok()??;
    Some(Account {
        uid: entry.uid,
        gid: entry.gid,
    })
}

pub(super) fn account_by_name(name: &str) -> Option<Account> {
    let name = std::ffi::CString::new(name).ok()?;
    account_entry(posix::account_by_name(&name))
}

pub(super) fn account_by_id(uid: u32) -> Option<Account> {
    account_entry(posix::account_by_uid(uid))
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
    let error = |what: &str, why: io::Error| fail(what, why);
    if org_host::home::euid() != 0 {
        return Ok(());
    }
    // The account's groups: all of them where the database names the account, else its
    // primary group alone.
    let named = posix::account_by_uid(account.uid)
        .ok()
        .flatten()
        .and_then(|entry| entry.name);
    let grouped = match named {
        Some(name) => posix::initialize_groups(&name, account.gid),
        None => posix::set_groups(&[account.gid]),
    };
    grouped.map_err(|why| error("acting as the service user", why))?;
    // Root sets the real, effective and saved IDs alike.
    posix::set_gid(account.gid)
        .and_then(|()| posix::set_uid(account.uid))
        .map_err(|why| error("acting as the service user", why))?;
    let dropped = posix::real_uid() == account.uid
        && posix::effective_uid() == account.uid
        && posix::real_gid() == account.gid
        && posix::effective_gid() == account.gid
        && posix::set_uid(0).is_err();
    if !dropped {
        return Err("acting as the service user: root's identity was not given up".into());
    }
    Ok(())
}

/// Runs `become` and then `work` in a forked child, which writes its answer (JSON) to a pipe
/// and ends; this process reads it within `limit`. The process must have one thread (see
/// [`as_account`]): `org_platform::posix::answer_from_child` refuses to fork otherwise.
pub(super) fn in_child<T: serde::Serialize + serde::de::DeserializeOwned>(
    become_child: impl FnOnce() -> Result<(), String>,
    work: impl FnOnce() -> Result<T, String>,
    limit: Duration,
) -> Result<T, String> {
    let bytes = posix::answer_from_child(
        |out| child_answer(become_child, work, out),
        limit,
        LOOKUP_BYTES,
    )
    .map_err(|refused| match refused {
        ChildAnswerError::TimedOut => {
            format!("the lookup did not end within {}s", limit.as_secs())
        }
        ChildAnswerError::TooLarge => "the lookup answered with too much".into(),
        ChildAnswerError::NotStarted(why) | ChildAnswerError::Read(why) => {
            fail("finding the service user's instance", why)
        }
    })?;
    decode_answer(&bytes)
}

/// What the child of [`in_child`] writes: `become` then `work`'s result as JSON, or that the
/// lookup failed when either panics.
pub(super) fn child_answer<T: serde::Serialize>(
    become_child: impl FnOnce() -> Result<(), String>,
    work: impl FnOnce() -> Result<T, String>,
    out: &mut dyn Write,
) {
    let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        become_child().and_then(|()| work())
    }))
    .unwrap_or_else(|_| Err("the lookup failed".into()));
    let bytes = serde_json::to_vec(&answer).unwrap_or_default();
    let _ = out.write_all(&bytes);
}

/// The answer [`child_answer`] wrote.
pub(super) fn decode_answer<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    serde_json::from_slice::<Result<T, String>>(bytes)
        .map_err(|_| "the lookup did not answer".to_owned())?
}
