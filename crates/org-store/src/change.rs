//! The commit contract: what an operation is, and what it leaves behind.

use std::{fmt, sync::Arc};

use serde::{Deserialize, Serialize};
use sha2::Digest;

use crate::error::{EngineError, StoreError};

/// Shared immutable bytes: file contents and replies.
pub type Bytes = Arc<[u8]>;

/// A SHA-256 of bytes as stored (`raw-bytes/1`). Text form: 64 lowercase hex digits.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sha256([u8; 32]);

impl Sha256 {
    pub fn of(bytes: &[u8]) -> Self {
        Self(sha2::Sha256::digest(bytes).into())
    }

    pub fn of_parts(parts: &[&[u8]]) -> Self {
        let mut hasher = sha2::Sha256::new();
        for part in parts {
            hasher.update(part);
        }
        Self(hasher.finalize().into())
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn from_hex(text: &str) -> Option<Self> {
        let text = text.as_bytes();
        if text.len() != 64 {
            return None;
        }
        let digit = |byte: u8| match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        };
        let mut out = [0u8; 32];
        for (i, pair) in text.as_chunks::<2>().0.iter().enumerate() {
            out[i] = digit(pair[0])? << 4 | digit(pair[1])?;
        }
        Some(Self(out))
    }

    pub fn to_hex(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sha256({self})")
    }
}

impl Serialize for Sha256 {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Sha256 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::from_hex(&text)
            .ok_or_else(|| serde::de::Error::custom("not a SHA-256 in lowercase hex"))
    }
}

/// Temporary files of publication start with this, so no record may.
pub(crate) const TEMP_PREFIX: &str = ".store-tmp-";

/// One safe path component: what a file name and an ID must both be. Only the portable
/// set `[a-z0-9._-]`: no file system folds, normalizes or refuses any of it, so two
/// accepted paths never name one file and every accepted path can be written. (No
/// uppercase: a case-insensitive volume would make `A` and `a` one file. UUIDv7 text and
/// every file name of the data contract fit.)
fn component_ok(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 255
        && text != "."
        && text != ".."
        && !text.starts_with(TEMP_PREFIX)
        && text
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'))
}

/// An identifier a caller supplies. The store mints none and checks only that it is one
/// safe path component; the family that owns an ID validates its form (UUIDv7).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Id(Arc<str>);

impl Id {
    pub fn parse(text: &str) -> Result<Self, StoreError> {
        if !component_ok(text) {
            return Err(StoreError::invalid("an ID must be one safe path component"));
        }
        Ok(Self(text.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Id {
    type Error = StoreError;
    fn try_from(text: String) -> Result<Self, StoreError> {
        Self::parse(&text)
    }
}

impl From<Id> for String {
    fn from(id: Id) -> String {
        id.0.as_ref().to_owned()
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({})", self.0)
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
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DurablePath(Arc<str>);

impl DurablePath {
    /// The longest path, in bytes: it must fit a file-system path with the root before it.
    pub const MAX_BYTES: usize = 512;

    pub fn parse(text: &str) -> Result<Self, StoreError> {
        if text.len() > Self::MAX_BYTES {
            return Err(StoreError::invalid("a durable path is at most 512 bytes"));
        }
        let mut components = text.split('/');
        let tree = components.next().unwrap_or_default();
        if !Tree::ALL.iter().any(|known| known.name() == tree) {
            return Err(StoreError::invalid(
                "a durable path starts with a known tree",
            ));
        }
        if !components.all(component_ok) {
            return Err(StoreError::invalid(
                "a durable path has only normal components",
            ));
        }
        Ok(Self(text.into()))
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

    pub fn join(&self, component: &str) -> Result<Self, StoreError> {
        if !component_ok(component) {
            return Err(StoreError::invalid(
                "a durable path has only normal components",
            ));
        }
        Ok(Self(format!("{}/{component}", self.0).into()))
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

impl TryFrom<String> for DurablePath {
    type Error = StoreError;
    fn try_from(text: String) -> Result<Self, StoreError> {
        Self::parse(&text)
    }
}

impl From<DurablePath> for String {
    fn from(path: DurablePath) -> String {
        path.0.as_ref().to_owned()
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

/// When the office may answer (ADR-0014). Declared by the family that owns the operation.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ack {
    /// Appended and readable. A power cut before the next group flush can take it back.
    Committed,
    /// As committed, and the journal is fully flushed through its entry.
    Durable,
}

/// One file an operation leaves behind, in the exact bytes the data contract defines.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FileChange {
    /// Immutable (an event, a message). Other bytes at that path are a fault.
    Create(DurablePath, Bytes),
    /// Mutable (a task, a team). Replaces what this office wrote last.
    Put(DurablePath, Bytes),
    Remove(DurablePath),
}

impl FileChange {
    pub fn path(&self) -> &DurablePath {
        match self {
            FileChange::Create(path, _) | FileChange::Put(path, _) | FileChange::Remove(path) => {
                path
            }
        }
    }

    pub fn bytes(&self) -> Option<&Bytes> {
        match self {
            FileChange::Create(_, bytes) | FileChange::Put(_, bytes) => Some(bytes),
            FileChange::Remove(_) => None,
        }
    }
}

/// Everything one admitted operation commits, as one unit.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Change {
    pub ack: Ack,
    pub files: Vec<FileChange>,
    pub reply: Bytes,
}

impl Change {
    pub fn new(ack: Ack) -> Self {
        Self {
            ack,
            files: Vec::new(),
            reply: Bytes::from(Vec::new()),
        }
    }

    pub fn create(mut self, path: DurablePath, bytes: impl Into<Bytes>) -> Self {
        self.files.push(FileChange::Create(path, bytes.into()));
        self
    }

    pub fn put(mut self, path: DurablePath, bytes: impl Into<Bytes>) -> Self {
        self.files.push(FileChange::Put(path, bytes.into()));
        self
    }

    pub fn remove(mut self, path: DurablePath) -> Self {
        self.files.push(FileChange::Remove(path));
        self
    }

    pub fn reply(mut self, reply: impl Into<Bytes>) -> Self {
        self.reply = reply.into();
        self
    }
}

/// Why a decision produced no change.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Refusal {
    /// Admitted and rejected. With an operation ID this is a receipt-only entry, so a
    /// retry gets the same answer. It keeps no origin position it took.
    Rejected(Bytes),
    /// Refused before admission: nothing is recorded.
    Invalid(StoreError),
}

impl Refusal {
    pub fn rejected(reply: impl Into<Bytes>) -> Self {
        Self::Rejected(reply.into())
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(StoreError::Invalid(message.into()))
    }
}

impl From<StoreError> for Refusal {
    fn from(error: StoreError) -> Self {
        Self::Invalid(error)
    }
}

impl From<EngineError> for Refusal {
    fn from(error: EngineError) -> Self {
        Self::Invalid(StoreError::Engine(error))
    }
}

/// Who asked.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Employee(Id),
    System,
}

/// What a retry is matched on: the receipt identity.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Operation {
    pub id: Id,
    pub kind: &'static str,
    pub actor: Actor,
    pub fingerprint: Sha256,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Completed,
    Rejected,
}

/// The record of one admitted operation, with its original reply.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Receipt {
    pub id: Id,
    pub kind: String,
    pub actor: Actor,
    pub fingerprint: Sha256,
    pub outcome: Outcome,
    /// The class the original was answered in. A retry waits for the same.
    pub ack: Ack,
    /// The journal entry that holds it.
    pub ordinal: u64,
    pub at: Timestamp,
    pub reply: Bytes,
}

/// Unix time in milliseconds, UTC.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Timestamp(pub u64);

/// The writer's clock. A decision reads no other.
pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

/// The machine's wall clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        Timestamp(u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(text: &str) -> DurablePath {
        DurablePath::parse(text).unwrap()
    }

    #[test]
    fn a_durable_path_is_relative_normal_and_under_a_known_tree() {
        for good in [
            "state",
            "state/organization.yaml",
            "recovery/commands/01900000-0000-7000-8000-000000000001.json",
            "local/offices/a/probe/x",
            "state/teams/a_b-c.d/0.yaml",
        ] {
            assert_eq!(path(good).as_str(), good);
        }
        let long = "a".repeat(256);
        for bad in [
            "",
            "/state/a",
            "state/../authority/a",
            "state/./a",
            "state//a",
            "state/a/",
            "runtime/a",
            "State/a",
            // Only the portable set: nothing a file system folds, normalizes or refuses.
            "state/Teams/a",
            "state/teams/ünïcode/team.yaml",
            "state/a b",
            "state/a:b",
            "state/a*",
            "state/a\nb",
            "state/a\0b",
            "state/a\\b",
            "../state/a",
            "state/.store-tmp-1-1",
            &format!("state/{long}"),
        ] {
            assert!(DurablePath::parse(bad).is_err(), "accepted {bad:?}");
        }
        assert!(DurablePath::parse(&format!("state/{}", "a".repeat(255))).is_ok());
        for bad in [
            "",
            "A",
            "01900000-0000-7000-8000-00000000000A",
            "a/b",
            "é",
            "a b",
        ] {
            assert!(Id::parse(bad).is_err(), "accepted the ID {bad:?}");
        }
        assert!(Id::parse("01900000-0000-7000-8000-00000000000a").is_ok());
        assert_eq!(path("state/a/b").parent(), Some(path("state/a")));
        assert_eq!(path("state").parent(), None);
        let deep = format!("state/{}", "abcdefg/".repeat(64));
        assert!(DurablePath::parse(&deep[..512]).is_ok());
        assert!(DurablePath::parse(&format!("{}x", &deep[..512])).is_err());
    }

    #[test]
    fn paths_order_by_bytes_and_prefixes_match_whole_components() {
        let teams = path("state/teams");
        assert!(path("state/teams/a/team.yaml").is_under(&teams));
        assert!(path("state/teams").is_under(&teams));
        assert!(!path("state/teams-old/a").is_under(&teams));
        assert!(!path("state").is_under(&teams));
        assert_eq!(teams.join("a").unwrap(), path("state/teams/a"));
        assert!(teams.join("a/b").is_err());
        assert!(teams.join("..").is_err());
        assert_eq!(path("local/a").tree(), Tree::Local);
        assert!(!path("state").names_a_file());
        assert!(path("state/a").names_a_file());
        let mut sorted = [path("state/b"), path("state/a/z"), path("state/a-")];
        sorted.sort();
        let texts: Vec<_> = sorted.iter().map(DurablePath::as_str).collect();
        assert_eq!(texts, ["state/a-", "state/a/z", "state/b"]);
        assert_eq!(
            serde_json::from_str::<DurablePath>(r#""state/a""#).unwrap(),
            path("state/a")
        );
        assert!(serde_json::from_str::<DurablePath>(r#""state/../a""#).is_err());
    }

    #[test]
    fn an_id_is_one_safe_path_component() {
        let id = Id::parse("01900000-0000-7000-8000-000000000001").unwrap();
        assert_eq!(id.as_str(), "01900000-0000-7000-8000-000000000001");
        assert_eq!(
            serde_json::to_string(&id).unwrap(),
            r#""01900000-0000-7000-8000-000000000001""#
        );
        for bad in ["", ".", "..", "a/b", "a\\b", "a\u{7}", &"a".repeat(256)] {
            assert!(Id::parse(bad).is_err(), "accepted {bad:?}");
            assert!(
                serde_json::from_value::<Id>(serde_json::Value::String(bad.to_owned())).is_err()
            );
        }
    }

    #[test]
    fn a_change_lists_its_files_in_order_with_its_class_and_reply() {
        let change = Change::new(Ack::Durable)
            .put(path("state/a"), b"one".to_vec())
            .create(path("state/b"), b"two".to_vec())
            .remove(path("state/c"))
            .reply(b"done".to_vec());
        assert_eq!(change.ack, Ack::Durable);
        assert_eq!(&*change.reply, b"done");
        let seen: Vec<_> = change
            .files
            .iter()
            .map(|file| (file.path().as_str(), file.bytes().map(|b| b.to_vec())))
            .collect();
        assert_eq!(
            seen,
            [
                ("state/a", Some(b"one".to_vec())),
                ("state/b", Some(b"two".to_vec())),
                ("state/c", None)
            ]
        );
        assert!(matches!(
            Refusal::invalid("no"),
            Refusal::Invalid(StoreError::Invalid(_))
        ));
        assert!(matches!(
            Refusal::rejected(b"r".to_vec()),
            Refusal::Rejected(_)
        ));
    }

    #[test]
    fn hashes_and_actors_have_one_text_form() {
        let hash = Sha256::of(b"abc");
        assert_eq!(
            hash.to_hex(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(Sha256::from_hex(&hash.to_hex()), Some(hash));
        assert_eq!(Sha256::of_parts(&[b"a", b"bc"]), hash);
        assert_eq!(Sha256::from_hex(&hash.to_hex().to_uppercase()), None);
        assert_eq!(Sha256::from_hex("ab"), None);
        assert_eq!(
            serde_json::to_string(&Actor::System).unwrap(),
            r#""system""#
        );
        assert_eq!(
            serde_json::to_string(&Actor::Employee(Id::parse("e1").unwrap())).unwrap(),
            r#"{"employee":"e1"}"#
        );
        assert_eq!(
            serde_json::to_string(&Ack::Committed).unwrap(),
            r#""committed""#
        );
    }
}
