//! A fake `agentic-org-office` for `org-host`'s tests. **Never shipped:** it is built
//! only with the `test-support` feature, which only the host's own tests turn on.
//!
//! It follows the launch protocol (`org-office-launch`) without a database or a key store, and
//! does what the test scripted. Like the real server it speaks only launch
//! (`org_office_launch::read_first_line`): the first stdin line names the rendezvous socket. The fake connects
//! to it, sends `ready_for_secrets`, receives the handoff header byte by byte (no descriptor on
//! any header byte), then, when the header names descriptors (create and start), receives the
//! transfer socket on its marker byte and answers `fds_received`, then takes the host's writer
//! control on the transfer socket as the real server does (every office needs it, before it
//! creates or opens anything): the `writer_control` record, the custody channel it carries and
//! the host's admission on that channel, which must be valid and name this office (for a
//! create, its organization and owner member; for a start, the stored office's organization),
//! or the run fails with `storage_error` as the real server's does (the organization and owner
//! after the channel's receipt, as there); it answers `fds_received` for the channel and holds
//! the channel, never sending a request on it. Known gaps against the real server, which no
//! host test here relies on: a start checks neither the admission's owner member nor the
//! instance against what the create stored (the fake stores neither); a create does not
//! resume a folder a failed create left (anything but the custody marker or its staging files
//! there is `already_exists`, where the real server allows other plain files and resumes); a
//! fatal descriptor receive is reported as `storage_error` rather than a silent exit; the
//! create line's content (names, role, `hq_creation`, socket length) is checked only by the
//! shared launch decoder, so where the real server refuses such a line before the writer
//! control (without its receipt), the fake takes the control and creates; nor does it make
//! the HQ binding checks the real server makes after the receipt (`storage_error`). A discard names none: no
//! marker is read and no `fds_received` is sent. Events then go on the rendezvous stream; stdout
//! carries nothing, and a refusal before the rendezvous is connected is only recorded. The
//! host's lifetime is stdin or the rendezvous stream, whichever ends first (the real server
//! watches only the rendezvous; the host's shutdown closes both).
//! Past the writer control the transfer socket is held but never read: a host that transfers a
//! channel descriptor gets no receipt, so offices needing those can't run on the fake.
//!
//! The script is `<ORG_OFFICE_HOME>/fake-script.json`:
//! ```json
//! {"create": [{"do": "fail", "code": "invalid_name"}, {"do": "ready"}],
//!  "01ABC…": {"do": "crash"}}
//! ```
//! keyed by `"create"` (for `--create`), `"discard"` (for `--discard <id>`) or the office id
//! (for `--office <id>`). A list gives
//! one step per run of that key (the last one repeats); a missing key means `ready`. A step:
//! - `do`: `ready` (default), `fail` (with `code`), `crash` (exit 3 without a line, `at`
//!   `start` (default), `stored` (`--create`: after writing `office.json`, before `created`),
//!   `created` or `ready`), `hang` (never report anything).
//! - `stdin`: read this text as its standard input instead of what the host writes there (the
//!   host's pipe is kept open, unread, so the host's write still succeeds): a first line the
//!   host never wrote, or, with an empty text, a standard input that ends before any first
//!   line. What a shell wrapper around a real server would do with `printf … |` or
//!   `</dev/null`, in a compiled program a host can start on every system.
//! - `ignore_eof`: on stdin EOF, keep running (until SIGKILLed) instead of exiting 0.
//! - `linger` (with `fail`): after the `failed` line, keep running (until SIGKILLed).
//! - `orphan_stdout_ms`: first start a `sleep` of this long that holds the fake's stdout, so the
//!   control pipe stays open after the fake exits (recorded as `orphan <key> <pid> <sleep pid>`).
//! - `hang` and `fail` also work `at` `stored` (`--create`: after writing `office.json`, before
//!   `created`).
//! - `corrupt_office_file` (`--create`): write an `office.json` that isn't valid.
//! - `stderr_bytes`: write this many bytes (`z`s) to stderr first, and nothing else to stderr.
//! - `delay_ms`: wait this long before `created` / `ready`.
//! - `ready_delay_ms`: wait this long more just before `ready` (after `created`).
//! - `report` (`--create`): fields of the office it stores and reports in `created` that
//!   differ from the create message: `office_id`, `org_id` (both UUIDv7, or the create fails
//!   with `storage_error`), `org_name`, `office_name` or `role`.
//!
//! Its first stdin line must match its flag, as the contract says: a launch `create` line with
//! `--create`, a `start` line with `--office`, a `discard` line with `--discard`. A malformed,
//! other-version or other line is refused before any scripted step (for `--discard`, before its
//! step too): it records `first-line-refused` and exits 1 (reporting `failed` only if the
//! rendezvous is already connected). EOF before the first line means the host is gone: it exits 0.
//! It never logs a secret: for the database and grant keys it records only a fingerprint (the
//! 64-bit FNV-1a hash of the 32 bytes, as 16 hex digits) in `fake.log`, so tests can tell
//! secrets apart without seeing them.
//!
//! On `--create` it uses the office folder the host reserved custody in (or makes it), refusing
//! one that holds anything but the host's custody marker (`already_exists`), and holds
//! `office.lock` before anything else (so a failure or crash leaves an unfinished folder, like
//! a real server could), then writes a `db` file (its stand-in for the databases, holding no
//! secret), and writes `office.json` (version 2, UUIDv7 IDs) last.
//! `ready` binds a real socket that answers `hello`. `--discard <id>` takes the office lock
//! (with the usual wait), refuses a folder holding the host's custody marker (`storage_error`,
//! checked first, as the real server refuses a reserved folder whatever it holds) or
//! `office.json` (`already_exists`), removes the folder and reports `discarded` (`fail`,
//! `crash` and `hang` work there too).
//!
//! It records what it does in `<home>/fake.log`, one line each:
//! `start <key> <pid> <run> <unix ms>`, `launch <key> <pid> v1`, `instance <key> <pid> <id>`,
//! `grant <key> <pid> <fingerprint>`, `recovery <key> <pid> present` (create),
//! `transfer <key> <pid> received` (create and start),
//! `writer <key> <pid> <organization id>` (create and start: the admission it was given),
//! `secrets-key <key> <pid> <fingerprint or null>` (`secrets-key discard <pid> <fingerprint or null> <id>`
//! for `--discard`), `env <key> <pid> <its environment variables' names>`,
//! `fds <key> <pid> <inherited fds above 2, or ->`, `first-line-refused <key> <pid>`,
//! `eof-before-first-line <key> <pid>`, `eof-exit <key> <pid>`, `eof-ignored <key> <pid>`, and
//! `leak <key> <pid> <where>` if something that looks like a secret (64 lowercase hex digits)
//! reached it where none may: its `argv` or an `env:<NAME>` variable.

use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::io::{self, BufRead, BufReader, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use org_office_launch::writer::{marker, Admission};
use org_office_launch::{
    office_dir, FailCode, FirstLine, OfficeFile, ServerEvent, CREATE_FLAG, DISCARD_FLAG,
    OFFICE_FILE_NAME, OFFICE_FLAG, OFFICE_LOCK_FILE_NAME, OFFICE_LOCK_WAIT,
};
use org_office_launch::{Handoff, HostFirstLine, RendezvousEvent, TransferRecord};
use org_proto::frame;
use org_proto::location::{selected_office_socket_in, OFFICE_HOME_ENV, OFFICE_RUNTIME_DIR_ENV};
use org_proto::OfficeRole;
use org_proto::{
    ApiError, ErrorCode, Hello, HelloResult, OfficeHello, OfficeInfo, OfficeSecretsKey,
    OfficeStatus, Request, Response,
};
use org_socket::rights::{DatagramReceiver, ReceiveError, StreamReceiver};
use serde::Serialize;
use serde_json::Value;

/// The fake's stand-in for an office's databases, in its folder.
const DB_FILE_NAME: &str = "db";

const CRASH: i32 = 3;

/// The rendezvous stream, once connected: every event goes here.
static RENDEZVOUS: OnceLock<Mutex<UnixStream>> = OnceLock::new();
/// The launch transfer socket, held for the process's life (read only for the writer control).
static TRANSFER: OnceLock<OwnedFd> = OnceLock::new();
/// The host's writer-custody channel, held (never written) for the process's life.
static WRITER: OnceLock<UnixStream> = OnceLock::new();
/// The organization and owner member the host's writer admission named: a create checks its
/// line's, a start its stored office's organization.
static ADMITTED: OnceLock<(String, String)> = OnceLock::new();

/// A first line and its handoff, the secrets noted as fingerprints only.
enum Launch {
    Create(CreateFields),
    Start,
    /// A discard's handoff carries no secret: the databases are plain SQLite, so its folder is
    /// removed whatever it holds.
    Discard,
}

/// What a create line names.
struct CreateFields {
    office_id: String,
    org_id: String,
    owner_member_id: String,
    org_name: String,
    office_name: String,
    role: OfficeRole,
}

struct Step {
    action: String,
    code: String,
    at: String,
    ignore_eof: bool,
    stderr_bytes: u64,
    delay: Duration,
    report: Value,
    linger: bool,
    corrupt_office_file: bool,
    orphan_stdout_ms: Option<u64>,
    ready_delay: Duration,
    stdin: Option<String>,
}

fn main() {
    // First, before this process opens anything: what it inherited beyond stdin, stdout and
    // stderr (the host must give it nothing else).
    let inherited = inherited_fds();
    let home = match std::env::var_os(OFFICE_HOME_ENV) {
        Some(home) if Path::new(&home).is_absolute() => PathBuf::from(home),
        _ => exit(2),
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (office_id, discard) = match args.as_slice() {
        [flag] if flag == CREATE_FLAG => (None, false),
        [flag, id] if flag == OFFICE_FLAG => (Some(id.clone()), false),
        [flag, id] if flag == DISCARD_FLAG => (Some(id.clone()), true),
        _ => fail(FailCode::InvalidRequest),
    };
    let key = match (&office_id, discard) {
        (_, true) => "discard".to_owned(),
        (Some(id), false) => id.clone(),
        (None, false) => "create".to_owned(),
    };
    let run = bump_run(&home, &key);
    record(
        &home,
        &format!("fds {key} {} {inherited}", std::process::id()),
    );
    check_no_token_in_argv_or_env(&home, &key);
    record_env_names(&home, &key);
    let step = step(&home, &key, run);
    if let Some(ms) = step.orphan_stdout_ms {
        orphan_stdout(&home, &key, ms);
    }
    record(
        &home,
        &format!(
            "start {key} {} {run} {}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ),
    );
    if step.stderr_bytes > 0 {
        // Not a hex digit, so the tests' check for leaked tokens can't mistake it for one.
        let chunk = vec![b'z'; 4096];
        let mut left = step.stderr_bytes as usize;
        while left > 0 {
            let n = left.min(chunk.len());
            let _ = io::stderr().write_all(&chunk[..n]);
            left -= n;
        }
    }

    if let Some(text) = &step.stdin {
        replace_stdin(text);
    }
    let mut stdin = BufReader::new(io::stdin());
    if discard {
        discard_office(
            &home,
            office_id.as_deref().unwrap_or(""),
            &step,
            &key,
            &mut stdin,
        );
    }
    // The socket is the requested office's, even when `report` makes the stored office claim
    // another id: a refused create must not replace another office's live socket.
    let (office, lock, socket_id) = match office_id {
        None => {
            let create = match first_line(&home, &key, &mut stdin, FirstLine::Create, None) {
                Launch::Create(create) => create,
                _ => refuse_first_line(&home, &key, FailCode::InvalidRequest),
            };
            // As the real server, after the custody receipt: the admission is for this
            // organization and this owner.
            if ADMITTED.get() != Some(&(create.org_id.clone(), create.owner_member_id.clone())) {
                fail(FailCode::StorageError);
            }
            let create_id = create.office_id.clone();
            let dir = office_dir(&home, &create.office_id).unwrap_or_else(|| {
                fail(FailCode::InvalidRequest);
            });
            match fs::DirBuilder::new().mode(0o700).create(&dir) {
                Ok(()) => {}
                // The folder the host reserved custody in: only its marker may be there.
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists && only_custody(&dir) => {}
                Err(_) => fail(FailCode::AlreadyExists),
            }
            let lock = take_lock(&dir);
            if fs::write(dir.join(DB_FILE_NAME), b"the fake's databases").is_err() {
                fail(FailCode::StorageError);
            }
            let reported = |field: &str, sent: String| {
                step.report
                    .get(field)
                    .and_then(Value::as_str)
                    .map_or(sent, str::to_owned)
            };
            let role = step
                .report
                .get("role")
                .and_then(|role| serde_json::from_value(role.clone()).ok())
                .unwrap_or(create.role);
            let office = OfficeFile::selected(
                reported("office_id", create.office_id),
                reported("office_name", create.office_name),
                role,
                reported("org_id", create.org_id),
                reported("org_name", create.org_name),
                now_rfc3339(),
            )
            // As the real server, a marker it can't write is a storage error.
            .unwrap_or_else(|_| fail(FailCode::StorageError));
            act_before(&step, &home, &key, &mut stdin);
            if step.corrupt_office_file {
                if fs::write(dir.join(OFFICE_FILE_NAME), b"{not an office").is_err() {
                    fail(FailCode::StorageError);
                }
            } else {
                write_office_file(&dir, &office);
            }
            if step.at == "stored" {
                match step.action.as_str() {
                    "crash" => exit(CRASH),
                    "fail" => fail(step_code(&step)),
                    "hang" => {
                        wait_for_eof(&step, &home, &key, &mut stdin);
                        exit(0);
                    }
                    _ => {}
                }
            }
            emit(&ServerEvent::Created {
                office: office.clone(),
            });
            if step.action == "crash" && step.at == "created" {
                exit(CRASH);
            }
            let socket_id = create_id;
            (office, lock, socket_id)
        }
        Some(id) => {
            match first_line(&home, &key, &mut stdin, FirstLine::Start, Some(&id)) {
                Launch::Start => {}
                _ => refuse_first_line(&home, &key, FailCode::InvalidRequest),
            }
            let dir = office_dir(&home, &id).unwrap_or_else(|| fail(FailCode::InvalidRequest));
            // As the real server: an office that isn't there is an invalid start (fatal),
            // decided under its lock.
            if !dir.is_dir() {
                fail(FailCode::InvalidRequest);
            }
            let lock = take_lock(&dir);
            if !dir.join(OFFICE_FILE_NAME).exists() {
                fail(FailCode::InvalidRequest);
            }
            let bytes = fs::read(dir.join(OFFICE_FILE_NAME))
                .unwrap_or_else(|_| fail(FailCode::StorageError));
            let office = OfficeFile::from_slice(&bytes).unwrap_or_else(|e| fail(e.fail_code()));
            // As the real server: the admission is for the stored office's organization.
            if ADMITTED.get().map(|(org, _)| org) != Some(&office.org_id) {
                fail(FailCode::StorageError);
            }
            act_before(&step, &home, &key, &mut stdin);
            let socket_id = office.office_id.clone();
            (office, lock, socket_id)
        }
    };

    // Every office the host starts is named by UUIDv7: its socket is the selected one.
    let runtime =
        std::env::var_os(OFFICE_RUNTIME_DIR_ENV).map_or_else(|| home.join("run"), PathBuf::from);
    let socket = selected_office_socket_in(&runtime, &socket_id)
        .unwrap_or_else(|| fail(FailCode::InvalidRequest));
    let listener = bind(&socket);
    let served = office.clone();
    thread::spawn(move || serve(&listener, &served));
    thread::sleep(step.ready_delay);
    emit(&ServerEvent::Ready {
        socket: socket.to_string_lossy().into_owned(),
        pid: std::process::id(),
    });
    if step.action == "crash" && step.at == "ready" {
        exit(CRASH);
    }
    wait_for_eof(&step, &home, &key, &mut stdin);
    let _ = fs::remove_file(&socket);
    drop(lock);
    exit(0);
}

/// Makes `text` this process's whole standard input (`stdin` in the script): a pipe that
/// holds `text` and has no writer left replaces descriptor 0. The host's pipe moves to
/// another descriptor and stays open, never read, for the life of the process.
fn replace_stdin(text: &str) {
    let mut ends = [0 as libc::c_int; 2];
    // SAFETY: `pipe` writes two descriptors into the array it is given.
    if unsafe { libc::pipe(ends.as_mut_ptr()) } != 0 {
        exit(2);
    }
    let [read_end, write_end] = ends;
    // A first line is far below a pipe's capacity, so the write cannot block.
    // SAFETY: `write_end` is a pipe this function made, and `text` is valid for its length.
    let written = unsafe { libc::write(write_end, text.as_ptr().cast(), text.len()) };
    // SAFETY: descriptors this function owns; 0 is this process's standard input. `dup`
    // keeps the host's pipe open on a new descriptor before 0 is replaced.
    unsafe {
        libc::close(write_end);
        if written != text.len() as isize
            || libc::dup(0) < 0
            || libc::dup2(read_end, 0) != 0
            || libc::close(read_end) != 0
        {
            exit(2);
        }
    }
}

/// Records the names of the environment variables it got: `env <key> <pid> <names,sorted>`.
fn record_env_names(home: &Path, key: &str) {
    let mut names: Vec<String> = std::env::vars_os()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    names.sort();
    record(
        home,
        &format!("env {key} {} {}", std::process::id(), names.join(",")),
    );
}

/// Records a leak if a value looks like a token (64 lowercase hex digits in a row): `argv` for
/// an argument, `env:<NAME>` for an environment variable (its name only, never its value).
fn check_no_token_in_argv_or_env(home: &Path, key: &str) {
    let pid = std::process::id();
    if std::env::args_os().any(|arg| has_hex_run(&arg)) {
        record(home, &format!("leak {key} {pid} argv"));
    }
    for (name, value) in std::env::vars_os() {
        if has_hex_run(&value) {
            record(
                home,
                &format!("leak {key} {pid} env:{}", name.to_string_lossy()),
            );
        }
    }
}

fn has_hex_run(value: &std::ffi::OsStr) -> bool {
    let mut run = 0;
    value.as_encoded_bytes().iter().any(|b| {
        run = if matches!(b, b'0'..=b'9' | b'a'..=b'f') {
            run + 1
        } else {
            0
        };
        run >= 64
    })
}

/// The open fds from 3 to 255, comma-separated (`-` for none).
fn inherited_fds() -> String {
    let open: Vec<String> = (3..256)
        // SAFETY: F_GETFD only reads an fd's flags; it fails harmlessly for a closed fd.
        .filter(|&fd| unsafe { libc::fcntl(fd, libc::F_GETFD) } >= 0)
        .map(|fd| fd.to_string())
        .collect();
    if open.is_empty() {
        "-".to_owned()
    } else {
        open.join(",")
    }
}

/// The first stdin line, checked against the flag (`expected`, and the flag's office id,
/// `flag_id`), then the rendezvous handshake. An unreadable or mismatched line (another
/// version's included) is refused here. EOF before it means the host is gone: exit 0 without a
/// line.
fn first_line(
    home: &Path,
    key: &str,
    stdin: &mut impl BufRead,
    expected: FirstLine,
    flag_id: Option<&str>,
) -> Launch {
    let pid = std::process::id();
    let line = match org_office_launch::read_first_line(stdin, expected) {
        Ok(Some(line)) => line,
        Ok(None) => {
            record(home, &format!("eof-before-first-line {key} {pid}"));
            exit(0);
        }
        Err(e) => refuse_first_line(home, key, e.fail_code()),
    };
    record(home, &format!("launch {key} {pid} v1"));
    let Some(office) = line.office() else {
        refuse_first_line(home, key, FailCode::InvalidRequest)
    };
    record(
        home,
        &format!("instance {key} {pid} {}", office.instance_id),
    );
    // As the real server: the line's office must be the flag's, before connecting.
    if flag_id.is_some_and(|id| office.office_id != id) {
        refuse_first_line(home, key, FailCode::InvalidRequest)
    }
    let handoff = rendezvous(home, key, &line, &office.rendezvous);
    match (line, handoff) {
        (
            HostFirstLine::Create(create),
            Handoff::Create {
                office_secrets_key,
                grant_key,
                ..
            },
        ) => {
            note_office_secrets_key(home, key, Some(&office_secrets_key), "");
            note_grant(home, key, &grant_key);
            record(home, &format!("recovery {key} {pid} present"));
            Launch::Create(CreateFields {
                office_id: create.office.office_id,
                org_id: create.org_id,
                owner_member_id: create.owner_member_id,
                org_name: create.org_name,
                office_name: create.office_name,
                role: create.role,
            })
        }
        (
            HostFirstLine::Start(_),
            Handoff::Start {
                office_secrets_key,
                grant_key,
                ..
            },
        ) => {
            note_office_secrets_key(home, key, office_secrets_key.as_ref(), "");
            note_grant(home, key, &grant_key);
            Launch::Start
        }
        (HostFirstLine::Discard(_), Handoff::Discard { .. }) => Launch::Discard,
        _ => refuse_first_line(home, key, FailCode::InvalidRequest),
    }
}

/// The launch handshake on the rendezvous socket at `path`, as the real server does it. Any fault
/// fails the run (`storage_error`; `invalid_request` for a bad header, a fatal receive, or a
/// header whose op isn't `line`'s). As in the real server, events go on the stream as soon as it is connected, so a
/// failure during the handshake reaches the host there, never on stdout.
fn rendezvous(home: &Path, key: &str, line: &HostFirstLine, path: &str) -> Handoff {
    let mut stream = UnixStream::connect(path).unwrap_or_else(|_| fail(FailCode::StorageError));
    let events = stream
        .try_clone()
        .unwrap_or_else(|_| fail(FailCode::StorageError));
    let _ = RENDEZVOUS.set(Mutex::new(events));
    if frame::write_message(&mut stream, &org_office_launch::ReadyForSecrets).is_err() {
        bail(&stream, FailCode::StorageError);
    }
    let fence = Mutex::new(());
    let mut receiver = StreamReceiver::new(
        stream
            .try_clone()
            .unwrap_or_else(|_| bail(&stream, FailCode::StorageError)),
    );
    // Reserved once, so a header holding keys never reallocates; zeroed after the decode.
    let mut header = Vec::with_capacity(org_office_launch::TRANSFER_HEADER_MAX_BYTES + 1);
    loop {
        let (byte, fds) = receive(&stream, || receiver.receive_byte(0, &fence));
        if !fds.is_empty() {
            bail(&stream, FailCode::InvalidRequest);
        }
        if byte == b'\n' {
            break;
        }
        if header.len() == org_office_launch::TRANSFER_HEADER_MAX_BYTES {
            zero(&mut header);
            bail(&stream, FailCode::InvalidRequest);
        }
        header.push(byte);
    }
    let decoded = org_office_launch::decode_handoff(&header);
    zero(&mut header);
    let handoff = decoded.unwrap_or_else(|_| bail(&stream, FailCode::InvalidRequest));
    // The op check comes right after the decode, before any marker or receipt, as in the real
    // server: a mismatched handoff never gets its descriptors acknowledged.
    if !matches!(
        (line, &handoff),
        (HostFirstLine::Create(_), Handoff::Create { .. })
            | (HostFirstLine::Start(_), Handoff::Start { .. })
            | (HostFirstLine::Discard(_), Handoff::Discard { .. })
    ) {
        record(
            home,
            &format!("handoff-op-refused {key} {}", std::process::id()),
        );
        bail(&stream, FailCode::InvalidRequest);
    }
    if !handoff.fds().is_empty() {
        let mut fds = receive(&stream, || {
            receiver.receive_marker(handoff.fds().len(), org_office_launch::FD_MARKER, &fence)
        });
        let Some(transfer) = fds.pop() else {
            bail(&stream, FailCode::StorageError)
        };
        let _ = TRANSFER.set(transfer);
        record(
            home,
            &format!("transfer {key} {} received", std::process::id()),
        );
        let receipt = RendezvousEvent::FdsReceived {
            of: "handoff".into(),
        };
        if org_office_launch::write_rendezvous_event(&mut stream, &receipt).is_err() {
            bail(&stream, FailCode::StorageError);
        }
        writer_control(home, key, line, &stream, &fence);
    }
    // The receiver set the shared descriptor non-blocking. With it gone, the stream blocks
    // again, so events are written whole and the lifetime read waits for the host's end.
    drop(receiver);
    if stream.set_nonblocking(false).is_err() {
        fail(FailCode::StorageError);
    }
    handoff
}

/// Fails the run during the handshake: the receiver may have left the shared descriptor
/// non-blocking, so it blocks again first and the `failed` event is written whole.
fn bail(stream: &UnixStream, code: FailCode) -> ! {
    let _ = stream.set_nonblocking(false);
    fail(code)
}

/// The host's writer control, the first record on the transfer socket after the handoff, as the
/// real server takes it: the `writer_control` record naming this office with one descriptor,
/// the host's admission first on that channel (valid, for this instance and office; the
/// admission's organization and owner are checked after the receipt, in `main`), then
/// `fds_received` for the channel. Anything else fails the run with `storage_error`, as the
/// real server fails it (custody that needs recovery; the host retries it): an office without
/// custody never opens.
fn writer_control(
    home: &Path,
    key: &str,
    line: &HostFirstLine,
    stream: &UnixStream,
    fence: &Mutex<()>,
) {
    let Some(office) = line.office() else {
        bail(stream, FailCode::InvalidRequest)
    };
    let transfer = TRANSFER
        .get()
        .and_then(|fd| fd.try_clone().ok())
        .unwrap_or_else(|| bail(stream, FailCode::StorageError));
    let mut receiver = DatagramReceiver::new(UnixDatagram::from(transfer));
    let mut payload = [0; org_office_launch::TRANSFER_HEADER_MAX_BYTES];
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut received = loop {
        match receiver.receive_up_to(&mut payload, 1, fence) {
            Ok(received) => break received,
            Err(ReceiveError::WouldBlock) if Instant::now() < deadline => {
                let fd = receiver
                    .fd()
                    .unwrap_or_else(|| bail(stream, FailCode::StorageError));
                match org_socket::poll_readable(fd.as_raw_fd(), org_socket::READ_TICK) {
                    Ok(_) => {}
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => bail(stream, FailCode::StorageError),
                }
            }
            Err(_) => bail(stream, FailCode::StorageError),
        }
    };
    let Ok(TransferRecord::WriterControl {
        office_id,
        channel_id,
    }) = org_office_launch::decode_transfer_record(&payload[..received.bytes])
    else {
        bail(stream, FailCode::StorageError)
    };
    let Some(descriptor) = received.descriptors.pop() else {
        bail(stream, FailCode::StorageError)
    };
    if office_id != office.office_id || !received.descriptors.is_empty() {
        bail(stream, FailCode::StorageError);
    }
    let writer = org_socket::rights::connected_unix_stream(descriptor)
        .unwrap_or_else(|_| bail(stream, FailCode::StorageError));
    if writer.set_nonblocking(false).is_err()
        || writer
            .set_read_timeout(Some(Duration::from_secs(5)))
            .is_err()
    {
        bail(stream, FailCode::StorageError);
    }
    let mut reader = BufReader::new(
        writer
            .try_clone()
            .unwrap_or_else(|_| bail(stream, FailCode::StorageError)),
    );
    let admission = match org_office_launch::writer::read::<Admission>(&mut reader) {
        Ok(Some(admission)) => admission,
        _ => bail(stream, FailCode::StorageError),
    };
    // Only the admission's own rules and its scope here, as the real server's install: the
    // organization and owner are compared after the receipt (`main`).
    if !admission.check()
        || admission.identity.instance_id != office.instance_id
        || admission.identity.office_id != office.office_id
    {
        bail(stream, FailCode::StorageError);
    }
    let _ = WRITER.set(writer);
    let _ = ADMITTED.set((
        admission.identity.organization_id.clone(),
        admission.owner_member.clone(),
    ));
    record(
        home,
        &format!(
            "writer {key} {} {}",
            std::process::id(),
            admission.identity.organization_id
        ),
    );
    let mut out = stream;
    let receipt = RendezvousEvent::FdsReceived { of: channel_id };
    if org_office_launch::write_rendezvous_event(&mut out, &receipt).is_err() {
        bail(stream, FailCode::StorageError);
    }
}

/// Whether `dir` holds nothing but the host's custody marker (or its staging files).
fn only_custody(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|entries| {
        entries
            .flatten()
            .all(|entry| marker::is_custody_entry(&entry.file_name().to_string_lossy()))
    })
}

/// One receive on the (non-blocking) rendezvous, waiting for readiness while it would block,
/// as the real server does. A fatal receive fails the run: the process must exit then.
fn receive<T>(stream: &UnixStream, mut call: impl FnMut() -> Result<T, ReceiveError>) -> T {
    loop {
        match call() {
            Ok(value) => return value,
            Err(ReceiveError::WouldBlock) => {
                match org_socket::poll_readable(stream.as_raw_fd(), org_socket::READ_TICK) {
                    Ok(_) => {}
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => bail(stream, FailCode::StorageError),
                }
            }
            // As the real server: a fatal receive (a descriptor on a header byte, a wrong
            // marker, the stream ending mid-header) is an invalid handoff.
            Err(ReceiveError::Fatal(_)) => bail(stream, FailCode::InvalidRequest),
        }
    }
}

fn zero(bytes: &mut [u8]) {
    for b in bytes.iter_mut() {
        // SAFETY: `b` is a valid, aligned, exclusive reference to a u8.
        unsafe { std::ptr::write_volatile(b, 0) };
    }
}

/// Records the grant key's fingerprint (never the key) in `fake.log`.
fn note_grant(home: &Path, key: &str, grant: &org_proto::GrantKey) {
    record(
        home,
        &format!("grant {key} {} {}", std::process::id(), fingerprint(grant)),
    );
}

fn refuse_first_line(home: &Path, key: &str, code: FailCode) -> ! {
    record(
        home,
        &format!("first-line-refused {key} {}", std::process::id()),
    );
    fail(code)
}

/// Records the office secrets key's fingerprint (never the key), or `null` (a start without
/// one, and every discard), in `fake.log`. `more` is appended (the office id, for a discard).
fn note_office_secrets_key(
    home: &Path,
    key: &str,
    office_secrets_key: Option<&OfficeSecretsKey>,
    more: &str,
) {
    let fingerprint = office_secrets_key.map_or_else(|| "null".to_owned(), fingerprint);
    record(
        home,
        format!(
            "secrets-key {key} {} {fingerprint} {more}",
            std::process::id()
        )
        .trim_end(),
    );
}

/// The 64-bit FNV-1a hash of a secret's 32 bytes, as 16 hex digits: tells secrets apart
/// without revealing them. Its only way out is its serialization (64 hex digits), decoded here
/// from a buffer that is zeroed afterwards.
fn fingerprint(secret: &impl Serialize) -> String {
    let mut json = Vec::with_capacity(128); // big enough never to reallocate
    serde_json::to_writer(&mut json, secret).expect("a secret serializes");
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for pair in json[1..json.len() - 1].chunks(2) {
        let byte = (nibble(pair[0]) << 4) | nibble(pair[1]);
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    for b in json.iter_mut() {
        // SAFETY: `b` is a valid, aligned, exclusive reference to a u8.
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    format!("{hash:016x}")
}

fn nibble(hex: u8) -> u8 {
    match hex {
        b'0'..=b'9' => hex - b'0',
        _ => hex - b'a' + 10,
    }
}

/// `--discard <id>`: removes an unfinished office's folder, then exits. It needs no key.
fn discard_office(home: &Path, id: &str, step: &Step, key: &str, stdin: &mut impl BufRead) -> ! {
    if !matches!(
        first_line(home, key, stdin, FirstLine::Discard, Some(id)),
        Launch::Discard
    ) {
        refuse_first_line(home, key, FailCode::InvalidRequest)
    }
    note_office_secrets_key(home, key, None, id);
    let dir = office_dir(home, id).unwrap_or_else(|| fail(FailCode::InvalidRequest));
    act_before(step, home, key, stdin);
    if dir.exists() {
        // As the real server: a folder the host reserved custody in is never discarded,
        // whatever else it holds, with the same fixed storage error.
        if marker::holds_custody(&dir).unwrap_or(true) {
            fail(FailCode::StorageError);
        }
        let lock = take_lock(&dir);
        if dir.join(OFFICE_FILE_NAME).exists() {
            fail(FailCode::AlreadyExists);
        }
        if fs::remove_dir_all(&dir).is_err() {
            fail(FailCode::StorageError);
        }
        drop(lock);
    }
    emit(&ServerEvent::Discarded);
    exit(0);
}

/// What happens before `created` (create) or `ready` (office): fail, crash, hang or delay.
fn act_before(step: &Step, home: &Path, key: &str, stdin: &mut impl BufRead) {
    match step.action.as_str() {
        "fail" if step.at != "stored" => {
            let code = step_code(step);
            if step.linger {
                emit(&ServerEvent::Failed {
                    code,
                    message: "the fake server was told to fail and linger".into(),
                });
                loop {
                    thread::sleep(Duration::from_secs(3600));
                }
            }
            fail(code);
        }
        "crash" if step.at == "start" => exit(CRASH),
        "hang" if step.at != "stored" => {
            wait_for_eof(step, home, key, stdin);
            exit(0);
        }
        _ => thread::sleep(step.delay),
    }
}

/// Blocks until stdin closes, then returns, unless the step ignores EOF.
fn wait_for_eof(step: &Step, home: &Path, key: &str, stdin: &mut impl BufRead) {
    let stream = RENDEZVOUS
        .get()
        .and_then(|stream| stream.lock().ok()?.try_clone().ok());
    match stream {
        None => {
            let mut sink = Vec::new();
            let _ = stdin.read_to_end(&mut sink);
        }
        // The host is gone when either its stdin or the rendezvous stream ends.
        Some(mut stream) => {
            let (ended, eof) = mpsc::channel();
            let stdin_ended = ended.clone();
            thread::spawn(move || {
                let _ = io::stdin().lock().read_to_end(&mut Vec::new());
                let _ = stdin_ended.send(());
            });
            thread::spawn(move || {
                let _ = stream.read_to_end(&mut Vec::new());
                let _ = ended.send(());
            });
            let _ = eof.recv();
        }
    }
    if step.ignore_eof {
        record(home, &format!("eof-ignored {key} {}", std::process::id()));
        loop {
            thread::sleep(Duration::from_secs(3600));
        }
    }
    record(home, &format!("eof-exit {key} {}", std::process::id()));
}

fn fail(code: FailCode) -> ! {
    emit(&ServerEvent::Failed {
        code,
        message: "the fake server was told to fail".into(),
    });
    exit(1);
}

/// Writes an event to the host on the rendezvous stream. Before it is connected there is nowhere
/// to report, as for the real server: nothing is written (stdout carries nothing).
fn emit(event: &ServerEvent) {
    let written = match RENDEZVOUS.get() {
        Some(stream) => {
            let mut stream = stream.lock().unwrap_or_else(|p| p.into_inner());
            org_office_launch::write_rendezvous_event(
                &mut *stream,
                &RendezvousEvent::Server(event.clone()),
            )
            .is_ok()
        }
        None => true,
    };
    if !written {
        exit(4);
    }
}

/// Holds the office lock like a real server: waits up to `OFFICE_LOCK_WAIT`, then `in_use`.
fn take_lock(dir: &Path) -> File {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(OFFICE_LOCK_FILE_NAME))
        .unwrap_or_else(|_| fail(FailCode::StorageError));
    let deadline = Instant::now() + OFFICE_LOCK_WAIT;
    while file.try_lock().is_err() {
        if Instant::now() > deadline {
            fail(FailCode::InUse);
        }
        thread::sleep(Duration::from_millis(20));
    }
    file
}

/// The step's `code` (an unknown one is `storage_error`).
fn step_code(step: &Step) -> FailCode {
    serde_json::from_value(Value::String(step.code.clone())).unwrap_or(FailCode::StorageError)
}

fn write_office_file(dir: &Path, office: &OfficeFile) {
    let tmp = dir.join("office.json.tmp");
    let bytes = office
        .to_vec()
        .unwrap_or_else(|_| fail(FailCode::InvalidRequest));
    let written = File::create(&tmp)
        .and_then(|mut f| f.write_all(&bytes).and_then(|()| f.sync_all()))
        .and_then(|()| fs::rename(&tmp, dir.join(OFFICE_FILE_NAME)));
    if written.is_err() {
        fail(FailCode::StorageError);
    }
}

fn bind(path: &Path) -> UnixListener {
    let _ = fs::remove_file(path);
    let listener = UnixListener::bind(path).unwrap_or_else(|_| fail(FailCode::StorageError));
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    listener
}

/// Answers `hello` with this office's identity; any other method is unknown.
fn serve(listener: &UnixListener, office: &OfficeFile) {
    for stream in listener.incoming().flatten() {
        let office = office.clone();
        thread::spawn(move || serve_connection(stream, &office));
    }
}

fn serve_connection(stream: UnixStream, office: &OfficeFile) {
    let Ok(mut out) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    while let Ok(Some(request)) = frame::read_message::<Request>(&mut reader) {
        let response = if request.is::<Hello>() {
            let result = HelloResult::office(
                "fake",
                "This Mac",
                OfficeHello {
                    organization_name: office.org_name.clone(),
                    office: OfficeInfo {
                        id: office.office_id.clone(),
                        organization_id: office.org_id.clone(),
                        name: office.office_name.clone(),
                        role: office.role,
                        host: "This Mac".into(),
                        endpoint: None,
                        status: OfficeStatus::Active,
                        status_reason: None,
                        next_restart_at: None,
                    },
                },
            );
            Response::ok::<Hello>(request.id, &result).expect("serializes")
        } else {
            Response::error(
                request.id,
                ApiError::new(ErrorCode::UnknownMethod, "the fake only knows hello"),
            )
        };
        if frame::write_message(&mut out, &response).is_err() {
            return;
        }
    }
}

/// Counts this key's runs (1 for the first), under an exclusive lock on its counter file.
fn bump_run(home: &Path, key: &str) -> u64 {
    let dir = home.join("fake-runs");
    let _ = fs::create_dir_all(&dir);
    let Ok(mut file) = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(dir.join(key))
    else {
        return 1;
    };
    let _ = file.lock();
    let _ = file.write_all(b".");
    file.metadata().map(|m| m.len()).unwrap_or(1)
}

fn step(home: &Path, key: &str, run: u64) -> Step {
    let script: Value = fs::read(home.join("fake-script.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(Value::Null);
    let entry = match script.get(key) {
        Some(Value::Array(steps)) if !steps.is_empty() => {
            let index = usize::try_from(run.saturating_sub(1)).unwrap_or(usize::MAX);
            steps[index.min(steps.len() - 1)].clone()
        }
        Some(step) => step.clone(),
        None => Value::Null,
    };
    let text = |name: &str, default: &str| {
        entry
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or(default)
            .to_owned()
    };
    Step {
        action: text("do", "ready"),
        code: text("code", "storage_error"),
        at: text("at", "start"),
        ignore_eof: entry
            .get("ignore_eof")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        stderr_bytes: entry
            .get("stderr_bytes")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        delay: Duration::from_millis(entry.get("delay_ms").and_then(Value::as_u64).unwrap_or(0)),
        report: entry.get("report").cloned().unwrap_or(Value::Null),
        linger: entry
            .get("linger")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        corrupt_office_file: entry
            .get("corrupt_office_file")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        orphan_stdout_ms: entry.get("orphan_stdout_ms").and_then(Value::as_u64),
        ready_delay: Duration::from_millis(
            entry
                .get("ready_delay_ms")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        ),
        stdin: entry
            .get("stdin")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

/// Starts a `sleep` of `ms` that inherits (and so holds open) the fake's stdout.
fn orphan_stdout(home: &Path, key: &str, ms: u64) {
    let child = std::process::Command::new("/bin/sleep")
        .arg(format!("{}.{:03}", ms / 1000, ms % 1000))
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if let Ok(child) = child {
        record(
            home,
            &format!("orphan {key} {} {}", std::process::id(), child.id()),
        );
    }
}

fn record(home: &Path, line: &str) {
    if let Ok(mut file) = OpenOptions::new()
        .append(true)
        .create(true)
        .open(home.join("fake.log"))
    {
        let _ = file.write_all(format!("{line}\n").as_bytes());
    }
}

/// Now as RFC 3339 UTC with milliseconds, e.g. `2026-11-08T02:10:00.000Z`.
fn now_rfc3339() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let (secs, millis) = (now.as_secs(), now.subsec_millis());
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Howard Hinnant's civil-from-days.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}
