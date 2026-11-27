//! The one path validator of a backup (`.agents/rules` section 2): every path read from the
//! durable root or from an archive becomes a [`RelPath`] before it is joined, opened or
//! created, and the durable layout's policy says which of them a backup carries.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The archive member that holds the manifest; always the first.
pub const MANIFEST_MEMBER: &str = "manifest.json";
/// The prefix of every member below the durable root.
pub const DURABLE_PREFIX: &str = "durable/";
/// The longest relative path a backup carries.
pub const MAX_PATH_BYTES: usize = 1024;
/// The deepest folder a backup walks.
pub const MAX_DEPTH: usize = 64;
/// The private task notes database, relative to the durable root.
pub const NOTES_DB: &str = "private/task-notes.db";
/// HQ's private file space database, relative to the durable root.
pub const USER_SPACE_DB: &str = "private/user-space.db";

/// A WAL-mode private SQLite database a backup carries as a consistent `VACUUM INTO` copy and
/// a verification runs SQLite's integrity check on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PrivateDb {
    /// `private/task-notes.db`.
    TaskNotes,
    /// `private/user-space.db`.
    UserSpace,
}

impl PrivateDb {
    pub(crate) const ALL: [PrivateDb; 2] = [PrivateDb::TaskNotes, PrivateDb::UserSpace];

    /// Its one canonical path below the durable root.
    pub(crate) fn path(self) -> &'static str {
        match self {
            PrivateDb::TaskNotes => NOTES_DB,
            PrivateDb::UserSpace => USER_SPACE_DB,
        }
    }

    /// Its file name, for the scratch copy.
    pub(crate) fn file_name(self) -> &'static str {
        match self {
            PrivateDb::TaskNotes => "task-notes.db",
            PrivateDb::UserSpace => "user-space.db",
        }
    }

    /// What it is, for a message.
    pub(crate) fn what(self) -> &'static str {
        match self {
            PrivateDb::TaskNotes => "private task notes database",
            PrivateDb::UserSpace => "private file space database",
        }
    }
}

/// A path below the durable root: UTF-8, `/`-separated, each component non-empty, not `.`
/// or `..`, without NUL or another control character, at most [`MAX_PATH_BYTES`] long.
/// Ordered by its bytes, which is the archive's member order.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RelPath(String);

impl RelPath {
    /// Checks `text`; the refusal says why, with `text` escaped.
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.is_empty() {
            return Err("an empty path".into());
        }
        if text.len() > MAX_PATH_BYTES {
            return Err(format!(
                "a path over {MAX_PATH_BYTES} bytes: {}",
                shown(text)
            ));
        }
        if text.starts_with('/') {
            return Err(format!("an absolute path: {}", shown(text)));
        }
        for component in text.split('/') {
            check_component(component).map_err(|why| format!("{why}: {}", shown(text)))?;
        }
        Ok(Self(text.to_owned()))
    }

    /// This path with one more component.
    pub fn child(&self, name: &str) -> Result<Self, String> {
        Self::parse(&format!("{}/{name}", self.0))
    }

    /// The first component alone.
    pub fn top(name: &str) -> Result<Self, String> {
        check_component(name).map_err(|why| format!("{why}: {}", shown(name)))?;
        Self::parse(name)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }

    /// The folder that holds this path, if it is not at the top.
    pub fn parent(&self) -> Option<RelPath> {
        self.0
            .rsplit_once('/')
            .map(|(parent, _)| Self(parent.to_owned()))
    }

    /// `root` joined with each component: never anything but normal components.
    pub fn under(&self, root: &Path) -> PathBuf {
        let mut path = root.to_path_buf();
        for component in self.components() {
            path.push(component);
        }
        path
    }

    /// The archive member name.
    pub fn member(&self) -> String {
        format!("{DURABLE_PREFIX}{}", self.0)
    }

    /// The path an archive member names, which must be below [`DURABLE_PREFIX`].
    pub fn from_member(member: &str) -> Result<Self, String> {
        if member.starts_with('/') {
            return Err(format!("an absolute path: {}", shown(member)));
        }
        if member.split('/').any(|c| c == "..") {
            return Err(format!("a path that climbs out (`..`): {}", shown(member)));
        }
        let rest = member
            .strip_prefix(DURABLE_PREFIX)
            .ok_or_else(|| format!("a member outside `durable/`: {}", shown(member)))?;
        Self::parse(rest.strip_suffix('/').unwrap_or(rest))
    }
}

impl TryFrom<String> for RelPath {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        Self::parse(&text)
    }
}

impl From<RelPath> for String {
    fn from(path: RelPath) -> String {
        path.0
    }
}

fn check_component(component: &str) -> Result<(), &'static str> {
    match component {
        "" => Err("an empty path component"),
        "." | ".." => Err("a `.` or `..` path component"),
        _ if component.chars().any(char::is_control) => {
            Err("a control character in a path component")
        }
        _ => Ok(()),
    }
}

/// `text` with control and other non-printing characters escaped, for a message.
pub fn shown(text: &str) -> String {
    text.chars()
        .flat_map(|c| {
            if c.is_control() || c == '\u{200e}' || c == '\u{200f}' {
                c.escape_unicode().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

/// What a backup does with one entry of the durable root.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Disposition {
    /// Carried as it is; a link or a special file here fails the backup.
    Keep,
    /// Inside an agent's private home, which the agent itself writes: folders and regular
    /// files are carried, while a link or a special file (a runtime's `latest` link, a
    /// socket) is left out, never followed and never archived.
    AgentHome,
    /// Disposable: an enrollment's scratch folder, the unattached uploads waiting in
    /// `attachment-staging/`, an agent's runtime or temporary folder, and the host toolchain
    /// caches the office links into an agent's home again on its next
    /// session (`.cargo/registry`, `.cargo/git`, a legacy `.rustup`).
    Skip,
    /// A WAL-mode private database: carried as a consistent copy.
    PrivateDatabase(PrivateDb),
    /// A `-wal`, `-shm` or `-journal` file beside a private database: in the copy already.
    PrivateSidecar,
}

impl Disposition {
    /// Whether a backup ever carries a path with this disposition; an archive that lists
    /// one was not written by a backup.
    pub(crate) fn is_carried(self) -> bool {
        !matches!(self, Disposition::Skip | Disposition::PrivateSidecar)
    }
}

/// The durable layout's policy for `path` (every tree under `<durable>/` is carried; these
/// are the exceptions). A satellite's `blob-outbox/` (`TASK-98.6`) is carried on purpose: it
/// holds the only copy of shared files HQ has not acknowledged yet. Applies to folders and files alike. Each name is compared ASCII
/// case-insensitively: on a case-insensitive file system (macOS's default APFS)
/// `private/User-Space.db-WAL` or `.../Enroll-Tmp` is the very file or folder the policy
/// names, so it must get the same answer ([`Manifest`](crate::manifest::Manifest) then
/// refuses any spelling but the canonical one where that matters).
pub(crate) fn disposition(path: &RelPath) -> Disposition {
    let folded: Vec<String> = path.components().map(str::to_ascii_lowercase).collect();
    let parts: Vec<&str> = folded.iter().map(String::as_str).collect();
    match parts.as_slice() {
        ["local", "offices", _, "enroll-tmp" | "attachment-staging", ..]
        | ["local", "offices", _, "agents", _, "runtime" | "tmp", ..]
        | ["local", "offices", _, "agents", _, "home", ".cargo", "registry" | "git", ..]
        | ["local", "offices", _, "agents", _, "home", ".rustup", ..] => Disposition::Skip,
        ["local", "offices", _, "agents", _, "home", _, ..] => Disposition::AgentHome,
        ["private", "task-notes.db"] => Disposition::PrivateDatabase(PrivateDb::TaskNotes),
        ["private", "user-space.db"] => Disposition::PrivateDatabase(PrivateDb::UserSpace),
        ["private", "task-notes.db-wal"
        | "task-notes.db-shm"
        | "task-notes.db-journal"
        | "user-space.db-wal"
        | "user-space.db-shm"
        | "user-space.db-journal"] => Disposition::PrivateSidecar,
        _ => Disposition::Keep,
    }
}
