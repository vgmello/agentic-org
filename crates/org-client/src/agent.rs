//! An agent's connection to its office, for the agents' CLI (`agent-os`): find the office on
//! this Mac (ADR-0003 §2.1), then open a connection bound as the agent.
//!
//! - [`find_office`] picks the office deterministically: the one asked for (`--office` or
//!   `ORG_OFFICE`), else the only office on this Mac with an `office.json`. It never guesses from
//!   which sockets happen to be open, and it asks the host nothing.
//! - [`spool_send`] writes a send for an office that isn't reachable into that office's own spool
//!   (ADR-0003 §3.5), for the office to ingest when it starts.
//! - [`open_as`] connects to that office's socket (retrying for up to [`CONNECT_RETRY`] while
//!   the server may be restarting), checks the protocol version, and binds as the member with no
//!   token (an agent run by hand, ADR-0003 M-A). Its error says whether the office was
//!   [`OpenError::Unreachable`], so the caller may fall back to its spool, or answered and
//!   [`OpenError::Refused`].

use std::fmt;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use org_office_launch::{
    is_plain_id, office_dir, OfficeFile, MAX_OFFICE_FILE_BYTES, OFFICES_DIR_NAME, OFFICE_FILE_NAME,
};
use org_proto::msg::{MsgBindResult, SpooledSend};

use crate::{Client, ClientError};

/// How long [`open_as`] keeps trying to connect while nothing listens on the office's socket
/// (the host may be restarting the server), backing off between tries (ADR-0003 §3.5).
pub const CONNECT_RETRY: Duration = Duration::from_millis(500);

/// How long [`open_as`] gives `hello` and `msg.bind` each. A server that accepts connections but
/// doesn't answer (hung, stopped) counts as unreachable well before a caller's own deadline.
pub const SETUP_TIMEOUT: Duration = Duration::from_secs(3);

/// The first pause between connection tries; each pause doubles, up to [`MAX_PAUSE`].
const FIRST_PAUSE: Duration = Duration::from_millis(25);
const MAX_PAUSE: Duration = Duration::from_millis(200);

/// Why no office could be chosen.
#[derive(Debug)]
pub enum FindOfficeError {
    /// The office asked for isn't a plain id (1 to 64 ASCII letters and digits).
    NotAPlainId,
    /// The office asked for has no valid `office.json` in this Mac's office home (a typo, or an
    /// office of another Mac).
    NotOnThisMac(String),
    /// More than one office on this Mac has an `office.json`, and none was asked for. Their
    /// ids, sorted.
    Ambiguous(Vec<String>),
    /// The offices folder couldn't be read.
    Io(io::Error),
}

impl fmt::Display for FindOfficeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FindOfficeError::NotAPlainId => {
                write!(f, "the office id must be 1 to 64 ASCII letters and digits")
            }
            FindOfficeError::NotOnThisMac(id) => write!(f, "office {id} isn't on this Mac"),
            FindOfficeError::Ambiguous(ids) => write!(
                f,
                "{} offices are on this Mac ({}); set ORG_OFFICE or pass --office <id>",
                ids.len(),
                ids.join(", ")
            ),
            FindOfficeError::Io(e) => write!(f, "can't read the offices folder: {e}"),
        }
    }
}

impl std::error::Error for FindOfficeError {}

/// The office to talk to: `chosen` (from `--office` or `ORG_OFFICE`) if given and it is an
/// office of this Mac, else the only office in `<home>/offices/` with a valid `office.json` that
/// names its folder. `None` when no
/// office was asked for and there is none: the Mac has no office (yet).
///
/// Folders without an `office.json` (an unfinished create), with one that is unreadable,
/// invalid, a symlink, not owned by this user, or for another office, are not offices.
pub fn find_office(home: &Path, chosen: Option<&str>) -> Result<Option<String>, FindOfficeError> {
    if let Some(id) = chosen {
        if !is_plain_id(id) {
            return Err(FindOfficeError::NotAPlainId);
        }
        // A named office must be one of this Mac's (a typo is told, not tried as a socket).
        let folder = home.join(OFFICES_DIR_NAME).join(id);
        let is_office = fs::symlink_metadata(&folder).is_ok_and(|m| m.file_type().is_dir())
            && read_office_file(&folder.join(OFFICE_FILE_NAME))
                .is_some_and(|office| office.names_folder(id));
        return if is_office {
            Ok(Some(id.to_owned()))
        } else {
            Err(FindOfficeError::NotOnThisMac(id.to_owned()))
        };
    }
    let offices = match fs::read_dir(home.join(OFFICES_DIR_NAME)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(FindOfficeError::Io(e)),
    };
    let mut found = Vec::new();
    for entry in offices {
        let entry = entry.map_err(FindOfficeError::Io)?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        // Real folders only: a symlink isn't an office of this home.
        let is_dir = fs::symlink_metadata(entry.path()).is_ok_and(|m| m.file_type().is_dir());
        if !is_plain_id(&name) || !is_dir {
            continue;
        }
        if let Some(office) = read_office_file(&entry.path().join(OFFICE_FILE_NAME)) {
            if office.names_folder(&name) {
                found.push(name);
            }
        }
    }
    found.sort();
    match found.len() {
        0 => Ok(None),
        1 => Ok(found.pop()),
        _ => Err(FindOfficeError::Ambiguous(found)),
    }
}

/// `office.json`, read the way its contract asks (`OfficeFile::from_slice`): without following
/// a symlink, only a regular file owned by this user, at most `MAX_OFFICE_FILE_BYTES`. `None`
/// when there is none or it fails any of that.
fn read_office_file(path: &Path) -> Option<OfficeFile> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.uid() != effective_uid() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_OFFICE_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    OfficeFile::from_slice(&bytes).ok()
}

fn effective_uid() -> u32 {
    org_socket::effective_uid()
}

/// The office spool's folders, in the office's folder (ADR-0003 §3.5): a send is written in
/// `spool/tmp/` and renamed into `spool/new/<id>.json`, where the office ingests it.
pub const SPOOL_DIR_NAME: &str = "spool";
const SPOOL_TMP: &str = "tmp";
const SPOOL_NEW: &str = "new";

/// The largest spooled send the office ingests (ADR-0003 §3.5, `M4`).
pub const MAX_SPOOLED_BYTES: usize = 512 * 1024;

/// Why [`spool_send`] wrote nothing.
#[derive(Debug)]
pub enum SpoolSendError {
    /// The office isn't a finished office of this Mac: no real folder, or one without its
    /// `office.json` (an unfinished create, which the host discards).
    NoSuchOffice,
    /// A send the office's ingest would refuse (a field over its limit, a kind, priority, id
    /// or name it doesn't take), or one over [`MAX_SPOOLED_BYTES`].
    Invalid(&'static str),
    /// A spool folder that isn't a real folder (e.g. a symlink): refused.
    NotAFolder(PathBuf),
    Io(io::Error),
}

impl fmt::Display for SpoolSendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpoolSendError::NoSuchOffice => write!(f, "that office isn't on this Mac"),
            SpoolSendError::Invalid(why) => write!(f, "{why}"),
            SpoolSendError::NotAFolder(path) => {
                write!(f, "{} isn't a plain folder", path.display())
            }
            SpoolSendError::Io(e) => write!(f, "can't write to the office's spool: {e}"),
        }
    }
}

impl std::error::Error for SpoolSendError {}

impl From<io::Error> for SpoolSendError {
    fn from(error: io::Error) -> Self {
        SpoolSendError::Io(error)
    }
}

/// Writes `send` into the spool of office `send.office_id` in `home`, for the office to ingest
/// when it next starts (ADR-0003 §3.5): `<office folder>/spool/tmp/<id>.json` (folders `0700`,
/// file `0600`, created new, never through a symlink), flushed with `F_FULLFSYNC`, moved into
/// `spool/new/<id>.json` in one step (never replacing a file already there), then `new/`
/// flushed. The office must be a finished office of this Mac (its folder holds its
/// `office.json`); this never creates an office. A send the office's ingest would refuse is
/// refused here. Returns the file's path.
pub fn spool_send(home: &Path, send: &SpooledSend) -> Result<PathBuf, SpoolSendError> {
    if !is_plain_id(&send.id) {
        return Err(SpoolSendError::Invalid("the message id isn't a plain id"));
    }
    check_ingestible(send)?;
    let office = office_dir(home, &send.office_id).ok_or(SpoolSendError::NoSuchOffice)?;
    // A finished office of this Mac: a folder without its `office.json` is an unfinished create,
    // which the host discards (with anything spooled in it).
    let finished = real_dir(&office)?
        && read_office_file(&office.join(OFFICE_FILE_NAME))
            .is_some_and(|file| file.names_folder(&send.office_id));
    if !finished {
        return Err(SpoolSendError::NoSuchOffice);
    }
    let bytes = serde_json::to_vec(send)
        .map_err(|_| SpoolSendError::Invalid("the message can't be written"))?;
    if bytes.len() > MAX_SPOOLED_BYTES {
        return Err(SpoolSendError::Invalid(
            "the message is too large to spool (512 KiB at most)",
        ));
    }
    let spool = office.join(SPOOL_DIR_NAME);
    let tmp = spool.join(SPOOL_TMP);
    let new = spool.join(SPOOL_NEW);
    for dir in [&spool, &tmp, &new] {
        owner_only_dir(dir)?;
    }
    let name = format!("{}.json", send.id);
    let staged = tmp.join(&name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&staged)?;
    let written = file.write_all(&bytes).and_then(|()| full_sync(&file));
    drop(file);
    let placed = new.join(&name);
    if let Err(error) = written.and_then(|()| rename_new(&staged, &placed)) {
        let _ = fs::remove_file(&staged);
        return Err(error.into());
    }
    // The send is in place: the office will ingest it. A failed flush of the folder can't undo
    // that (and saying "not queued" would invite a duplicate), so it is not an error.
    if let Ok(folder) = File::open(&new) {
        let _ = full_sync(&folder);
    }
    Ok(placed)
}

/// Moves `from` to `to` in one step, never replacing a file already at `to`
/// (`org_platform::fs::rename_noreplace`). A filesystem that can't do that refuses; nothing
/// is linked and unlinked instead.
fn rename_new(from: &Path, to: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = |p: &Path| {
        CString::new(p.as_os_str().as_bytes())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
    };
    let (from, to) = (c(from)?, c(to)?);
    org_platform::fs::rename_noreplace(&from, &to)
}

/// The office's ingest applies every `msg.send` rule to a spooled send (`SpooledSend`), plus
/// rules on `from`. Refuse here what can be told without the office (the limits, kinds,
/// priority, ids and reserved names), so a send is never reported queued and then
/// dead-lettered for one of those. (Whether the members exist only the office can tell.)
fn check_ingestible(send: &SpooledSend) -> Result<(), SpoolSendError> {
    use org_proto::msg::{
        json_string_len, MessagePriority, KIND_DELIVERY_FAILED, KIND_REQUEST, MAX_BODY_BYTES,
        MAX_HANDLE_BYTES, MAX_ID_BYTES, MAX_META_BYTES, MAX_SUBJECT_BYTES, OFFICE_SENDER,
    };
    let invalid = |why| Err(SpoolSendError::Invalid(why));
    if send.v != org_proto::msg::SPOOL_VERSION {
        return invalid("unknown spool version");
    }
    if org_proto::ulid_time_ms(&send.id).is_none() {
        return invalid("the message id isn't a ULID");
    }
    if send.kind.is_empty()
        || json_string_len(&send.kind) > MAX_ID_BYTES
        || send.kind == KIND_REQUEST
        || send.kind == KIND_DELIVERY_FAILED
    {
        return invalid("that kind can't be spooled");
    }
    if !matches!(
        send.priority,
        MessagePriority::Normal | MessagePriority::High | MessagePriority::Urgent
    ) {
        return invalid("unknown priority");
    }
    if json_string_len(&send.body) > MAX_BODY_BYTES {
        return invalid("the body is too large (256 KiB at most)");
    }
    if send
        .subject
        .as_deref()
        .is_some_and(|s| json_string_len(s) > MAX_SUBJECT_BYTES)
    {
        return invalid("the subject is too large (1 KiB at most)");
    }
    // Message ids are ULIDs, so any other reply_to names no message.
    if send
        .reply_to
        .as_deref()
        .is_some_and(|r| json_string_len(r) > MAX_ID_BYTES || org_proto::ulid_time_ms(r).is_none())
    {
        return invalid("reply_to isn't a message id");
    }
    let meta_bytes = if send.meta.is_empty() {
        0
    } else {
        serde_json::to_vec(&send.meta)
            .map_err(|_| SpoolSendError::Invalid("the meta can't be written"))?
            .len()
    };
    if meta_bytes > MAX_META_BYTES {
        return invalid("the meta is too large (64 KiB at most)");
    }
    for who in [&send.to, &send.from] {
        if json_string_len(who) > MAX_HANDLE_BYTES {
            return invalid("a member name is too long");
        }
    }
    // Members are compared trimmed of ASCII whitespace, as the office does.
    let to = send.to.trim_ascii();
    if to.is_empty() {
        return invalid("no recipient");
    }
    if to.eq_ignore_ascii_case(OFFICE_SENDER) {
        return invalid("'office' isn't a recipient");
    }
    // Ingest takes a spooled send only from an agent: never from the office or the owner.
    let from = send.from.trim_ascii();
    let run_address = from
        .get(..org_proto::RUN_HANDLE_PREFIX.len())
        .is_some_and(|p| p.eq_ignore_ascii_case(org_proto::RUN_HANDLE_PREFIX));
    if from.is_empty()
        || run_address
        || from.eq_ignore_ascii_case(OFFICE_SENDER)
        || from.eq_ignore_ascii_case(org_proto::OWNER_HANDLE)
    {
        return invalid("that sender can't spool a message");
    }
    Ok(())
}

/// Whether `path` is a real folder (not a symlink to one); `false` if it doesn't exist.
fn real_dir(path: &Path) -> Result<bool, SpoolSendError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.file_type().is_dir()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Makes `dir` (`0700`) if it's missing; refuses anything there that isn't a real folder of
/// this user, and makes it owner-only.
fn owner_only_dir(dir: &Path) -> Result<(), SpoolSendError> {
    match DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    let metadata = fs::symlink_metadata(dir)?;
    if !metadata.file_type().is_dir() || metadata.uid() != effective_uid() {
        return Err(SpoolSendError::NotAFolder(dir.to_path_buf()));
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Flushes `file` to the disk itself (`fsync`, and on macOS then `F_FULLFSYNC`). A failed
/// flush is an error, never a weaker flush.
fn full_sync(file: &File) -> io::Result<()> {
    org_platform::fs::full_sync(file)
}

/// Why [`open_as`] gave no connection.
#[derive(Debug)]
pub enum OpenError {
    /// The office didn't answer: nothing listens on its socket (after [`CONNECT_RETRY`]), the
    /// socket isn't one, the connection failed or closed before the bind was answered, or the
    /// office said it isn't ready yet (`not_ready`, `unavailable`).
    /// Nothing was asked of the office, so a caller may safely fall back (e.g. spool a send).
    Unreachable(ClientError),
    /// The office answered and said no (a refused bind, another protocol version, an answer
    /// that doesn't fit), or the setup can never work (a socket path that's too long).
    Refused(ClientError),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::Unreachable(e) | OpenError::Refused(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for OpenError {}

impl OpenError {
    /// Sorts an error from connecting, `hello` or `msg.bind`: before the office has answered
    /// anything, a failed connection means the office isn't there to answer.
    fn from_setup(error: ClientError) -> Self {
        match error {
            ClientError::NotRunning(_)
            | ClientError::Io(_)
            | ClientError::Timeout
            | ClientError::Disconnected
            | ClientError::OfficeNotReachable => OpenError::Unreachable(error),
            // Starting up or too busy to take the connection: nothing was asked of it yet.
            ClientError::Api(ref api)
                if matches!(
                    api.code,
                    org_proto::ErrorCode::Unavailable | org_proto::ErrorCode::NotReady
                ) =>
            {
                OpenError::Unreachable(error)
            }
            other => OpenError::Refused(other),
        }
    }
}

/// Connects to office `office_id` in `home` and binds the connection as `member` with no
/// token (see [`Client::bind_member`]). Retries the connection for up to [`CONNECT_RETRY`]
/// while nothing listens; checks the protocol version and that the server is the office asked
/// for (two offices whose ids end alike would share a socket name).
pub fn open_as(
    home: &Path,
    office_id: &str,
    member: &str,
) -> Result<(Client, MsgBindResult), OpenError> {
    let client = connect_retrying(office_id, home, CONNECT_RETRY).map_err(OpenError::from_setup)?;
    let hello = client
        .hello_with_timeout(SETUP_TIMEOUT)
        .map_err(OpenError::from_setup)?;
    if hello.office.as_ref().map(|o| o.office.id.as_str()) != Some(office_id) {
        return Err(OpenError::Refused(another_office()));
    }
    let bound = client
        .bind_member_with_timeout(member, SETUP_TIMEOUT)
        .map_err(OpenError::from_setup)?;
    if bound.office_id != office_id {
        return Err(OpenError::Refused(another_office()));
    }
    Ok((client, bound))
}

fn another_office() -> ClientError {
    ClientError::Protocol("the server on that socket is another office".into())
}

/// [`Client::connect_office_id`], tried again with a growing pause while nothing listens
/// ([`ClientError::NotRunning`]), until `budget` would be passed.
fn connect_retrying(office_id: &str, home: &Path, budget: Duration) -> Result<Client, ClientError> {
    let give_up_at = Instant::now() + budget;
    let mut pause = FIRST_PAUSE;
    loop {
        match Client::connect_office_id(office_id, home) {
            Err(ClientError::NotRunning(_)) if Instant::now() + pause <= give_up_at => {
                std::thread::sleep(pause);
                pause = (pause * 2).min(MAX_PAUSE);
            }
            outcome => return outcome,
        }
    }
}

#[cfg(test)]
mod tests;
