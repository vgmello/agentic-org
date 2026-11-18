//! # org-office-launch: how the host starts an office server
//!
//! The private contract between `org-host` (the login service) and `agentic-org-office` (one
//! process per office). The app never uses it. `agent-os` uses only [`OfficeFile`], to read
//! `office.json` (never by hand: the type checks the version, the ids, the names and `created_at`).
//! The app and `agent-os` speak `org-proto` to the servers. See
//! `docs/design/offices-hq-and-satellites.md` ("Processes on one Mac") and the host/server boundary
//! design (`docs/design/office-host-boundary.md`).
//!
//! **A contract (owner: ada): additive and versioned.** Every line and `office.json` carries `"v"`.
//! Readers ignore unknown fields and unknown server events ([`ServerEvent::Unknown`]), and decode
//! unknown failure codes as [`FailCode::StorageError`]. A server parses only its first stdin line
//! ([`v3::read_first_line`]). A field a create can't work without (e.g. a satellite's HQ
//! address) arrives with a role or op that older servers refuse, never as a field they would
//! silently ignore. `v` is reserved in every line: no op or event has a field named `v`.
//!
//! The types check their own rules (plain ids, a role this version can create) whenever they
//! are decoded or encoded, by any serde path. The `"v"` of a launch line is checked by
//! [`v3::read_first_line`] and [`v3::read_server_event`], so read and write lines only through
//! the [`v3`] functions. Launch version 3 ([`v3::LAUNCH_VERSION`]) is the only one: version 2
//! (secrets and the app token on stdin, events on stdout) was removed in A1.5.
//!
//! ## Starting a server
//! ```text
//! agentic-org-office --create            # create a new office from the create message, then serve it
//! agentic-org-office --office <id>       # serve an existing office
//! agentic-org-office --discard <id>      # remove an unfinished office (no office.json), then exit
//! ```
//! - The host makes `<home>` and `<home>/offices/` (owner-only) and sets
//!   `ORG_OFFICE_HOME` ([`org_proto::location::OFFICE_HOME_ENV`]) on the child, so both use the
//!   same office home. It also supplies an absolute `ORG_OFFICE_RUNTIME_DIR`
//!   ([`org_proto::location::OFFICE_RUNTIME_DIR_ENV`]) for the private socket directory,
//!   independently of the data root. The legacy host supplies `<home>/run/` explicitly.
//!   Release workers require this value. Only debug workers fall back to `<home>/run/`
//!   when it is absent; an empty or relative override is refused without fallback.
//! - **stdin carries only the first line from the host.** It matches the flag ([`FirstLine`]):
//!   a [`v3::HostFirstLine::Create`] with `--create`, a [`v3::HostFirstLine::Start`] with
//!   `--office`, a [`v3::HostFirstLine::Discard`] with `--discard`. It is non-secret and names
//!   the per-spawn rendezvous socket (see [`v3`]). A server whose first line is malformed, of
//!   another version (version 2 included), an op it doesn't know, or for another flag exits
//!   non-zero before it dials the rendezvous, logging to stderr only. EOF on stdin before the
//!   first line, or in the middle of it ([`LaunchError::is_host_gone`]), means the host is gone:
//!   the server exits 0. After the first line, a server only drains stdin and discards what it
//!   reads without parsing it. Its lifetime then follows the rendezvous connection: EOF there
//!   means the host is gone.
//! - **Versions.** A launch line of a version the reader doesn't take is
//!   [`LaunchError::UnsupportedVersion`], reported as [`FailCode::UnsupportedVersion`] where
//!   there is a channel to report it on. `office.json` has its own version
//!   ([`OFFICE_FILE_VERSION`], [`SELECTED_OFFICE_FILE_VERSION`]).
//! - **Descriptors:** no child of a server can read the host's lines or write a control line.
//!   First thing at startup, before any thread or child starts, the server moves its stdin and
//!   stdout pipes to new close-on-exec descriptors and reopens 0 and 1 on `/dev/null`. Every
//!   other descriptor it opens is close-on-exec too, the `office.lock` one above all (a child that
//!   inherited it would keep the lock after the server died). A child's stdin and stdout are
//!   `/dev/null` unless it's given its own pipes; stderr (the logs) is inherited. The host opens
//!   its own descriptors close-on-exec, so no child holds `host.lock`.
//! - **No app token** travels at launch: a worker's persons act only through owner grants.
//! - **The database key** ([`OfficeSecretsKey`], 32 bytes) and the grant key travel only in the rendezvous
//!   handoff ([`v3::Handoff`]), never on stdin. The host creates each office's key and stores it
//!   in its own key store before the office's first `--create`, and sends it on every create,
//!   start and discard handoff. The server never creates, stores, loads or deletes a key, and
//!   never touches the Keychain: it uses only the key its handoff carries. After it first opens
//!   the databases it wipes every copy it holds, except the one its store keeps for its reader
//!   pool: in memory only, zeroed on drop, never logged. Wiping is best effort: the key's bytes
//!   live on the heap, so moving it leaves no copy, but buffers it passed through can. A key that
//!   doesn't open an office's existing databases is [`FailCode::KeyMissing`], and the server
//!   deletes nothing on it.
//! - **Zeroing is best effort.** [`v3::read_first_line`] and the handoff readers zero their own
//!   buffers and never reallocate them with a secret inside. Copies outside this crate remain:
//!   the caller's reader and writer buffers (e.g. `Stdin`'s, which may keep the line for the
//!   process's life; read stdin unbuffered or through a small buffer you control), and serde's
//!   buffers when a secret arrives with JSON escapes. See `org_proto`'s `OfficeSecretsKey` docs.
//! - **stdout carries nothing.** Worker events ([`ServerEvent`]) travel as v3 lines over the
//!   rendezvous connection; the host treats stdout as untrusted. Logs go to stderr.
//! - Lines are JSON, one object per line, at most [`org_proto::MAX_FRAME_BYTES`]
//!   ([`org_proto::frame`]). Use [`v3::write_first_line`], [`v3::read_first_line`],
//!   [`v3::write_server_event`] and [`v3::read_server_event`].
//! - The server listens on [`org_proto::location::office_socket_in`] using the selected
//!   runtime directory; the host computes the same path and only cross-checks
//!   [`ServerEvent::Ready`]'s `socket`. [`org_proto::location::office_socket_path`] remains
//!   the helper for the legacy `<home>/run/` layout.
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
//! The host runs `--discard <id>` for every plain-id office folder without `office.json` (a
//! folder with an unreadable or invalid `office.json` is logged and left alone; once
//! `office.json` exists the host never discards the office: it adopts or quarantines it). Its
//! first line is a [`v3::HostFirstLine::Discard`]; its handoff carries the office's key, or
//! `"db_key": null` when the host has none for that id.
//! An office with no folder: the server takes no lock, creates nothing and reports `discarded`.
//! The server takes the office lock (same wait) and removes only an office that never finished.
//! It refuses ([`FailCode::AlreadyExists`]) a folder that holds `office.json`. With a key, it
//! deletes only when no database file exists, or when it can open the databases with that key
//! and they hold no committed organization. With `db_key: null`, it deletes only a folder with
//! no database file. A committed organization is [`FailCode::AlreadyExists`]; anything else
//! (databases it can't open or check, or any database file when it has no key) is
//! [`FailCode::StorageError`]. Either way the host logs it and leaves the folder for a person,
//! so a real office that lost its `office.json` is never destroyed automatically. Otherwise it
//! removes the folder, reports [`ServerEvent::Discarded`] and exits 0.
//! The server no longer removes keys: after `discarded`, the host deletes its own key item for
//! that office. That is the only time the host deletes a key, a failed create included.
//!
//! **A refused create.** If a server reports a `created` office that isn't the one the host asked
//! for (a different office id, `org_id`, `org_name`, `office_name` or role), the host doesn't
//! adopt it. It stops the server
//! and moves the folder of the office id **it asked for** to `offices/.refused/<id>` (out of
//! the catalog: `.refused` isn't a plain id), keeping its data and its key item in the host's
//! store, for a person to look at. It never touches a folder named by a different id the server
//! reported; it only logs that id. Nothing is deleted. The host treats a folder whose
//! `office.json` names another id the same way at startup: it moves it to
//! `offices/.refused/<folder name>` and never starts it.
//! [`refused_dir`] and [`OfficeFile::names_folder`] give both sides one rule.

#![deny(unsafe_code)]

use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use org_proto::frame::FrameError;
use org_proto::OfficeRole;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An office's database key, re-exported from `org_proto` so the host and the server name it
/// through this contract. It travels only in the rendezvous handoff ([`v3::Handoff`]); see the
/// crate docs.
pub use org_proto::OfficeSecretsKey;
pub use org_proto::OFFICE_SECRETS_KEY_BYTES;

/// The version of `office.json`, separate from the launch lines' [`v3::LAUNCH_VERSION`]. Bumped only
/// for a breaking change to the file.
pub const OFFICE_FILE_VERSION: u32 = 1;
/// Additive selected carrier, with UUIDv7 entity IDs; legacy v1 is unchanged.
pub const SELECTED_OFFICE_FILE_VERSION: u32 = 2;

/// The command-line flag that creates a new office from the create message on stdin.
pub const CREATE_FLAG: &str = "--create";
/// The command-line flag, followed by the office id, that serves an existing office.
pub const OFFICE_FLAG: &str = "--office";
/// The command-line flag, followed by the office id, that removes an unfinished office.
pub const DISCARD_FLAG: &str = "--discard";

/// The folder under the office home that holds one folder per office.
pub const OFFICES_DIR_NAME: &str = "offices";
/// The folder inside [`OFFICES_DIR_NAME`] where refused and misplaced offices are moved, out of
/// the catalog (it isn't a plain id).
pub const REFUSED_DIR_NAME: &str = ".refused";

/// Where the host moves the folder of office `office_id` when it refuses it:
/// `<home>/offices/.refused/<office_id>`, or `None` if the id isn't plain. A name already taken
/// there is the host's to resolve (e.g. a suffix); the base path is this.
pub fn refused_dir(office_home: &Path, office_id: &str) -> Option<PathBuf> {
    is_storage_id(office_id).then(|| {
        office_home
            .join(OFFICES_DIR_NAME)
            .join(REFUSED_DIR_NAME)
            .join(office_id)
    })
}
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
    is_storage_id(office_id).then(|| office_home.join(OFFICES_DIR_NAME).join(office_id))
}

/// The longest plain id, the same cap as [`org_proto::location`].
pub const MAX_ID_CHARS: usize = 64;

/// Whether `id` is a plain id: 1 to [`MAX_ID_CHARS`] ASCII letters and digits.
pub fn is_plain_id(id: &str) -> bool {
    (1..=MAX_ID_CHARS).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Canonical lowercase UUIDv7 text. This validates syntax/version/variant, not
/// existence or authority. It never reformats an incoming identity.
pub fn is_selected_entity_id(id: &str) -> bool {
    org_proto::is_uuid_v7(id)
}
/// Path-safe runtime storage names only. Protocol callers must select their
/// versioned identity validator separately.
pub fn is_storage_id(id: &str) -> bool {
    is_plain_id(id) || is_selected_entity_id(id)
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
/// Its `"v"` is always [`OFFICE_FILE_VERSION`], written by this crate, never chosen by callers.
/// Every decode, including the `office` inside [`ServerEvent::Created`], checks the version and
/// that both ids are plain ([`is_plain_id`]); every encode checks the ids. Fields this build
/// doesn't know are kept and written back, so an older server that rewrites the file (e.g. after a
/// rename) never drops what a newer one added. A missing or unknown role decodes as
/// [`OfficeRole::Unknown`]; while it stays `Unknown`, the original value (or its absence) is
/// written back as it was. A file with no known role that wasn't read from disk can't be
/// written ([`LaunchError::Invalid`]): every office this build writes has a role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficeFile {
    version: u32,
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
            version: OFFICE_FILE_VERSION,
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

    /// Construct a new selected marker without changing legacy constructor rules.
    pub fn selected(
        office_id: impl Into<String>,
        office_name: impl Into<String>,
        role: OfficeRole,
        org_id: impl Into<String>,
        org_name: impl Into<String>,
        created_at: impl Into<String>,
    ) -> Result<Self, LaunchError> {
        let mut file = Self::new(office_id, office_name, role, org_id, org_name, created_at);
        file.version = SELECTED_OFFICE_FILE_VERSION;
        file.check_writable()?;
        Ok(file)
    }
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Decode `office.json`. Unknown fields are kept; another `v` is
    /// [`LaunchError::UnsupportedVersion`]; ids that aren't plain, names that break
    /// [`org_proto::OrganizationName`]'s rule (or aren't trimmed), a `created_at` that isn't
    /// `YYYY-MM-DDTHH:MM:SS.mmmZ`, or a file over [`MAX_OFFICE_FILE_BYTES`] are
    /// [`LaunchError::Invalid`]. A role this build doesn't know is kept as
    /// [`OfficeRole::Unknown`] and written back as it was.
    ///
    /// **The reader's duties** (the host and `agent-os`, which read the file; this crate does
    /// no I/O): check that `office_id` names the folder the file was read from, and read the file
    /// without following symlinks, only if it's a regular file owned by this user, capped at
    /// [`MAX_OFFICE_FILE_BYTES`]. The server writes it owner-only (0600), to a temp file that it
    /// fsyncs and renames into place, then fsyncs the folder.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, LaunchError> {
        if bytes.len() > MAX_OFFICE_FILE_BYTES {
            return Err(LaunchError::Invalid("office.json is too large"));
        }
        let version: Versioned = serde_json::from_slice(bytes).map_err(LaunchError::Json)?;
        check_office_file_version(version.v)?;
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

    /// Whether this file belongs in the folder named `folder_name`: its `office_id` must equal
    /// the folder's name exactly. A file that doesn't is a misplaced office (see the crate docs).
    pub fn names_folder(&self, folder_name: &str) -> bool {
        self.office_id == folder_name
    }

    fn check_ids(&self) -> Result<(), LaunchError> {
        let valid = match self.version {
            OFFICE_FILE_VERSION => is_plain_id(&self.office_id) && is_plain_id(&self.org_id),
            SELECTED_OFFICE_FILE_VERSION => {
                is_selected_entity_id(&self.office_id) && is_selected_entity_id(&self.org_id)
            }
            _ => false,
        };
        if valid {
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
        check_office_file_version(raw.v)?;
        let role = raw
            .role
            .as_ref()
            .and_then(|value| OfficeRole::deserialize(value).ok())
            .unwrap_or(OfficeRole::Unknown);
        let file = OfficeFile {
            version: raw.v,
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
                return Err(serde::ser::Error::custom("office.json needs a known role"));
            }
            (known, _) => Some(serde_json::to_value(known).map_err(serde::ser::Error::custom)?),
        };
        RawOfficeFileRef {
            v: self.version,
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

/// A worker event to the host. Workers send it as a v3 line over the rendezvous connection
/// ([`v3::write_server_event`]), never on stdout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ServerEvent {
    /// `--create` only: the databases and `office.json` are durable. Comes before
    /// [`ServerEvent::Ready`].
    Created { office: OfficeFile },
    /// The socket is bound and accepting: `hello` answers. `socket` is for logs and as a
    /// cross-check: the host reaches the office at the path it computes itself
    /// ([`org_proto::location::office_socket_path`]).
    Ready { socket: String, pid: u32 },
    /// `--discard` only: the unfinished office's folder is gone (or never existed). The server
    /// exits 0 right after; the host then deletes its own key item for the office.
    Discarded,
    /// The server can't go on; it exits non-zero right after. The server never deletes a key.
    /// After a failed `--create` the host keeps the key it made: if `office.json` is in place
    /// the office is stored and the host adopts it; if not, it discards the unfinished office
    /// with that key and deletes the key only after [`ServerEvent::Discarded`], since a database
    /// left behind without its key could never be checked or discarded. `message` is untrusted
    /// text: the host escapes control characters and caps its length before logging it.
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
    /// `--office`: the key the host sent doesn't open the office's existing databases. The
    /// server deletes nothing on it, and it is never retried automatically. The host also shows
    /// an office as `key_missing` when its own key item is gone, without starting a server.
    /// `--discard` reports [`FailCode::StorageError`] instead.
    KeyMissing,
    /// The office lock wasn't free within [`OFFICE_LOCK_WAIT`].
    InUse,
    /// Anything else: I/O, the database; for `--discard`, databases it can't open or check, and
    /// any database file when the host sent no key. Also every code this build doesn't know.
    #[serde(other)]
    StorageError,
}

/// Reading or writing a launch line, or decoding `office.json`, failed.
#[derive(Debug)]
pub enum LaunchError {
    /// The pipe failed or a line was malformed (see [`LaunchError::closes_stream`]).
    Frame(FrameError),
    /// A pipe or rendezvous line whose `v` isn't one the reader expects: `found` is the line's,
    /// `supported` the versions that reader takes (`[3]`).
    UnsupportedVersion {
        found: u32,
        supported: &'static [u32],
    },
    /// An `office.json` whose `v` this build doesn't support (other than
    /// [`OFFICE_FILE_VERSION`]).
    UnsupportedOfficeFileVersion(u32),
    /// Not valid JSON for the expected shape, or a type's own rule failed while decoding.
    Json(serde_json::Error),
    /// Well-formed, but breaks a rule of this contract.
    Invalid(&'static str),
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchError::Frame(e) => write!(f, "launch pipe: {e}"),
            LaunchError::UnsupportedVersion { found, supported } => {
                write!(
                    f,
                    "launch version {found} isn't supported (this build speaks "
                )?;
                for (i, v) in supported.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" or ")?;
                    }
                    write!(f, "{v}")?;
                }
                f.write_str(")")
            }
            LaunchError::UnsupportedOfficeFileVersion(v) => write!(
                f,
                "office.json version {v} isn't supported (this build reads \
                 {OFFICE_FILE_VERSION} or {SELECTED_OFFICE_FILE_VERSION})"
            ),
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
            LaunchError::UnsupportedVersion { .. }
            | LaunchError::UnsupportedOfficeFileVersion(_)
            | LaunchError::Invalid(_) => None,
        }
    }
}

impl LaunchError {
    /// Whether the pipe is out of step after this error, so the reader must stop reading it.
    /// After the other errors the bad line was consumed whole.
    pub fn closes_stream(&self) -> bool {
        match self {
            LaunchError::Frame(e) => e.closes_stream(),
            LaunchError::UnsupportedVersion { .. }
            | LaunchError::UnsupportedOfficeFileVersion(_)
            | LaunchError::Json(_)
            | LaunchError::Invalid(_) => false,
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
            LaunchError::UnsupportedVersion { .. }
            | LaunchError::UnsupportedOfficeFileVersion(_) => FailCode::UnsupportedVersion,
            LaunchError::Frame(FrameError::Io(_) | FrameError::Truncated) => FailCode::StorageError,
            LaunchError::Frame(_) | LaunchError::Json(_) | LaunchError::Invalid(_) => {
                FailCode::InvalidRequest
            }
        }
    }
}

/// Which first line a server expects, from the flag it was started with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstLine {
    /// [`CREATE_FLAG`]: a [`v3::HostFirstLine::Create`].
    Create,
    /// [`OFFICE_FLAG`]: a [`v3::HostFirstLine::Start`].
    Start,
    /// [`DISCARD_FLAG`]: a [`v3::HostFirstLine::Discard`].
    Discard,
}

/// One line, without its `\n`, read into a buffer reserved up front for the largest line, so it
/// never reallocates and leaves a copy of a secret behind. It is zeroed on every error.
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
/// [`org_proto::AppToken`]).
#[allow(unsafe_code)]
fn zero(bytes: &mut [u8]) {
    for b in bytes.iter_mut() {
        // SAFETY: `b` is a valid, aligned, exclusive reference to a u8.
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

/// The `"v"` of `office.json`, read before the rest so a newer file reports its version.
#[derive(Deserialize)]
struct Versioned {
    v: u32,
}

fn check_office_file_version(v: u32) -> Result<(), LaunchError> {
    if matches!(v, OFFICE_FILE_VERSION | SELECTED_OFFICE_FILE_VERSION) {
        Ok(())
    } else {
        Err(LaunchError::UnsupportedOfficeFileVersion(v))
    }
}

pub mod recovery;
pub mod v3;
pub mod writer;

#[cfg(test)]
mod tests;
