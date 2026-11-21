//! The key store the host uses: office secrets keys as the items of one credential store
//! (`org_platform_host::credentials`). This view keeps what is the host's own: an item's name is
//! the office id and must be a storage id, a key is only ever added, and what a stored key looks
//! like. The platform keeps the bytes and applies the system's protection.
//!
//! **The Keychain store** (release builds on macOS): the item's value is the key's 32 bytes. The
//! store is opened when it is chosen, which reads and writes nothing.
//!
//! **The release directory store** (Linux, where the setup record names a credential
//! directory, in every build): owner-only files
//! `<credential directory>/office-keys/<office id>.key`, each the key's 32 bytes, in the
//! credential directory the host opened and checked at its start (`release_store`). It is the
//! platform's directory store below that directory, held by this host alone.
//!
//! **The development store** (debug builds without a recorded credential directory):
//! owner-only files `<dir>/<office id>.key`, or
//! `<dir>/<instance id>/<office id>.key` for a registered instance, each 64 hex digits, the same
//! format as `agentic-org-office`'s former `dev-keys` store. So a debug host, and every test,
//! never writes a real Keychain item. **Never** for real data: the files are plain text. It is
//! the platform's directory store, with its checks: the folder and the one above an instance's
//! are this user's with mode `0700`, a key file is a regular file of this user with mode `0600`,
//! and nothing repairs either. The folder may serve several homes at once, so the store is
//! opened shared. It is opened at its first use: the first `create` makes the folder if it's
//! missing; until then a missing folder holds no key.
//!
//! **No store** (release builds on a system with no protected store): [`UnsupportedKeys`] has
//! nothing to open. It never falls back to another system's store or to plain files: every call
//! is [`KeyStoreError::Unsupported`], so the host creates no office, starts none with a key it
//! cannot keep, and deletes nothing.

use std::fs;
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use org_office_launch::is_storage_id;
use org_platform_host::credentials::{
    credentials, CredentialError, CredentialLocation, Credentials, DirectoryStore, ItemName,
    ItemStore, ReleaseItems, ReleaseRoot, Secret, Sharing,
};
use org_proto::{OfficeSecretsKey, OFFICE_SECRETS_KEY_BYTES};

use super::{KeyStoreError, OfficeSecretsKeys};
use crate::secrets::zero;
use crate::sync::lock;

/// A development key's file: 64 hex digits.
const HEX_LEN: usize = OFFICE_SECRETS_KEY_BYTES * 2;

/// What follows the office id in a development key's file name.
const DEV_SUFFIX: &str = ".key";

/// The office keys' item store below a release directory. It is persisted state: another
/// directory name or suffix is another, empty, store. One host has the release directory at
/// a time, so the item store is held alone.
const RELEASE_ITEMS: ReleaseItems<'static> = ReleaseItems {
    directory: "office-keys",
    suffix: DEV_SUFFIX,
    sharing: Sharing::Exclusive,
};

/// The same item store as the host declares it when it opens the release directory
/// (`release_store`): its directory, its suffix, and this view's name rule, so that only
/// what this view would list is counted as a key there.
pub(crate) const RELEASE_ITEM_STORE: ItemStore<'static> = ItemStore {
    directory: RELEASE_ITEMS.directory,
    suffix: RELEASE_ITEMS.suffix,
    is_item: is_storage_id,
};

/// What the Keychain store's predecessor (`security_framework::passwords`) reported for a reply
/// that held no data: `errSecParam`. Such a reply is not a missing key.
const NOT_DATA: i32 = -50;

/// Office keys in one credential store (see the module docs).
pub(super) struct OfficeKeys {
    source: Source,
}

// A release build never chooses the development store (`keys::store_for`).
#[cfg_attr(not(debug_assertions), allow(dead_code))]
enum Source {
    /// A release build's protected store (the Keychain, or an item store below a release
    /// directory), open since it was chosen. A key is its 32 bytes.
    Protected(Credentials),
    /// The development store, opened at its first use. A key is 64 hex digits.
    Development(Development),
}

struct Development {
    /// The development key folder, absolute.
    dir: PathBuf,
    /// The registered instance whose keys these are, if any: its folder inside `dir`.
    instance_id: Option<String>,
    opened: Mutex<Option<Arc<Credentials>>>,
}

impl OfficeKeys {
    /// Keys as the items of the Keychain store `store`.
    // Debug builds never choose this store (`keys::store_for`); its tests still run there.
    #[cfg_attr(debug_assertions, allow(dead_code))]
    pub(super) fn keychain(store: Credentials) -> Self {
        Self {
            source: Source::Protected(store),
        }
    }

    /// Keys as the items of the office keys' item store below the release directory `root`,
    /// which is opened (and made, if it is missing) now.
    pub(super) fn release_directory(root: &ReleaseRoot) -> Result<Self, CredentialError> {
        Ok(Self {
            source: Source::Protected(root.store(RELEASE_ITEMS)?),
        })
    }

    /// The development store in `dir` (absolute), or in `dir/<instance id>`.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub(super) fn development(dir: &Path, instance_id: Option<&str>) -> Self {
        Self {
            source: Source::Development(Development {
                dir: dir.to_path_buf(),
                instance_id: instance_id.map(str::to_owned),
                opened: Mutex::new(None),
            }),
        }
    }

    /// Runs `op` on the store. `None`: the development folder isn't there and `create` didn't
    /// ask for it, so the store holds nothing.
    fn with<T>(
        &self,
        create: bool,
        op: impl FnOnce(&Credentials) -> Result<T, KeyStoreError>,
    ) -> Result<Option<T>, KeyStoreError> {
        match &self.source {
            Source::Protected(store) => op(store).map(Some),
            Source::Development(development) => match development.open(create)? {
                Some(store) => op(&store).map(Some),
                None => Ok(None),
            },
        }
    }
}

impl Development {
    /// The open store, opened now if it wasn't. `create`: make the folder if it's missing
    /// (owner-only). Without it a missing folder is `None`, and nothing is made.
    fn open(&self, create: bool) -> Result<Option<Arc<Credentials>>, KeyStoreError> {
        let mut opened = lock(&self.opened);
        if let Some(store) = &*opened {
            return Ok(Some(Arc::clone(store)));
        }
        if create {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&self.dir)?;
        }
        let below: Vec<&str> = self.instance_id.as_deref().into_iter().collect();
        let location = CredentialLocation::Directory(DirectoryStore {
            anchor: &self.dir,
            below: &below,
            create,
            // A development key folder may serve several homes, and so several hosts, at once.
            sharing: Sharing::Shared,
            suffix: DEV_SUFFIX,
        });
        match credentials(location) {
            Ok(store) => {
                let store = Arc::new(store);
                *opened = Some(Arc::clone(&store));
                Ok(Some(store))
            }
            Err(CredentialError::Io(io::ErrorKind::NotFound)) if !create => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

/// The item of office `office_id`. Ids are plain (callers check), but this names an item and,
/// in the development store, a file: be sure.
fn item(office_id: &str) -> Result<ItemName, KeyStoreError> {
    if !is_storage_id(office_id) {
        return Err(KeyStoreError::NotPlainId);
    }
    ItemName::new(office_id).map_err(|_| KeyStoreError::NotPlainId)
}

impl OfficeSecretsKeys for OfficeKeys {
    fn create(&self, office_id: &str, key: &OfficeSecretsKey) -> Result<(), KeyStoreError> {
        let name = item(office_id)?;
        // An add, never a replace: an existing item is refused, so a key is never replaced, even
        // one a racing process just made.
        let added = self.with(true, |store| {
            let added = match &self.source {
                Source::Protected(_) => store.add(&name, key.expose_bytes()),
                Source::Development(_) => {
                    let mut hex = [0u8; HEX_LEN];
                    encode_hex(key.expose_bytes(), &mut hex);
                    let added = store.add(&name, &hex);
                    zero(&mut hex);
                    added
                }
            };
            added.map_err(|failure| {
                if failure.temporary_left {
                    crate::host_log::log("could not clean up a temporary office key file");
                }
                failure.kind.into()
            })
        })?;
        debug_assert!(added.is_some(), "a create opens its store");
        Ok(())
    }

    fn load(&self, office_id: &str) -> Result<Option<OfficeSecretsKey>, KeyStoreError> {
        let name = item(office_id)?;
        let loaded = self.with(false, |store| Ok(store.load(&name)?))?;
        let Some(Some(secret)) = loaded else {
            return Ok(None);
        };
        let key = match &self.source {
            Source::Protected(_) => raw_key(&secret),
            // The server's format allowed whitespace around the digits.
            Source::Development(_) => decode_hex(secret.expose().trim_ascii()),
        };
        key.ok_or(KeyStoreError::Corrupt).map(Some)
    }

    fn delete(&self, office_id: &str) -> Result<(), KeyStoreError> {
        let name = item(office_id)?;
        self.with(false, |store| Ok(store.delete(&name)?))?;
        Ok(())
    }

    fn list(&self) -> Result<Vec<String>, KeyStoreError> {
        // Attributes and names only: never the keys.
        let listing = self.with(false, |store| Ok(store.list(is_storage_id)?))?;
        Ok(listing.map_or_else(Vec::new, |listing| {
            listing
                .names
                .iter()
                .map(|name| name.as_str().to_owned())
                .collect()
        }))
    }
}

/// The office secrets keys of a system with no protected store: refuses every call (see the
/// module docs).
// Debug builds never choose it (`keys::store_for`); its tests still run there.
#[cfg_attr(debug_assertions, allow(dead_code))]
pub(super) struct UnsupportedKeys;

impl OfficeSecretsKeys for UnsupportedKeys {
    fn create(&self, _office_id: &str, _key: &OfficeSecretsKey) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::Unsupported)
    }

    fn load(&self, _office_id: &str) -> Result<Option<OfficeSecretsKey>, KeyStoreError> {
        Err(KeyStoreError::Unsupported)
    }

    fn delete(&self, _office_id: &str) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::Unsupported)
    }

    fn list(&self) -> Result<Vec<String>, KeyStoreError> {
        Err(KeyStoreError::Unsupported)
    }
}

impl From<CredentialError> for KeyStoreError {
    /// Kinds and codes only, as the platform gives them: never an item's bytes or a path.
    fn from(error: CredentialError) -> Self {
        let refused = |text: &'static str| KeyStoreError::Io(io::Error::other(text));
        match error {
            CredentialError::Exists => KeyStoreError::Exists,
            CredentialError::Corrupt => KeyStoreError::Corrupt,
            CredentialError::InvalidName => KeyStoreError::NotPlainId,
            CredentialError::InteractionNotAllowed => KeyStoreError::InteractionNotAllowed,
            CredentialError::Os(status) => KeyStoreError::Os(status),
            CredentialError::Malformed => KeyStoreError::Os(NOT_DATA),
            CredentialError::Io(kind) => KeyStoreError::Io(io::Error::from(kind)),
            CredentialError::Refused(_) => KeyStoreError::Unsupported,
            // Only opening a release directory answers this; no item operation does. The path
            // is reported where the store is opened, never here.
            CredentialError::Root(_) => {
                refused("the credential directory or a directory above it broke a rule")
            }
            CredentialError::TooLarge => refused("the item is too large to be a key"),
            CredentialError::Unsafe => refused(
                "the key folder or a file in it isn't this user's alone (folder mode 0700, file \
                 mode 0600), or the folder was replaced",
            ),
            CredentialError::InUse => refused("the key folder is held by another process"),
            CredentialError::CaseAlias => {
                refused("the key folder holds a name that differs only by case")
            }
        }
    }
}

/// The 32 bytes of a protected store's item as a key, or `None` if it has another length.
fn raw_key(secret: &Secret) -> Option<OfficeSecretsKey> {
    let mut array = *<&[u8; OFFICE_SECRETS_KEY_BYTES]>::try_from(secret.expose()).ok()?;
    let key = OfficeSecretsKey::from_bytes(array);
    zero(&mut array);
    Some(key)
}

const HEX: &[u8; 16] = b"0123456789abcdef";

fn encode_hex(bytes: &[u8; OFFICE_SECRETS_KEY_BYTES], out: &mut [u8; HEX_LEN]) {
    for (i, byte) in bytes.iter().enumerate() {
        out[2 * i] = HEX[usize::from(byte >> 4)];
        out[2 * i + 1] = HEX[usize::from(byte & 0x0f)];
    }
}

/// 64 hex digits (either case, as the server's store read them) as a key, or `None`.
fn decode_hex(text: &[u8]) -> Option<OfficeSecretsKey> {
    if text.len() != HEX_LEN {
        return None;
    }
    let mut bytes = [0u8; OFFICE_SECRETS_KEY_BYTES];
    let mut ok = true;
    for (i, pair) in text.as_chunks::<2>().0.iter().enumerate() {
        match (nibble(pair[0]), nibble(pair[1])) {
            (Some(hi), Some(lo)) => bytes[i] = (hi << 4) | lo,
            _ => ok = false,
        }
    }
    // `from_bytes` takes a copy; the local array is zeroed either way.
    let key = ok.then(|| OfficeSecretsKey::from_bytes(bytes));
    zero(&mut bytes);
    key
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
