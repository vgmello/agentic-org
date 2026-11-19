//! The release directory: a release build's protected store where the system has no
//! Keychain (GNU/Linux; Linux host design section 3.2, ADR-0017 decision 8).
//!
//! The host's secrets are owner-only files in one directory outside its data root, at
//! a path setup recorded. [`release_directory_with_stores`] opens that directory once,
//! checks it and takes the lock that admits one host; the host then opens its item
//! stores below it ([`ReleaseRoot::store`]) with the directory store's engine, so
//! every rule of an item is that engine's (`directory.rs`).
//!
//! # Layout
//!
//! ```text
//! <path>/                            the release directory, mode 0700
//!     .lock                          mode 0600, empty; locked while a host has the directory open
//!     <item store>/                  one directory per item store, mode 0700 (a directory store)
//! ```
//!
//! The host names its item stores when it opens the directory ([`ItemStore`]): only
//! those are looked into, and only those can be opened. Anything else in the release
//! directory is not the store's: it is never opened, changed or counted, whatever it
//! is. A volume's root has a `lost+found` that root owns, and it is left alone like
//! every other unknown entry.
//!
//! # Checks when the directory opens
//!
//! In this order; the first one that fails refuses, with the rule and the path of the
//! directory that broke it ([`RootRefusal`]). Nothing is created before the last check
//! but one has passed, and nothing ever repairs an owner or a mode.
//!
//! 1. The path is absolute and has no `..` component.
//! 2. It is walked from `/`, one component at a time, each opened relative to the one
//!    before with `openat(O_DIRECTORY | O_NOFOLLOW)`: no component is a symbolic link.
//! 3. Every directory above the release directory, `/` included, is owned by root or
//!    by the service user (this process's effective user), is not writable by others,
//!    and is not writable by its group, with two exceptions: a root-owned directory
//!    with the sticky bit, and a directory of the service user whose group is that
//!    user's private group (its primary group, named as the user, with no other
//!    member in its member list). A directory admitted by either exception carries no
//!    POSIX access ACL. Each is judged before anything below it is opened.
//! 4. The release directory is owned by the service user and its mode is exactly
//!    `0700`.
//! 5. Its filesystem is ext4 (ext2 and ext3 share ext4's type number), XFS, Btrfs or
//!    F2FS. Network, user-space and memory filesystems are refused, and so is every
//!    other one (an overlay, for one): a key that is lost or shared loses or exposes
//!    an office's private keys.
//! 6. It is disjoint from the data root as this process sees the two, by the device
//!    and inode numbers of held descriptors, walking `..` from each up to `/`: it is
//!    not the data root, not inside it, and does not contain it.
//! 7. It is disjoint from the data root inside the filesystem as well. A bind mount
//!    shows a directory somewhere else, and the walk of check 6 then leaves through
//!    the mount point, not through the directory's own parent. So each of the two is
//!    placed in its filesystem from the mount table (`/proc/self/mountinfo`: the root
//!    its mount shows, then its path below the mount point), and two places in one
//!    filesystem must not be equal or inside each other. Where the place of one of
//!    the two cannot be had, the directory is refused ([`RootRule::Undetermined`]),
//!    never accepted on doubt. The two are found in the table by the mount ids the
//!    kernel gives for their descriptors, so for this check the machine's other mounts
//!    are not read beyond their ids and cannot refuse a store, whatever they are.
//!
//!    Nothing is mounted inside it either: no mount of the mount table has a mount
//!    point below its path ([`RootRule::MountInside`], naming that mount point). A
//!    volume mounted there, whatever its filesystem, owner or mode, would take whatever
//!    is written below it out of the store's volume (two volumes of one filesystem share
//!    a device number, so nothing else tells them apart). Only for this is every line's
//!    mount point read; a line whose mount point cannot be read refuses
//!    ([`RootRule::Undetermined`]).
//! 8. When a mount point is required (a container's mounted secrets volume), it is
//!    the root of a mount (`statx`, `STATX_ATTR_MOUNT_ROOT`; a kernel that cannot say
//!    refuses), and it is on another volume than the data: it is not inside the
//!    directory that the data root's mount shows, is not that directory, and does not
//!    contain it ([`RootRule::InsideDataVolume`], [`RootRule::ContainsDataVolume`]).
//!    Check 7 compares the store with the data root itself, and a data root need not
//!    be the top of its volume: with the data volume at `/data` and the instance's
//!    root at `/data/instance`, `-v D:/data -v D/secrets:/secrets` puts the store
//!    beside the instance root and inside the data volume. Where no mount point is
//!    required this is not asked: the data root's mount is then the disk's own, which
//!    shows everything on it.
//! 9. Each of the host's item stores that exists is a directory of the service user
//!    with mode exactly `0700`, not followed if it is a link
//!    ([`RootRule::ItemStoreOwner`], [`RootRule::ItemStoreMode`],
//!    [`RootRule::ItemStoreNotADirectory`], each naming the item store). One that does
//!    not exist yet is made when it is opened.
//! 10. When items are expected, the item stores hold at least one item
//!     ([`RootRule::CredentialsMissing`] otherwise; see below).
//! 11. `.lock` must be a regular file of the service user with mode `0600` and one
//!     link, and is locked exclusively (`flock`) until the [`ReleaseRoot`] and every
//!     store opened from it are dropped. A second host is [`RootRule::InUse`]. A
//!     missing lock is made, and its mode is set on the descriptor that made it,
//!     whatever the process's file-creation mask is; an existing one is never
//!     changed.
//!
//! # A development build's key folder
//!
//! A development build keeps its development stores in a folder of plain files. In a
//! container (the foreground mode) that folder is one folder directly in the release
//! directory, the mounted secrets volume ([`ReleaseRoot::development_folder`]): made there
//! (mode `0700`) if it is missing, from the release directory's own descriptor, and opened
//! from it without following a link. It must be a directory of the service user with mode
//! exactly `0700`, and it must be on the release directory's own mount: the kernel's mount
//! id for it (`statx`, `STATX_MNT_ID`; a kernel that cannot say refuses) is the release
//! directory's. Another volume mounted on it is refused whatever its filesystem, owner or
//! mode, so that a subdirectory of the data volume mounted at `/secrets/dev-keys` (two
//! named volumes on one filesystem have the same device number) never holds the keys.
//! Nothing is made inside such a mount: `mkdirat` finds its mount point and makes nothing.
//! The development stores the host opens from the folder ([`DevelopmentFolder::store`])
//! hold the release directory's lock while they live, as an item store does.
//!
//! # What the private-group exception does not establish
//!
//! "Private" is what two entries of the account database say: the group is the
//! service user's primary group, has the user's name, and lists no other member. An
//! account that has the same group as its *primary* group is in no member list and is
//! not detected; it could rename a group-writable directory above the store and put an
//! older copy of the store in its place. Accounts are not enumerated to find one
//! (`crate::account` says why). A deployment that needs this closed keeps the store
//! below directories its group cannot write.
//!
//! # A store found empty
//!
//! `expect_items` is the host saying that its records name keys. It is checked here,
//! when the directory opens, over all of the host's item stores and before any of them
//! can be opened: no caller can open one view of an empty store and create a fresh key
//! in it.
//!
//! An item, for this check, is exactly what the directory store's engine would give
//! the host: a file in one of the item stores whose name is an item name followed by
//! that store's suffix, which the host's own name rule for that store accepts
//! ([`ItemStore::is_item`]), and which is a regular file of the service user with mode
//! `0600` and one link. A file with such a name that is not such a file is not counted
//! as missing: it refuses by its own rule ([`RootRule::ItemUnsafe`], naming the item
//! store), so that a store restored without its modes is reported as that. Other files
//! in an item store, and everything outside the item stores, are not items.
//!
//! The check is about the store as a whole: it notices a release directory that was
//! lost and replaced by an empty one. It does not say that each item a record names
//! is there; the host's views answer that item by item.
//!
//! # Where a path may appear
//!
//! Errors of this capability carry kinds, never bytes, names or paths, with one
//! exception: [`RootRefusal`], which only the entries of this module return, carries
//! the path of the directory that broke a rule. That path is configuration (a
//! directory setup recorded, one above it, or one of the host's item stores), never a
//! secret or an item's name, and the host must be able to say which directory to fix.
//! Item operations and the `Debug` output of every location still show no path.

use std::fmt;
use std::io;
use std::os::fd::BorrowedFd;
use std::path::{Path, PathBuf};

use super::{native, CredentialError, Credentials, Sharing};

/// Where a release directory is, and what the host knows about it.
#[derive(Clone, Copy)]
pub struct ReleaseDirectory<'a> {
    /// The directory, as setup recorded it: absolute, without `..`.
    pub path: &'a Path,
    /// The host's data root, as a directory descriptor the host holds. The release
    /// directory must be disjoint from it.
    pub data_root: BorrowedFd<'a>,
    /// Whether the release directory must be the root of a mount (a container's
    /// mounted secrets volume).
    pub mount_point_required: bool,
    /// Whether the host's records say that keys exist. An empty store is then refused.
    pub expect_items: bool,
}

// No path in the `Debug` output of a location.
impl fmt::Debug for ReleaseDirectory<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReleaseDirectory")
            .field("path", &"<redacted>")
            .field("mount_point_required", &self.mount_point_required)
            .field("expect_items", &self.expect_items)
            .finish_non_exhaustive()
    }
}

/// One item store below a release directory.
#[derive(Clone, Copy, Debug)]
pub struct ReleaseItems<'a> {
    /// The store's directory: one name directly below the release directory, with the
    /// syntax of an item name. It is made if it is missing (mode `0700`).
    pub directory: &'a str,
    /// What follows an item's name in its file name; may be empty.
    pub suffix: &'a str,
    /// Whether this handle admits other handles on the same item store. The release
    /// directory itself always admits one host only.
    pub sharing: Sharing,
}

/// One of the host's item stores, as the host declares it when it opens the release
/// directory.
#[derive(Clone, Copy, Debug)]
pub struct ItemStore<'a> {
    /// The store's directory: one name directly below the release directory, with the
    /// syntax of an item name.
    pub directory: &'a str,
    /// What follows an item's name in its file name; may be empty.
    pub suffix: &'a str,
    /// The host's own name rule for this store's items, as it gives it to
    /// [`Credentials::list`]: asked only about names that are an item name.
    pub is_item: fn(&str) -> bool,
}

/// The release directory at `location`, opened, checked and locked, for a host whose
/// item stores are `item_stores` (the module documentation lists the checks). Only
/// those stores can be opened from the result, and none is opened yet.
///
/// GNU/Linux only. Every other system refuses with
/// [`CredentialError::Refused`] before anything is read: a release build on macOS
/// keeps its secrets in the Keychain, and nothing falls back to a directory.
pub fn release_directory_with_stores(
    location: ReleaseDirectory<'_>,
    item_stores: &[ItemStore<'_>],
) -> Result<ReleaseRoot, CredentialError> {
    native::release_directory(&location, item_stores).map(|root| ReleaseRoot { root })
}

/// [`release_directory_with_stores`] for a caller that opens no item store: the
/// directory is opened, checked and locked, and nothing can be opened from the result.
/// Setup checks a directory it has just made this way. With `expect_items` it always
/// refuses: no item store is declared, so none holds an item.
pub fn release_directory(location: ReleaseDirectory<'_>) -> Result<ReleaseRoot, CredentialError> {
    release_directory_with_stores(location, &[])
}

/// One open release directory. It holds the directory's lock, and so does every store
/// opened from it: the lock is released when the last of them is dropped.
pub struct ReleaseRoot {
    // Visible to the adapters' tests, which open a root through an adapter's own entry.
    pub(super) root: native::ReleaseRoot,
}

impl ReleaseRoot {
    /// The item store `items` names below this release directory: a directory store,
    /// with every check that store makes when it opens and on every operation. It must
    /// be one of the stores the directory was opened for, by its directory and its
    /// suffix; any other is [`CredentialError::InvalidName`], and nothing is made.
    pub fn store(&self, items: ReleaseItems<'_>) -> Result<Credentials, CredentialError> {
        self.root.store(&items).map(|store| Credentials { store })
    }

    /// A development build's key folder `name` directly in this release directory, made
    /// if it is missing and checked (the module documentation says how). `name` has the
    /// syntax of an item name and is none of the item stores the directory was opened
    /// for, or the answer is [`CredentialError::InvalidName`] and nothing is made. A
    /// folder that breaks a rule is [`CredentialError::Root`] with the rule and the
    /// folder's path; it is never repaired.
    pub fn development_folder(&self, name: &str) -> Result<DevelopmentFolder, CredentialError> {
        self.root
            .development_folder(name)
            .map(|folder| DevelopmentFolder { folder })
    }
}

/// A development build's key folder in a release directory, opened and checked
/// ([`ReleaseRoot::development_folder`]). It holds the release directory's lock, and so
/// does every store opened from it.
pub struct DevelopmentFolder {
    folder: native::DevelopmentFolder,
}

impl DevelopmentFolder {
    /// The directory store that the single names `below` lead to from this folder (made
    /// if missing), opened from the folder's own descriptor: a directory store with every
    /// check it makes when it opens and on every operation.
    pub fn store(
        &self,
        below: &[&str],
        sharing: Sharing,
        suffix: &str,
    ) -> Result<Credentials, CredentialError> {
        self.folder
            .store(below, sharing, suffix)
            .map(|store| Credentials { store })
    }
}

// As for `ReleaseRoot`: a drop on every target.
impl Drop for DevelopmentFolder {
    fn drop(&mut self) {}
}

impl fmt::Debug for DevelopmentFolder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DevelopmentFolder")
    }
}

// Dropping the handle gives up its hold on the directory's lock. It is a drop on every
// target, including those where no value of the adapter's type exists, so that a
// consumer that drops one on purpose reads the same to every target's lints.
impl Drop for ReleaseRoot {
    fn drop(&mut self) {}
}

impl fmt::Debug for ReleaseRoot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ReleaseRoot")
    }
}

/// Why a release directory was refused: the rule, and the directory that broke it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootRefusal {
    /// The rule that was broken.
    pub rule: RootRule,
    /// The directory that broke it: the release directory, or one above it. For a
    /// rule about the path itself, the path as it was given.
    pub path: PathBuf,
}

impl fmt::Display for RootRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.rule)
    }
}

impl std::error::Error for RootRefusal {}

/// A rule of the release directory. Each is one check of [`release_directory`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootRule {
    /// The path is not absolute.
    Relative,
    /// The path has a `..` component.
    ParentComponent,
    /// The path holds a NUL byte.
    NotAPath,
    /// A component of the path is not a directory. A symbolic link is one such: no
    /// component is followed.
    NotADirectory,
    /// A component of the path could not be opened or examined, with this kind of
    /// error (a missing directory is [`io::ErrorKind::NotFound`]).
    Unavailable(io::ErrorKind),
    /// A directory above the release directory is owned by neither root nor the
    /// service user.
    AncestorOwner,
    /// A directory above the release directory is writable by others.
    AncestorWritableByOthers,
    /// A directory above the release directory is writable by its group, and is
    /// neither a root-owned sticky directory nor a directory of the service user
    /// whose group is that user's private group.
    AncestorWritableByGroup,
    /// A directory above the release directory is writable by its group under one of
    /// the two exceptions, and carries a POSIX access ACL: who can write to it is then
    /// not what its mode says.
    AncestorAcl,
    /// The release directory is not owned by the service user.
    StoreOwner,
    /// The release directory's mode is not exactly `0700`.
    StoreMode,
    /// The release directory is on a filesystem that cannot hold the store.
    Filesystem {
        /// Which kind of filesystem was found.
        class: FilesystemClass,
        /// The filesystem's type number, as `statfs` gives it.
        kind: u64,
    },
    /// The release directory is the data root.
    IsDataRoot,
    /// The release directory is inside the data root.
    InsideDataRoot,
    /// The release directory contains the data root.
    ContainsDataRoot,
    /// The data root's descriptor could not be examined, with this kind of error.
    DataRoot(io::ErrorKind),
    /// Whether the release directory and the data root share a directory inside one
    /// filesystem could not be determined from the mount table, for this reason. It
    /// is refused, never assumed.
    Undetermined(MountDoubt),
    /// A mount point is required, and the release directory is inside the directory
    /// that the data root's mount shows, or is that directory: the secrets are in the
    /// data volume.
    InsideDataVolume,
    /// A mount point is required, and the release directory contains the directory
    /// that the data root's mount shows: the data volume is in the secrets volume.
    ContainsDataVolume,
    /// A mount point is required and the release directory is not the root of a mount.
    NotAMountPoint,
    /// Something is mounted inside the release directory. The path is the mount point.
    MountInside,
    /// A mount point is required and this kernel does not say whether a directory is
    /// one (before Linux 5.8).
    MountPointUnknown,
    /// The host's records say keys exist, and the release directory holds no item.
    CredentialsMissing,
    /// One of the host's item stores is not owned by the service user. The path is
    /// the item store's.
    ItemStoreOwner,
    /// The mode of one of the host's item stores is not exactly `0700`. The path is
    /// the item store's.
    ItemStoreMode,
    /// What stands where one of the host's item stores goes is not a directory (a
    /// link is not followed). The path is the item store's.
    ItemStoreNotADirectory,
    /// Items are expected, and a file named like an item is not a regular file of the
    /// service user with mode `0600` and one link. The path is its item store's; the
    /// item is not named.
    ItemUnsafe,
    /// The lock file is not a regular file of the service user with mode `0600` and
    /// one link.
    LockFile,
    /// The lock file could not be opened or made, with this kind of error, and
    /// nothing shows that it breaks [`RootRule::LockFile`].
    LockUnavailable(io::ErrorKind),
    /// Another host has the release directory open.
    InUse,
    /// What stands where the development key folder goes is not a directory (a link is
    /// not followed). The path is the folder's.
    DevelopmentFolderNotADirectory,
    /// The development key folder could not be made, opened or examined, with this kind
    /// of error. The path is the folder's.
    DevelopmentFolderUnavailable(io::ErrorKind),
    /// The development key folder is on another mount than the release directory: a
    /// volume mounted on it. The path is the folder's.
    DevelopmentFolderOnAnotherMount,
    /// This kernel does not say which mount the development key folder or the release
    /// directory is on (before Linux 5.8). The path is the folder's.
    DevelopmentFolderMountUnknown,
    /// The development key folder is not owned by the service user. The path is the
    /// folder's.
    DevelopmentFolderOwner,
    /// The development key folder's mode is not exactly `0700`. The path is the folder's.
    DevelopmentFolderMode,
}

impl fmt::Display for RootRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Relative => f.write_str("the credential directory's path is not absolute"),
            Self::ParentComponent => {
                f.write_str("the credential directory's path has a `..` component")
            }
            Self::NotAPath => f.write_str("the credential directory's path holds a NUL byte"),
            Self::NotADirectory => f.write_str(
                "this part of the credential directory's path is not a directory (a symbolic \
                 link is never followed)",
            ),
            Self::Unavailable(kind) => write!(
                f,
                "this part of the credential directory's path could not be opened or examined: \
                 {kind}"
            ),
            Self::AncestorOwner => f.write_str(
                "this directory, above the credential directory, is owned by neither root nor \
                 the service user",
            ),
            Self::AncestorWritableByOthers => {
                f.write_str("this directory, above the credential directory, is writable by others")
            }
            Self::AncestorWritableByGroup => f.write_str(
                "this directory, above the credential directory, is writable by its group (only \
                 a root-owned sticky directory may be, or a directory of the service user whose \
                 group is that user's private group)",
            ),
            Self::AncestorAcl => f.write_str(
                "this directory, above the credential directory, is writable by its group and \
                 carries an access control list, so its mode does not say who can write to it; \
                 remove the list (setfacl -b) or the group's write access",
            ),
            Self::StoreOwner => {
                f.write_str("the credential directory is not owned by the service user")
            }
            Self::StoreMode => f.write_str("the credential directory's mode is not 0700"),
            Self::Filesystem { class, kind } => write!(
                f,
                "the credential directory must be on ext4, XFS, Btrfs or F2FS; network, \
                 user-space and memory filesystems are refused, and so is every other one \
                 (found {class}, type {kind:#x})"
            ),
            Self::IsDataRoot => f.write_str("the credential directory is the data root"),
            Self::InsideDataRoot => f.write_str("the credential directory is inside the data root"),
            Self::ContainsDataRoot => {
                f.write_str("the credential directory contains the data root")
            }
            Self::DataRoot(kind) => write!(
                f,
                "the data root could not be compared with the credential directory: {kind}"
            ),
            Self::Undetermined(doubt) => write!(
                f,
                "whether the credential directory and the data root share a directory could \
                 not be determined from the mount table ({doubt}); it is refused, not assumed"
            ),
            Self::InsideDataVolume => f.write_str(
                "the credential directory is inside the data volume (the directory that the \
                 data root's mount shows); the secrets and the data must be two separate \
                 volumes, not one inside the other",
            ),
            Self::ContainsDataVolume => f.write_str(
                "the credential directory contains the data volume (the directory that the \
                 data root's mount shows); the secrets and the data must be two separate \
                 volumes, not one inside the other",
            ),
            Self::NotAMountPoint => f.write_str(
                "the credential directory must be a mount point (a mounted secrets volume) and \
                 is not one",
            ),
            Self::MountInside => f.write_str(
                "another volume is mounted here, inside the credential directory; the keys \
                 stay in the credential directory's own volume, so mount nothing inside it",
            ),
            Self::MountPointUnknown => f.write_str(
                "the credential directory must be a mount point, and this kernel does not say \
                 whether it is one",
            ),
            Self::CredentialsMissing => f.write_str(
                "credentials missing: the host's records say keys exist, and the credential \
                 directory holds none; restore the directory, nothing was created",
            ),
            Self::ItemStoreOwner => f.write_str(
                "this item store of the credential directory is not owned by the service user",
            ),
            Self::ItemStoreMode => f.write_str(
                "the mode of this item store of the credential directory is not 0700 (a \
                 restore that lost the modes leaves this; set it back, nothing is repaired here)",
            ),
            Self::ItemStoreNotADirectory => f.write_str(
                "this item store of the credential directory is not a directory (a symbolic \
                 link is never followed)",
            ),
            Self::ItemUnsafe => f.write_str(
                "an item in this item store of the credential directory is not a regular file \
                 of the service user with mode 0600 and one link (a restore that lost the modes \
                 leaves this; set them back, nothing is repaired here)",
            ),
            Self::LockFile => f.write_str(
                "the credential directory's lock file (.lock) is not a regular file of the \
                 service user with mode 0600 and one link",
            ),
            Self::LockUnavailable(kind) => write!(
                f,
                "the credential directory's lock file (.lock) could not be opened: {kind}"
            ),
            Self::InUse => f.write_str("the credential directory is in use by another host"),
            Self::DevelopmentFolderNotADirectory => f.write_str(
                "the development key folder in the credential directory is not a folder of \
                 its own (a symbolic link is never followed)",
            ),
            Self::DevelopmentFolderUnavailable(kind) => write!(
                f,
                "the development key folder in the credential directory could not be made or \
                 examined: {kind}"
            ),
            Self::DevelopmentFolderOnAnotherMount => f.write_str(
                "the development key folder is another volume mounted in the credential \
                 directory; the keys stay in the secrets volume itself, so mount nothing there",
            ),
            Self::DevelopmentFolderMountUnknown => f.write_str(
                "this kernel does not say whether the development key folder is on the \
                 credential directory's own mount",
            ),
            Self::DevelopmentFolderOwner => f.write_str(
                "the development key folder in the credential directory is not owned by the \
                 service user",
            ),
            Self::DevelopmentFolderMode => f.write_str(
                "the development key folder's mode is not 0700 (nothing is repaired here)",
            ),
        }
    }
}

/// Why the mount table could not say where a directory is inside its filesystem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountDoubt {
    /// The kernel names no mount for a directory (before Linux 5.8).
    NoMountId,
    /// The mount table, or a directory's own path, could not be read, with this kind
    /// of error (no `/proc`, for one).
    Unreadable(io::ErrorKind),
    /// The mount table is not what the kernel writes, where it matters: a line whose
    /// mount id cannot be read (it could be the line of a mount that is looked for), a
    /// table that was cut off, or the line of a mount that is looked for.
    Malformed,
    /// The mount a directory is in shows no directory: its root is not a path (the
    /// mount of a namespace file, for one).
    RootNotAPath,
    /// The mount a directory is in is not in the mount table.
    NotListed,
    /// The mount a directory is in is in the mount table more than once.
    Ambiguous,
    /// The mount a directory is in shows a directory that was deleted.
    DeletedRoot,
    /// A directory's path is not below the mount point of its mount: this process
    /// cannot reach it by a path, or it was moved or deleted while it was examined.
    PathOutsideMount,
}

impl fmt::Display for MountDoubt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoMountId => f.write_str("this kernel names no mount for a directory"),
            Self::Unreadable(kind) => write!(f, "the mount table could not be read: {kind}"),
            Self::Malformed => f.write_str("the mount table is not what the kernel writes"),
            Self::RootNotAPath => f.write_str("a directory's mount shows no directory"),
            Self::NotListed => f.write_str("a directory's mount is not in the mount table"),
            Self::Ambiguous => {
                f.write_str("a directory's mount is in the mount table more than once")
            }
            Self::DeletedRoot => {
                f.write_str("a directory's mount shows a directory that was deleted")
            }
            Self::PathOutsideMount => f.write_str(
                "a directory has no path below its mount (unreachable, moved or deleted)",
            ),
        }
    }
}

/// The kind of filesystem a release directory was refused for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilesystemClass {
    /// A network or cluster filesystem (NFS, SMB, 9p, Ceph and the like).
    Network,
    /// A user-space filesystem (FUSE, which sshfs, virtiofs and Docker Desktop's file
    /// sharing use).
    UserSpace,
    /// A memory filesystem (`tmpfs`, `ramfs`, `hugetlbfs`): Docker and Kubernetes
    /// secrets are such mounts, and are not a store.
    Memory,
    /// Any other filesystem that is not one of the four the store accepts.
    Other,
}

impl fmt::Display for FilesystemClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Network => "a network filesystem",
            Self::UserSpace => "a user-space filesystem",
            Self::Memory => "a memory filesystem",
            Self::Other => "another filesystem",
        })
    }
}
