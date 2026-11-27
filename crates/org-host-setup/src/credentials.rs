//! Step 3's credential directory, checked as the host will check it, and a development
//! build's key folder.

use std::os::fd::AsFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use org_host::instance::setup::{Input, Service};

use super::adapters::{CredentialDirectory, Platform};
use super::store_directories;

/// Makes the credential directory of instance `instance_id` below `base` and checks it as
/// the host will at every start: outside the data root `data_root`, owner-only, on a
/// filesystem that can hold it, below directories nobody else can change. Returns its
/// absolute path, for the record. Every folder this makes has mode 700; an existing one is
/// left as it is, and a wrong one is refused with its path and the rule it broke, never
/// repaired. A refused location is left exactly as it was found: nothing this call made
/// remains.
///
/// `base` is normalized first, as the record will demand, so that a base spelled with `.`
/// or a repeated separator is recorded in its one spelling and one with `..` is refused
/// before anything is made. `judged_from` is where the walk that judges the directories
/// above the store begins: `/` in the host's composition; a test whose temporary directory
/// is below a shared one passes its own base, as it passes the platform's test entry.
pub(crate) fn make_credential_directory(
    base: &Path,
    instance_id: &str,
    data_root: std::os::fd::BorrowedFd<'_>,
    judged_from: &Path,
    enter: impl FnOnce(
        org_platform_host::credentials::ReleaseDirectory<'_>,
    ) -> Result<
        org_platform_host::credentials::ReleaseRoot,
        org_platform_host::credentials::CredentialError,
    >,
) -> Result<PathBuf, String> {
    const NO_PATH: &str = "the credential directory has no absolute path; nothing was saved";
    if !org_host::instance::registry::valid_instance_id(instance_id) {
        return Err(NO_PATH.into());
    }
    let base = org_host::instance::setup::normalized(base).map_err(|_| NO_PATH.to_owned())?;
    // A registered service's store is a directory of the account, not a mount.
    use_credential_directory(&base.join(instance_id), data_root, judged_from, enter)
}

/// Checks a registered service's credential directory using the host's layout rules.
/// The host's two item stores are declared and the credential directory must be apart
/// from the account's records. The host checks the instance binding when it opens.
pub(super) fn open_as_the_host_will(
    records: std::os::fd::BorrowedFd<'_>,
) -> impl FnOnce(
    org_platform_host::credentials::ReleaseDirectory<'_>,
) -> Result<
    org_platform_host::credentials::ReleaseRoot,
    org_platform_host::credentials::CredentialError,
> + '_ {
    move |location| {
        org_platform_host::credentials::release_directory_apart_from(
            location,
            &org_host::credential_store_root::ITEM_STORES,
            &[records],
        )
    }
}

/// [`open_as_the_host_will`] for a new reservation of a registered service whose
/// configuration names the credential directory: the directory must also be unused, as a
/// new foreground reservation's volume must be ([`unused_mounted_store`]). One bound to
/// another instance, or holding credentials without a binding, is refused before the
/// service is registered, not at its first start.
pub(super) fn unused_as_the_host_will(
    records: std::os::fd::BorrowedFd<'_>,
) -> impl FnOnce(
    org_platform_host::credentials::ReleaseDirectory<'_>,
) -> Result<
    org_platform_host::credentials::ReleaseRoot,
    org_platform_host::credentials::CredentialError,
> + '_ {
    move |location| {
        org_platform_host::credentials::check_unused_release_directory(
            location,
            &org_host::credential_store_root::ITEM_STORES,
            &[records],
        )?;
        open_as_the_host_will(records)(location)
    }
}

/// A new foreground reservation requires an unused volume. The host claims it with its
/// recorded instance ID when it starts.
pub(super) fn unused_mounted_store(
    path: &Path,
    data_root: std::os::fd::BorrowedFd<'_>,
    records: std::os::fd::BorrowedFd<'_>,
) -> Result<PathBuf, String> {
    check_mounted_store(path, data_root, |location| {
        org_platform_host::credentials::check_unused_release_directory(
            location,
            &org_host::credential_store_root::ITEM_STORES,
            &[records],
        )
    })
}

pub(super) fn check_mounted_store(
    path: &Path,
    data_root: std::os::fd::BorrowedFd<'_>,
    check: impl FnOnce(
        org_platform_host::credentials::ReleaseDirectory<'_>,
    ) -> Result<(), org_platform_host::credentials::CredentialError>,
) -> Result<PathBuf, String> {
    check(org_platform_host::credentials::ReleaseDirectory {
        path,
        data_root,
        mount_point_required: true,
        expect_items: false,
        instance_id: None,
    })
    .map_err(|refused| {
        format!(
            "the credential directory cannot be used: {refused}; in the foreground mode \
             it is a mounted volume that belongs to the service user with mode 700, and \
             setup does not make it"
        )
    })?;
    Ok(path.to_owned())
}

/// The account's records (`base`, which holds the setup record and the instance locator),
/// opened as a directory for the credential directory's check ([`open_as_the_host_will`]).
pub(super) fn open_records(base: &Path) -> Result<std::os::fd::OwnedFd, String> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(base.as_os_str().as_bytes())
        .map_err(std::io::Error::other)
        .and_then(|path| store_directories::open_directory(None, &path))
        .map_err(|e| format!("the account's records {}: {e}", base.display()))
}

/// What can refuse a configuration from a file, asked before anything is saved or reserved
/// (X5 review V1, and its second round), so that a refused file leaves nothing and a
/// corrected one works on the next run:
///
/// - the address must be free now, in either service mode: a registered service is started
///   by setup, and a foreground instance is served by this process right after it. The
///   address is reserved with the instance and never changes, so a port another program
///   holds is refused here and not found by the web process after the instance was set up.
///   The address is bound here and returned, so it stays this process's while the instance
///   is reserved with it (X5 review, third round, P2): no other program can take it between
///   this check and the reservation. A foreground setup hands it to the web process's first
///   start ([`run`](super::run)); a registered one keeps it until it registers the service;
/// - the credential directory the file names must pass the host's own check. The check
///   compares it with the data folder, which is reached as a reservation reaches it (from
///   `/`, following no link) and, when it is missing, made for the question and removed
///   again before this returns, whatever the answer ([`org_host::instance::RootProbe`]). What
///   the check itself makes (the directories of a registered service's store, the store's
///   lock file) is removed again too: preflight leaves nothing it made (X5 review, fourth
///   round, P2-6). The data folder is opened as a directory only, so a path that is
///   something else (a FIFO) is refused, never waited on.
pub(super) fn preflight(
    input: &Input,
    records: std::os::fd::BorrowedFd<'_>,
) -> Result<org_host::BrowserListener, String> {
    let listen = &input.listen;
    let held = org_host::BrowserListener::bind(listen).map_err(|e| {
        let why = if e.kind() == std::io::ErrorKind::AddrInUse {
            "is in use by another program; choose another port in the file, or stop that \
             program"
                .to_owned()
        } else {
            format!("cannot be listened on ({e}); choose another address in the file")
        };
        format!(
            "port {} on {} {why}; nothing was saved",
            listen.port(),
            listen.address()
        )
    })?;
    let Some(credentials) = &input.credentials else {
        return Ok(held);
    };
    let root = org_host::instance::RootProbe::open(&input.root)
        .map_err(|e| format!("the data folder {}: {e}", input.root.display()))
        .map_err(|refused| format!("{refused}; nothing was saved"))?;
    let checked = if input.service == Service::Foreground {
        unused_mounted_store(credentials, root.descriptor(), records)
    } else {
        credential_directory(
            credentials,
            root.descriptor(),
            Path::new("/"),
            false,
            unused_as_the_host_will(records),
        )
    };
    // The data folder goes again if this made it.
    drop(root);
    checked
        .map(|_| held)
        .map_err(|refused| format!("{refused}; nothing was saved"))
}

/// The credential directory at exactly `path` (absolute and normalized), checked as the
/// host will check it at every start, and returned for the record.
///
/// For a registered service, missing directories are made below checked ancestors and
/// taken away again if the check refuses. No item is expected yet.
pub(crate) fn use_credential_directory(
    path: &Path,
    data_root: std::os::fd::BorrowedFd<'_>,
    judged_from: &Path,
    enter: impl FnOnce(
        org_platform_host::credentials::ReleaseDirectory<'_>,
    ) -> Result<
        org_platform_host::credentials::ReleaseRoot,
        org_platform_host::credentials::CredentialError,
    >,
) -> Result<PathBuf, String> {
    credential_directory(path, data_root, judged_from, true, enter)
}

/// [`use_credential_directory`]; with `keep` false, a question asked before the reservation:
/// whatever the answer, what this call made is removed again before it returns (the
/// directories it made, and the store's lock file the check makes in a store directory that
/// had none).
pub(super) fn credential_directory(
    path: &Path,
    data_root: std::os::fd::BorrowedFd<'_>,
    judged_from: &Path,
    keep: bool,
    enter: impl FnOnce(
        org_platform_host::credentials::ReleaseDirectory<'_>,
    ) -> Result<
        org_platform_host::credentials::ReleaseRoot,
        org_platform_host::credentials::CredentialError,
    >,
) -> Result<PathBuf, String> {
    let made = store_directories::make(path, Some(data_root), judged_from)
        .map_err(|refused| format!("the credential directory cannot be used: {refused}"))?;
    // When this check is to leave nothing, remove a newly made lock file while it is held.
    let lock_made_here = (!keep)
        .then(|| made.directory().try_clone_to_owned().ok())
        .flatten()
        .filter(|store| store_directories::lacks(store.as_fd(), c".lock"));
    // Open it once as the host does, so that a directory the host would refuse is refused
    // now, with the same words, and not at the service's first start.
    let checked = enter(org_platform_host::credentials::ReleaseDirectory {
        path,
        data_root,
        mount_point_required: false,
        expect_items: false,
        instance_id: None,
    });
    // While the store is still locked by this check, so that no other opener holds a lock
    // on the file that goes.
    if let (Some(store), Ok(_)) = (lock_made_here, &checked) {
        // The held store directory and a NUL-terminated single name.
        let _ = org_platform::posix::remove_at(store, c".lock", 0);
    }
    let checked = checked.map(drop);
    if !keep || checked.is_err() {
        made.remove();
    }
    checked.map_err(|refused| format!("the credential directory cannot be used: {refused}"))?;
    Ok(path.to_owned())
}

/// What the registered service of a development build needs before anything is saved: its
/// development key folder, where its host keeps its keys when the record names no
/// credential directory (on a system that keeps a host's secrets in the Keychain). `None`:
/// a release build, the foreground mode (whose host makes its own folder), or a record that
/// names a credential directory (X5 review V-a and P3-1). A folder that is not an absolute
/// path is refused: the service does not run in the folder setup ran in.
pub(super) fn development_keys(
    input: &Input,
    recorded: Option<bool>,
    folder: Option<&Path>,
    platform: &Platform,
) -> Result<Option<PathBuf>, String> {
    // `recorded`: whether the record of a reserved instance names a credential directory;
    // `None` before the reservation, when this run decides it.
    let named = recorded.unwrap_or_else(|| {
        input.credentials.is_some() || matches!(platform.credentials, CredentialDirectory::Below(_))
    });
    if !cfg!(debug_assertions) || input.service == Service::Foreground || named {
        return Ok(None);
    }
    match folder {
        Some(folder) => org_host::instance::setup::normalized(folder)
            .ok()
            .filter(|folder| folder.parent().is_some())
            .map(Some)
            .ok_or_else(|| {
                format!(
                    "ORG_OFFICE_DEV_KEY_DIR ({}) is not an absolute folder: the service does not \
                     run in this folder; name the development key folder by its absolute path; \
                     nothing was changed",
                    folder.display()
                )
            }),
        None => Err(
            "this development build's service keeps its keys in a development key \
             folder: set ORG_OFFICE_DEV_KEY_DIR to an absolute folder outside the instance's \
             data folder, then run setup again; nothing was changed"
                .into(),
        ),
    }
}

/// Makes a development build's key folder `folder` (absolute and normalized) if it is
/// missing, and checks it as the host's development store will open it: a folder of this
/// user with mode 700, its last component not a link, outside the instance's data folder
/// `root`. Folders above it that are missing are made first (mode 700), one at a time below
/// the deepest one that exists, which must be root's or this user's and not writable by
/// others. It is made from its parent's descriptor, and the mode is set on what was made,
/// whatever the process's mask is; an existing folder is never changed.
pub(super) fn development_folder(folder: &Path, root: &Path) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    let refuse = |why: String| {
        format!(
            "the development key folder {} {why}; set ORG_OFFICE_DEV_KEY_DIR to an absolute \
             folder of this user outside the instance's data folder",
            folder.display()
        )
    };
    let inside = |folder: &Path| folder.starts_with(root) || root.starts_with(folder);
    let (Some(parent), Some(name)) = (folder.parent(), folder.file_name()) else {
        return Err(refuse("is not a folder".into()));
    };
    let contain = || {
        refuse(format!(
            "and the instance's data folder {} contain each other: the keys would be in every \
             copy of the data",
            root.display()
        ))
    };
    if inside(folder) {
        return Err(contain());
    }
    if !parent.is_dir() {
        let existing = parent
            .ancestors()
            .find(|at| at.is_dir())
            .ok_or_else(|| refuse("has no folder above it".into()))?;
        let anchor = std::fs::canonicalize(existing)
            .map_err(|e| refuse(format!("cannot be made: {}: {e}", existing.display())))?;
        let rest = parent.strip_prefix(existing).unwrap_or(Path::new(""));
        store_directories::make(&anchor.join(rest), None, &anchor)
            .map_err(|why| refuse(format!("cannot be made: {why}")))?;
    }
    let resolved = std::fs::canonicalize(parent).map(|parent| parent.join(name));
    let root_resolved = std::fs::canonicalize(root);
    if matches!((&resolved, &root_resolved), (Ok(f), Ok(r)) if f.starts_with(r) || r.starts_with(f))
    {
        return Err(contain());
    }
    // The folder above may be reached through a link (a symlinked home), as the host's
    // store reaches it: only the folder itself is opened without following one.
    let parent_fd = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(parent)
            .map(std::os::fd::OwnedFd::from)
            .map_err(|e| {
                refuse(format!(
                    "cannot be made: its folder {}: {e}",
                    parent.display()
                ))
            })?
    };
    let leaf =
        std::ffi::CString::new(name.as_bytes()).map_err(|_| refuse("is not a path".into()))?;
    // An open directory and a NUL-terminated single name.
    let made = match org_platform::posix::make_directory_at(&parent_fd, &leaf, 0o700) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(error) => return Err(refuse(format!("cannot be made: {error}"))),
    };
    let opened = store_directories::open_directory(Some(&parent_fd), &leaf)
        .map_err(|e| refuse(format!("is not a folder this user's keys can be in ({e})")))?;
    let file = std::fs::File::from(opened);
    if made {
        file.set_permissions(std::fs::Permissions::from_mode(0o700))
            .map_err(|e| refuse(format!("cannot be made private: {e}")))?;
    }
    let found = file
        .metadata()
        .map_err(|e| refuse(format!("cannot be read: {e}")))?;
    if found.uid() != org_host::home::euid() || found.mode() & 0o7777 != 0o700 {
        return Err(refuse(format!(
            "must belong to this user with mode 700 (it has mode {:o})",
            found.mode() & 0o7777
        )));
    }
    Ok(())
}
