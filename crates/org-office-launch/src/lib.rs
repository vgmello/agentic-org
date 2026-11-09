//! # org-office-launch: how the host starts an office server
//!
//! The private contract between `org-office-host` (the login service) and `org-office-server` (one
//! process per office). The app never uses it. `org-agent` uses only [`OfficeFile`], to read
//! `office.json` (never by hand: the type checks the version, the ids, the names and `created_at`).
//! The app and `org-agent` speak `org-proto` to the servers. See
//! `docs/design/offices-hq-and-satellites.md` ("Processes on one Mac") and the host/server boundary
//! design (`docs/design/office-host-boundary.md`).
//!
//! **A contract (owner: ada): additive and versioned.** Every line and `office.json` carries `"v"`.
//! Readers ignore unknown fields and unknown server events ([`ServerEvent::Unknown`]), and decode
//! unknown failure codes as [`FailCode::StorageError`]. Two exceptions guard the secrets: a first
//! line that carries `app_token` or `db_key` is refused, and the hand-off line ([`HandOff`])
//! refuses every field it doesn't know. A server parses only its first stdin line
//! ([`read_first_host_message`]). A field a create can't work without (e.g. a satellite's HQ
//! address) arrives with a role or op that older servers refuse, never as a field they would
//! silently ignore. `v` is reserved in every line: no op or event has a field named `v`.
//!
//! The types check their own rules (plain ids, a role this version can create, a well-formed
//! [`KeySocketId`]) whenever they are decoded or encoded, by any serde path. The `"v"` of a line
//! is checked by [`read_host_message`], [`read_hand_off`] and [`read_server_event`], so read and
//! write lines only through them.
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
//! - **stdin is the control pipe from the host.** Its first line matches the flag
//!   ([`FirstLine`]): a [`HostMessage::Create`] with `--create`, a [`HostMessage::Start`] with
//!   `--office`, a [`HostMessage::Discard`] with `--discard`. A server whose first line is
//!   malformed, an op it doesn't know, or for another flag fails with
//!   [`FailCode::InvalidRequest`]. EOF on stdin means the host is gone: the server shuts down
//!   cleanly and exits 0, also when the EOF comes before the first line (then without a
//!   `failed` line: nobody is left to read it), and when the pipe ends in the middle of the first
//!   line ([`LaunchError::is_host_gone`]). After the first line, a server only drains stdin
//!   and discards what it reads, known ops included, without parsing it, so nothing on a later
//!   line can stop it from seeing the EOF.
//! - **No secret travels on stdin, or on any descriptor a server inherits.** A first line carries
//!   only non-secrets and a [`KeySocketId`]; the app token and the database key come on the key
//!   hand-off (below). A first line that carries `app_token` or `db_key` is refused
//!   ([`FailCode::InvalidRequest`]).
//! - **Version 3** moved the secrets off stdin to the key hand-off. A line of another version,
//!   version 2 included, is [`LaunchError::UnsupportedVersion`] ("launch version 2 isn't
//!   supported"), reported as [`FailCode::UnsupportedVersion`]. `office.json` has its own version,
//!   [`OFFICE_FILE_VERSION`], still 1: its shape didn't change, so existing files still read.
//!   There is no migration: version 3 changes only the launch lines (`office.json` and the
//!   host's key store are unchanged), only development builds ever spoke version 2, and the host
//!   and the server ship together.
//! - **Descriptors:** no child of a server can read the host's lines or write a control line.
//!   First thing at startup, before any thread or child starts, the server moves its stdin and
//!   stdout pipes to new close-on-exec descriptors and reopens 0 and 1 on `/dev/null`. Every
//!   other descriptor it opens is close-on-exec too: the `office.lock` one above all (a child that
//!   inherited it would keep the lock after the server died), and the key hand-off connection,
//!   which it drops right after reading the hand-off line. A child's stdin and stdout are
//!   `/dev/null` unless it's given its own pipes; stderr (the logs) is inherited. The host opens
//!   its own descriptors close-on-exec, so no child holds `host.lock` or a key socket.
//! - **The app token** ([`AppToken`], ADR-0003 §2.1) travels only on the hand-off line, on every
//!   create and start (and discard, which doesn't use it). The host makes a new one for each
//!   office each time the host starts and keeps it only in memory. The server keeps it only in
//!   memory and never logs it.
//! - **The database key** ([`DbKey`], 32 bytes) also travels only on the hand-off line. The host
//!   creates each office's key and stores it in its own key store before the office's first
//!   `--create`, and sends it on every create, start and discard hand-off. The server never
//!   creates, stores, loads or deletes a key, and never touches the Keychain: it uses only the key
//!   on its hand-off line. After it first opens the databases it wipes every copy it holds,
//!   except the one its store keeps for its reader pool: in memory only, zeroed on drop, never
//!   logged. Wiping is best effort, as for the app token (see `org_proto`'s token docs): the key's
//!   bytes live on the heap, so moving it leaves no copy, but buffers it passed through can. A key
//!   that doesn't open an office's existing databases is [`FailCode::KeyMissing`], and the server
//!   deletes nothing on it.
//! - **Zeroing is best effort.** [`read_hand_off`] and [`write_hand_off`] (and the first-line
//!   reader and writer, which hold no secret now) zero their own buffers and never reallocate them
//!   with a secret inside. Copies outside this crate remain: the caller's reader and writer
//!   buffers (read the hand-off connection unbuffered or through a small buffer you control, and
//!   drop it right after), and serde's buffers when a secret arrives with JSON escapes (on a
//!   first line, serde may buffer its fields before it refuses a secret there). See
//!   `org_proto`'s `AppToken` and `DbKey` docs.
//! - **stdout carries only [`ServerEvent`] lines.** Logs go to stderr.
//! - Lines are JSON, one object per line, at most [`org_proto::MAX_FRAME_BYTES`]
//!   ([`org_proto::frame`]). Use [`read_first_host_message`], [`write_host_message`],
//!   [`read_hand_off`], [`write_hand_off`], [`read_server_event`] and [`write_server_event`].
//! - The server listens on [`org_proto::location::office_socket_path`]; the host computes the
//!   same path and only cross-checks [`ServerEvent::Ready`]'s `socket`.
//!
//! ## The key hand-off
//! The secrets reach a server only over a connection the server opens itself, after its exec, to
//! a one-shot socket the host listens on, and only once the host has checked that the connecting
//! process is the one it admitted. A process that inherited the server's stdin (e.g. a helper
//! forked before the exec) can read only the first line, which holds no secret, and its own
//! connection carries the wrong audit token.
//!
//! 1. **Before the spawn**, the host makes a fresh [`KeySocketId`] from 8 random bytes
//!    ([`KeySocketId::generate_from`]) and binds a listening Unix socket at [`key_socket_path`],
//!    `<home>/run/k-<16 hex>.sock` ([`KEY_SOCKET_PREFIX`], [`KEY_SOCKET_SUFFIX`]), mode 0600, in
//!    the 0700 `run/`. One socket per spawn; the path must pass
//!    [`org_proto::location::check_socket_path`]. Its name is the longest in `run/` (23 bytes,
//!    against an office socket's 17), so the host's up-front path check at startup includes a key
//!    socket's path (e.g. for `KeySocketId::generate_from([0; 8])`). The host also removes
//!    leftover `k-*.sock` files from an earlier run at startup, and treats a name already taken
//!    at bind as a reason to make a fresh id. (Office sockets are named by plain ids, which have
//!    no `-`, so the names never collide.)
//! 2. The host spawns the server suspended, takes and checks its audit token (admits it), resumes
//!    it, and writes the first line, which carries the id ([`HostMessage`]).
//! 3. **The server**, right after its first line and before anything else (before the office lock
//!    or any database, for create, start and discard alike), builds the path itself with
//!    [`key_socket_path`] from its own office home and the id: it never takes a path from the
//!    host. It `lstat`s the path, which must be a socket, owned by the server's uid, mode 0600,
//!    and not a symlink; anything else is [`FailCode::InvalidRequest`] with fixed text, and
//!    nothing is read. It connects with a 5 s deadline for the connect and the read.
//! 4. **The host** accepts exactly one connection, within 10 s. It samples the peer's audit token
//!    at accept (`LOCAL_PEERTOKEN`); the connection is the admitted server's only if that token
//!    equals the admitted one in full, pid version included, and the peer has sent nothing. Then
//!    the host writes the hand-off line ([`write_hand_off`]), unlinks the socket and closes the
//!    connection, in that order. A second connection is refused and logged. On a timeout or a
//!    refused peer, the host kills the child, unlinks the socket, writes nothing, and reports
//!    `storage_error`. Debug builds check that `LOCAL_PEERPID` is the child's pid instead of the
//!    audit token; the exchange is otherwise the same.
//! 5. **The server** reads the one hand-off line ([`read_hand_off`]), which zeroes it, and drops
//!    the connection (close-on-exec): the exchange is one way, with no ack and no reconnect. EOF
//!    before the whole line means the host is gone ([`LaunchError::is_host_gone`]): it exits 0
//!    without a `failed` line. Every other failure (the deadline, a malformed line, a missing
//!    key) is a `failed` line with fixed text (never the error's text) and
//!    [`LaunchError::fail_code`]'s code.
//!
//! No build, debug builds included, reads a secret from stdin: the hand-off is the only way in.
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
//! first line is a [`HostMessage::Discard`], and its hand-off line carries the office's key, or
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

use org_proto::frame::{self, FrameError};
use org_proto::{AppToken, OfficeRole};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An office's database key, re-exported from `org_proto` so the host and the server name it
/// through this contract. See [`HandOff::db_key`] and the crate docs.
pub use org_proto::DbKey;
pub use org_proto::DB_KEY_BYTES;

/// The version of the launch lines (host messages, the hand-off line and server events). Bumped
/// only for a breaking change. Version 2 added the database key; version 3 moved the secrets
/// from the first line to the key hand-off.
pub const LAUNCH_VERSION: u32 = 3;

/// The version of `office.json`, separate from the pipe lines' [`LAUNCH_VERSION`]. Bumped only
/// for a breaking change to the file.
pub const OFFICE_FILE_VERSION: u32 = 1;

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
    is_plain_id(office_id).then(|| {
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
    is_plain_id(office_id).then(|| office_home.join(OFFICES_DIR_NAME).join(office_id))
}

/// The folder under the office home that holds the sockets, the same as
/// [`org_proto::location::office_socket_path`]'s.
const RUN_DIR_NAME: &str = "run";

/// How a key socket's file name starts: `k-<id>.sock`. A plain id has no `-`, so a key socket
/// never takes an office socket's name.
pub const KEY_SOCKET_PREFIX: &str = "k-";
/// How a key socket's file name ends: `k-<id>.sock`.
pub const KEY_SOCKET_SUFFIX: &str = ".sock";
/// A [`KeySocketId`]'s length: 16 lowercase hex characters (8 random bytes).
pub const KEY_SOCKET_ID_CHARS: usize = 16;

/// Names one spawn's key socket: exactly [`KEY_SOCKET_ID_CHARS`] lowercase hex characters, never
/// a path. It travels on the first line ([`CreateOffice::key_socket_id`], [`StartOffice`],
/// [`DiscardOffice`]); the server turns it into a path itself with [`key_socket_path`]. It isn't
/// secret: the socket only answers the admitted server (see "The key hand-off" in the crate
/// docs).
///
/// Every way to make one checks the rule: [`KeySocketId::parse`], decoding, and
/// [`KeySocketId::generate_from`], which can't break it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct KeySocketId(String);

impl KeySocketId {
    /// The id for 8 random bytes, as lowercase hex. The caller makes them with a secure random
    /// source: this crate does no I/O.
    pub fn generate_from(bytes: [u8; KEY_SOCKET_ID_CHARS / 2]) -> Self {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let hex = bytes
            .iter()
            .flat_map(|b| [HEX[usize::from(b >> 4)], HEX[usize::from(b & 0x0f)]])
            .map(char::from)
            .collect();
        KeySocketId(hex)
    }

    /// An id from its text: exactly [`KEY_SOCKET_ID_CHARS`] characters, each `0`-`9` or `a`-`f`.
    /// Anything else (uppercase, another length, a path) is [`LaunchError::Invalid`].
    pub fn parse(text: &str) -> Result<Self, LaunchError> {
        if text.len() == KEY_SOCKET_ID_CHARS
            && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            Ok(KeySocketId(text.to_owned()))
        } else {
            Err(LaunchError::Invalid(BAD_KEY_SOCKET_ID))
        }
    }

    /// The id's text, 16 lowercase hex characters.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for KeySocketId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The one error for an id that breaks the rule.
const BAD_KEY_SOCKET_ID: &str = "a key socket id must be 16 lowercase hex characters";

impl<'de> Deserialize<'de> for KeySocketId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        KeySocketId::parse(&text).map_err(|_| serde::de::Error::custom(BAD_KEY_SOCKET_ID))
    }
}

/// The key socket for `id` in `office_home`: `<home>/run/k-<id>.sock`. The host binds it; the
/// server builds the same path from its own office home and never takes one from the host. Both
/// check it with [`org_proto::location::check_socket_path`] before use.
pub fn key_socket_path(office_home: &Path, id: &KeySocketId) -> PathBuf {
    office_home.join(RUN_DIR_NAME).join(format!(
        "{KEY_SOCKET_PREFIX}{}{KEY_SOCKET_SUFFIX}",
        id.as_str()
    ))
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
        check_office_file_version(raw.v)?;
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
            v: OFFICE_FILE_VERSION,
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

/// A line from the host to a server, on the server's stdin. It holds no secret: the secrets come
/// on the key hand-off ([`HandOff`]), named by the line's [`KeySocketId`]. Write it with
/// [`write_host_message`], which adds the version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[non_exhaustive]
pub enum HostMessage {
    /// Create this office (the first line with [`CREATE_FLAG`]).
    Create(CreateOffice),
    /// Serve the existing office (the first line with [`OFFICE_FLAG`]).
    Start(StartOffice),
    /// Remove the unfinished office (the first line with [`DISCARD_FLAG`]).
    Discard(DiscardOffice),
    /// An op this build doesn't know. As the first line it is [`FailCode::InvalidRequest`]
    /// ([`read_first_host_message`]); later lines aren't parsed at all. It is never written.
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateOffice {
    pub org_id: String,
    pub org_name: String,
    pub office_id: String,
    pub office_name: String,
    pub role: OfficeRole,
    /// The key socket this spawn's secrets come on: the office's app token for this host run and
    /// the database key the host made and stored before it started the server ([`HandOff`]).
    pub key_socket_id: KeySocketId,
}

/// What a server started with [`OFFICE_FLAG`] gets on its first line: the key socket on which
/// its hand-off ([`HandOff`]) brings the office's app token for this host run and its database
/// key from the host's key store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "RawStartOffice")]
pub struct StartOffice {
    pub key_socket_id: KeySocketId,
}

/// What a server started with [`DISCARD_FLAG`] gets on its first line: the key socket its
/// hand-off comes on. The hand-off's key may be `null` (the host has none for that office); see
/// "Discarding an unfinished office" in the crate docs for what each form may delete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "RawDiscardOffice")]
pub struct DiscardOffice {
    pub key_socket_id: KeySocketId,
}

/// A start as it is read, refusing the secrets an older host would have put on it.
#[derive(Deserialize)]
struct RawStartOffice {
    key_socket_id: KeySocketId,
    #[serde(default, rename = "app_token", deserialize_with = "no_secret")]
    _app_token: NoSecret,
    #[serde(default, rename = "db_key", deserialize_with = "no_secret")]
    _db_key: NoSecret,
}

impl From<RawStartOffice> for StartOffice {
    fn from(raw: RawStartOffice) -> Self {
        StartOffice {
            key_socket_id: raw.key_socket_id,
        }
    }
}

/// A discard as it is read, refusing the secrets an older host would have put on it.
#[derive(Deserialize)]
struct RawDiscardOffice {
    key_socket_id: KeySocketId,
    #[serde(default, rename = "app_token", deserialize_with = "no_secret")]
    _app_token: NoSecret,
    #[serde(default, rename = "db_key", deserialize_with = "no_secret")]
    _db_key: NoSecret,
}

impl From<RawDiscardOffice> for DiscardOffice {
    fn from(raw: RawDiscardOffice) -> Self {
        DiscardOffice {
            key_socket_id: raw.key_socket_id,
        }
    }
}

/// A secret field's absence on a first line. Its only value is "absent": a present one is refused
/// by [`no_secret`].
#[derive(Default)]
struct NoSecret;

/// Refuses a secret field on a first line, without reading (or echoing) its value.
fn no_secret<'de, D: Deserializer<'de>>(_: D) -> Result<NoSecret, D::Error> {
    Err(serde::de::Error::custom(
        "a first line carries no secret: app_token and db_key come on the key hand-off",
    ))
}

/// Decodes a key that must be present but may be `null`. (A plain `Option` field would read a
/// missing one as `None`.)
fn nullable_key<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DbKey>, D::Error> {
    Option::<DbKey>::deserialize(d)
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
    key_socket_id: KeySocketId,
    #[serde(default, rename = "app_token", deserialize_with = "no_secret")]
    _app_token: NoSecret,
    #[serde(default, rename = "db_key", deserialize_with = "no_secret")]
    _db_key: NoSecret,
}

/// A create as it is written, borrowing from a [`CreateOffice`].
#[derive(Serialize)]
struct RawCreateOfficeRef<'a> {
    org_id: &'a str,
    org_name: &'a str,
    office_id: &'a str,
    office_name: &'a str,
    role: OfficeRole,
    key_socket_id: &'a KeySocketId,
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
            key_socket_id: &self.key_socket_id,
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
            key_socket_id: raw.key_socket_id,
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
    /// Bad or missing arguments, a first line that's malformed, carries a secret or doesn't match
    /// the flag, a key socket that fails the server's checks, or a malformed or over-limit
    /// hand-off line (a `null` key where one is required included).
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
    /// A pipe line's `v` this build doesn't support (other than [`LAUNCH_VERSION`]).
    UnsupportedVersion(u32),
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
            LaunchError::UnsupportedVersion(v) => write!(
                f,
                "launch version {v} isn't supported (this build speaks {LAUNCH_VERSION})"
            ),
            LaunchError::UnsupportedOfficeFileVersion(v) => write!(
                f,
                "office.json version {v} isn't supported (this build reads \
                 {OFFICE_FILE_VERSION})"
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
            LaunchError::UnsupportedVersion(_)
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
            LaunchError::UnsupportedVersion(_)
            | LaunchError::UnsupportedOfficeFileVersion(_)
            | LaunchError::Json(_)
            | LaunchError::Invalid(_) => false,
        }
    }

    /// Whether the host went away while a line was coming: the first line's pipe ended mid-line,
    /// or the key hand-off's connection ended before its whole line ([`read_hand_off`]). Treat it
    /// like EOF: exit 0 without a `failed` line.
    pub fn is_host_gone(&self) -> bool {
        matches!(self, LaunchError::Frame(FrameError::Truncated))
    }

    /// The code a server reports when it can't use what the host sent: a broken pipe is
    /// [`FailCode::StorageError`] (retried), a bad line [`FailCode::InvalidRequest`] (not
    /// retried), another version [`FailCode::UnsupportedVersion`]. Check
    /// [`LaunchError::is_host_gone`] first: then there's nobody to report to.
    pub fn fail_code(&self) -> FailCode {
        match self {
            LaunchError::UnsupportedVersion(_) | LaunchError::UnsupportedOfficeFileVersion(_) => {
                FailCode::UnsupportedVersion
            }
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
    /// [`CREATE_FLAG`]: a [`HostMessage::Create`].
    Create,
    /// [`OFFICE_FLAG`]: a [`HostMessage::Start`].
    Start,
    /// [`DISCARD_FLAG`]: a [`HostMessage::Discard`].
    Discard,
}

/// Read a server's first stdin line, the one rule for both sides. `Ok(None)` means EOF before
/// it (the host is gone: exit 0, no `failed` line). An unknown op, or another flag's op, is
/// [`LaunchError::Invalid`] (report [`LaunchError::fail_code`], i.e. `invalid_request`).
pub fn read_first_host_message(
    r: &mut impl BufRead,
    expected: FirstLine,
) -> Result<Option<HostMessage>, LaunchError> {
    let Some(message) = read_host_message(r)? else {
        return Ok(None);
    };
    match (&message, expected) {
        (HostMessage::Create(_), FirstLine::Create)
        | (HostMessage::Start(_), FirstLine::Start)
        | (HostMessage::Discard(_), FirstLine::Discard) => Ok(Some(message)),
        _ => Err(LaunchError::Invalid(
            "the first line isn't the op this server's flag expects",
        )),
    }
}

/// Read one host message. `Ok(None)` means the host closed the pipe. The line it was read from
/// is zeroed afterwards (it holds no secret in this version: defence in depth). A server reads its
/// first line with [`read_first_host_message`], which also checks the op.
pub fn read_host_message(r: &mut impl BufRead) -> Result<Option<HostMessage>, LaunchError> {
    match read_secret_line(r)? {
        Some(mut line) => decode_host_line(&mut line).map(Some),
        None => Ok(None),
    }
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

/// Decode a host line, then zero it, on every path.
fn decode_host_line(line: &mut [u8]) -> Result<HostMessage, LaunchError> {
    let message = versioned(line)
        .and_then(|()| serde_json::from_slice::<HostMessage>(line).map_err(LaunchError::Json));
    zero(line);
    message
}

/// Write one host message, then flush. [`HostMessage::Unknown`] and a create that breaks
/// [`CreateOffice`]'s rules are refused, and nothing is written. The line holds no secret; it is
/// still written in one piece through the zeroed path the hand-off line uses.
pub fn write_host_message(w: &mut impl Write, message: &HostMessage) -> Result<(), LaunchError> {
    match message {
        HostMessage::Create(create) => create.check()?,
        HostMessage::Start(_) | HostMessage::Discard(_) => {}
        HostMessage::Unknown => return Err(LaunchError::Invalid("can't send an unknown op")),
    }
    let mut bytes = encode_launch_line(message)?;
    send_and_zero(w, &mut bytes)
}

/// The secrets a server gets on the key hand-off, read with [`read_hand_off`] right after its
/// first line. Not `Clone`; its `Debug` shows neither secret (both types redact themselves).
///
/// On the wire: `{"v":3,"app_token":"<64 hex>","db_key":"<64 hex>"|null}`, one line, written
/// only by [`write_hand_off`]. Every field is required (`db_key` may be `null` only where
/// [`read_hand_off`] allows it) and any other field is refused.
#[derive(Debug, PartialEq, Eq)]
pub struct HandOff {
    /// The office's app token for this host run.
    pub app_token: AppToken,
    /// The office's database key: on a create, the one the host made and stored before it
    /// started the server (the server creates the databases with it and never stores it); on a
    /// start, the one from the host's key store. `None` only on a discard for an office the host
    /// holds no key for.
    pub db_key: Option<DbKey>,
}

/// The hand-off as it is read, without its `v` (checked first) and refusing any other field.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawHandOff {
    /// Checked by `versioned` before the shape is read.
    #[serde(rename = "v")]
    _v: u32,
    app_token: AppToken,
    #[serde(deserialize_with = "nullable_key")]
    db_key: Option<DbKey>,
}

/// The hand-off as it is written, borrowing the secrets. No `Debug`: nothing should print it.
#[derive(Serialize)]
struct HandOffRef<'a> {
    app_token: &'a AppToken,
    db_key: Option<&'a DbKey>,
}

/// Write the hand-off line on the key socket's connection, then flush: the office's app token and
/// its database key (`None` is written as `"db_key": null`, only for a discard the host holds no
/// key for). It borrows the secrets, for a host that keeps them elsewhere (e.g. the app token in
/// an `Arc` for its whole run). The line is encoded into a buffer of exactly its size, written in
/// one piece, and zeroed afterwards whether the write worked or not. Never log it.
pub fn write_hand_off(
    w: &mut impl Write,
    app_token: &AppToken,
    db_key: Option<&DbKey>,
) -> Result<(), LaunchError> {
    let mut bytes = encode_launch_line(&HandOffRef { app_token, db_key })?;
    send_and_zero(w, &mut bytes)
}

/// Read the one hand-off line from the key socket's connection, for the server started with
/// `first_line`'s flag. A create and a start need the database key: a `null` one is
/// [`LaunchError::Invalid`]; a discard takes `null` too.
///
/// The line is read with the frame limit into a buffer that never reallocates and is zeroed on
/// every path, like the first line. Its `v` is checked before its shape
/// ([`LaunchError::UnsupportedVersion`]); a missing or unknown field is [`LaunchError::Json`].
/// EOF before a whole line (no bytes at all included) is
/// `LaunchError::Frame(FrameError::Truncated)`, for which [`LaunchError::is_host_gone`] is true:
/// the host is gone, so exit 0 silently. An empty line (a bare `\n`) is a malformed line instead.
/// Report anything else with fixed text, never the error's own text.
pub fn read_hand_off(r: &mut impl BufRead, first_line: FirstLine) -> Result<HandOff, LaunchError> {
    let Some(mut line) = read_secret_line(r)? else {
        return Err(LaunchError::Frame(FrameError::Truncated));
    };
    let hand_off = decode_hand_off(&mut line)?;
    if hand_off.db_key.is_none() && first_line != FirstLine::Discard {
        return Err(LaunchError::Invalid(
            "the hand-off line has no database key for a create or a start",
        ));
    }
    Ok(hand_off)
}

/// Decode a hand-off line, then zero it, on every path.
fn decode_hand_off(line: &mut [u8]) -> Result<HandOff, LaunchError> {
    let hand_off = versioned(line).and_then(|()| {
        serde_json::from_slice::<RawHandOff>(line)
            .map(|raw| HandOff {
                app_token: raw.app_token,
                db_key: raw.db_key,
            })
            .map_err(LaunchError::Json)
    });
    zero(line);
    hand_off
}

/// One write of the whole line and a flush, then the buffer is zeroed (it may hold secrets),
/// whether the write worked or not.
fn send_and_zero(w: &mut impl Write, line: &mut [u8]) -> Result<(), LaunchError> {
    let result = w
        .write_all(line)
        .and_then(|()| w.flush())
        .map_err(|e| LaunchError::Frame(FrameError::Io(e)));
    zero(line);
    result
}

/// Encode a launch line from the host (a host message or the hand-off line) as one line with its
/// `\n` and the version, into a buffer of exactly that size (measured first), so it never
/// reallocates and leaves a copy of a secret behind. The caller zeroes it after use.
fn encode_launch_line<T: Serialize>(message: &T) -> Result<Vec<u8>, LaunchError> {
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

fn check_version(v: u32, supported: u32) -> Result<(), LaunchError> {
    if v == supported {
        Ok(())
    } else {
        Err(LaunchError::UnsupportedVersion(v))
    }
}

fn check_office_file_version(v: u32) -> Result<(), LaunchError> {
    if v == OFFICE_FILE_VERSION {
        Ok(())
    } else {
        Err(LaunchError::UnsupportedOfficeFileVersion(v))
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
    check_version(version.v, LAUNCH_VERSION)
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
