//! The host's item stores below the release directory: which exist, each judged, and
//! how many items each holds.

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::fd::AsFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::descriptors::{entries, is_regular, open_at};
use super::refused;
use crate::credentials::rules;
use crate::credentials::{directory, CredentialError, ItemStore, RootRule};

/// One of the host's item stores that exists below the release directory: its
/// declaration, its path, and its directory open for reading.
pub(super) type ExistingStore<'a, 'b> = (&'a ItemStore<'b>, PathBuf, File);

/// The host's item stores that exist below the release directory `root` (at
/// `root_path`), each opened without following a link and judged: a directory of the
/// service user with mode exactly `0700`, or its own rule with its own path. One that
/// does not exist is not in the answer; the engine makes it when it is opened. Nothing
/// else in the release directory is looked at.
pub(super) fn item_store_directories<'a, 'b>(
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
pub(super) fn count_items(
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
