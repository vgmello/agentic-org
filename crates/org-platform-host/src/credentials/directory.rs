//! The directory store: items as owner-only files in one directory.
//!
//! The macOS and GNU/Linux adapters share it; the refusing adapter never reaches it, so
//! it is not compiled for a refusing target. It serves development builds on both
//! systems, and on GNU/Linux each item store below a release directory (`release.rs`),
//! which hands it a directory it has already opened and checked ([`Store::below`]).
//! Values are plain files: it protects them from other users, never from a process of
//! this user.
//!
//! # Layout
//!
//! ```text
//! <anchor>/<below…>/                 the store's directory, mode 0700
//!     .lock                          mode 0600, empty; locked while a handle lives
//!     .put.<32 hex digits>           mode 0600; a value on its way to being published
//!     <item name><suffix>            mode 0600, one link; the value's bytes and nothing else
//! ```
//!
//! An item name never starts with `.`, so no item can be the lock or a temporary file.
//! Files whose name does not end in the store's suffix, or whose name without it is not
//! an item name, are not items: they are never opened, changed or removed.
//!
//! # Checks when the store opens
//!
//! - The anchor is absolute and has no `..`. Its last component is opened without
//!   following a symbolic link (`O_DIRECTORY | O_NOFOLLOW`).
//! - The anchor and every directory below it down to the store's own are directories
//!   owned by this process's effective user with mode exactly `0700`. Each directory
//!   below the anchor is opened relative to its parent's descriptor, without following a
//!   link. A missing one is created with `mkdirat` when the location says so, and its
//!   parent is then flushed.
//! - `.lock` is opened relative to the store's directory without following a link,
//!   created if missing, and must be a regular file of this user with mode exactly
//!   `0600` and one link.
//! - The lock: an exclusive handle takes an exclusive `flock` and holds it for its
//!   life; a second handle is refused. A shared handle holds a shared `flock` for its
//!   life, so shared handles admit each other and an exclusive one is refused while any
//!   lives.
//! - Temporary files a crashed writer left are removed, but only by a handle that
//!   holds the lock alone at that moment, so that a live writer's file is never taken
//!   from under it. Each is checked like an item first; one that fails refuses the
//!   open and is left alone.
//!
//! Nothing repairs an owner or a mode.
//!
//! # Checks on every operation
//!
//! - The path the store was opened at still names the directory this handle holds
//!   (device and inode), and that directory is still owned by this user with mode
//!   `0700`. A moved or replaced directory refuses every operation.
//! - No other name in the directory equals the item's file name without regard to
//!   ASCII case.
//! - The item is opened relative to the held directory with `O_NOFOLLOW` and
//!   `O_NONBLOCK`, and must be a regular file of this user with mode exactly `0600` and
//!   one link. At most [`MAX_ITEM_BYTES`] are read; a longer file is
//!   [`CredentialError::Corrupt`].
//!
//! # Writing
//!
//! The value goes to a new temporary file (`O_EXCL`, mode set to `0600` whatever the
//! process's mask is), which is flushed to the device. An add then renames it to the
//! item's name with the system's exclusive rename, which never replaces an entry; a
//! replace renames it over the item. The directory is flushed, and on macOS the file is
//! flushed to the device once more. A failed write removes its temporary file.

use std::ffi::{CStr, CString, OsString};
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use super::{
    native, CredentialError, DirectoryStore, ItemName, Listing, Secret, Sharing, WriteError,
    MAX_ITEM_BYTES,
};

/// The lock file's name.
const LOCK: &str = ".lock";

/// What every temporary file's name starts with.
const TEMPORARY: &str = ".put.";

/// The longest suffix a store may give its item files.
const MAX_SUFFIX_BYTES: usize = 16;

/// How often a shared handle asks again for its lock while another handle holds the
/// lock alone, and how long it waits in between. A shared handle holds it alone only
/// while it removes leftover temporary files.
const SHARE_ATTEMPTS: u32 = 100;
const SHARE_WAIT: Duration = Duration::from_millis(2);

/// One open directory store.
pub(super) struct Store {
    /// The store's directory, held for the handle's life.
    directory: File,
    /// The path the directory was opened at, with the anchor's links resolved.
    path: PathBuf,
    suffix: String,
    /// The lock file. The mutex also makes this handle's operations take turns.
    gate: Mutex<File>,
}

impl Drop for Store {
    fn drop(&mut self) {
        // Forked descriptors may outlive this owner; close alone cannot release the lock.
        let file = self.gate.get_mut().unwrap_or_else(|e| e.into_inner());
        // Drop cannot report unlock errors; the File still closes afterward.
        let _ = file.unlock();
    }
}

/// What a file must be.
#[derive(Clone, Copy)]
enum Kind {
    Directory,
    File,
}

/// How one publication ended without an error.
enum Published {
    /// The item holds the value.
    Stored,
    /// An add found the item already there; it keeps its value.
    Collided,
}

impl Store {
    pub(super) fn open(location: &DirectoryStore<'_>) -> Result<Self, CredentialError> {
        let anchor = location.anchor;
        if !anchor.is_absolute()
            || anchor
                .components()
                .any(|c| matches!(c, Component::ParentDir))
        {
            return Err(CredentialError::Unsafe);
        }
        if !valid_suffix(location.suffix)
            || location.below.iter().any(|name| !super::valid_name(name))
        {
            return Err(CredentialError::InvalidName);
        }
        // Without a trailing `/` or `.`, so that the last component itself is the one
        // that is not followed.
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(anchor.components().collect::<PathBuf>())?;
        check(&directory, Kind::Directory)?;
        let path = anchor.canonicalize()?;
        Self::below(
            directory,
            path,
            location.below,
            location.create,
            location.sharing,
            location.suffix,
        )
    }

    /// The store in the directory that the single names `below` lead to from `anchor`
    /// (none: `anchor` itself). The caller opened `anchor` without following a link,
    /// checked it, and names it by `path`, which holds no link. The names and the
    /// suffix are checked here as well, for a caller that did not come through
    /// [`Store::open`].
    pub(super) fn below(
        anchor: File,
        mut path: PathBuf,
        below: &[&str],
        create: bool,
        sharing: Sharing,
        suffix: &str,
    ) -> Result<Self, CredentialError> {
        if !valid_suffix(suffix) || below.iter().any(|name| !super::valid_name(name)) {
            return Err(CredentialError::InvalidName);
        }
        let mut directory = anchor;
        for name in below {
            directory = child(&directory, name, create)?;
            path.push(name);
        }
        let lock = at(
            &directory,
            LOCK,
            libc::O_RDWR | libc::O_CREAT | libc::O_NONBLOCK,
        )?;
        check(&lock, Kind::File)?;
        let alone = match lock.try_lock() {
            Ok(()) => true,
            Err(fs::TryLockError::WouldBlock) => false,
            Err(fs::TryLockError::Error(e)) => return Err(e.into()),
        };
        if !alone {
            match sharing {
                Sharing::Exclusive => return Err(CredentialError::InUse),
                Sharing::Shared => share(&lock)?,
            }
        }
        let mut store = Self {
            directory,
            path,
            suffix: suffix.to_owned(),
            gate: Mutex::new(lock),
        };
        if alone {
            // Only the handle that holds the lock alone may remove a temporary file:
            // any other could take a live writer's.
            for raw in store.names()? {
                if let Some(name) = raw.to_str().filter(|n| n.starts_with(TEMPORARY)) {
                    store.item(name)?;
                    store.remove(name)?;
                }
            }
            if sharing == Sharing::Shared {
                share(store.gate.get_mut().unwrap_or_else(|e| e.into_inner()))?;
            }
        }
        Ok(store)
    }

    pub(super) fn add(&self, name: &ItemName, value: &[u8]) -> Result<(), WriteError> {
        self.write(name, value, true)
    }

    pub(super) fn replace(&self, name: &ItemName, value: &[u8]) -> Result<(), WriteError> {
        self.write(name, value, false)
    }

    pub(super) fn load(&self, name: &ItemName) -> Result<Option<Secret>, CredentialError> {
        let guard = self.gate.lock().map_err(|_| CredentialError::Unsafe)?;
        let Some(file) = self.item(&self.file_name(name))? else {
            return Ok(None);
        };
        // The opened inode stays stable across a concurrent replacement.
        drop(guard);
        // Reserve the bound before reading so no reallocation leaves a stale secret
        // buffer.
        let mut value = Secret::from(Vec::with_capacity(MAX_ITEM_BYTES + 1));
        file.take(MAX_ITEM_BYTES as u64 + 1)
            .read_to_end(&mut value.0)?;
        if value.0.len() > MAX_ITEM_BYTES {
            return Err(CredentialError::Corrupt);
        }
        Ok(Some(value))
    }

    pub(super) fn delete(&self, name: &ItemName) -> Result<(), CredentialError> {
        let _guard = self.gate.lock().map_err(|_| CredentialError::Unsafe)?;
        let file = self.file_name(name);
        if self.item(&file)?.is_some() {
            self.remove(&file)?;
        }
        Ok(())
    }

    pub(super) fn list(&self, is_item: &dyn Fn(&str) -> bool) -> Result<Listing, CredentialError> {
        let _guard = self.gate.lock().map_err(|_| CredentialError::Unsafe)?;
        let mut listing = Listing::default();
        for raw in self.names()? {
            if raw == LOCK {
                continue;
            }
            let item = raw.to_str().and_then(|file| {
                let name = file.strip_suffix(self.suffix.as_str())?;
                (super::valid_name(name) && is_item(name)).then_some((file, name))
            });
            let Some((file, name)) = item else {
                listing.ignored += 1;
                continue;
            };
            if self.item(file)?.is_some() {
                listing.names.push(ItemName(name.to_owned()));
            }
        }
        listing.names.sort();
        Ok(listing)
    }

    fn file_name(&self, name: &ItemName) -> String {
        format!("{}{}", name.as_str(), self.suffix)
    }

    fn write(&self, name: &ItemName, value: &[u8], exclusive: bool) -> Result<(), WriteError> {
        if value.len() > MAX_ITEM_BYTES {
            return Err(CredentialError::TooLarge.into());
        }
        let _guard = self.gate.lock().map_err(|_| CredentialError::Unsafe)?;
        let target = self.file_name(name);
        if self.item(&target)?.is_some() && exclusive {
            return Err(CredentialError::Exists.into());
        }
        let temporary = temporary_name()?;
        // O_EXCL is defence in depth against a random temporary-name collision.
        let mut file = at(
            &self.directory,
            &temporary,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )?;
        match self.publish(&mut file, &temporary, &target, value, exclusive) {
            Ok(Published::Stored) => Ok(()),
            Ok(Published::Collided) => Err(CredentialError::Exists.into()),
            Err(kind) => Err(WriteError {
                kind,
                temporary_left: self.remove(&temporary).is_err(),
            }),
        }
    }

    fn publish(
        &self,
        file: &mut File,
        temporary: &str,
        target: &str,
        value: &[u8],
        exclusive: bool,
    ) -> Result<Published, CredentialError> {
        // The process's mask may have taken the owner's own bits away.
        file.set_permissions(Permissions::from_mode(0o600))?;
        check(file, Kind::File)?;
        file.write_all(value)?;
        org_platform::fs::full_sync(file)?;
        #[cfg(any(test, feature = "test-support"))]
        run_hook(&super::testing::PUBLISH_HOOK, &self.path);
        let (from, to) = (cstring(temporary)?, cstring(target)?);
        let renamed = if exclusive {
            // Never replaces: an entry of that name, whatever it is, refuses the rename.
            org_platform::fs::rename_noreplace_at(&self.directory, &from, &self.directory, &to)
        } else {
            let dir = self.directory.as_raw_fd();
            // SAFETY: a retained directory descriptor and two checked, NUL-terminated
            // single names that live across the call.
            match unsafe { libc::renameat(dir, from.as_ptr(), dir, to.as_ptr()) } {
                0 => Ok(()),
                _ => Err(io::Error::last_os_error()),
            }
        };
        if let Err(error) = renamed {
            if !exclusive || error.kind() != io::ErrorKind::AlreadyExists {
                return Err(error.into());
            }
            #[cfg(any(test, feature = "test-support"))]
            run_hook(&super::testing::COLLISION_HOOK, &self.path);
            // The entry that won must itself be an item; one that is gone again, or is
            // no item, is not a stored value.
            self.item(target)?.ok_or(CredentialError::Unsafe)?;
            self.remove(temporary)?;
            return Ok(Published::Collided);
        }
        self.sync_directory()?;
        // Flush publication after its parent sync, in addition to the payload flush
        // before publication. Not a power-loss proof.
        native::flush_published(file)?;
        Ok(Published::Stored)
    }

    /// The directory at the store's path is still the one this handle holds.
    fn verify(&self) -> Result<(), CredentialError> {
        let path = fs::symlink_metadata(&self.path)?;
        let held = self.directory.metadata()?;
        if !path.is_dir() || path.dev() != held.dev() || path.ino() != held.ino() {
            return Err(CredentialError::Unsafe);
        }
        check(&self.directory, Kind::Directory)
    }

    /// Every name in the directory but `.` and `..`.
    fn names(&self) -> Result<Vec<OsString>, CredentialError> {
        self.verify()?;
        let file = at(&self.directory, ".", libc::O_RDONLY | libc::O_DIRECTORY)?;
        // SAFETY: fdopendir takes ownership on success; otherwise File closes the fd.
        let stream = unsafe { libc::fdopendir(file.as_raw_fd()) };
        if stream.is_null() {
            return Err(io::Error::last_os_error().into());
        }
        std::mem::forget(file);
        let stream = Entries(stream);
        let mut names = Vec::new();
        loop {
            // readdir reports the end of the directory and a failure the same way.
            org_platform::fs::set_errno(0)?;
            // SAFETY: stream owns a live directory stream that no other thread reads.
            let entry = unsafe { libc::readdir(stream.0) };
            if entry.is_null() {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(0) {
                    return Err(error.into());
                }
                break;
            }
            // SAFETY: readdir's NUL-terminated name remains valid until the next call.
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if name != b"." && name != b".." {
                names.push(OsString::from_vec(name.to_vec()));
            }
        }
        self.verify()?;
        Ok(names)
    }

    /// The file `name` of the directory, opened and checked, or `None` if there is none.
    fn item(&self, name: &str) -> Result<Option<File>, CredentialError> {
        self.verify()?;
        if self.names()?.iter().any(|raw| {
            raw.to_str()
                .is_some_and(|n| n != name && n.eq_ignore_ascii_case(name))
        }) {
            return Err(CredentialError::CaseAlias);
        }
        let file = match at(&self.directory, name, libc::O_RDONLY | libc::O_NONBLOCK) {
            Ok(file) => file,
            Err(CredentialError::Io(io::ErrorKind::NotFound)) => return Ok(None),
            Err(e) => return Err(e),
        };
        check(&file, Kind::File)?;
        Ok(Some(file))
    }

    fn remove(&self, name: &str) -> Result<(), CredentialError> {
        let name = cstring(name)?;
        // SAFETY: retained directory fd and checked NUL-free single component.
        if unsafe { libc::unlinkat(self.directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::NotFound {
                return Err(e.into());
            }
        }
        self.sync_directory()
    }

    fn sync_directory(&self) -> Result<(), CredentialError> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(kind) = super::testing::DIRECTORY_SYNC_FAULTS
            .with(|faults| faults.borrow_mut().pop_front())
            .flatten()
        {
            return Err(CredentialError::Io(kind));
        }
        self.directory.sync_all()?;
        Ok(())
    }
}

/// Take a test's hook out of `slot` and run it with the store's directory.
#[cfg(any(test, feature = "test-support"))]
fn run_hook(
    slot: &'static std::thread::LocalKey<std::cell::RefCell<Option<super::testing::Hook>>>,
    directory: &std::path::Path,
) {
    let hook = slot.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook(directory);
    }
}

/// Take the shared lock on `lock`, which this handle may hold exclusively. Another
/// handle holds it alone only for the moment it removes leftover temporary files, so a
/// refusal is asked again a bounded number of times; a holder that stays is an
/// exclusive handle.
fn share(lock: &File) -> Result<(), CredentialError> {
    for _ in 0..SHARE_ATTEMPTS {
        match lock.try_lock_shared() {
            Ok(()) => return Ok(()),
            Err(fs::TryLockError::WouldBlock) => std::thread::sleep(SHARE_WAIT),
            Err(fs::TryLockError::Error(e)) => return Err(e.into()),
        }
    }
    Err(CredentialError::InUse)
}

fn valid_suffix(suffix: &str) -> bool {
    suffix.len() <= MAX_SUFFIX_BYTES
        && suffix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// A fresh temporary file name: [`TEMPORARY`] and 32 hex digits from the system's
/// random source.
fn temporary_name() -> Result<String, CredentialError> {
    let mut random = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut random)?;
    let mut name = String::from(TEMPORARY);
    for byte in random {
        name.push_str(&format!("{byte:02x}"));
    }
    Ok(name)
}

fn cstring(name: &str) -> Result<CString, CredentialError> {
    if name.contains('/') {
        return Err(CredentialError::InvalidName);
    }
    CString::new(name).map_err(|_| CredentialError::InvalidName)
}

/// `file` is what it must be: this user's, a directory with mode `0700` or a regular
/// file with mode `0600` and one link.
fn check(file: &File, kind: Kind) -> Result<(), CredentialError> {
    let m = file.metadata()?;
    // SAFETY: geteuid has no preconditions and cannot fail.
    let owned = m.uid() == unsafe { libc::geteuid() };
    let shaped = match kind {
        Kind::Directory => m.is_dir() && m.mode() & 0o7777 == 0o700,
        Kind::File => m.is_file() && m.mode() & 0o7777 == 0o600 && m.nlink() == 1,
    };
    if owned && shaped {
        Ok(())
    } else {
        Err(CredentialError::Unsafe)
    }
}

/// Open `name` in `directory`, never following a link in `name` itself.
fn at(directory: &File, name: &str, flags: i32) -> Result<File, CredentialError> {
    let name = cstring(name)?;
    let flags = flags | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: live directory fd and checked leaf; successful open transfers a new fd to File.
    let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags, 0o600) };
    if fd < 0 {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: fd is fresh and uniquely owned.
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// The directory `name` in `parent`, opened and checked; made first if it is missing
/// and `create` says so.
fn child(parent: &File, name: &str, create: bool) -> Result<File, CredentialError> {
    if create {
        let name_c = cstring(name)?;
        // SAFETY: retained parent fd and checked leaf, mode excludes other users.
        if unsafe { libc::mkdirat(parent.as_raw_fd(), name_c.as_ptr(), 0o700) } != 0 {
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::AlreadyExists {
                return Err(e.into());
            }
        } else {
            parent.sync_all()?;
        }
    }
    let child = at(parent, name, libc::O_RDONLY | libc::O_DIRECTORY)?;
    check(&child, Kind::Directory)?;
    Ok(child)
}

struct Entries(*mut libc::DIR);

impl Drop for Entries {
    fn drop(&mut self) {
        // SAFETY: this guard exclusively owns the successful fdopendir result.
        unsafe {
            libc::closedir(self.0);
        }
    }
}
