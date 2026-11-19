//! Where the host keeps its secrets: one item store behind one contract.
//!
//! Linux host design section 3.2. The host (`org-host`) is the only consumer. It keeps
//! the policy: which store a build uses, the Keychain access group and service names,
//! the directory, which names exist and which of them are add-only, what a value means
//! and every text it logs. It passes that policy here as plain data
//! ([`CredentialLocation`]). This module keeps bytes under names and applies the
//! system's protection; it names no service, group, instance or product.
//!
//! # What the contract promises
//!
//! - [`Credentials::add`] only adds. An item that is already there is
//!   [`CredentialError::Exists`] and is left as it is; a failed add leaves no item.
//! - [`Credentials::replace`] stores the value whether or not the item exists.
//! - [`Credentials::load`] of a missing item is `Ok(None)`;
//!   [`Credentials::delete`] of a missing item is `Ok(())`.
//! - A value is at most [`MAX_ITEM_BYTES`] long. A longer one is refused before
//!   anything is written, and a stored item that is longer is
//!   [`CredentialError::Corrupt`].
//! - No error, `Debug` or `Display` output holds a secret byte or an item's name:
//!   errors are kinds, a Keychain status or an I/O error kind. None holds a path
//!   either, with one exception: a release directory that is refused when it opens
//!   ([`CredentialError::Root`]) names the directory that broke a rule, because that
//!   path is configuration, not a secret, and the host has to say which directory to
//!   fix. A [`Secret`] zeroes its bytes when dropped and prints as `<redacted>`.
//!
//! # Systems
//!
//! - macOS: [`CredentialLocation::Keychain`] is the Data Protection keychain: generic
//!   passwords of one service in one access group, readable after the first unlock on
//!   this device only and never synchronized. [`CredentialLocation::Directory`] is the
//!   directory store below.
//!   A release build's protected store is the Keychain ([`protected_store`]).
//! - GNU/Linux: the directory store. A Keychain location is refused. A release
//!   build's protected store is a release directory ([`release_directory`]): a
//!   checked, locked directory outside the data root, below which the host opens
//!   directory stores.
//! - Every other Unix target: every location is refused, and there is no protected
//!   store.
//!
//! # The directory store
//!
//! One directory of owner-only files: every development build's store on macOS and
//! GNU/Linux, and the store of each item store below a release directory. Its layout
//! and every check it makes are described in `directory.rs`; the release directory's
//! own checks are described in `release.rs`.

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]

use std::fmt;
use std::io;
use std::path::Path;

use org_platform::Refused;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as native;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod linux;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use linux as native;

// Unused in a build for macOS or GNU/Linux, where only its tests call it.
#[cfg_attr(
    any(target_os = "macos", all(target_os = "linux", target_env = "gnu")),
    allow(dead_code)
)]
mod refused;
#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
use refused as native;

// The directory store: code only the two real adapters use. It is not compiled for a
// refusing target.
#[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
mod directory;

#[cfg(all(
    test,
    any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))
))]
mod adapter_tests;

// The release directory's contract: its types and its entry, on every target.
mod release;
pub use release::{
    release_directory, release_directory_with_stores, FilesystemClass, ItemStore, MountDoubt,
    ReleaseDirectory, ReleaseItems, ReleaseRoot, RootRefusal, RootRule,
};

// The release directory's rules, pure. Only the GNU/Linux adapter calls them; every
// target tests them.
#[cfg(any(all(target_os = "linux", target_env = "gnu"), test))]
mod rules;

// Where a directory is inside its filesystem, from the mount table, pure. As above.
#[cfg(any(all(target_os = "linux", target_env = "gnu"), test))]
mod mounts;

#[cfg(any(test, feature = "test-support"))]
pub(crate) mod testing;

#[cfg(test)]
mod tests;

/// The longest value an item holds, in bytes.
pub const MAX_ITEM_BYTES: usize = 16 * 1024;

/// The longest [`ItemName`], in bytes.
pub const MAX_NAME_BYTES: usize = 128;

/// Where one item store is. The host chooses the variant; nothing here chooses one
/// for it or falls back from one to the other.
#[derive(Clone, Copy)]
pub enum CredentialLocation<'a> {
    /// The system's protected store for a release: on macOS the Data Protection
    /// keychain. Refused on every system without one.
    Keychain(KeychainItems<'a>),
    /// A directory of owner-only files.
    Directory(DirectoryStore<'a>),
}

// No path in any output of this capability: a location prints which kind of store it
// is and nothing of where.
impl fmt::Debug for CredentialLocation<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Keychain(items) => f.debug_tuple("Keychain").field(items).finish(),
            Self::Directory(store) => f.debug_tuple("Directory").field(store).finish(),
        }
    }
}

impl fmt::Debug for DirectoryStore<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DirectoryStore")
            .field("anchor", &"<redacted>")
            .field("below", &"<redacted>")
            .field("create", &self.create)
            .field("sharing", &self.sharing)
            .field("suffix", &self.suffix)
            .finish()
    }
}

/// The Keychain items of one store: every item is a generic password of `service` in
/// `access_group`, and its account is the item's name.
#[derive(Clone, Copy, Debug)]
pub struct KeychainItems<'a> {
    /// One of the host's own `keychain-access-groups`. The host checks its own
    /// entitlement and hardened runtime before it asks for this store.
    pub access_group: &'a str,
    /// The service the items are filed under. It is persisted state: another string is
    /// another, empty, store.
    pub service: &'a str,
}

/// One directory store.
#[derive(Clone, Copy)]
pub struct DirectoryStore<'a> {
    /// An existing directory, absolute and without `..`, owned by this user with mode
    /// exactly `0700`. Its last component is not followed if it is a symbolic link.
    pub anchor: &'a Path,
    /// The store's directory, as single names below `anchor` (none: the anchor itself
    /// is the store). Each is opened without following a link and must be owned by this
    /// user with mode exactly `0700`.
    pub below: &'a [&'a str],
    /// Whether a missing directory in `below` is created (mode `0700`). Without it a
    /// missing one is [`CredentialError::Io`] with [`io::ErrorKind::NotFound`], and
    /// nothing is created but the store's lock file.
    pub create: bool,
    /// Whether this handle admits other handles on the same directory.
    pub sharing: Sharing,
    /// What follows an item's name in its file name, for example `.key`; may be empty.
    /// Files that do not end in it are not items of this store.
    pub suffix: &'a str,
}

/// Who else may hold a directory store while this handle lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sharing {
    /// Nobody: a second handle, in this or another process, is
    /// [`CredentialError::InUse`] until this one is dropped.
    Exclusive,
    /// Other shared handles. An exclusive handle is refused while any shared one lives.
    Shared,
}

/// The item store at `location`.
///
/// For a Keychain location nothing is read or written: the first operation is the
/// first call into the Keychain. For a directory location the directory is opened,
/// checked and locked, and temporary files a crashed writer left are removed.
pub fn credentials(location: CredentialLocation<'_>) -> Result<Credentials, CredentialError> {
    native::open(location).map(|store| Credentials { store })
}

/// The kind of protected store a release build keeps its secrets in on this system.
/// The host selects its store on this value and never on the system's name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtectedStore {
    /// The Data Protection keychain (macOS): [`CredentialLocation::Keychain`] opens it.
    Keychain,
    /// A release directory (GNU/Linux): [`release_directory`] opens it.
    ReleaseDirectory,
}

/// Which protected store a release build has on this system: the Keychain on macOS, a
/// release directory on GNU/Linux. Every other target has none and refuses.
pub fn protected_store() -> Result<ProtectedStore, Refused> {
    native::protected_store()
}

/// One open item store. Dropping it releases a directory store's lock.
pub struct Credentials {
    store: native::Store,
}

impl Credentials {
    /// Store `value` as the new item `name`. An existing item is
    /// [`CredentialError::Exists`] and keeps its value.
    pub fn add(&self, name: &ItemName, value: &[u8]) -> Result<(), WriteError> {
        self.store.add(name, value)
    }

    /// Store `value` as item `name`, replacing the item's value if it exists.
    pub fn replace(&self, name: &ItemName, value: &[u8]) -> Result<(), WriteError> {
        self.store.replace(name, value)
    }

    /// The value of item `name`, or `None` if there is no such item.
    pub fn load(&self, name: &ItemName) -> Result<Option<Secret>, CredentialError> {
        self.store.load(name)
    }

    /// Delete item `name`. A missing item is `Ok`.
    pub fn delete(&self, name: &ItemName) -> Result<(), CredentialError> {
        self.store.delete(name)
    }

    /// The names of the store's items for which `is_item` answers true, sorted, never
    /// their values. `is_item` is the consumer's own name rule; the store asks it only
    /// about names that are an [`ItemName`]. Every other entry is counted in
    /// [`Listing::ignored`] and otherwise left alone.
    pub fn list(&self, is_item: impl Fn(&str) -> bool) -> Result<Listing, CredentialError> {
        self.store.list(&is_item)
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Credentials")
    }
}

/// What [`Credentials::list`] found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Listing {
    /// The items, sorted by name.
    pub names: Vec<ItemName>,
    /// How many entries of the store are not such an item: a name that is no
    /// [`ItemName`], or one the consumer's rule refused. A directory store's own lock
    /// file is not counted; a temporary file is.
    pub ignored: usize,
}

/// The name of one item: 1 to [`MAX_NAME_BYTES`] ASCII letters, digits, `.`, `-` and
/// `_`, not starting with `.`. That is what a file name and a Keychain account can
/// both hold, and it leaves names that start with `.` to the directory store itself.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemName(String);

impl ItemName {
    /// `name` as an item name, or [`CredentialError::InvalidName`].
    pub fn new(name: &str) -> Result<Self, CredentialError> {
        if valid_name(name) {
            Ok(Self(name.to_owned()))
        } else {
            Err(CredentialError::InvalidName)
        }
    }

    /// The name's text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn valid_name(name: &str) -> bool {
    (1..=MAX_NAME_BYTES).contains(&name.len())
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// The value of one item. Its bytes are zeroed when it is dropped, and it prints as
/// `<redacted>`.
pub struct Secret(Vec<u8>);

impl Secret {
    /// The value's bytes.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }

    /// The value's bytes, in the buffer they were read into (no copy is made). The
    /// caller now owns them and zeroes them.
    pub fn into_bytes(mut self) -> Vec<u8> {
        std::mem::take(&mut self.0)
    }
}

impl From<Vec<u8>> for Secret {
    /// Takes over `bytes`: no copy is made, and the buffer is zeroed when the secret
    /// is dropped.
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        zero(&mut self.0);
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Overwrites with zeros in a way the optimizer cannot remove (best effort).
fn zero(bytes: &mut [u8]) {
    for byte in bytes.iter_mut() {
        // SAFETY: `byte` is a valid, aligned, exclusive reference to a u8.
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

/// Why a store call failed. Kinds only: never a secret byte or an item's name, and a
/// path only in [`CredentialError::Root`].
#[derive(Debug, PartialEq, Eq)]
pub enum CredentialError {
    /// This system has no adapter for the location; nothing was read or changed.
    Refused(Refused),
    /// [`release_directory`]: the release directory, or a directory above it, broke a
    /// rule. This is the one error that names a path: the directory to fix. No item
    /// operation returns it.
    Root(RootRefusal),
    /// The text is not an [`ItemName`], or a directory name in a location is not one.
    InvalidName,
    /// The value is longer than [`MAX_ITEM_BYTES`]; nothing was written.
    TooLarge,
    /// [`Credentials::add`]: the item exists and was left as it is.
    Exists,
    /// The stored item is longer than [`MAX_ITEM_BYTES`].
    Corrupt,
    /// The system answered with something that is not what was asked for (the
    /// Keychain: a reply that is not data, or not a list of attribute dictionaries).
    Malformed,
    /// A directory, the lock or an item failed a check: not the expected kind of file,
    /// another owner, another mode, more than one link, or the store's directory is no
    /// longer the one this handle opened.
    Unsafe,
    /// Another handle holds the directory store.
    InUse,
    /// The directory store holds a name that differs from this one only by case.
    CaseAlias,
    /// The Keychain failed with this `OSStatus`.
    Os(i32),
    /// A directory store's file operation failed with this kind of error.
    Io(io::ErrorKind),
}

impl fmt::Display for CredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(refused) => refused.fmt(f),
            Self::Root(refusal) => refusal.fmt(f),
            Self::InvalidName => f.write_str("the name is not a credential item name"),
            Self::TooLarge => f.write_str("the value is too large for a credential item"),
            Self::Exists => f.write_str("the credential item already exists"),
            Self::Corrupt => f.write_str("the stored credential item is too large"),
            Self::Malformed => f.write_str("the credential store's answer is not an item"),
            Self::Unsafe => f.write_str("the credential store failed an ownership or mode check"),
            Self::InUse => f.write_str("the credential store is in use"),
            Self::CaseAlias => {
                f.write_str("the credential store holds a name that differs only by case")
            }
            Self::Os(status) => write!(f, "the Keychain failed (OSStatus {status})"),
            Self::Io(kind) => write!(f, "the credential store's file operation failed: {kind}"),
        }
    }
}

impl std::error::Error for CredentialError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Refused(refused) => Some(refused),
            Self::Root(refusal) => Some(refusal),
            _ => None,
        }
    }
}

impl From<io::Error> for CredentialError {
    /// The error's kind only: an `io::Error` can carry a path in its message.
    fn from(error: io::Error) -> Self {
        Self::Io(error.kind())
    }
}

impl From<Refused> for CredentialError {
    fn from(refused: Refused) -> Self {
        Self::Refused(refused)
    }
}

/// Why [`Credentials::add`] or [`Credentials::replace`] failed.
///
/// An error does not always mean that nothing was stored. A directory store publishes
/// the value and then flushes the directory; when that flush fails, the call fails
/// with [`CredentialError::Io`] although the item already holds the new value, which
/// is then visible but not known to be on disk. Load the item before deciding how to
/// recover. [`CredentialError::Exists`], [`CredentialError::TooLarge`],
/// [`CredentialError::CaseAlias`] and [`CredentialError::Unsafe`] always leave the
/// store's items as they were.
#[derive(Debug, PartialEq, Eq)]
pub struct WriteError {
    /// Why.
    pub kind: CredentialError,
    /// A directory store could not remove the temporary file this write made. The
    /// file holds the value until the store is next opened without another holder,
    /// which removes it.
    pub temporary_left: bool,
}

impl From<CredentialError> for WriteError {
    fn from(kind: CredentialError) -> Self {
        Self {
            kind,
            temporary_left: false,
        }
    }
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl std::error::Error for WriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.kind.source()
    }
}

/// What every refusing adapter in this capability answers.
fn refusal() -> Refused {
    Refused::Unsupported {
        capability: "credentials",
        os: std::env::consts::OS,
    }
}
