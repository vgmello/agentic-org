//! Host registry/TLS/recovery custody, separate from office secrets keys and their sweep.
//!
//! [`HostSecrets`] is a typed view ([`Custody`]) over one credential store of the platform
//! (`org_platform_host::credentials`). The view keeps what is the host's own: which names exist
//! ([`Name::parse`]), which of them are add-only, the value limit, the error kinds and every
//! line it logs. The platform keeps the bytes. Which store a build uses is decided here
//! ([`store_for`]): a release build the system's protected store, whichever kind it is: the
//! Data Protection Keychain, in the access group compiled in and under this instance's
//! service, after the host checked its own signature; or `host-secrets/<name>` below the
//! credential directory setup recorded, which the host opened and checked at its start
//! (`credential_store_root`). A debug build uses that credential directory too where its setup record
//! names one, and otherwise the platform's directory store under the development key folder.
//!
//! The development store is plain files: `<base>/host-secrets/<instance id>/<name>`, held by
//! one host at a time. A put error can follow publication: the value may be visible but not
//! durable. Reload the item before deciding how to recover; an error does not imply rollback.

#[cfg(test)]
mod tests;

use std::{fmt, io};

use org_platform::Refused;
use org_platform_host::credentials::{
    credentials, CredentialError, CredentialLocation, Credentials, ItemName, ItemStore,
    KeychainItems, ProtectedStore, ReleaseItems, ReleaseRoot, Sharing,
};

pub const MAX_BYTES: usize = 16 * 1024;
const _: () = assert!(MAX_BYTES == org_platform_host::credentials::MAX_ITEM_BYTES);

/// The Keychain service the host secrets of instance `instance` are filed under; the account is
/// the secret's name. It is persisted state: another string is another, empty, store.
fn keychain_service(instance: &str) -> String {
    format!("org.agentic.host-secret.instance.{instance}")
}

/// The host secrets' item store below a release directory. It is persisted state: another
/// directory name is another, empty, store. One host has the release directory at a time, so
/// the item store is held alone.
const RELEASE_ITEMS: ReleaseItems<'static> = ReleaseItems {
    directory: "host-secrets",
    suffix: "",
    sharing: Sharing::Exclusive,
};

/// The same item store as the host declares it when it opens the release directory
/// (`credential_store_root`): its directory, its suffix, and this view's name rule, so that only
/// what this view would list is counted as a host secret there.
pub(crate) const RELEASE_ITEM_STORE: ItemStore<'static> = ItemStore {
    directory: RELEASE_ITEMS.directory,
    suffix: RELEASE_ITEMS.suffix,
    is_item: is_name,
};

fn is_name(name: &str) -> bool {
    Name::parse(name).is_ok()
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Name(String);

impl Name {
    pub fn parse(name: &str) -> Result<Self, Error> {
        let valid = name == "registry"
            || name == "writer-device"
            || name.strip_prefix("writer.").is_some_and(|hex| {
                hex.len() == 64
                    && hex
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            || name.strip_prefix("tls.").is_some_and(|hex| {
                hex.len() == 64
                    && hex
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            || name
                .strip_prefix("recovery.")
                .is_some_and(org_proto::is_uuid_v7);
        valid
            .then(|| Self(name.to_owned()))
            .ok_or(Error::InvalidName)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn is_add_only(&self) -> bool {
        self.0.starts_with("tls.") || self.0 == "writer-device"
    }
}

pub struct Value(Vec<u8>);

impl Value {
    pub fn new(mut bytes: Vec<u8>) -> Result<Self, Error> {
        if bytes.len() > MAX_BYTES {
            crate::secrets::zero(&mut bytes);
            return Err(Error::TooLarge);
        }
        Ok(Self(bytes))
    }
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
    fn wipe(&mut self) {
        #[cfg(test)]
        tests::WIPES.with(|n| n.set(n.get() + 1));
        crate::secrets::zero(&mut self.0);
    }
}
impl Drop for Value {
    fn drop(&mut self) {
        self.wipe();
    }
}
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}
impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

pub trait HostSecrets: Send + Sync {
    /// TLS items are add-only; a duplicate preserves the first stored value.
    /// In development custody, an error may follow publication without durability;
    /// reload before deciding recovery, since an error does not imply rollback.
    fn put(&self, name: &Name, value: &Value) -> Result<(), Error>;
    fn load(&self, name: &Name) -> Result<Option<Value>, Error>;
    fn delete(&self, name: &Name) -> Result<(), Error>;
    fn list(&self) -> Result<Vec<Name>, Error>;
}

/// Fixed kinds only: filesystem errors cannot carry a secret or a path into logs.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    InvalidName,
    TooLarge,
    Corrupt,
    Unsafe,
    InUse,
    CaseAlias,
    InteractionNotAllowed,
    Os(i32),
    Io(io::ErrorKind),
}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e.kind())
    }
}
impl From<CredentialError> for Error {
    /// The platform's kinds, as this view has always named them.
    fn from(e: CredentialError) -> Self {
        match e {
            CredentialError::InvalidName => Self::InvalidName,
            CredentialError::TooLarge => Self::TooLarge,
            // The Keychain answered with something that is not a value or a list of items.
            CredentialError::Corrupt | CredentialError::Malformed => Self::Corrupt,
            // `Exists` reaches this only if an add-only rule was not applied: fail closed.
            CredentialError::Unsafe | CredentialError::Exists => Self::Unsafe,
            CredentialError::InUse => Self::InUse,
            CredentialError::CaseAlias => Self::CaseAlias,
            CredentialError::InteractionNotAllowed => Self::InteractionNotAllowed,
            CredentialError::Os(status) => Self::Os(status),
            CredentialError::Io(kind) => Self::Io(kind),
            CredentialError::Refused(_) => Self::Io(io::ErrorKind::Unsupported),
            // A release directory that broke a rule. Only opening one answers this; no item
            // operation does. Kinds only here: the path is reported where the store is opened.
            CredentialError::Root(_) => Self::Unsafe,
        }
    }
}

/// Host secrets as the items of one credential store (see the module docs).
pub(crate) struct Custody {
    store: Credentials,
}

/// The development custody under the name the tests of other modules open it by
/// (`hostdb`, `provisioning`, `writer`).
#[cfg(all(test, debug_assertions))]
pub(crate) mod dev {
    pub(crate) use super::Custody as DevSecrets;
}

impl Custody {
    /// The development custody of `instance`: the directory `host-secrets/<instance>` under
    /// `base`, made if it is missing, and held by this handle alone until it is dropped.
    /// `base` already exists, is owner-only, and is the shared development-key base; it must be
    /// outside the office `home`.
    #[cfg(debug_assertions)]
    pub(crate) fn open(
        base: &std::path::Path,
        home: &std::path::Path,
        instance: &str,
    ) -> Result<Self, Error> {
        use org_platform_host::credentials::DirectoryStore;
        use std::path::Component;
        if !base.is_absolute()
            || base.components().any(|c| matches!(c, Component::ParentDir))
            || !org_proto::is_uuid_v7(instance)
        {
            return Err(Error::Unsafe);
        }
        // Before anything is made: plain-text secrets never go into the office home.
        if base.canonicalize()?.starts_with(home.canonicalize()?) {
            return Err(Error::Unsafe);
        }
        let store = credentials(CredentialLocation::Directory(DirectoryStore {
            anchor: base,
            below: &["host-secrets", instance],
            create: true,
            sharing: Sharing::Exclusive,
            suffix: "",
        }))?;
        Ok(Self { store })
    }

    /// The Keychain custody of `instance` in the access group `group`. Nothing is read or
    /// written until the first operation.
    #[cfg_attr(
        all(debug_assertions, not(test)),
        expect(dead_code, reason = "release selection")
    )]
    fn keychain(group: &str, instance: &str) -> Result<Self, SelectionError> {
        if !org_proto::is_uuid_v7(instance) {
            return Err(SelectionError::InvalidInstance);
        }
        match credentials(CredentialLocation::Keychain(KeychainItems {
            access_group: group,
            service: &keychain_service(instance),
        })) {
            Ok(store) => Ok(Self { store }),
            Err(CredentialError::Refused(_)) => Err(SelectionError::Unsupported),
            Err(_) => Err(SelectionError::InvalidInstance),
        }
    }
}

impl Custody {
    /// The custody of `instance` below the release directory `root`: its `host-secrets` item
    /// store, opened (and made, if it is missing) now and held by this handle alone. The
    /// host has already verified the directory's binding to this instance.
    fn release_directory(root: &ReleaseRoot, instance: &str) -> Result<Self, SelectionError> {
        if !org_proto::is_uuid_v7(instance) {
            return Err(SelectionError::InvalidInstance);
        }
        match root.store(RELEASE_ITEMS) {
            Ok(store) => Ok(Self { store }),
            Err(CredentialError::Refused(_)) => Err(SelectionError::Unsupported),
            Err(e) => Err(SelectionError::Storage(e.into())),
        }
    }
}

fn item(name: &Name) -> Result<ItemName, Error> {
    ItemName::new(name.as_str()).map_err(|_| Error::InvalidName)
}

impl HostSecrets for Custody {
    fn put(&self, name: &Name, value: &Value) -> Result<(), Error> {
        if value.expose().len() > MAX_BYTES {
            return Err(Error::TooLarge);
        }
        let item = item(name)?;
        let written = if name.is_add_only() {
            match self.store.add(&item, value.expose()) {
                // A duplicate preserves the first stored value.
                Err(failure) if failure.kind == CredentialError::Exists => Ok(()),
                written => written,
            }
        } else {
            self.store.replace(&item, value.expose())
        };
        written.map_err(|failure| {
            if failure.temporary_left {
                crate::host_log::log("could not clean up a temporary host secret");
            }
            failure.kind.into()
        })
    }
    fn load(&self, name: &Name) -> Result<Option<Value>, Error> {
        // The platform's buffer becomes the value's: no copy, and it is wiped on drop.
        Ok(self
            .store
            .load(&item(name)?)?
            .map(|secret| Value(secret.into_bytes())))
    }
    fn delete(&self, name: &Name) -> Result<(), Error> {
        Ok(self.store.delete(&item(name)?)?)
    }
    fn list(&self) -> Result<Vec<Name>, Error> {
        let listing = self.store.list(|name| Name::parse(name).is_ok())?;
        for _ in 0..listing.ignored {
            crate::host_log::log("ignored an invalid host-secret item name");
        }
        Ok(listing
            .names
            .iter()
            .filter_map(|name| Name::parse(name.as_str()).ok())
            .collect())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SelectionError {
    InvalidInstance,
    #[cfg(debug_assertions)]
    NoDevBase,
    NoGroup,
    NotEntitled,
    NotHardened,
    /// This system has no protected store for a release build's secrets.
    Unsupported,
    /// A release build on a system that keeps its secrets in a credential directory has none
    /// open.
    NoCredentialDirectory,
    /// The store's directory could not be opened: the development folder, or the item store
    /// below the credential directory. A kind, never a path.
    Storage(Error),
}
/// The host secrets of `instance` (see the module docs). `release_directory` is the
/// credential directory the host opened at its start, where this system's protected store
/// is one and the setup record names it: a debug build uses it too, and its development
/// folder only without one.
pub(crate) fn store_for(
    dev_base: Option<&std::path::Path>,
    home: &std::path::Path,
    instance: &str,
    own_signing: impl FnOnce() -> Option<crate::code_signing::Signing>,
    release_directory: Option<&ReleaseRoot>,
) -> Result<std::sync::Arc<dyn HostSecrets>, SelectionError> {
    if !org_proto::is_uuid_v7(instance) {
        return Err(SelectionError::InvalidInstance);
    }
    #[cfg(debug_assertions)]
    {
        let _ = own_signing;
        // A credential directory the host opened (its setup record names one) holds this
        // build's secrets as it holds a release build's.
        if let Some(root) = release_directory {
            return Custody::release_directory(root, instance).map(|s| std::sync::Arc::new(s) as _);
        }
        let base = dev_base.ok_or(SelectionError::NoDevBase)?;
        Custody::open(base, home, instance)
            .map(|s| std::sync::Arc::new(s) as _)
            .map_err(SelectionError::Storage)
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = (dev_base, home);
        selected_release_store(
            org_platform_host::credentials::protected_store(),
            option_env!("ORG_HOST_KEYCHAIN_GROUP"),
            instance,
            own_signing,
            release_directory,
        )
    }
}
/// A release build's custody on a system whose protected store is `store`: `host-secrets`
/// below the open release directory where it is one, and the Keychain custody otherwise. A
/// system without a protected store takes the Keychain's path too, and is refused there as
/// it always was.
#[cfg_attr(
    all(debug_assertions, not(test)),
    expect(dead_code, reason = "release selection")
)]
pub(crate) fn selected_release_store(
    store: Result<ProtectedStore, Refused>,
    group: Option<&str>,
    instance: &str,
    own_signing: impl FnOnce() -> Option<crate::code_signing::Signing>,
    release_directory: Option<&ReleaseRoot>,
) -> Result<std::sync::Arc<dyn HostSecrets>, SelectionError> {
    match store {
        Ok(ProtectedStore::ReleaseDirectory) => {
            let root = release_directory.ok_or(SelectionError::NoCredentialDirectory)?;
            Custody::release_directory(root, instance).map(|s| std::sync::Arc::new(s) as _)
        }
        Ok(ProtectedStore::Keychain) | Err(_) => release_store(group, instance, own_signing),
    }
}
#[cfg_attr(
    all(debug_assertions, not(test)),
    expect(dead_code, reason = "release selection")
)]
fn release_store(
    group: Option<&str>,
    instance: &str,
    own_signing: impl FnOnce() -> Option<crate::code_signing::Signing>,
) -> Result<std::sync::Arc<dyn HostSecrets>, SelectionError> {
    if !org_proto::is_uuid_v7(instance) {
        return Err(SelectionError::InvalidInstance);
    }
    let group = group
        .filter(|g| !g.trim().is_empty())
        .ok_or(SelectionError::NoGroup)?;
    let signing = own_signing().ok_or(SelectionError::NotHardened)?;
    if !signing
        .keychain_access_groups
        .as_ref()
        .is_some_and(|groups| groups.iter().any(|g| g == group))
    {
        return Err(SelectionError::NotEntitled);
    }
    if !crate::code_signing::hardened_and_clean(&signing) {
        return Err(SelectionError::NotHardened);
    }
    Custody::keychain(group, instance).map(|s| std::sync::Arc::new(s) as _)
}
