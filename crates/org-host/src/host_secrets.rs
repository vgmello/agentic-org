//! Host registry/TLS/recovery custody, separate from office database keys and their sweep.

#[cfg(debug_assertions)]
pub(crate) mod dev;
#[cfg(target_os = "macos")]
mod keychain;
#[cfg(test)]
mod tests;

use std::{fmt, io};

pub(crate) const MAX_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Name(String);

impl Name {
    #[cfg_attr(
        not(any(test, debug_assertions)),
        expect(dead_code, reason = "activated by 2b.2b hostdb")
    )]
    pub(crate) fn parse(name: &str) -> Result<Self, Error> {
        let valid = name == "registry"
            || name.strip_prefix("tls.").is_some_and(|hex| {
                hex.len() == 64
                    && hex
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            || name
                .strip_prefix("recovery.")
                .is_some_and(org_office_launch::is_plain_id);
        valid
            .then(|| Self(name.to_owned()))
            .ok_or(Error::InvalidName)
    }
    #[cfg_attr(
        not(any(test, debug_assertions)),
        expect(dead_code, reason = "activated by 2b.2b hostdb")
    )]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
    #[cfg_attr(
        not(any(test, debug_assertions, target_os = "macos")),
        expect(dead_code, reason = "host custody adapters")
    )]
    fn is_add_only(&self) -> bool {
        self.0.starts_with("tls.")
    }
}

pub(crate) struct Value(Vec<u8>);

impl Value {
    #[cfg_attr(not(test), expect(dead_code, reason = "activated by 2b.2b hostdb"))]
    pub(crate) fn new(mut bytes: Vec<u8>) -> Result<Self, Error> {
        if bytes.len() > MAX_BYTES {
            crate::secrets::zero(&mut bytes);
            return Err(Error::TooLarge);
        }
        Ok(Self(bytes))
    }
    #[cfg_attr(
        not(any(test, debug_assertions)),
        expect(dead_code, reason = "activated by 2b.2b hostdb")
    )]
    pub(crate) fn expose(&self) -> &[u8] {
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

#[cfg_attr(not(test), expect(dead_code, reason = "activated by 2b.2b hostdb"))]
pub(crate) trait HostSecrets: Send + Sync {
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
#[cfg_attr(
    not(debug_assertions),
    expect(dead_code, reason = "activated by 2b.2b hostdb")
)]
pub(crate) enum Error {
    InvalidName,
    TooLarge,
    Corrupt,
    Unsafe,
    InUse,
    CaseAlias,
    #[cfg(target_os = "macos")]
    Os(i32),
    Io(io::ErrorKind),
}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e.kind())
    }
}

#[derive(Debug, PartialEq, Eq)]
enum SelectionError {
    InvalidInstance,
    #[cfg(debug_assertions)]
    NoDevBase,
    NoGroup,
    NotEntitled,
    NotHardened,
    #[cfg(not(target_os = "macos"))]
    Unsupported,
    #[cfg(debug_assertions)]
    Storage(Error),
}
#[cfg_attr(not(test), expect(dead_code, reason = "hostdb activation"))]
fn store_for(
    dev_base: Option<&std::path::Path>,
    home: &std::path::Path,
    instance: &str,
    own_signing: impl FnOnce() -> Option<crate::code_signing::Signing>,
) -> Result<std::sync::Arc<dyn HostSecrets>, SelectionError> {
    if org_proto::ulid_time_ms(instance).is_none() {
        return Err(SelectionError::InvalidInstance);
    }
    #[cfg(debug_assertions)]
    {
        let _ = own_signing;
        let base = dev_base.ok_or(SelectionError::NoDevBase)?;
        dev::DevSecrets::open(base, home, instance)
            .map(|s| std::sync::Arc::new(s) as _)
            .map_err(SelectionError::Storage)
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = (dev_base, home);
        release_store(
            option_env!("ORG_HOST_KEYCHAIN_GROUP"),
            instance,
            own_signing,
        )
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
    if org_proto::ulid_time_ms(instance).is_none() {
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
    #[cfg(target_os = "macos")]
    {
        keychain::Keychain::new(group, instance)
            .map(|s| std::sync::Arc::new(s) as _)
            .map_err(|_| SelectionError::InvalidInstance)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(SelectionError::Unsupported)
    }
}
