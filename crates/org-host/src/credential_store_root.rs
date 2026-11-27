//! The credential directory of a release host (Linux host design section 3.2, LNX-4).
//!
//! On a system whose protected store is a directory, a release host keeps its office
//! secrets keys and its own secrets in one directory outside its data root. This module
//! opens that directory once per host start, from the path the setup record holds (never
//! from the environment), and hands it to the two views, which open their item stores below
//! it (`keys`, `host_secrets`). The platform makes every check and holds the lock that
//! admits one host (`org_platform_host::credentials::release_directory`); a refusal names
//! the directory and the rule it broke, and the host does not start.
//!
//! A development build on such a system opens the directory too when its setup record names
//! one, with every check a release build makes, and then keeps its keys there exactly as a
//! release build does (X5 review, fourth round: a lost secrets volume, a volume mounted
//! inside it and a registered service's recorded directory are judged the same in every
//! build). A development build whose record names none keeps its development stores in the
//! folder its environment names, and a system whose protected store is the Keychain has none
//! to open.

use std::os::fd::BorrowedFd;
use std::path::Path;

use org_platform::Refused;
use org_platform_host::credentials::{
    release_directory_apart_from, CredentialError, ItemStore, ProtectedStore, ReleaseDirectory,
    ReleaseRoot,
};

/// The host's item stores below its credential directory, as each view defines its own:
/// the office secrets keys and the host's own secrets. The platform judges these two
/// directories when it opens the credential directory, counts only what the two views would
/// list when the records say keys exist, and opens no other.
pub const ITEM_STORES: [ItemStore<'static>; 2] = [
    crate::keys::RELEASE_ITEM_STORE,
    crate::host_secrets::RELEASE_ITEM_STORE,
];

/// What the host knows of its credential directory when it starts.
pub(crate) struct Facts<'a> {
    /// The reserved instance that must own this store.
    pub(crate) instance_id: &'a str,
    /// The absolute path the setup record holds, if it holds one.
    pub(crate) recorded: Option<&'a Path>,
    /// The instance's data root, as the descriptor the host holds its lock on.
    pub(crate) data_root: BorrowedFd<'a>,
    /// Whether the instance is the foreground process of a container, where the directory
    /// must be a mounted volume (setup's service mode; increment X5 records it).
    pub(crate) foreground: bool,
    /// Whether the host's records say that keys exist: the store must then hold some.
    pub(crate) expect_items: bool,
    /// The account's records (the setup record and the instance locator), as a descriptor
    /// the host holds: the directory must be apart from them, and from their volume, as it
    /// is from the data root's.
    pub(crate) records: Option<BorrowedFd<'a>>,
}

/// The release directory this host keeps its secrets in, opened, checked and locked, or
/// `None` where it keeps them elsewhere: a system whose protected store is not a directory
/// (`store`), and a development build (`release_build` is false) whose record names none.
/// Where it is a directory and a release build's record names none, or the directory breaks
/// a rule, the text says which directory and why, and nothing was created.
pub(crate) fn open(
    release_build: bool,
    store: Result<ProtectedStore, Refused>,
    facts: Facts<'_>,
) -> Result<Option<ReleaseRoot>, String> {
    let account_data: Vec<BorrowedFd<'_>> = facts.records.into_iter().collect();
    open_with(release_build, store, facts, |location| {
        #[cfg(test)]
        if let Some(base) = WALK_FROM.with(|from| from.borrow().clone()) {
            return org_platform_host::testing::credentials::release_directory_below_apart_from(
                &base,
                location,
                &ITEM_STORES,
                &account_data,
            );
        }
        release_directory_apart_from(location, &ITEM_STORES, &account_data)
    })
}

// A test's host start walks to its credential directory from this directory instead of from
// `/`: a shared temporary directory is writable by others, which the ancestor rule refuses.
#[cfg(test)]
thread_local! {
    pub(crate) static WALK_FROM: std::cell::RefCell<Option<std::path::PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// [`open`] with the platform's entry as a parameter: the host's is
/// `release_directory_apart_from` for [`ITEM_STORES`] and the account's records, which walks
/// from `/`; a test passes the platform's test entry, which starts the same walk at the
/// test's own directory.
fn open_with(
    release_build: bool,
    store: Result<ProtectedStore, Refused>,
    facts: Facts<'_>,
    enter: impl FnOnce(ReleaseDirectory<'_>) -> Result<ReleaseRoot, CredentialError>,
) -> Result<Option<ReleaseRoot>, String> {
    if store != Ok(ProtectedStore::ReleaseDirectory) {
        return Ok(None);
    }
    let Some(path) = facts.recorded else {
        return if release_build {
            Err(NOT_RECORDED.into())
        } else {
            Ok(None)
        };
    };
    enter(ReleaseDirectory {
        path,
        data_root: facts.data_root,
        mount_point_required: facts.foreground,
        expect_items: facts.expect_items,
        instance_id: Some(facts.instance_id),
    })
    .map(Some)
    // The platform's text: the directory, then the rule it broke.
    .map_err(|refused| refused.to_string())
}

/// What a start says when the setup record names no credential directory.
pub(crate) const NOT_RECORDED: &str = "setup recorded no credential directory for this \
     instance, and this host keeps its secrets in one: run agentic-org setup again; nothing \
     was created";

/// Whether the host's records say that keys exist: an office has a folder in the office
/// home (`catalog::any_office_folder`). An office's folder is made only after its office
/// secrets key was stored, so a home with one and a store without any item is a store that
/// was lost, and the host must not start on it and create fresh keys.
pub(crate) fn records_say_keys_exist(office_home: &Path) -> std::io::Result<bool> {
    crate::catalog::any_office_folder(office_home)
}

#[cfg(test)]
mod tests;
