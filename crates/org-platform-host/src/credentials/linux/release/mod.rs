//! GNU/Linux: the release directory's live probes, and the order of its checks.
//!
//! Every decision is a pure function of `rules.rs`; this module opens the directories,
//! asks the kernel and the account database for the facts, and stops at the first
//! rule that is broken, naming the directory that broke it. The contract, the layout
//! and the list of checks are in `release.rs`.
//!
//! The walk holds one descriptor at a time. A directory above the release directory
//! is opened with `O_PATH`: the service user needs to pass through it, not to list it
//! (a home directory's parent may be searchable only), and `fstat` and `openat` work
//! on such a descriptor. Each is judged before anything below it is opened, so an
//! unsafe directory is never walked through.
//!
//! Three probes read `/proc`: a directory's access ACL (through its descriptor's entry
//! there, `crate::account`), a directory's own path (the same entry, read as a link)
//! and the mount table (`/proc/self/mountinfo`). Without `/proc` the first refuses a
//! group-writable directory above the store, and the other two refuse every store:
//! where the two directories are inside their filesystems is then not known
//! (`mounts/`).

mod descriptors;
mod item_stores;
mod lock;
mod mountinfo;

use std::ffi::{CStr, CString, OsStr};
use std::fs::{File, Permissions};
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::account;
use crate::credentials::mounts::{self, Table};
use crate::credentials::rules::{self, Ancestor};
use crate::credentials::{
    directory, valid_name, CredentialError, ItemStore, ReleaseDirectory, ReleaseItems, RootRefusal,
    RootRule, MAX_NAME_BYTES,
};

use self::descriptors::{
    chain, entries, filesystem_kind, mount_attributes, open_absolute, open_at,
};
use self::item_stores::{count_items, item_store_directories, ExistingStore};
use self::lock::open_lock;
use self::mountinfo::{mount_table, place};

/// The lock file's name. An item store's directory has the syntax of an item name,
/// which never starts with `.`, so none can be the lock.
const LOCK: &CStr = c".lock";
const LOCK_NAME: &str = ".lock";
const INSTANCE_MARKER: &CStr = c".instance";

/// One open release directory (see `release.rs`). The directory's lock is held by the
/// shared [`Root`], which this handle and every store opened from it keep alive.
pub(in crate::credentials) struct ReleaseRoot(Arc<Root>);

/// The checked release directory and its lock.
pub(super) struct Root {
    /// The release directory, held since it was checked.
    directory: File,
    /// Its path: `/` and the single names that were walked, so it holds no link.
    path: PathBuf,
    /// The lock file, locked exclusively.
    lock: File,
    /// The item stores the directory was opened for: directory and suffix of each.
    /// Only these can be opened from it.
    stores: Vec<(String, String)>,
}

impl Drop for Root {
    fn drop(&mut self) {
        // A descriptor a forked child inherited may outlive this owner; closing
        // alone would then not release the lock. Drop cannot report a failure, and
        // the file still closes afterwards.
        let _ = self.lock.unlock();
    }
}

pub(in crate::credentials) fn release_directory(
    location: &ReleaseDirectory<'_>,
    item_stores: &[ItemStore<'_>],
    account_data: &[BorrowedFd<'_>],
) -> Result<ReleaseRoot, CredentialError> {
    declared(item_stores)?;
    open(
        absolute_walk(location.path)?,
        location,
        item_stores,
        account_data,
    )
}

pub(in crate::credentials) fn check_unused_release_directory(
    location: &ReleaseDirectory<'_>,
    item_stores: &[ItemStore<'_>],
    account_data: &[BorrowedFd<'_>],
) -> Result<(), CredentialError> {
    declared(item_stores)?;
    check_unused(
        absolute_walk(location.path)?,
        location,
        item_stores,
        account_data,
    )
}

fn absolute_walk(path: &Path) -> Result<Walk<'_>, CredentialError> {
    let names = rules::components(path).map_err(|rule| refused(rule, path))?;
    let start = Path::new("/");
    let root = open_absolute(start).map_err(|error| refused(component_rule(&error), start))?;
    Ok(Walk { start, root, names })
}

/// The declared item stores as the root keeps them, or [`CredentialError::InvalidName`]
/// if one is not a single item-store name with a suffix the engine takes, or two share
/// a directory.
fn declared(item_stores: &[ItemStore<'_>]) -> Result<Vec<(String, String)>, CredentialError> {
    let mut stores: Vec<(String, String)> = Vec::new();
    for store in item_stores {
        if !valid_name(store.directory)
            || !directory::valid_suffix(store.suffix)
            || stores.iter().any(|(taken, _)| taken == store.directory)
        {
            return Err(CredentialError::InvalidName);
        }
        stores.push((store.directory.to_owned(), store.suffix.to_owned()));
    }
    Ok(stores)
}

/// The release directory at `location`, with the walk started at the directory `base`
/// instead of at `/`: for tests, whose temporary directories are below a directory
/// that is writable by others. `base` and everything below it is opened and judged
/// exactly as in a host's walk; the directories above `base` are not. The path must be
/// `base` or below it, spelled with `base` as its prefix.
#[cfg(any(test, feature = "test-support"))]
pub(in crate::credentials) fn release_directory_below(
    base: &Path,
    location: &ReleaseDirectory<'_>,
    item_stores: &[ItemStore<'_>],
    account_data: &[BorrowedFd<'_>],
) -> Result<ReleaseRoot, CredentialError> {
    let below = location
        .path
        .strip_prefix(base)
        .map_err(|_| refused(RootRule::Relative, location.path))?;
    // The same syntax rule, on the part of the path that is walked.
    let names = rules::names(below).map_err(|rule| refused(rule, location.path))?;
    declared(item_stores)?;
    let root = open_absolute(base).map_err(|error| refused(component_rule(&error), base))?;
    open(
        Walk {
            start: base,
            root,
            names,
        },
        location,
        item_stores,
        account_data,
    )
}

/// A walk to a release directory: the directory it starts at, and the single names
/// from there. A host's walk starts at `/`; that is the only start `release_directory`
/// makes. The start itself is judged like every directory below it.
struct Walk<'a> {
    /// The path of the directory the walk starts at.
    start: &'a Path,
    /// That directory, open.
    root: File,
    /// The names from there down to the release directory, which is the last.
    names: Vec<&'a OsStr>,
}

fn open(
    walk: Walk<'_>,
    location: &ReleaseDirectory<'_>,
    item_stores: &[ItemStore<'_>],
    account_data: &[BorrowedFd<'_>],
) -> Result<ReleaseRoot, CredentialError> {
    if location.instance_id.is_some_and(|id| !valid_name(id)) {
        return Err(CredentialError::InvalidName);
    }
    let (directory, path, service_user) = checked_directory(walk, location, account_data)?;
    let existing = item_store_directories(&directory, &path, item_stores, service_user)?;
    if location.expect_items {
        let mut items = 0;
        for (store, store_path, store_directory) in &existing {
            items += count_items(store, store_path, store_directory)?;
        }
        rules::items_present(true, items).map_err(|rule| refused(rule, &path))?;
    }
    if let Some(id) = location.instance_id {
        check_binding(&directory, &path, &existing, Some(id))?;
    }

    let lock = open_lock(&directory, &path, service_user, true)?
        .expect("opening with creation always takes the lock or refuses");
    if let Some(id) = location.instance_id {
        // Reopen the item directories under the lock: another host may have used the
        // volume since the checks above. Only a still-unused directory can be claimed.
        let existing = item_store_directories(&directory, &path, item_stores, service_user)?;
        if !check_binding(&directory, &path, &existing, Some(id))? {
            publish_binding(&directory, id)
                .map_err(|_| refused(RootRule::InstanceMarker, &path))?;
        }
        // Also retry the directory flush if an earlier start failed after the rename.
        directory
            .sync_all()
            .map_err(|_| refused(RootRule::InstanceMarker, &path))?;
    }
    Ok(ReleaseRoot(Arc::new(Root {
        directory,
        path,
        lock,
        stores: declared(item_stores)?,
    })))
}

fn check_unused(
    walk: Walk<'_>,
    location: &ReleaseDirectory<'_>,
    item_stores: &[ItemStore<'_>],
    account_data: &[BorrowedFd<'_>],
) -> Result<(), CredentialError> {
    let (directory, path, service_user) = checked_directory(walk, location, account_data)?;
    // An existing lock can be taken briefly; a preflight must never create or unlink one.
    let _lock = open_lock(&directory, &path, service_user, false)?;
    let existing = item_store_directories(&directory, &path, item_stores, service_user)?;
    check_binding(&directory, &path, &existing, None).map(|_| ())
}

fn checked_directory(
    walk: Walk<'_>,
    location: &ReleaseDirectory<'_>,
    account_data: &[BorrowedFd<'_>],
) -> Result<(File, PathBuf, u32), CredentialError> {
    // SAFETY: no arguments and no side effects; it cannot fail.
    let service_user = unsafe { libc::geteuid() };
    let mut private_group = PrivateGroup::NotAsked;
    let mut path = walk.start.to_path_buf();
    let mut current = walk.root;
    let directory = match walk.names.split_last() {
        Some((store, above)) => {
            for name in above {
                judge_ancestor(&current, &path, service_user, &mut private_group)?;
                path.push(name);
                current = open_at(current.as_fd(), name, libc::O_PATH)
                    .map_err(|error| refused(component_rule(&error), &path))?;
            }
            judge_ancestor(&current, &path, service_user, &mut private_group)?;
            path.push(store);
            open_at(current.as_fd(), store, libc::O_RDONLY)
        }
        // The walk's start is the release directory itself: nothing is above it.
        None => open_at(current.as_fd(), OsStr::new("."), libc::O_RDONLY),
    }
    .map_err(|error| refused(component_rule(&error), &path))?;
    let refuse = |rule| refused(rule, &path);
    let unavailable = |error: io::Error| refused(RootRule::Unavailable(error.kind()), &path);

    let found = directory.metadata().map_err(unavailable)?;
    rules::store(found.uid(), found.mode(), service_user).map_err(refuse)?;
    rules::filesystem(filesystem_kind(directory.as_fd()).map_err(unavailable)?).map_err(refuse)?;
    // Disjoint as this process sees the two directories...
    let store_chain = chain(directory.as_fd()).map_err(unavailable)?;
    let data_chain = chain(location.data_root)
        .map_err(|error| refused(RootRule::DataRoot(error.kind()), &path))?;
    rules::disjoint(&store_chain, &data_chain).map_err(refuse)?;
    // ...and inside the filesystem, where a bind mount cannot hide what it shows. Any
    // doubt about either place refuses.
    let undetermined = |doubt| refused(RootRule::Undetermined(doubt), &path);
    let table = mount_table().map_err(undetermined)?;
    let table = Table::of(&table).map_err(undetermined)?;
    let (store_place, store_mount, _) = place(directory.as_fd(), &table).map_err(undetermined)?;
    let (data_place, data_mount, data_path) =
        place(location.data_root, &table).map_err(undetermined)?;
    mounts::disjoint(&store_place, &data_place).map_err(refuse)?;
    // Nothing mounted below it, at any depth.
    if let Some(mount_point) = table
        .mounted_inside(path.as_os_str().as_bytes())
        .map_err(undetermined)?
    {
        return Err(refused(
            RootRule::MountInside,
            Path::new(OsStr::from_bytes(&mount_point)),
        ));
    }
    if location.mount_point_required {
        let (attributes, reported) = mount_attributes(directory.as_fd()).map_err(unavailable)?;
        rules::mount_point(attributes, reported).map_err(refuse)?;
        // A volume of its own: not anywhere in what the data root's mount shows.
        let data_volume = mounts::shown(&data_mount).map_err(undetermined)?;
        mounts::separate_volumes(&store_place, &data_volume).map_err(refuse)?;
    }
    // The account's other persistent data (its records, for one), by the same three rules:
    // a data root on a volume of its own (`/data/instance` mounted inside `/data`) must not
    // let the store sit in the volume that holds the rest (X5 review, fourth round, P1).
    let mut kept_apart = vec![data_path];
    for other in account_data {
        let apart = |rule: RootRule| match rule {
            RootRule::DataRoot(kind) => refuse(RootRule::AccountDataUnavailable(kind)),
            _ => refuse(RootRule::AccountData),
        };
        let other_chain = chain(*other)
            .map_err(|error| refuse(RootRule::AccountDataUnavailable(error.kind())))?;
        rules::disjoint(&store_chain, &other_chain).map_err(apart)?;
        let (other_place, other_mount, other_path) = place(*other, &table).map_err(undetermined)?;
        mounts::disjoint(&store_place, &other_place).map_err(apart)?;
        if location.mount_point_required {
            let other_volume = mounts::shown(&other_mount).map_err(undetermined)?;
            mounts::separate_volumes(&store_place, &other_volume).map_err(apart)?;
        }
        kept_apart.push(other_path);
    }
    // Nor reached through another mount (X5 final review): the secrets volume mounted a
    // second time, a folder of it mounted elsewhere, or a mount of a directory that holds
    // it. Asked last, so that the rules above name a store inside the data first.
    let aliases = table
        .aliases(&store_mount, &store_place)
        .map_err(undetermined)?;
    let elsewhere = |alias: &mounts::Alias, rule| {
        refused(rule, Path::new(OsStr::from_bytes(&alias.mount_point)))
    };
    // A mounted secrets volume is mounted once, at the credential directory.
    if let Some(alias) = aliases.first().filter(|_| location.mount_point_required) {
        return Err(elsewhere(alias, RootRule::MountedElsewhere));
    }
    // Everywhere, nothing of it is reached inside the data root or the account's records.
    for directory in &kept_apart {
        for alias in &aliases {
            if alias
                .within(directory.as_os_str().as_bytes())
                .map_err(undetermined)?
            {
                return Err(elsewhere(alias, RootRule::MountedInData));
            }
        }
    }
    Ok((directory, path, service_user))
}

/// Whether the marker already matches. A missing marker is allowed only on a store
/// with no credentials. With no expected ID, any existing binding refuses.
fn check_binding(
    directory: &File,
    path: &Path,
    existing: &[ExistingStore<'_, '_>],
    instance_id: Option<&str>,
) -> Result<bool, CredentialError> {
    let unsafe_marker = |_| refused(RootRule::InstanceMarker, path);
    if let Some(marker) = directory::open_item(directory, ".instance").map_err(unsafe_marker)? {
        let mut bytes = Vec::new();
        marker
            .take(MAX_NAME_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| refused(RootRule::InstanceMarker, path))?;
        let owner = std::str::from_utf8(&bytes)
            .ok()
            .filter(|name| valid_name(name))
            .ok_or_else(|| refused(RootRule::InstanceMarker, path))?;
        if Some(owner) != instance_id {
            return Err(refused(RootRule::WrongInstance, path));
        }
        return Ok(true);
    }
    for (_, path, store) in existing {
        let entries =
            entries(store.as_fd()).map_err(|e| refused(RootRule::Unavailable(e.kind()), path))?;
        // A temporary or malformed item also prevents claiming an unbound store. Opening
        // its item stores could otherwise clean up files from an earlier host.
        if entries.iter().any(|name| name != LOCK_NAME) {
            return Err(refused(RootRule::CredentialsPresent, path));
        }
    }
    Ok(false)
}

fn publish_binding(directory: &File, instance_id: &str) -> Result<(), CredentialError> {
    let temporary = directory::temporary_name()?;
    let mut file = directory::at(
        directory,
        &temporary,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
    )?;
    let temporary = CString::new(temporary).map_err(|_| CredentialError::InvalidName)?;
    let mut publish = || -> io::Result<()> {
        file.set_permissions(Permissions::from_mode(0o600))?;
        file.write_all(instance_id.as_bytes())?;
        file.sync_all()?;
        org_platform::fs::rename_noreplace_at(directory, &temporary, directory, INSTANCE_MARKER)
    };
    let result = publish();
    if result.is_err() {
        // SAFETY: the held directory and the name this call created.
        unsafe { libc::unlinkat(directory.as_raw_fd(), temporary.as_ptr(), 0) };
    }
    result.map_err(Into::into)
}

impl ReleaseRoot {
    pub(in crate::credentials) fn store(
        &self,
        items: &ReleaseItems<'_>,
    ) -> Result<super::Store, CredentialError> {
        // Only a store the directory was opened for: those are the ones that were
        // judged and counted when it opened.
        if !self
            .0
            .stores
            .iter()
            .any(|(directory, suffix)| directory == items.directory && suffix == items.suffix)
        {
            return Err(CredentialError::InvalidName);
        }
        // The engine opens the item store's directory relative to this descriptor,
        // without following a link, and checks it as it checks every directory.
        let anchor = self.0.directory.try_clone()?;
        let store = directory::Store::below(
            anchor,
            self.0.path.clone(),
            &[items.directory],
            true,
            items.sharing,
            items.suffix,
        )?;
        Ok(super::Store {
            directory: store,
            _root: Some(Arc::clone(&self.0)),
        })
    }
}

fn refused(rule: RootRule, path: &Path) -> CredentialError {
    CredentialError::Root(RootRefusal {
        rule,
        path: path.to_owned(),
    })
}

/// The rule a component that could not be opened broke: one that is not a directory,
/// a symbolic link among them (`O_NOFOLLOW` answers `ELOOP`, with `O_PATH` and
/// `O_DIRECTORY` together `ENOTDIR`), or one that is unavailable.
fn component_rule(error: &io::Error) -> RootRule {
    match error.raw_os_error() {
        Some(libc::ELOOP | libc::ENOTDIR) => RootRule::NotADirectory,
        _ => RootRule::Unavailable(error.kind()),
    }
}

/// The service user's private group, asked for at most once per open and only when a
/// directory's rule depends on it.
enum PrivateGroup {
    NotAsked,
    Known(Option<libc::gid_t>),
}

/// Judges the directory `ancestor`, at `path`, as a directory above the release
/// directory.
fn judge_ancestor(
    ancestor: &File,
    path: &Path,
    service_user: libc::uid_t,
    private_group: &mut PrivateGroup,
) -> Result<(), CredentialError> {
    let found = ancestor
        .metadata()
        .map_err(|error| refused(RootRule::Unavailable(error.kind()), path))?;
    let group = if rules::needs_private_group(found.uid(), found.mode(), service_user) {
        if let PrivateGroup::NotAsked = private_group {
            // An account database that cannot be read vouches for no group.
            *private_group =
                PrivateGroup::Known(account::private_group_of(service_user).ok().flatten());
        }
        match private_group {
            PrivateGroup::Known(group) => *group,
            PrivateGroup::NotAsked => None,
        }
    } else {
        None
    };
    let mut facts = Ancestor {
        owner: found.uid(),
        group: found.gid(),
        mode: found.mode(),
        service_user,
        private_group: group,
        access_acl: false,
    };
    rules::ancestor(&facts).map_err(|rule| refused(rule, path))?;
    if rules::needs_access_acl(facts.mode) {
        // Admitted by a group-write exception: only without an access ACL. One that
        // cannot be read is not known to be absent.
        facts.access_acl = account::has_access_acl(ancestor.as_fd())
            .map_err(|error| refused(RootRule::Unavailable(error.kind()), path))?;
        rules::ancestor(&facts).map_err(|rule| refused(rule, path))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
