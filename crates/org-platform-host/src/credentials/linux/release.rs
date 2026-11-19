//! GNU/Linux: the release directory's live probes, and the order of its checks.
//!
//! Every decision is a pure function of `rules.rs`; this file opens the directories,
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
//! (`mounts.rs`).

use std::ffi::{CStr, CString, OsStr, OsString};
use std::fs::{File, Permissions, TryLockError};
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::account;
use crate::credentials::mounts::{self, Mount, Place, Table};
use crate::credentials::rules::{self, Ancestor, Identity};
use crate::credentials::{
    directory, valid_name, CredentialError, ItemStore, MountDoubt, ReleaseDirectory, ReleaseItems,
    RootRefusal, RootRule, MAX_NAME_BYTES,
};

/// The lock file's name. An item store's directory has the syntax of an item name,
/// which never starts with `.`, so none can be the lock.
const LOCK: &CStr = c".lock";
const LOCK_NAME: &str = ".lock";
const INSTANCE_MARKER: &CStr = c".instance";

/// The kernel's table of this process's mounts.
const MOUNT_TABLE: &str = "/proc/self/mountinfo";

/// The most directories a walk up to `/` passes before it gives up: more than any
/// path the kernel resolves can have.
const DEEPEST: usize = 4096;

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

fn owned(fd: libc::c_int) -> io::Result<File> {
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a new descriptor that nothing else owns.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn c_name(name: &OsStr) -> io::Result<CString> {
    CString::new(name.as_bytes()).map_err(|_| io::ErrorKind::InvalidInput.into())
}

/// Opens the directory at the absolute path `path` for passing through. Only the
/// start of a walk is opened by path.
fn open_absolute(path: &Path) -> io::Result<File> {
    let path = c_name(path.as_os_str())?;
    let flags = libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: a NUL-terminated path that outlives the call, which keeps nothing.
    owned(unsafe { libc::open(path.as_ptr(), flags) })
}

/// Opens the directory `name` in `parent`, never following a link: `access` is
/// `O_PATH` to pass through it or `O_RDONLY` to read it.
fn open_at(parent: BorrowedFd<'_>, name: &OsStr, access: libc::c_int) -> io::Result<File> {
    let name = c_name(name)?;
    let flags = access | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: a valid directory descriptor and a NUL-terminated name that outlives
    // the call, which keeps nothing.
    owned(unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) })
}

/// The type number of the filesystem `fd` is on, as `fstatfs` gives it.
fn filesystem_kind(fd: BorrowedFd<'_>) -> io::Result<u64> {
    // SAFETY: an all-zero `statfs` is a valid value for the call to overwrite.
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: a valid descriptor and a valid out-pointer.
    if unsafe { libc::fstatfs(fd.as_raw_fd(), &mut stat) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // The number is 32 bits wide; where the field is a signed 32-bit word, a number
    // with the top bit set arrives sign-extended.
    Ok(stat.f_type as u64 & 0xffff_ffff)
}

fn identity(found: &std::fs::Metadata) -> Identity {
    (found.dev(), found.ino())
}

/// The directory `fd` and every directory above it up to `/`, each identified through
/// a held descriptor: `..` is opened relative to the one before, until a directory is
/// its own parent.
fn chain(fd: BorrowedFd<'_>) -> io::Result<Vec<Identity>> {
    // A descriptor of this function's own for the same open directory.
    let first = File::from(fd.try_clone_to_owned()?).metadata()?;
    if !first.is_dir() {
        return Err(io::ErrorKind::NotADirectory.into());
    }
    let mut chain = vec![identity(&first)];
    let mut above = open_at(fd, OsStr::new(".."), libc::O_PATH)?;
    loop {
        let next = identity(&above.metadata()?);
        if chain.last() == Some(&next) {
            return Ok(chain);
        }
        if chain.len() >= DEEPEST {
            return Err(io::ErrorKind::InvalidData.into());
        }
        chain.push(next);
        above = open_at(above.as_fd(), OsStr::new(".."), libc::O_PATH)?;
    }
}

/// What `statx` says of the directory `fd`: its attributes, and the mask of the
/// attributes this kernel reports.
fn mount_attributes(fd: BorrowedFd<'_>) -> io::Result<(u64, u64)> {
    // SAFETY: an all-zero `statx` is a valid value for the call to overwrite.
    let mut stat: libc::statx = unsafe { std::mem::zeroed() };
    // SAFETY: a valid descriptor, an empty NUL-terminated path with `AT_EMPTY_PATH`
    // (the descriptor itself is examined), and a valid out-pointer.
    let status = unsafe {
        libc::statx(
            fd.as_raw_fd(),
            c"".as_ptr(),
            libc::AT_EMPTY_PATH,
            libc::STATX_TYPE,
            &mut stat,
        )
    };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((stat.stx_attributes, stat.stx_attributes_mask))
}

/// One of the host's item stores that exists below the release directory: its
/// declaration, its path, and its directory open for reading.
type ExistingStore<'a, 'b> = (&'a ItemStore<'b>, PathBuf, File);

/// The host's item stores that exist below the release directory `root` (at
/// `root_path`), each opened without following a link and judged: a directory of the
/// service user with mode exactly `0700`, or its own rule with its own path. One that
/// does not exist is not in the answer; the engine makes it when it is opened. Nothing
/// else in the release directory is looked at.
fn item_store_directories<'a, 'b>(
    root: &File,
    root_path: &Path,
    item_stores: &'a [ItemStore<'b>],
    service_user: libc::uid_t,
) -> Result<Vec<ExistingStore<'a, 'b>>, CredentialError> {
    let mut existing = Vec::new();
    for store in item_stores {
        let path = root_path.join(store.directory);
        let unavailable = |error: io::Error| refused(RootRule::Unavailable(error.kind()), &path);
        // To judge it, not to read it: a directory this user cannot read is judged too.
        let held = match open_at(root.as_fd(), OsStr::new(store.directory), libc::O_PATH) {
            Ok(held) => held,
            Err(error) => match error.raw_os_error() {
                Some(libc::ENOENT) => continue,
                Some(libc::ELOOP | libc::ENOTDIR) => {
                    return Err(refused(RootRule::ItemStoreNotADirectory, &path))
                }
                _ => return Err(unavailable(error)),
            },
        };
        let found = held.metadata().map_err(unavailable)?;
        rules::item_store(found.uid(), found.mode(), service_user)
            .map_err(|rule| refused(rule, &path))?;
        let readable =
            open_at(held.as_fd(), OsStr::new("."), libc::O_RDONLY).map_err(unavailable)?;
        existing.push((store, path, readable));
    }
    Ok(existing)
}

/// How many items the item store `store` holds, in its directory `directory` at
/// `path`: the files the directory store's engine would give the host. A name counts
/// if it is an item name with the store's suffix (the engine's rule) that the host's
/// own name rule accepts, and its file then has to be what the engine requires of an
/// item (a regular file of this user, mode `0600`, one link); one that is not refuses
/// with [`RootRule::ItemUnsafe`] and is not counted as missing. Every other entry is
/// no item of the store.
fn count_items(
    store: &ItemStore<'_>,
    path: &Path,
    directory: &File,
) -> Result<usize, CredentialError> {
    let unavailable = |error: io::Error| refused(RootRule::Unavailable(error.kind()), path);
    let mut items = 0;
    for entry in entries(directory.as_fd()).map_err(unavailable)? {
        let Some(file) = entry.to_str() else { continue };
        let named = directory::item_name(file, store.suffix).is_some_and(store.is_item);
        if !named {
            continue;
        }
        match directory::open_item(directory, file) {
            Ok(Some(_)) => items += 1,
            // Gone again since the directory was read.
            Ok(None) => {}
            Err(CredentialError::Io(kind))
                if is_regular(directory.as_fd(), &entry).map_err(unavailable)? =>
            {
                return Err(refused(RootRule::Unavailable(kind), path))
            }
            // Not a regular file (a link is never followed), or one with another
            // owner, mode or link count.
            Err(_) => return Err(refused(RootRule::ItemUnsafe, path)),
        }
    }
    Ok(items)
}

/// Whether the entry `name` of `directory` is a regular file. A link is not followed;
/// an entry that is gone again is no file.
fn is_regular(directory: BorrowedFd<'_>, name: &OsStr) -> io::Result<bool> {
    let name = c_name(name)?;
    // SAFETY: an all-zero `stat` is a valid value for the call to overwrite.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: a valid directory descriptor, a NUL-terminated name that outlives the
    // call, and a valid out-pointer.
    let status = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            name.as_ptr(),
            &mut stat,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if status != 0 {
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::NotFound {
            Ok(false)
        } else {
            Err(error)
        };
    }
    Ok(stat.st_mode & libc::S_IFMT == libc::S_IFREG)
}

/// Every name in `directory` but `.` and `..`, read through a descriptor of its own,
/// so that the held one's position is never moved.
fn entries(directory: BorrowedFd<'_>) -> io::Result<Vec<OsString>> {
    let fresh = open_at(directory, OsStr::new("."), libc::O_RDONLY)?;
    // SAFETY: a valid directory descriptor. On success the stream owns it; on failure
    // `fresh` still does and closes it.
    let stream = unsafe { libc::fdopendir(fresh.as_raw_fd()) };
    if stream.is_null() {
        return Err(io::Error::last_os_error());
    }
    std::mem::forget(fresh);
    let stream = Stream(stream);
    let mut names = Vec::new();
    loop {
        // `readdir` reports the end of the directory and a failure the same way.
        org_platform::fs::set_errno(0)?;
        // SAFETY: `stream` owns a live directory stream that no other thread reads.
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(0) {
                return Err(error);
            }
            return Ok(names);
        }
        // SAFETY: `readdir`'s NUL-terminated name stays valid until the next call.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if name != b"." && name != b".." {
            names.push(OsString::from_vec(name.to_vec()));
        }
    }
}

struct Stream(*mut libc::DIR);

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: this guard exclusively owns the successful `fdopendir` result.
        unsafe {
            libc::closedir(self.0);
        }
    }
}

/// Opens and takes the lock file of the release directory `directory` (at `path`), never
/// following a link. `O_NONBLOCK`: a FIFO at the name must not block the host; the
/// lock rule then refuses it.
///
/// A missing lock is made by this call alone (`O_EXCL`), and its mode is then set to
/// `0600` on the descriptor that made it: the process's file-creation mask may have
/// taken the owner's own bits away, and a lock made unusable would refuse the store
/// for good. An existing lock is opened as it is and never changed.
///
/// A lock that cannot be opened is refused by the lock's own rule where what stands
/// there visibly breaks it (another owner, another mode, not a regular file), and as
/// [`RootRule::LockUnavailable`] otherwise.
fn open_lock(
    directory: &File,
    path: &Path,
    service_user: libc::uid_t,
    create: bool,
) -> Result<Option<File>, CredentialError> {
    let flags = libc::O_RDWR | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
    let at = directory.as_raw_fd();
    // SAFETY: a valid directory descriptor and a NUL-terminated name; the mode is
    // read only with O_CREAT.
    let opened = if create {
        match owned(unsafe {
            libc::openat(
                at,
                LOCK.as_ptr(),
                flags | libc::O_CREAT | libc::O_EXCL,
                0o600,
            )
        }) {
            Ok(lock) => lock
                .set_permissions(Permissions::from_mode(0o600))
                .map(|()| lock),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                // SAFETY: the same descriptor and name, without O_CREAT.
                owned(unsafe { libc::openat(at, LOCK.as_ptr(), flags) })
            }
            Err(error) => Err(error),
        }
    } else {
        // SAFETY: the same descriptor and name, without O_CREAT.
        match owned(unsafe { libc::openat(at, LOCK.as_ptr(), flags) }) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            opened => opened,
        }
    };
    let unavailable = |kind| refused(RootRule::LockUnavailable(kind), path);
    let lock = opened.map_err(|error| {
        let broken = match error.raw_os_error() {
            // A link, a directory or a device node at the lock's name is no lock file.
            Some(libc::ELOOP | libc::EISDIR | libc::ENXIO) => true,
            // What stands there, as far as it can be seen without opening it.
            _ => std::fs::symlink_metadata(path.join(LOCK_NAME)).is_ok_and(|found| {
                rules::lock_file(
                    found.is_file(),
                    found.uid(),
                    found.mode(),
                    found.nlink(),
                    service_user,
                )
                .is_err()
            }),
        };
        let rule = if broken {
            RootRule::LockFile
        } else {
            RootRule::LockUnavailable(error.kind())
        };
        refused(rule, path)
    })?;
    let found = lock.metadata().map_err(|error| unavailable(error.kind()))?;
    rules::lock_file(
        found.is_file(),
        found.uid(),
        found.mode(),
        found.nlink(),
        service_user,
    )
    .map_err(|rule| refused(rule, path))?;
    match lock.try_lock() {
        Ok(()) => Ok(Some(lock)),
        Err(TryLockError::WouldBlock) => Err(refused(RootRule::InUse, path)),
        Err(TryLockError::Error(error)) => Err(unavailable(error.kind())),
    }
}

/// The bytes of the kernel's mount table of this process, or the doubt that not
/// having them leaves.
fn mount_table() -> Result<Vec<u8>, MountDoubt> {
    let unreadable = |error: io::Error| MountDoubt::Unreadable(error.kind());
    let mut table = Vec::new();
    File::open(MOUNT_TABLE)
        .map_err(unreadable)?
        .take(mounts::LARGEST_TABLE as u64 + 1)
        .read_to_end(&mut table)
        .map_err(unreadable)?;
    Ok(table)
}

/// Where the open directory `fd` is inside its filesystem, the mount it is in, and its
/// path from this process's root: the mount the kernel says it is in (`statx`), found in
/// the mount table `table` by its id, and the directory's path (its descriptor's entry
/// in `/proc/self/fd`), placed below that mount.
///
/// The path is only believed if it names the same directory the descriptor holds: a
/// directory that was deleted or moved has a path that does not.
fn place(fd: BorrowedFd<'_>, table: &Table<'_>) -> Result<(Place, Mount, PathBuf), MountDoubt> {
    let unreadable = |error: io::Error| MountDoubt::Unreadable(error.kind());
    // SAFETY: an all-zero `statx` is a valid value for the call to overwrite.
    let mut stat: libc::statx = unsafe { std::mem::zeroed() };
    // SAFETY: a valid descriptor, an empty NUL-terminated path with `AT_EMPTY_PATH`
    // (the descriptor itself is examined), and a valid out-pointer.
    let status = unsafe {
        libc::statx(
            fd.as_raw_fd(),
            c"".as_ptr(),
            libc::AT_EMPTY_PATH,
            libc::STATX_MNT_ID,
            &mut stat,
        )
    };
    if status != 0 {
        return Err(unreadable(io::Error::last_os_error()));
    }
    if stat.stx_mask & libc::STATX_MNT_ID == 0 {
        return Err(MountDoubt::NoMountId);
    }
    // The mount first: where its root was deleted, no path names the directory.
    let mount = table.mount(stat.stx_mnt_id)?;
    let path =
        std::fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd())).map_err(unreadable)?;
    let held = File::from(fd.try_clone_to_owned().map_err(unreadable)?)
        .metadata()
        .map_err(unreadable)?;
    let named = std::fs::metadata(&path).map_err(|_| MountDoubt::PathOutsideMount)?;
    if identity(&named) != identity(&held) {
        return Err(MountDoubt::PathOutsideMount);
    }
    let place = mounts::place(&mount, path.as_os_str().as_bytes())?;
    Ok((place, mount, path))
}

#[cfg(test)]
mod tests;
