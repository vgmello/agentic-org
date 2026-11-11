//! Host registry/TLS/recovery custody, separate from office database keys and their sweep.

#[cfg(debug_assertions)]
pub(crate) mod dev;
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
    Io(io::ErrorKind),
}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e.kind())
    }
}
