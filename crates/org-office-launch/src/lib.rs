//! # org-office-launch: how the host starts an office server
//!
//! The private contract between `org-office-host` (the login service) and `org-office-server` (one
//! process per office). The app never uses it. `org-agent` uses only [`OfficeFile`], to read
//! `office.json` (never by hand: the type checks the version, the ids, the names and `created_at`).
//! The app and `org-agent` speak `org-proto` to the servers. See
//! `docs/design/offices-hq-and-satellites.md` ("Processes on one Mac") and the host/server boundary
//! design (`docs/design/office-host-boundary.md`).
//!
//! **A contract (owner: ada): additive and versioned.** Every line and `office.json` carries
//! `"v"`. Readers ignore unknown fields, unknown host ops after the first line
//! ([`HostMessage::Unknown`]; as the first line it is [`FailCode::InvalidRequest`]) and unknown
//! server events ([`ServerEvent::Unknown`]), and decode unknown failure codes as
//! [`FailCode::StorageError`]. A field a create can't work without (e.g. a satellite's HQ
//! address) arrives with a role or op that older servers refuse, never as a field they would
//! silently ignore. `v` is reserved in every line: no op or event has a field named `v`.
//!
//! The types check their own rules (plain ids, a role this version can create) whenever they
//! are decoded or encoded, by any serde path. The `"v"` of a pipe line is checked by
//! [`read_host_message`] and [`read_server_event`], so read and write lines only through them.
//!
//! ## Starting a server
//! ```text
//! org-office-server --create            # create a new office from the create message, then serve it
//! org-office-server --office <id>       # serve an existing office
//! org-office-server --discard <id>      # remove an unfinished office (no office.json), then exit
//! ```
//! - The host makes `<home>`, `<home>/offices/` and `<home>/run/` (owner-only) and sets
//!   `ORG_OFFICE_HOME` ([`org_proto::location::OFFICE_HOME_ENV`]) on the child, so both use the
//!   same office home.
//! - **stdin is the control pipe from the host.** Its first line matches the flag: a
//!   [`HostMessage::Create`] with `--create`, a [`HostMessage::Start`] with `--office`. A server
//!   whose first line is malformed, an op it doesn't know, or for the other flag fails with
//!   [`FailCode::InvalidRequest`]. EOF on stdin means the host is gone: the server shuts down
//!   cleanly and exits 0, also when the EOF comes before the first line (then without a
//!   `failed` line: nobody is left to read it), and when the pipe ends in the middle of the first
//!   line ([`LaunchError::is_host_gone`]). After the first line, a v1 server only drains stdin
//!   and discards what it reads, known ops included, without parsing it, so nothing on a later
//!   line can stop it from seeing the EOF. `--discard` reads no line.
//! - **Descriptors:** no child of a server can read the host's lines or write a control line.
//!   First thing at startup, before any thread or child starts, the server moves its stdin and
//!   stdout pipes to new close-on-exec descriptors and reopens 0 and 1 on `/dev/null`. Every
//!   other descriptor it opens is close-on-exec too, the `office.lock` one above all (a child that
//!   inherited it would keep the lock after the server died). A child's stdin and stdout are
//!   `/dev/null` unless it's given its own pipes; stderr (the logs) is inherited. The host opens
//!   its own descriptors close-on-exec, so no child holds `host.lock`.
//! - **The app token** ([`AppToken`], ADR-0003 §2.1) travels only in that first line, on every
//!   start. The host makes a new one for each office each time the host starts and keeps it only
//!   in memory. The server keeps it only in memory and never logs it. The database key never
//!   crosses the pipe: each server creates and keeps its own (ADR-0002).
//! - **Zeroing is best effort.** [`read_host_message`] and [`write_host_message`] zero their own
//!   buffers and never reallocate them with the token inside. Copies outside this crate remain:
//!   the caller's reader and writer buffers (e.g. `Stdin`'s, which may keep the line for the
//!   process's life; read stdin unbuffered or through a small buffer you control), and serde's
//!   buffers when the token arrives with JSON escapes. See `org_proto`'s `AppToken` docs.
//! - **stdout carries only [`ServerEvent`] lines.** Logs go to stderr.
//! - Lines are JSON, one object per line, at most [`org_proto::MAX_FRAME_BYTES`]
//!   ([`org_proto::frame`]). Use [`read_host_message`], [`write_host_message`],
//!   [`read_server_event`] and [`write_server_event`].
//! - The server listens on [`org_proto::location::office_socket_path`]; the host computes the
//!   same path and only cross-checks [`ServerEvent::Ready`]'s `socket`.
//!
//! ## An office's folder
//! `<home>/offices/<office-id>/` ([`office_dir`]), made by the server (owner-only) on
//! `--create`. The server holds [`OFFICE_LOCK_FILE_NAME`] with an exclusive `flock` from before
//! it opens anything until it exits, waiting up to [`OFFICE_LOCK_WAIT`] for it, so the lock is
//! held exactly while a server runs that office. It writes [`OFFICE_FILE_NAME`] **last** on
//! `--create` (a temp file, fsync it, rename it into place, then fsync the folder): an office
//! folder without it is an unfinished create.
//!
//! ## Discarding an unfinished office
//! An unfinished create can leave a Keychain key the host can't delete (the item belongs to the
//! server). So the host runs `--discard <id>` for every plain-id office folder without
//! `office.json` (a folder with an unreadable or invalid `office.json` is logged and left alone).
//! The server takes the office lock (same wait) and removes only an office that never finished.
//! It refuses ([`FailCode::AlreadyExists`]) a folder that holds `office.json`, and deletes only
//! when no database file exists, or when it can open the databases and they hold no committed
//! organization. A committed organization is [`FailCode::AlreadyExists`], and databases it can't
//! open or check are [`FailCode::StorageError`]; either way the host logs it and leaves the
//! folder for a person, so a real office that lost its `office.json` is never destroyed
//! automatically. Otherwise it deletes the office's key if there is one, removes the folder,
//! reports [`ServerEvent::Discarded`] and exits 0. Discarding an office that has no folder and no
//! key also reports `discarded`.
//!
//! **A refused create.** If a server reports a `created` office that isn't the one the host asked
//! for (another id, organization, name or role), the host doesn't adopt it. It stops the server
//! and moves the folder of the office id **it asked for** to `offices/.refused/<id>` (out of
//! the catalog: `.refused` isn't a plain id), keeping its data and leaving its key, for a person to
//! look at. It never touches a folder named by a different id the server reported; it only logs
//! that id. Nothing is deleted. The host treats a folder whose `office.json` names another id
//! the same way at startup: it moves it to `offices/.refused/<folder name>` and never starts it.

#![deny(unsafe_code)]

use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use org_proto::frame::{self, FrameError};
use org_proto::{AppToken, OfficeRole};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The version of the pipe messages and of `office.json`. Bumped only for a breaking change.
pub const LAUNCH_VERSION: u32 = 1;

/// The command-line flag that creates a new office from the create message on stdin.
pub const CREATE_FLAG: &str = "--create";
/// The command-line flag, followed by the office id, that serves an existing office.
pub const OFFICE_FLAG: &str = "--office";
/// The command-line flag, followed by the office id, that removes an unfinished office.
pub const DISCARD_FLAG: &str = "--discard";

/// The folder under the office home that holds one folder per office.
pub const OFFICES_DIR_NAME: &str = "offices";
/// The office's description, in its folder. Plaintext, no secrets.
pub const OFFICE_FILE_NAME: &str = "office.json";
/// The office lock, in its folder: held exactly while a server runs that office.
pub const OFFICE_LOCK_FILE_NAME: &str = "office.lock";
/// How long a starting server waits for the office lock before failing with
/// [`FailCode::InUse`].
pub const OFFICE_LOCK_WAIT: Duration = Duration::from_secs(10);
/// The largest `office.json`, so a reader can cap its read and a `created` event always fits a
/// line.
pub const MAX_OFFICE_FILE_BYTES: usize = 64 * 1024;

/// The folder of the office `office_id` in `office_home`, or `None` if the id isn't a plain id
/// (non-empty, ASCII letters and digits only), the same rule as
/// [`org_proto::location::office_socket_path`]. Ids arrive through pipes and files, so this
/// never lets one point outside `offices/`.
pub fn office_dir(office_home: &Path, office_id: &str) -> Option<PathBuf> {
    is_plain_id(office_id).then(|| office_home.join(OFFICES_DIR_NAME).join(office_id))
}

/// The longest plain id, the same cap as [`org_proto::location`].
pub const MAX_ID_CHARS: usize = 64;

/// Whether `id` is a plain id: 1 to [`MAX_ID_CHARS`] ASCII letters and digits.
pub fn is_plain_id(id: &str) -> bool {
    (1..=MAX_ID_CHARS).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Whether `text` is an RFC 3339 UTC timestamp with milliseconds, `YYYY-MM-DDTHH:MM:SS.mmmZ`,
/// with each field in its range (days up to 31; the calendar isn't checked further).
fn is_timestamp(text: &str) -> bool {
    let b = text.as_bytes();
    if b.len() != 24 {
        return false;
    }
    let shape = b"dddd-dd-ddTdd:dd:dd.dddZ";
    let digits_ok = b.iter().zip(shape).all(|(&c, &s)| {
        if s == b'd' {
            c.is_ascii_digit()
        } else {
            c == s
        }
    });
    if !digits_ok {
        return false;
    }
    let num = |from: usize, to: usize| {
        b[from..to]
            .iter()
            .fold(0u32, |n, &c| n * 10 + u32::from(c - b'0'))
    };
    (1..=12).contains(&num(5, 7))
        && (1..=31).contains(&num(8, 10))
        && num(11, 13) < 24
        && num(14, 16) < 60
        && num(17, 19) < 60
}

/// What `office.json` holds. The server writes it; the host reads it to list this machine's
/// organizations and offices.
///
/// Its `"v"` is always [`LAUNCH_VERSION`], written by this crate, never chosen by callers. Every
/// decode, including the `office` inside [`ServerEvent::Created`], checks the version and that
/// both ids are plain ([`is_plain_id`]); every encode checks the ids. Fields this build doesn't
/// know are kept and written back, so an older server that rewrites the file (e.g. after a
/// rename) never drops what a newer one added. A missing or unknown role decodes as
/// [`OfficeRole::Unknown`]; while it stays `Unknown`, the original value (or its absence) is
/// written back as it was. A file with no known role that wasn't read from disk can't be
/// written ([`LaunchError::Invalid`]): every office this build writes has a role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficeFile {
    pub office_id: String,
    pub office_name: String,
    pub role: OfficeRole,
    pub org_id: String,
    pub org_name: String,
    /// When the organization was created: RFC 3339 UTC with milliseconds.
    pub created_at: String,
    /// Fields this build doesn't know, kept as they were.
    extra: serde_json::Map<String, serde_json::Value>,
    /// The stored `role` when this build doesn't know it, written back while `role` stays
    /// [`OfficeRole::Unknown`]. `None`: the file wasn't read with an unknown role.
    unknown_role: Option<StoredRole>,
}

/// A role this build didn't know, as it was stored.
#[derive(Debug, Clone, PartialEq, Eq)]
enum StoredRole {
    /// The file had no `role` key.
    Absent,
    /// The file's value, `null` included.
    Value(serde_json::Value),
}

impl OfficeFile {
    pub fn new(
        office_id: impl Into<String>,
        office_name: impl Into<String>,
        role: OfficeRole,
        org_id: impl Into<String>,
        org_name: impl Into<String>,
        created_at: impl Into<String>,
    ) -> Self {
        OfficeFile {
            office_id: office_id.into(),
            office_name: office_name.into(),
            role,
            org_id: org_id.into(),
            org_name: org_name.into(),
            created_at: created_at.into(),
            extra: serde_json::Map::new(),
            unknown_role: None,
        }
    }

    /// Decode `office.json`. Unknown fields are kept; another `v` is
    /// [`LaunchError::UnsupportedVersion`]; ids that aren't plain, names that break
    /// [`org_proto::OrganizationName`]'s rule (or aren't trimmed), a `created_at` that isn't
    /// `YYYY-MM-DDTHH:MM:SS.mmmZ`, or a file over [`MAX_OFFICE_FILE_BYTES`] are
    /// [`LaunchError::Invalid`]. A role this build doesn't know is kept as
    /// [`OfficeRole::Unknown`] and written back as it was.
    ///
    /// **The reader's duties** (the host and `org-agent`, which read the file; this crate does
    /// no I/O): check that `office_id` names the folder the file was read from, and read the file
    /// without following symlinks, only if it's a regular file owned by this user, capped at
    /// [`MAX_OFFICE_FILE_BYTES`]. The server writes it owner-only (0600), to a temp file that it
    /// fsyncs and renames into place, then fsyncs the folder.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, LaunchError> {
        if bytes.len() > MAX_OFFICE_FILE_BYTES {
            return Err(LaunchError::Invalid("office.json is too large"));
        }
        let version: Versioned = serde_json::from_slice(bytes).map_err(LaunchError::Json)?;
        check_version(version.v)?;
        let raw: RawOfficeFile = serde_json::from_slice(bytes).map_err(LaunchError::Json)?;
        raw.try_into()
    }

    /// Encode as `office.json` (pretty JSON with a trailing newline). Anything [`from_slice`]
    /// would refuse, or an unknown role that wasn't read from disk, is refused, so a server can
    /// never write a file the host would reject.
    ///
    /// [`from_slice`]: OfficeFile::from_slice
    pub fn to_vec(&self) -> Result<Vec<u8>, LaunchError> {
        self.check_writable()?;
        let mut out = serde_json::to_vec_pretty(self).map_err(LaunchError::Json)?;
        out.push(b'\n');
        if out.len() > MAX_OFFICE_FILE_BYTES {
            return Err(LaunchError::Invalid("office.json would be too large"));
        }
        Ok(out)
    }

    /// What every write needs: what every read needs, plus a known role or the one it was read
    /// with.
    fn check_writable(&self) -> Result<(), LaunchError> {
        self.check_fields()?;
        if self.role == OfficeRole::Unknown && self.unknown_role.is_none() {
            return Err(LaunchError::Invalid("office.json needs a known role"));
        }
        Ok(())
    }

    /// What every read and write needs: plain ids, valid names, a well-formed `created_at`.
    fn check_fields(&self) -> Result<(), LaunchError> {
        self.check_ids()?;
        for name in [&self.org_name, &self.office_name] {
            match org_proto::OrganizationName::parse(name) {
                Ok(parsed) if parsed.as_str() == name.as_str() => {}
                _ => return Err(LaunchError::Invalid("office.json holds an invalid name")),
            }
        }
        if !is_timestamp(&self.created_at) {
            return Err(LaunchError::Invalid(
                "office.json holds an invalid created_at",
            ));
        }
        Ok(())
    }

    fn check_ids(&self) -> Result<(), LaunchError> {
        if is_plain_id(&self.office_id) && is_plain_id(&self.org_id) {
            Ok(())
        } else {
            Err(LaunchError::Invalid(
                "office.json holds an id that isn't plain",
            ))
        }
    }
}

/// `office.json` as stored, with its version.
#[derive(Deserialize)]
struct RawOfficeFile {
    v: u32,
    office_id: String,
    office_name: String,
    /// `None`: no key. `Some(Value::Null)`: `"role": null`.
    #[serde(default, deserialize_with = "present")]
    role: Option<serde_json::Value>,
    org_id: String,
    org_name: String,
    created_at: String,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

/// Decodes a present field as `Some`, even when its value is `null`.
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<serde_json::Value>, D::Error> {
    serde_json::Value::deserialize(d).map(Some)
}

/// `office.json` as written, borrowing from an [`OfficeFile`].
#[derive(Serialize)]
struct RawOfficeFileRef<'a> {
    v: u32,
    office_id: &'a str,
    office_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<serde_json::Value>,
    org_id: &'a str,
    org_name: &'a str,
    created_at: &'a str,
    #[serde(flatten)]
    extra: &'a serde_json::Map<String, serde_json::Value>,
}

impl TryFrom<RawOfficeFile> for OfficeFile {
    type Error = LaunchError;

    fn try_from(raw: RawOfficeFile) -> Result<Self, LaunchError> {
        check_version(raw.v)?;
        let role = raw
            .role
            .as_ref()
            .and_then(|value| OfficeRole::deserialize(value).ok())
            .unwrap_or(OfficeRole::Unknown);
        let file = OfficeFile {
            office_id: raw.office_id,
            office_name: raw.office_name,
            role,
            org_id: raw.org_id,
            org_name: raw.org_name,
            created_at: raw.created_at,
            extra: raw.extra,
            unknown_role: (role == OfficeRole::Unknown)
                .then(|| raw.role.map_or(StoredRole::Absent, StoredRole::Value)),
        };
        file.check_fields()?;
        Ok(file)
    }
}

impl Serialize for OfficeFile {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.check_fields().map_err(serde::ser::Error::custom)?;
        let role = match (self.role, &self.unknown_role) {
            (OfficeRole::Unknown, Some(StoredRole::Value(value))) => Some(value.clone()),
            (OfficeRole::Unknown, Some(StoredRole::Absent)) => None,
            (OfficeRole::Unknown, None) => {
                return Err(serde::ser::Error::custom("office.json needs a known role"))
            }
            (known, _) => Some(serde_json::to_value(known).map_err(serde::ser::Error::custom)?),
        };
        RawOfficeFileRef {
            v: LAUNCH_VERSION,
            office_id: &self.office_id,
            office_name: &self.office_name,
            role,
            org_id: &self.org_id,
            org_name: &self.org_name,
            created_at: &self.created_at,
            extra: &self.extra,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for OfficeFile {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        RawOfficeFile::deserialize(d)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

/// A line from the host to a server, on the server's stdin. Not `Clone`: it may hold the app
/// token.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[non_exhaustive]
pub enum HostMessage {
    /// Create this office (the first line with [`CREATE_FLAG`]).
    Create(CreateOffice),
    /// Serve the existing office (the first line with [`OFFICE_FLAG`]).
    Start(StartOffice),
    /// An op this build doesn't know. A server ignores it after the first line; as the first
    /// line it is [`FailCode::InvalidRequest`]. It is never written.
    #[serde(other, skip_serializing)]
    Unknown,
}

/// The details of a new office. The host has already checked the organization's name and its
/// uniqueness on this machine, and that the office id's socket handle is unique; the server
/// checks the name again with `org_proto`'s rule and refuses a folder that already exists.
///
/// Both ids must be plain, and this version creates only HQ offices ([`OfficeRole::Hq`]):
/// any other role is refused whenever a create is decoded or encoded. A satellite create comes
/// in a later version, with the fields it needs.
///
/// `org_name` and `office_name` travel in their [`org_proto::OrganizationName::as_str`] form
/// (already trimmed), because they go into `office.json` as they are. The server checks them
/// with that rule and fails with [`FailCode::InvalidName`] if they break it or aren't trimmed.
#[derive(Debug, PartialEq, Eq)]
pub struct CreateOffice {
    pub org_id: String,
    pub org_name: String,
    pub office_id: String,
    pub office_name: String,
    pub role: OfficeRole,
    /// The office's app token for this host run.
    pub app_token: AppToken,
}

/// What a server started with [`OFFICE_FLAG`] gets: the office's app token for this host run.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartOffice {
    pub app_token: AppToken,
}

impl CreateOffice {
    fn check(&self) -> Result<(), LaunchError> {
        if !is_plain_id(&self.org_id) || !is_plain_id(&self.office_id) {
            return Err(LaunchError::Invalid(
                "the create message holds an id that isn't plain",
            ));
        }
        if self.role != OfficeRole::Hq {
            return Err(LaunchError::Invalid("this version creates only HQ offices"));
        }
        Ok(())
    }
}

/// A create as it is read, without its `op` and `v`.
#[derive(Deserialize)]
struct RawCreateOffice {
    org_id: String,
    org_name: String,
    office_id: String,
    office_name: String,
    #[serde(default)]
    role: OfficeRole,
    app_token: AppToken,
}

/// A create as it is written, borrowing from a [`CreateOffice`].
#[derive(Serialize)]
struct RawCreateOfficeRef<'a> {
    org_id: &'a str,
    org_name: &'a str,
    office_id: &'a str,
    office_name: &'a str,
    role: OfficeRole,
    app_token: &'a AppToken,
}

impl Serialize for CreateOffice {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.check().map_err(serde::ser::Error::custom)?;
        RawCreateOfficeRef {
            org_id: &self.org_id,
            org_name: &self.org_name,
            office_id: &self.office_id,
            office_name: &self.office_name,
            role: self.role,
            app_token: &self.app_token,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for CreateOffice {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = RawCreateOffice::deserialize(d)?;
        let create = CreateOffice {
            org_id: raw.org_id,
            org_name: raw.org_name,
            office_id: raw.office_id,
            office_name: raw.office_name,
            role: raw.role,
            app_token: raw.app_token,
        };
        create.check().map_err(serde::de::Error::custom)?;
        Ok(create)
    }
}

/// A line from a server to the host, on the server's stdout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ServerEvent {
    /// `--create` only: the key, the databases and `office.json` are durable. Comes before
    /// [`ServerEvent::Ready`].
    Created { office: OfficeFile },
    /// The socket is bound and accepting: `hello` answers. `socket` is for logs and as a
    /// cross-check: the host reaches the office at the path it computes itself
    /// ([`org_proto::location::office_socket_path`]).
    Ready { socket: String, pid: u32 },
    /// `--discard` only: the unfinished office's key and folder are gone (or never existed).
    /// The server exits 0 right after.
    Discarded,
    /// The server can't go on; it exits non-zero right after. On a failed `--create` the
    /// server has already deleted the key it made.
    Failed { code: FailCode, message: String },
    /// An event this build doesn't know. The host ignores it; it is never written.
    #[serde(other, skip_serializing)]
    Unknown,
}

/// Why a server failed. A code this build doesn't know decodes as [`FailCode::StorageError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FailCode {
    /// Bad or missing arguments, or a first line that's malformed or doesn't match the flag.
    InvalidRequest,
    /// A `v` the server doesn't support.
    UnsupportedVersion,
    /// `--create` with a name that breaks the organization-name rule.
    InvalidName,
    /// `--create` for an office whose folder or `office.json` already exists, or `--discard`
    /// of an office that has `office.json` or a committed organization.
    AlreadyExists,
    /// The databases exist but their key doesn't (ADR-0002). Never retried automatically.
    /// `--discard` reports [`FailCode::StorageError`] instead.
    KeyMissing,
    /// The office lock wasn't free within [`OFFICE_LOCK_WAIT`].
    InUse,
    /// Anything else: I/O, the Keychain, the database; for `--discard`, databases it can't open
    /// or check. Also every code this build doesn't know.
    #[serde(other)]
    StorageError,
}

/// Reading or writing a launch line, or decoding `office.json`, failed.
#[derive(Debug)]
pub enum LaunchError {
    /// The pipe failed or a line was malformed (see [`LaunchError::closes_stream`]).
    Frame(FrameError),
    /// A `v` this build doesn't support.
    UnsupportedVersion(u32),
    /// Not valid JSON for the expected shape, or a type's own rule failed while decoding.
    Json(serde_json::Error),
    /// Well-formed, but breaks a rule of this contract.
    Invalid(&'static str),
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchError::Frame(e) => write!(f, "launch pipe: {e}"),
            LaunchError::UnsupportedVersion(v) => {
                write!(
                    f,
                    "launch version {v} isn't supported (this build speaks {LAUNCH_VERSION})"
                )
            }
            LaunchError::Json(e) => write!(f, "launch message: {e}"),
            LaunchError::Invalid(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for LaunchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LaunchError::Frame(e) => Some(e),
            LaunchError::Json(e) => Some(e),
            LaunchError::UnsupportedVersion(_) | LaunchError::Invalid(_) => None,
        }
    }
}

impl LaunchError {
    /// Whether the pipe is out of step after this error, so the reader must stop reading it.
    /// After the other errors the bad line was consumed whole.
    pub fn closes_stream(&self) -> bool {
        match self {
            LaunchError::Frame(e) => e.closes_stream(),
            LaunchError::UnsupportedVersion(_) | LaunchError::Json(_) | LaunchError::Invalid(_) => {
                false
            }
        }
    }

    /// Whether the host went away while a line was coming (the pipe ended mid-line): treat it
    /// like EOF, exit 0 without a `failed` line.
    pub fn is_host_gone(&self) -> bool {
        matches!(self, LaunchError::Frame(FrameError::Truncated))
    }

    /// The code a server reports when it can't use what the host sent: a broken pipe is
    /// [`FailCode::StorageError`] (retried), a bad line [`FailCode::InvalidRequest`] (not
    /// retried), another version [`FailCode::UnsupportedVersion`]. Check
    /// [`LaunchError::is_host_gone`] first: then there's nobody to report to.
    pub fn fail_code(&self) -> FailCode {
        match self {
            LaunchError::UnsupportedVersion(_) => FailCode::UnsupportedVersion,
            LaunchError::Frame(FrameError::Io(_) | FrameError::Truncated) => FailCode::StorageError,
            LaunchError::Frame(_) | LaunchError::Json(_) | LaunchError::Invalid(_) => {
                FailCode::InvalidRequest
            }
        }
    }
}

/// Read one host message. `Ok(None)` means the host closed the pipe. The line it was read from
/// is zeroed afterwards, since it may hold the app token.
pub fn read_host_message(r: &mut impl BufRead) -> Result<Option<HostMessage>, LaunchError> {
    match read_secret_line(r)? {
        Some(mut line) => decode_host_line(&mut line).map(Some),
        None => Ok(None),
    }
}

/// One line, without its `\n`, read into a buffer reserved up front for the largest line, so it
/// never reallocates and leaves a copy of the app token behind. It is zeroed on every error.
/// The same rules as [`frame::read_line`].
fn read_secret_line(r: &mut impl BufRead) -> Result<Option<Vec<u8>>, LaunchError> {
    let mut buf = Vec::with_capacity(org_proto::MAX_FRAME_BYTES + 1);
    Ok(read_secret_line_into(r, &mut buf)?.then_some(buf))
}

/// [`read_secret_line`] into a given buffer: `Ok(true)` with the line in it, `Ok(false)` at a clean
/// end, or an error with the buffer zeroed.
fn read_secret_line_into(r: &mut impl BufRead, buf: &mut Vec<u8>) -> Result<bool, LaunchError> {
    let limit = org_proto::MAX_FRAME_BYTES + 1; // room for the newline
    buf.clear();
    let read = r.by_ref().take(limit as u64).read_until(b'\n', buf);
    let outcome = match read {
        Err(e) => Err(FrameError::Io(e)),
        Ok(_) if buf.last() == Some(&b'\n') => {
            buf.pop();
            return Ok(true);
        }
        Ok(_) if buf.len() >= limit => Err(FrameError::TooLarge),
        Ok(_) if buf.is_empty() => return Ok(false),
        Ok(_) => Err(FrameError::Truncated),
    };
    zero(buf);
    outcome.map_err(LaunchError::Frame)
}

/// Decode a host line, then zero it, on every path.
fn decode_host_line(line: &mut [u8]) -> Result<HostMessage, LaunchError> {
    let message = versioned(line)
        .and_then(|()| serde_json::from_slice::<HostMessage>(line).map_err(LaunchError::Json));
    zero(line);
    message
}

/// Write one host message, then flush. [`HostMessage::Unknown`] and a create that breaks
/// [`CreateOffice`]'s rules are refused, and nothing is written.
pub fn write_host_message(w: &mut impl Write, message: &HostMessage) -> Result<(), LaunchError> {
    match message {
        HostMessage::Create(create) => create.check()?,
        HostMessage::Start(_) => {}
        HostMessage::Unknown => return Err(LaunchError::Invalid("can't send an unknown op")),
    }
    let mut line = encode_host_line(message)?;
    send_and_zero(w, &mut line)
}

/// One write of the whole line and a flush, then the buffer is zeroed (it holds the app token),
/// whether the write worked or not.
fn send_and_zero(w: &mut impl Write, line: &mut [u8]) -> Result<(), LaunchError> {
    let result = w
        .write_all(line)
        .and_then(|()| w.flush())
        .map_err(|e| LaunchError::Frame(FrameError::Io(e)));
    zero(line);
    result
}

/// Encode a host message as one line with its `\n`, into a buffer of exactly that size
/// (measured first), so it never reallocates and leaves a copy of the app token behind. The
/// caller zeroes it after use.
fn encode_host_line(message: &HostMessage) -> Result<Vec<u8>, LaunchError> {
    let body = Line {
        v: LAUNCH_VERSION,
        body: message,
    };
    let mut counter = ByteCounter(0);
    serde_json::to_writer(&mut counter, &body).map_err(LaunchError::Json)?;
    if counter.0 > org_proto::MAX_FRAME_BYTES {
        return Err(LaunchError::Frame(FrameError::TooLargeToSend));
    }
    let mut line = Vec::with_capacity(counter.0 + 1);
    if let Err(e) = serde_json::to_writer(&mut line, &body) {
        zero(&mut line);
        return Err(LaunchError::Json(e));
    }
    line.push(b'\n');
    Ok(line)
}

/// Counts the bytes written to it and keeps none.
struct ByteCounter(usize);

impl Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Overwrite with zeros in a way the optimizer can't remove (best effort, as for
/// [`AppToken`]).
#[allow(unsafe_code)]
fn zero(bytes: &mut [u8]) {
    for b in bytes.iter_mut() {
        // SAFETY: `b` is a valid, aligned, exclusive reference to a u8.
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

/// Read one server event. `Ok(None)` means the server closed its stdout (it exited).
pub fn read_server_event(r: &mut impl BufRead) -> Result<Option<ServerEvent>, LaunchError> {
    let Some(line) = read_versioned_line(r)? else {
        return Ok(None);
    };
    let event: ServerEvent = serde_json::from_slice(&line).map_err(LaunchError::Json)?;
    if let ServerEvent::Created { office } = &event {
        // The same limit as office.json itself, so the host never accepts an office the server
        // couldn't have stored.
        office.to_vec()?;
    }
    Ok(Some(event))
}

/// Write one server event, then flush. [`ServerEvent::Unknown`] and a created office that
/// [`OfficeFile::to_vec`] would refuse are refused, and nothing is written. Keep a `failed` message
/// short: a line over [`org_proto::MAX_FRAME_BYTES`] isn't sent ([`FrameError::TooLargeToSend`]).
pub fn write_server_event(w: &mut impl Write, event: &ServerEvent) -> Result<(), LaunchError> {
    match event {
        // Everything `to_vec` checks, the size included.
        ServerEvent::Created { office } => drop(office.to_vec()?),
        ServerEvent::Unknown => return Err(LaunchError::Invalid("can't send an unknown event")),
        ServerEvent::Ready { .. } | ServerEvent::Discarded | ServerEvent::Failed { .. } => {}
    }
    write_versioned(w, event)
}

#[derive(Deserialize)]
struct Versioned {
    v: u32,
}

#[derive(Serialize)]
struct Line<'a, T> {
    v: u32,
    #[serde(flatten)]
    body: &'a T,
}

fn check_version(v: u32) -> Result<(), LaunchError> {
    if v == LAUNCH_VERSION {
        Ok(())
    } else {
        Err(LaunchError::UnsupportedVersion(v))
    }
}

/// One line whose `v` this build supports, checked before its shape, so a newer peer gets
/// `UnsupportedVersion` rather than a decoding error.
fn read_versioned_line(r: &mut impl BufRead) -> Result<Option<Vec<u8>>, LaunchError> {
    let Some(line) = frame::read_line(r).map_err(LaunchError::Frame)? else {
        return Ok(None);
    };
    versioned(&line)?;
    Ok(Some(line))
}

/// Whether a line's `v` is one this build supports.
fn versioned(line: &[u8]) -> Result<(), LaunchError> {
    let version: Versioned = serde_json::from_slice(line).map_err(LaunchError::Json)?;
    check_version(version.v)
}

fn write_versioned<T: Serialize>(w: &mut impl Write, body: &T) -> Result<(), LaunchError> {
    let line = Line {
        v: LAUNCH_VERSION,
        body,
    };
    frame::write_message(w, &line).map_err(LaunchError::Frame)
}

#[cfg(test)]
mod tests;
