//! The release directory: a release build's protected store where the system has no
//! Keychain (GNU/Linux; Linux host design section 3.2, ADR-0017 decision 8).
//!
//! The host's secrets are owner-only files in one directory outside its data root, at
//! a path setup recorded. [`release_directory`] opens that directory once, checks it
//! and takes the lock that admits one host; the host then opens its item stores below
//! it ([`ReleaseRoot::store`]) with the directory store's engine, so every rule of an
//! item is that engine's (`directory.rs`).
//!
//! # Layout
//!
//! ```text
//! <path>/                            the release directory, mode 0700
//!     .lock                          mode 0600, empty; locked while a host has the directory open
//!     <item store>/                  one directory per item store, mode 0700 (a directory store)
//! ```
//!
//! Anything else in the release directory is left alone and never opened (a volume's
//! `lost+found`, for one).
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
//!    member in its member list). Each is judged before anything below it is opened.
//! 4. The release directory is owned by the service user and its mode is exactly
//!    `0700`.
//! 5. Its filesystem is ext4 (ext2 and ext3 share ext4's type number), XFS, Btrfs or
//!    F2FS. Network, user-space and memory filesystems are refused, and so is every
//!    other one (an overlay, for one): a key that is lost or shared loses or exposes
//!    an office's private keys.
//! 6. It is disjoint from the data root, by the device and inode numbers of held
//!    descriptors, walking `..` from each up to `/`: it is not the data root, not
//!    inside it, and does not contain it.
//! 7. When a mount point is required, it is the root of a mount (`statx`,
//!    `STATX_ATTR_MOUNT_ROOT`). A kernel that cannot say refuses.
//! 8. When items are expected, the directory holds at least one
//!    ([`RootRule::CredentialsMissing`] otherwise; see below).
//! 9. `.lock` is created if missing, must be a regular file of the service user with
//!    mode `0600` and one link, and is locked exclusively (`flock`) until the
//!    [`ReleaseRoot`] and every store opened from it are dropped. A second host is
//!    [`RootRule::InUse`].
//!
//! # A store found empty
//!
//! `expect_items` is the host saying that its records name keys. It is checked here,
//! in [`release_directory`], over the whole release directory and before any item
//! store can be opened: no caller can open one view of an empty store and create a
//! fresh key in it. An item, for this check, is a regular file whose name does not
//! start with `.` (the lock and the temporary files do) in a directory directly below
//! the release directory that the service user owns with mode `0700`. A directory
//! that is not such a one cannot be an item store and is not looked into.
//!
//! The check is about the store as a whole: it notices a release directory that was
//! lost and replaced by an empty one. It does not say that each item a record names
//! is there; the host's views answer that item by item.
//!
//! # Where a path may appear
//!
//! Errors of this capability carry kinds, never bytes, names or paths, with one
//! exception: [`RootRefusal`], which only [`release_directory`] returns, carries the
//! path of the directory that broke a rule. That path is configuration (a directory
//! setup recorded, or one above it), never a secret or an item's name, and the host
//! must be able to say which directory to fix. Item operations and the `Debug` output
//! of every location still show no path.

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

/// The release directory at `location`, opened, checked and locked (the module
/// documentation lists the checks). Nothing below it is opened yet.
///
/// GNU/Linux only. Every other system refuses with
/// [`CredentialError::Refused`] before anything is read: a release build on macOS
/// keeps its secrets in the Keychain, and nothing falls back to a directory.
pub fn release_directory(location: ReleaseDirectory<'_>) -> Result<ReleaseRoot, CredentialError> {
    native::release_directory(&location).map(|root| ReleaseRoot { root })
}

/// One open release directory. It holds the directory's lock, and so does every store
/// opened from it: the lock is released when the last of them is dropped.
pub struct ReleaseRoot {
    // Visible to the adapters' tests, which open a root through an adapter's own entry.
    pub(super) root: native::ReleaseRoot,
}

impl ReleaseRoot {
    /// The item store `items` names below this release directory: a directory store,
    /// with every check that store makes when it opens and on every operation.
    pub fn store(&self, items: ReleaseItems<'_>) -> Result<Credentials, CredentialError> {
        self.root.store(&items).map(|store| Credentials { store })
    }
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
    /// A mount point is required and the release directory is not the root of a mount.
    NotAMountPoint,
    /// A mount point is required and this kernel does not say whether a directory is
    /// one (before Linux 5.8).
    MountPointUnknown,
    /// The host's records say keys exist, and the release directory holds no item.
    CredentialsMissing,
    /// The lock file is not a regular file of the service user with mode `0600` and
    /// one link.
    LockFile,
    /// Another host has the release directory open.
    InUse,
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
            Self::NotAMountPoint => f.write_str(
                "the credential directory must be a mount point (a mounted secrets volume) and \
                 is not one",
            ),
            Self::MountPointUnknown => f.write_str(
                "the credential directory must be a mount point, and this kernel does not say \
                 whether it is one",
            ),
            Self::CredentialsMissing => f.write_str(
                "credentials missing: the host's records say keys exist, and the credential \
                 directory holds none; restore the directory, nothing was created",
            ),
            Self::LockFile => f.write_str(
                "the credential directory's lock file is not a regular file of the service user \
                 with mode 0600 and one link",
            ),
            Self::InUse => f.write_str("the credential directory is in use by another host"),
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
