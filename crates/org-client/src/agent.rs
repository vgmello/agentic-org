//! An agent's connection to its office, for the agents' CLI (`org-agent`): find the office on
//! this Mac (ADR-0003 §2.1), then open a connection bound as the agent.
//!
//! - [`find_office`] picks the office deterministically: the one asked for (`--office` or
//!   `ORG_OFFICE`), else the only office on this Mac with an `office.json`. It never guesses from
//!   which sockets happen to be open, and it asks the host nothing.
//! - [`open_as`] connects to that office's socket (retrying for up to [`CONNECT_RETRY`] while
//!   the server may be restarting), checks the protocol version, and binds as the member with no
//!   token (an agent run by hand, ADR-0003 M-A). Its error says whether the office was
//!   [`OpenError::Unreachable`], so the caller may fall back to its spool, or answered and
//!   [`OpenError::Refused`].

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::time::{Duration, Instant};

use org_office_launch::{
    is_plain_id, OfficeFile, MAX_OFFICE_FILE_BYTES, OFFICES_DIR_NAME, OFFICE_FILE_NAME,
};
use org_proto::msg::{MsgBind, MsgBindParams, MsgBindResult};

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

/// The office to talk to: `chosen` (from `--office` or `ORG_OFFICE`) if given, else the only
/// office in `<home>/offices/` with a valid `office.json` that names its folder. `None` when no
/// office was asked for and there is none: the Mac has no office (yet).
///
/// Folders without an `office.json` (an unfinished create), with one that is unreadable,
/// invalid, a symlink, not owned by this user, or for another office, are not offices.
pub fn find_office(home: &Path, chosen: Option<&str>) -> Result<Option<String>, FindOfficeError> {
    if let Some(id) = chosen {
        return if is_plain_id(id) {
            Ok(Some(id.to_owned()))
        } else {
            Err(FindOfficeError::NotAPlainId)
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
    // SAFETY: `geteuid` takes nothing, can't fail and touches no memory of ours.
    unsafe { libc::geteuid() }
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
        .call_with_timeout::<MsgBind>(
            &MsgBindParams {
                member: Some(member.to_owned()),
                app_token: None,
            },
            SETUP_TIMEOUT,
        )
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
mod tests {
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    use org_proto::OfficeRole;

    use super::*;

    const A: &str = "01OFFICE0000000000000000AA";
    const B: &str = "01OFFICE0000000000000000BB";

    fn home() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::Builder::new()
            .prefix("oca")
            .tempdir_in("/tmp")
            .unwrap();
        let home = dir.path().to_path_buf();
        (dir, home)
    }

    fn office_file(id: &str) -> Vec<u8> {
        OfficeFile::new(
            id,
            "HQ",
            OfficeRole::Hq,
            "01ORG0000000000000000ACME1",
            "Acme",
            "2026-11-09T10:00:00.000Z",
        )
        .to_vec()
        .unwrap()
    }

    /// `<home>/offices/<folder>/office.json` holding office `id`.
    fn put(home: &Path, folder: &str, id: &str) {
        let dir = home.join(OFFICES_DIR_NAME).join(folder);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(OFFICE_FILE_NAME), office_file(id)).unwrap();
    }

    #[test]
    fn the_office_asked_for_wins_and_must_be_a_plain_id() {
        let (_dir, home) = home();
        put(&home, A, A);
        put(&home, B, B);
        assert_eq!(find_office(&home, Some(B)).unwrap(), Some(B.to_owned()));
        // Asked for by id, it needn't have an office.json here: its socket decides.
        assert_eq!(find_office(&home, Some("X1")).unwrap(), Some("X1".into()));
        for bad in ["", "../x", "a/b", "a.b", &"a".repeat(65)] {
            assert!(
                matches!(
                    find_office(&home, Some(bad)),
                    Err(FindOfficeError::NotAPlainId)
                ),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn no_offices_folder_or_an_empty_one_is_no_office() {
        let (_dir, home) = home();
        assert_eq!(find_office(&home, None).unwrap(), None);
        fs::create_dir_all(home.join(OFFICES_DIR_NAME)).unwrap();
        assert_eq!(find_office(&home, None).unwrap(), None);
    }

    #[test]
    fn the_only_office_is_chosen_and_two_are_ambiguous() {
        let (_dir, home) = home();
        put(&home, A, A);
        assert_eq!(find_office(&home, None).unwrap(), Some(A.to_owned()));
        put(&home, B, B);
        match find_office(&home, None) {
            Err(FindOfficeError::Ambiguous(ids)) => assert_eq!(ids, vec![A, B]),
            other => panic!("{other:?}"),
        }
        let text = find_office(&home, None).unwrap_err().to_string();
        assert!(
            text.contains("ORG_OFFICE") && text.contains("--office"),
            "{text}"
        );
    }

    #[test]
    fn folders_that_are_not_offices_are_skipped() {
        let (_dir, home) = home();
        put(&home, A, A);
        let offices = home.join(OFFICES_DIR_NAME);
        // An unfinished create: no office.json.
        fs::create_dir_all(offices.join("UNFINISHED1")).unwrap();
        // An office.json for another office.
        put(&home, "MISPLACED1", B);
        // An invalid office.json.
        fs::create_dir_all(offices.join("GARBLED1")).unwrap();
        fs::write(offices.join("GARBLED1").join(OFFICE_FILE_NAME), b"{}").unwrap();
        // An office.json that is a symlink to a valid one.
        let elsewhere = home.join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        fs::write(elsewhere.join(OFFICE_FILE_NAME), office_file("LINKED1")).unwrap();
        fs::create_dir_all(offices.join("LINKED1")).unwrap();
        symlink(
            elsewhere.join(OFFICE_FILE_NAME),
            offices.join("LINKED1").join(OFFICE_FILE_NAME),
        )
        .unwrap();
        // A folder that is a symlink to a valid office.
        put(&home, "REAL2", "REAL2");
        let real = offices.join("REAL2");
        fs::rename(&real, home.join("REAL2")).unwrap();
        symlink(home.join("REAL2"), &real).unwrap();
        // Names that aren't plain ids, and a plain file.
        fs::create_dir_all(offices.join(".refused").join(B)).unwrap();
        fs::write(offices.join("FILE1"), b"x").unwrap();

        assert_eq!(find_office(&home, None).unwrap(), Some(A.to_owned()));
    }

    #[test]
    fn an_office_with_nothing_listening_is_unreachable_after_a_short_retry() {
        let (_dir, home) = home();
        fs::create_dir_all(home.join("run")).unwrap();
        let started = Instant::now();
        let error = match open_as(&home, A, "pixel") {
            Err(error) => error,
            Ok(_) => panic!("connected to nothing"),
        };
        assert!(
            matches!(error, OpenError::Unreachable(ClientError::NotRunning(_))),
            "{error:?}"
        );
        let took = started.elapsed();
        assert!(
            took >= Duration::from_millis(300),
            "gave up at once: {took:?}"
        );
        assert!(took < Duration::from_secs(3), "{took:?}");
    }

    #[test]
    fn a_server_that_accepts_but_never_answers_is_unreachable_within_the_setup_timeout() {
        let (_dir, home) = home();
        fs::create_dir_all(home.join("run")).unwrap();
        let socket = org_proto::location::office_socket_path(&home, A).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let (_release, held) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            let connection = listener.accept();
            let _ = held.recv();
            drop(connection);
        });
        let started = Instant::now();
        let error = match open_as(&home, A, "pixel") {
            Err(error) => error,
            Ok(_) => panic!("a silent server answered"),
        };
        assert!(
            matches!(error, OpenError::Unreachable(ClientError::Timeout)),
            "{error:?}"
        );
        assert!(started.elapsed() < SETUP_TIMEOUT + Duration::from_secs(2));
    }

    #[test]
    fn a_plain_file_where_the_socket_should_be_is_unreachable_without_connecting() {
        let (_dir, home) = home();
        fs::create_dir_all(home.join("run")).unwrap();
        let socket = org_proto::location::office_socket_path(&home, A).unwrap();
        fs::write(&socket, b"not a socket").unwrap();
        assert!(matches!(
            open_as(&home, A, "pixel"),
            Err(OpenError::Unreachable(ClientError::OfficeNotReachable))
        ));
    }

    #[test]
    fn an_office_not_ready_yet_is_unreachable_and_a_refusal_is_refused() {
        use org_proto::{ApiError, ErrorCode};
        for code in [ErrorCode::NotReady, ErrorCode::Unavailable] {
            assert!(matches!(
                OpenError::from_setup(ClientError::Api(ApiError::new(code, "starting"))),
                OpenError::Unreachable(_)
            ));
        }
        assert!(matches!(
            OpenError::from_setup(ClientError::Api(ApiError::new(
                ErrorCode::NotAuthorized,
                "no"
            ))),
            OpenError::Refused(_)
        ));
    }

    #[test]
    fn a_relative_home_or_a_socket_path_too_long_is_refused_not_unreachable() {
        assert!(matches!(
            open_as(Path::new("rel"), A, "pixel"),
            Err(OpenError::Refused(ClientError::NoOfficeHome))
        ));
        let long = PathBuf::from(format!("/tmp/{}", "x".repeat(120)));
        assert!(matches!(
            open_as(&long, A, "pixel"),
            Err(OpenError::Refused(ClientError::SocketPathTooLong(_)))
        ));
    }
}
