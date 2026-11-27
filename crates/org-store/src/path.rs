//! Durable paths: where a canonical file lives under `durable/`, and the naming rules every
//! stored name obeys.
//!
//! A leaf of the crate: it names no other module. Validation reports an [`InvalidPath`];
//! the store's public constructors (`DurablePath::parse`, `DurablePath::join` and
//! `TryFrom<String>`, beside the commit contract in `change`) report it as the store's
//! `Invalid` error, so a path error and a store error stay one acyclic pair.

use std::{fmt, sync::Arc};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Temporary files of publication start with this, so no record may.
pub(crate) const TEMP_PREFIX: &str = ".store-tmp-";

/// One safe path component: what a file name and an ID must both be. Only the portable
/// set `[a-z0-9._-]`: no file system folds, normalizes or refuses any of it, so two
/// accepted paths never name one file and every accepted path can be written. (No
/// uppercase: a case-insensitive volume would make `A` and `a` one file. UUIDv7 text and
/// every file name of the data contract fit.)
pub(crate) fn component_ok(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 255
        && text != "."
        && text != ".."
        && !text.starts_with(TEMP_PREFIX)
        && text
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'))
}

/// Why a text is not a durable path. Plain data; the store reports it as its `Invalid`
/// error with the same reason.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct InvalidPath(&'static str);

impl InvalidPath {
    pub(crate) fn reason(self) -> &'static str {
        self.0
    }
}

/// The same text the store's `Invalid` error shows for this reason.
impl fmt::Display for InvalidPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid: {}", self.0)
    }
}

/// The trees under `durable/`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Tree {
    State,
    Authority,
    Accounting,
    Messaging,
    Recovery,
    Budgets,
    Local,
}

impl Tree {
    pub const ALL: [Tree; 7] = [
        Tree::State,
        Tree::Authority,
        Tree::Accounting,
        Tree::Messaging,
        Tree::Recovery,
        Tree::Budgets,
        Tree::Local,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Tree::State => "state",
            Tree::Authority => "authority",
            Tree::Accounting => "accounting",
            Tree::Messaging => "messaging",
            Tree::Recovery => "recovery",
            Tree::Budgets => "budgets",
            Tree::Local => "local",
        }
    }
}

/// A relative path under `durable/`, `/`-separated: normal components only, the first
/// naming a known tree. Ordered by its bytes.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DurablePath(Arc<str>);

impl DurablePath {
    /// The longest path, in bytes: it must fit a file-system path with the root before it.
    /// Sized for the deepest data-contract path with headroom: a task comment of a team
    /// at the organization's deepest team chain (`MAX_TEAM_DEPTH = 16`,
    /// `state/teams/<16 UUIDs>/work/tasks/<uuid>/comments/<uuid>.md`, 790 bytes).
    pub const MAX_BYTES: usize = 1024;

    /// The one validator of a durable path's text.
    pub(crate) fn validate(text: &str) -> Result<Self, InvalidPath> {
        if text.len() > Self::MAX_BYTES {
            return Err(InvalidPath("a durable path is at most 1024 bytes"));
        }
        let mut components = text.split('/');
        let tree = components.next().unwrap_or_default();
        if !Tree::ALL.iter().any(|known| known.name() == tree) {
            return Err(InvalidPath("a durable path starts with a known tree"));
        }
        if !components.all(component_ok) {
            return Err(InvalidPath("a durable path has only normal components"));
        }
        Ok(Self(text.into()))
    }

    /// `self` with one more component below it.
    pub(crate) fn child(&self, component: &str) -> Result<Self, InvalidPath> {
        if !component_ok(component) {
            return Err(InvalidPath("a durable path has only normal components"));
        }
        Ok(Self(format!("{}/{component}", self.0).into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn tree(&self) -> Tree {
        let name = self.components().next().unwrap_or_default();
        Tree::ALL
            .into_iter()
            .find(|tree| tree.name() == name)
            .expect("validated at construction")
    }

    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }

    /// A tree alone names a directory; a file has at least one component below it.
    pub fn names_a_file(&self) -> bool {
        self.0.contains('/')
    }

    /// The path one component up; `None` for a tree.
    pub fn parent(&self) -> Option<Self> {
        let (above, _) = self.0.rsplit_once('/')?;
        Some(Self(above.into()))
    }

    /// `self` is `prefix` or lies under it, by whole components.
    pub fn is_under(&self, prefix: &DurablePath) -> bool {
        match self.0.strip_prefix(prefix.as_str()) {
            Some(rest) => rest.is_empty() || rest.starts_with('/'),
            None => false,
        }
    }
}

impl From<DurablePath> for String {
    fn from(path: DurablePath) -> String {
        path.0.as_ref().to_owned()
    }
}

/// Its text, as a string.
impl Serialize for DurablePath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

/// A string, validated like any other durable path.
impl<'de> Deserialize<'de> for DurablePath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::validate(&text).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for DurablePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for DurablePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DurablePath({})", self.0)
    }
}

#[cfg(test)]
mod tests;
