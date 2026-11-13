//! A fake `org-server` for `org-office-host`'s tests. **Never shipped:** it is built
//! only with the `test-support` feature, which only the host's own tests turn on.
//!
//! It follows the launch protocol (`org-office-launch` v3: the first line, then the rendezvous,
//! `ready_for_secrets`, the handoff read with `recvmsg` and its transfer descriptor, and
//! `fds_received`) without SQLCipher or a key store, and does what the test scripted. Its events
//! go over the rendezvous, and its lifetime is the rendezvous: it ends at its EOF. The script is `<ORG_OFFICE_HOME>/fake-script.json`:
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
//! - `ignore_eof`: on rendezvous EOF, keep running (until SIGKILLed) instead of exiting 0.
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
//!   differ from the create message: `office_id`, `org_id`, `org_name`, `office_name` or `role`.
//! - `list_dir`: first record this folder's entry names in `<home>/listed-<key>`.
//! - `forge_stdout`: right after the handoff, write `created` (a create, for the office asked
//!   for) or `discarded` (a discard) on stdout, which a v3 host must ignore.
//!
//! Its first stdin line must match its flag, as the contract says: a `create` message with
//! `--create`, a `start` message with `--office`, a `discard` message with `--discard`. A
//! malformed or other line is refused with `failed: invalid_request` (a broken pipe with
//! `storage_error`), before any scripted step (for `--discard`, before its step too). EOF before
//! the first line means the host is gone: it exits 0 without a line.
//! It never logs a secret: for the grant key and the database key it records only a fingerprint
//! (the 64-bit FNV-1a hash of the 32 bytes, as 16 hex digits), in `fake.log` and, unless
//! `stderr_bytes` is set, the grant key's on stderr (so in the office's log), so tests can tell
//! secrets apart without seeing them. Of a create's recovery code it records only that it came.
//!
//! On `--create` it makes the office folder and holds `office.lock` before anything else (so a
//! failure or crash leaves an unfinished folder, like a real server could), then writes a `db`
//! file (its stand-in for the databases, holding no secret), and writes `office.json` last.
//! `ready` binds a real socket that answers `hello`. `--discard <id>` takes the office lock
//! (with the usual wait), refuses a folder holding `office.json` (`already_exists`), refuses a
//! folder with a `db` file when its line has no key (`storage_error`, as a real server can't
//! check databases without their key), removes the folder and reports `discarded` (`fail`,
//! `crash` and `hang` work there too).
//!
//! It records what it does in `<home>/fake.log`, one line each:
//! `start <key> <pid> <run> <unix ms>`, `grant <key> <pid> <fingerprint>`,
//! `line <key> <pid> <instance_id> <owner_member_id or ->`, `transfer <key> <pid> <fds>`,
//! `recovery <key> <pid>`,
//! `dbkey <key> <pid> <fingerprint or null>` (`dbkey discard <pid> <fingerprint or null> <id>`
//! for `--discard`), `env <key> <pid> <its environment variables' names>`,
//! `fds <key> <pid> <inherited fds above 2, or ->`, `first-line-refused <key> <pid>`,
//! `eof-before-first-line <key> <pid>`, `eof-exit <key> <pid>`, `eof-ignored <key> <pid>`, and
//! `leak <key> <pid> <where>` if something that looks like a secret (64 lowercase hex digits)
//! reached it where none may: its `argv` or an `env:<NAME>` variable.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use org_office_launch::{
    office_dir, write_server_event, FailCode, FirstLine, OfficeFile, ServerEvent, CREATE_FLAG,
    DISCARD_FLAG, OFFICE_FILE_NAME, OFFICE_FLAG, OFFICE_LOCK_FILE_NAME, OFFICE_LOCK_WAIT,
};
use org_proto::frame;
use org_proto::location::{
    office_socket_in, office_socket_path, OFFICE_HOME_ENV, OFFICE_RUNTIME_DIR_ENV,
};
use org_proto::{
    ApiError, DbKey, ErrorCode, GrantKey, Hello, HelloResult, OfficeHello, OfficeInfo,
    OfficeStatus, Request, Response,
};
use serde::Serialize;
use serde_json::Value;

use org_office_launch::v3::{self, Handoff, HostFirstLine, RendezvousEvent};
use org_socket::rights::{ReceiveError, StreamReceiver};
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::{Mutex, OnceLock};

/// The rendezvous connection, once the handoff is done: events go here.
static RENDEZVOUS: OnceLock<Mutex<UnixStream>> = OnceLock::new();
/// The transfer descriptor it got, held for its life.
static TRANSFER: OnceLock<Vec<OwnedFd>> = OnceLock::new();

/// The fake's stand-in for an office's databases, in its folder.
const DB_FILE_NAME: &str = "db";

const CRASH: i32 = 3;

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
    list_dir: Option<String>,
    forge_stdout: bool,
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
    if let Some(dir) = &step.list_dir {
        let mut names: Vec<String> = fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        let _ = fs::write(home.join(format!("listed-{key}")), names.join("\n"));
    }
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
            let create = match first_line(&home, &key, &mut stdin, FirstLine::Create) {
                (
                    HostFirstLine::Create(create),
                    Handoff::Create {
                        db_key, grant_key, ..
                    },
                ) => {
                    note_token(&home, &key, &step, &grant_key);
                    note_db_key(&home, &key, Some(&db_key), "");
                    record(&home, &format!("recovery {key} {}", std::process::id()));
                    if step.forge_stdout {
                        let office = OfficeFile::new(
                            create.office.office_id.clone(),
                            create.office_name.clone(),
                            create.role,
                            create.org_id.clone(),
                            create.org_name.clone(),
                            now_rfc3339(),
                        );
                        let _ = write_server_event(
                            &mut io::stdout().lock(),
                            &ServerEvent::Created { office },
                        );
                    }
                    create
                }
                _ => refuse_first_line(&home, &key, FailCode::InvalidRequest),
            };
            let create_id = create.office.office_id.clone();
            let dir = office_dir(&home, &create.office.office_id).unwrap_or_else(|| {
                fail(FailCode::InvalidRequest);
            });
            if fs::DirBuilder::new().mode(0o700).create(&dir).is_err() {
                fail(FailCode::AlreadyExists);
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
            let office = OfficeFile::new(
                reported("office_id", create.office.office_id),
                reported("office_name", create.office_name),
                role,
                reported("org_id", create.org_id),
                reported("org_name", create.org_name),
                now_rfc3339(),
            );
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
            match first_line(&home, &key, &mut stdin, FirstLine::Start) {
                (
                    HostFirstLine::Start(_),
                    Handoff::Start {
                        db_key, grant_key, ..
                    },
                ) => {
                    note_token(&home, &key, &step, &grant_key);
                    note_db_key(&home, &key, Some(&db_key), "");
                }
                _ => refuse_first_line(&home, &key, FailCode::InvalidRequest),
            }
            let dir = office_dir(&home, &id).unwrap_or_else(|| fail(FailCode::InvalidRequest));
            let lock = take_lock(&dir);
            let bytes = fs::read(dir.join(OFFICE_FILE_NAME))
                .unwrap_or_else(|_| fail(FailCode::StorageError));
            let office = OfficeFile::from_slice(&bytes).unwrap_or_else(|e| fail(e.fail_code()));
            act_before(&step, &home, &key, &mut stdin);
            let socket_id = office.office_id.clone();
            (office, lock, socket_id)
        }
    };

    let socket = match std::env::var_os(OFFICE_RUNTIME_DIR_ENV) {
        Some(runtime) => office_socket_in(Path::new(&runtime), &socket_id),
        None => office_socket_path(&home, &socket_id),
    }
    .expect("a plain id");
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

/// The first stdin line, then the rendezvous it names: `ready_for_secrets`, the handoff read a
/// byte at a time with `recvmsg`, its descriptors, and `fds_received`. An unreadable line is
/// refused here; the caller checks that the ops match. EOF before it means the host is gone:
/// exit 0 without a line.
fn first_line(
    home: &Path,
    key: &str,
    stdin: &mut impl BufRead,
    expected: FirstLine,
) -> (HostFirstLine, Handoff) {
    let line = match v3::read_first_line(stdin, expected) {
        Ok(Some(line)) => line,
        Ok(None) => {
            record(
                home,
                &format!("eof-before-first-line {key} {}", std::process::id()),
            );
            exit(0);
        }
        Err(e) => refuse_first_line(home, key, e.fail_code()),
    };
    let office = line.office().expect("a known op");
    let owner = match &line {
        HostFirstLine::Create(create) => create.owner_member_id.as_str(),
        _ => "-",
    };
    record(
        home,
        &format!(
            "line {key} {} {} {owner}",
            std::process::id(),
            office.instance_id
        ),
    );
    let mut stream = UnixStream::connect(&office.rendezvous).unwrap_or_else(|_| exit(5));
    if frame::write_message(&mut stream, &v3::ReadyForSecrets).is_err() {
        exit(5);
    }
    let fence = Mutex::new(());
    let mut receiver = StreamReceiver::new(stream.try_clone().unwrap_or_else(|_| exit(5)));
    let mut receive = |count| loop {
        let _ = org_socket::poll_readable(stream.as_raw_fd(), Duration::from_secs(1));
        match receiver.receive_byte(count, &fence) {
            Ok(got) => return got,
            Err(ReceiveError::WouldBlock) => {}
            Err(ReceiveError::Fatal(_)) => exit(5),
        }
    };
    let mut header = Vec::new();
    loop {
        match receive(0) {
            (b'\n', _) => break,
            (byte, _) => header.push(byte),
        }
    }
    let handoff = v3::decode_handoff(&header).unwrap_or_else(|_| exit(5));
    if !handoff.fds().is_empty() {
        let (marker, fds) = receive(handoff.fds().len());
        if marker != v3::FD_MARKER {
            exit(5);
        }
        record(
            home,
            &format!("transfer {key} {} {}", std::process::id(), fds.len()),
        );
        let _ = TRANSFER.set(fds);
        let received = RendezvousEvent::FdsReceived {
            of: "handoff".into(),
        };
        if v3::write_rendezvous_event(&mut stream, &received).is_err() {
            exit(5);
        }
    }
    let _ = RENDEZVOUS.set(Mutex::new(stream));
    (line, handoff)
}

fn refuse_first_line(home: &Path, key: &str, code: FailCode) -> ! {
    record(
        home,
        &format!("first-line-refused {key} {}", std::process::id()),
    );
    fail(code)
}

/// Records the token's fingerprint (never the token) in `fake.log`, and on stderr unless the
/// step writes its own stderr bytes.
fn note_token(home: &Path, key: &str, step: &Step, token: &GrantKey) {
    let fingerprint = fingerprint(token);
    record(
        home,
        &format!("grant {key} {} {fingerprint}", std::process::id()),
    );
    if step.stderr_bytes == 0 {
        let _ = writeln!(
            io::stderr(),
            "fake office server {key}: got its handoff, grant key fingerprint {fingerprint}"
        );
    }
}

/// Records the database key's fingerprint (never the key), or `null`, in `fake.log`. `more` is
/// appended (the office id, for a discard).
fn note_db_key(home: &Path, key: &str, db_key: Option<&DbKey>, more: &str) {
    let fingerprint = db_key.map_or_else(|| "null".to_owned(), fingerprint);
    record(
        home,
        format!("dbkey {key} {} {fingerprint} {more}", std::process::id()).trim_end(),
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

/// `--discard <id>`: removes an unfinished office's folder, then exits. Without a key it
/// refuses a folder holding databases (`db`).
fn discard_office(home: &Path, id: &str, step: &Step, key: &str, stdin: &mut impl BufRead) -> ! {
    let db_key = match first_line(home, key, stdin, FirstLine::Discard) {
        (HostFirstLine::Discard(_), Handoff::Discard { db_key, .. }) => {
            if step.forge_stdout {
                let _ = write_server_event(&mut io::stdout().lock(), &ServerEvent::Discarded);
            }
            db_key
        }
        _ => refuse_first_line(home, key, FailCode::InvalidRequest),
    };
    note_db_key(home, key, db_key.as_ref(), id);
    let dir = office_dir(home, id).unwrap_or_else(|| fail(FailCode::InvalidRequest));
    act_before(step, home, key, stdin);
    if dir.exists() {
        let lock = take_lock(&dir);
        if dir.join(OFFICE_FILE_NAME).exists() {
            fail(FailCode::AlreadyExists);
        }
        if db_key.is_none() && dir.join(DB_FILE_NAME).exists() {
            fail(FailCode::StorageError);
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

/// Blocks until the rendezvous (or, before it, stdin) closes, then returns, unless the step
/// ignores EOF.
fn wait_for_eof(step: &Step, home: &Path, key: &str, stdin: &mut impl BufRead) {
    let mut sink = Vec::new();
    match RENDEZVOUS
        .get()
        .map(|stream| stream.lock().map(|s| s.try_clone()))
    {
        Some(Ok(Ok(mut stream))) => {
            let _ = io::Read::read_to_end(&mut stream, &mut sink);
        }
        _ => {
            let _ = stdin.read_to_end(&mut sink);
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

/// Writes an event on the rendezvous; before there is one (a refused first line), on stdout,
/// which the host never reads. A rendezvous the host already ended fails the write: the fake
/// then carries on to its EOF wait, which ends it cleanly.
fn emit(event: &ServerEvent) {
    match RENDEZVOUS.get() {
        Some(stream) => {
            if let Ok(mut stream) = stream.lock() {
                let event = RendezvousEvent::Server(event.clone());
                let _ = v3::write_rendezvous_event(&mut *stream, &event);
            }
        }
        None => {
            if write_server_event(&mut io::stdout().lock(), event).is_err() {
                exit(4);
            }
        }
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
        list_dir: entry
            .get("list_dir")
            .and_then(Value::as_str)
            .map(str::to_owned),
        forge_stdout: entry
            .get("forge_stdout")
            .and_then(Value::as_bool)
            .unwrap_or(false),
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
