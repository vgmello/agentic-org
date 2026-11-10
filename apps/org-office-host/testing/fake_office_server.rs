//! A fake `org-office-server` for `org-office-host`'s tests. **Never shipped:** it is built
//! only with the `test-support` feature, which only the host's own tests turn on.
//!
//! It follows the launch protocol (`org-office-launch`, version 3) without SQLCipher or a key
//! store, and does what the test scripted. The script is `<ORG_OFFICE_HOME>/fake-script.json`:
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
//!   differ from the create message: `office_id`, `org_id`, `org_name`, `office_name` or `role`.
//! - `hand_off`: how it takes its secrets after its first line. `connect` (default): as the
//!   contract says. `none`: it never connects (it waits for stdin EOF, then exits 0). `twice`: it
//!   opens two connections at once and reads the hand-off from the first, then records what the
//!   second got. `helper`: it doesn't connect itself; a helper process (another pid, so another
//!   peer) connects and records what it got, and the fake waits for stdin EOF. `impostor`:
//!   another process replaces the key socket with its own and serves a forged line; the fake
//!   connects to it, and must refuse it before reading anything (recorded as
//!   `hand-off-peer-refused <key> <pid>`; the impostor records `impostor-accepted <pid>`).
//!   Three more hold the connection the way the contract forbids, set up before the connect so
//!   the host's peer-fd check sees them however fast it accepts: `dup` (a second close-on-exec
//!   fd on it), `fork` (a forked child holds it too, until the fake exits) and `on_stderr` (the
//!   connection is moved onto fd 2, its only fd). The host must refuse each before writing.
//!   Otherwise its key connection is a socket it creates itself, close-on-exec, on one fd, that it
//!   never duplicates, passes or holds across a fork or spawn, as the contract says.
//! - `peer_check_delay_ms`: wait this long between connecting to the key socket and checking who
//!   its peer is (the host must keep its end open until the fake closes its own).
//!
//! It also runs as `--early-resumer <args>`, for the tests: the pre-exec image of the attack the
//! host's peer-fd check stops (`handoff`). Spawned (through a wrapper) as the server, it makes a
//! socket without close-on-exec, forks a helper that keeps it, and turns itself into the genuine
//! server (this fake, with `<args>`) with `posix_spawn(POSIX_SPAWN_SETEXEC |
//! POSIX_SPAWN_START_SUSPENDED)`, that socket on fd 2 and the environment
//! `OBJC_HELP=YES ORG_FAKE_STOP_AT_MAIN=1`. The helper waits for the key socket, stops the host
//! (`SIGSTOP`), connects the socket to it and shuts down its sending side, then resumes the
//! server: before `main`, libobjc writes its `OBJC_HELP` text to fd 2; the write fails (`EPIPE`)
//! but makes the server the socket's last user, so the host would take the helper's connection
//! for the server's. `ORG_FAKE_STOP_AT_MAIN` makes the server stop itself first thing in `main`
//! (recorded first, `stopped-at-main <pid>`), standing in for the attacker stopping it right after
//! that write (the real server would refuse
//! fd 2 at its fd hygiene and exit). Once it has stopped, the helper resumes the host and records
//! what its connection got: `early-resumer <key> <helper pid> <server pid> <bytes>`.
//!
//! It also runs as `--stdio-probe <1|2> <args>`, for the tests: it runs the fake with one end of a
//! socket pair as fd 1 or 2, and records whether the fake touched it (`stdio-probe <fd> <probe
//! pid> <the socket's last user> <bytes sent> <exit status>`).
//!
//! **Fd hygiene first** (the contract): before any other I/O on any descriptor, it `fstat`s fds
//! 0, 1 and 2. If any isn't a pipe, or a later step fails, it does no I/O on any inherited fd,
//! writes no line, records `fd-hygiene-refused <pid> <why>` in `fake.log` and exits with
//! `EXIT_LAUNCH_DESCRIPTORS_REFUSED` (78). Otherwise it closes every other open fd, listed by the
//! kernel (after recording them, `fds …`), and resets its thread's and its task's exception
//! ports, `EXC_CRASH` included (recorded as `fd-hygiene <pid> <note>`).
//!
//! **The key hand-off:** right after its first line, before anything else, it builds its key
//! socket's path from its own office home and the line's `key_socket_id` (`key_socket_path`),
//! `lstat`s it (a socket, owned by its uid, 0600, not a symlink; otherwise `invalid_request`),
//! connects, checks right away who is at the other end (the debug rule: the connection's peer,
//! `LOCAL_PEERPID`, must be its parent, the host, running as its user; otherwise
//! `invalid_request`, and nothing from the connection is used), and reads the hand-off line with
//! `read_hand_off`. EOF before the line means the host
//! closed it: it exits 0. A failed connect or read is `storage_error`.
//!
//! Its first stdin line must match its flag, as the contract says: a `create` message with
//! `--create`, a `start` message with `--office`, a `discard` message with `--discard`. A
//! malformed or other line is refused with `failed: invalid_request` (a broken pipe with
//! `storage_error`), before any scripted step (for `--discard`, before its step too). EOF before
//! the first line means the host is gone: it exits 0 without a line.
//! It never logs a secret: for the app token and the database key it records only a fingerprint
//! (the 64-bit FNV-1a hash of the 32 bytes, as 16 hex digits), in `fake.log` and, unless
//! `stderr_bytes` is set, the token's on stderr (so in the office's log), so tests can tell
//! secrets apart without seeing them.
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
//! `start <key> <pid> <run> <unix ms>`, `token <key> <pid> <fingerprint>`,
//! `dbkey <key> <pid> <fingerprint or null>` (`dbkey discard <pid> <fingerprint or null> <id>`
//! for `--discard`), `first-line <key> <pid> <the line as read>` (it holds no secret),
//! `key-socket <key> <pid> <mode in octal> <ours|not-ours>`, `hand-off-eof <key> <pid>`,
//! `no-connect <key> <pid>`, `other-connection <key> <pid> <helper|second> <bytes it got, or
//! connect-failed>`, `env <key> <pid> <its environment variables' names>`,
//! `fds <key> <pid> <inherited fds above 2, or ->`, `first-line-refused <key> <pid>`,
//! `eof-before-first-line <key> <pid>`, `eof-exit <key> <pid>`, `eof-ignored <key> <pid>`, and
//! `leak <key> <pid> <where>` if something that looks like a secret (64 lowercase hex digits)
//! reached it where none may: its `argv` or an `env:<NAME>` variable.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use org_office_launch::{
    key_socket_path, office_dir, read_hand_off, read_host_message, write_server_event, FailCode,
    FirstLine, HandOff, HostMessage, KeySocketId, OfficeFile, ServerEvent, CREATE_FLAG,
    DISCARD_FLAG, EXIT_LAUNCH_DESCRIPTORS_REFUSED, OFFICE_FILE_NAME, OFFICE_FLAG,
    OFFICE_LOCK_FILE_NAME, OFFICE_LOCK_WAIT,
};
use org_proto::frame;
use org_proto::location::{office_socket_path, OFFICE_HOME_ENV};
use org_proto::{
    ApiError, AppToken, DbKey, ErrorCode, Hello, HelloResult, OfficeHello, OfficeInfo,
    OfficeStatus, Request, Response,
};
use serde::Serialize;
use serde_json::Value;

/// The fake's stand-in for an office's databases, in its folder.
const DB_FILE_NAME: &str = "db";

const CRASH: i32 = 3;

/// The fake's own argument for its `hand_off: helper` process: `--key-helper <key> <path>`.
const KEY_HELPER_FLAG: &str = "--key-helper";

/// The fake's own argument for its `hand_off: impostor` process: `--impostor <path>`.
const IMPOSTOR_FLAG: &str = "--impostor";

/// The fake's own argument for the tests' stdio probe: `--stdio-probe <1|2> <the fake's args>`.
const STDIO_PROBE_FLAG: &str = "--stdio-probe";

/// The fake's own argument for the early resumer: `--early-resumer <the fake's args>`.
const EARLY_RESUMER_FLAG: &str = "--early-resumer";

/// Set (by the early resumer only): the fake stops itself first thing in `main`.
const STOP_AT_MAIN_ENV: &str = "ORG_FAKE_STOP_AT_MAIN";

/// How long a connection to the key socket may take to connect and answer.
const HAND_OFF_WAIT: Duration = Duration::from_secs(5);

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
    hand_off: String,
    peer_check_delay: Duration,
}

fn main() {
    // The early resumer's server, before anything else: stops, as if the attacker had stopped it
    // right after its runtime's pre-`main` write to fd 2 (see the module docs). Reading the
    // environment is no I/O on any descriptor.
    if std::env::var_os(STOP_AT_MAIN_ENV).is_some() {
        if let Some(home) = std::env::var_os(OFFICE_HOME_ENV) {
            // A file this process opens itself; the helper waits for this line.
            record(
                Path::new(&home),
                &format!("stopped-at-main {}", std::process::id()),
            );
        }
        // SAFETY: stops this process until it gets a SIGCONT; nothing else.
        unsafe { libc::raise(libc::SIGSTOP) };
    }
    // The contract's first act, before any other I/O on any descriptor: are 0, 1 and 2 pipes?
    // (`fstat` only.)
    let pipes = stdio_pipes();
    // Then what it inherited beyond them (the host must give it nothing else): `fcntl` only.
    let inherited = inherited_fds();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let home = std::env::var_os(OFFICE_HOME_ENV)
        .map(PathBuf::from)
        .filter(|home| home.is_absolute());
    let helper = matches!(
        args.first().map(String::as_str),
        Some(KEY_HELPER_FLAG | IMPOSTOR_FLAG | STDIO_PROBE_FLAG | EARLY_RESUMER_FLAG)
    );
    // A server refuses descriptors that aren't three pipes before anything else, its office home
    // included (only the record needs it).
    if !helper && pipes != [true; 3] {
        let why: Vec<&str> = ["stdin-not-a-pipe", "stdout-not-a-pipe", "stderr-not-a-pipe"]
            .into_iter()
            .zip(pipes)
            .filter_map(|(why, pipe)| (!pipe).then_some(why))
            .collect();
        refuse_descriptors(home.as_deref(), &why.join(","));
    }
    let Some(home) = home else { exit(2) };
    // The fake's own helper processes, which aren't servers.
    match args.as_slice() {
        [flag, key, path] if flag == KEY_HELPER_FLAG => {
            let got = other_connection(Path::new(path));
            record(
                &home,
                &format!("other-connection {key} {} helper {got}", std::process::id()),
            );
            exit(0);
        }
        [flag, path] if flag == IMPOSTOR_FLAG => impostor(&home, Path::new(path)),
        [flag, which, rest @ ..] if flag == STDIO_PROBE_FLAG => stdio_probe(&home, which, rest),
        [flag, rest @ ..] if flag == EARLY_RESUMER_FLAG => early_resumer(&home, rest),
        _ => {}
    }
    match fd_hygiene() {
        Ok(notes) => {
            for note in notes {
                record(&home, &format!("fd-hygiene {} {note}", std::process::id()));
            }
        }
        Err(why) => refuse_descriptors(Some(&home), why),
    }
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
            let create = match first_line(&home, &key, &mut stdin) {
                HostMessage::Create(create) => create,
                _ => refuse_first_line(&home, &key, FailCode::InvalidRequest),
            };
            let hand_off = take_hand_off(
                &home,
                &key,
                &step,
                &create.key_socket_id,
                FirstLine::Create,
                &mut stdin,
            );
            note_token(&home, &key, &step, &hand_off.app_token);
            note_db_key(&home, &key, hand_off.db_key.as_ref(), "");
            drop(hand_off);
            let create_id = create.office_id.clone();
            let dir = office_dir(&home, &create.office_id).unwrap_or_else(|| {
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
                reported("office_id", create.office_id),
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
            match first_line(&home, &key, &mut stdin) {
                HostMessage::Start(start) => {
                    let hand_off = take_hand_off(
                        &home,
                        &key,
                        &step,
                        &start.key_socket_id,
                        FirstLine::Start,
                        &mut stdin,
                    );
                    note_token(&home, &key, &step, &hand_off.app_token);
                    note_db_key(&home, &key, hand_off.db_key.as_ref(), "");
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

    let socket = office_socket_path(&home, &socket_id).expect("a plain id");
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

/// The contract's fd hygiene, the server's first act: fds 0, 1 and 2 must be pipes; every other
/// open fd is closed (found by listing them, `proc_pidinfo(PROC_PIDLISTFDS)`, not by counting up
/// to a limit the parent could have lowered); the thread's exception ports and the task's
/// (`EXC_CRASH` included, and `EXC_CORPSE_NOTIFY` where the kernel takes it) are reset. So the
/// only socket it ever touches is the key socket connection it makes itself, and no inherited
/// exception port can be handed its task. Returns notes to record (one word each), or why it
/// refused.
fn fd_hygiene() -> Result<Vec<&'static str>, &'static str> {
    for fd in open_fds().ok_or("open-fds-not-listed")? {
        if fd > 2 {
            // SAFETY: closes an inherited fd; nothing of this process uses it yet.
            unsafe { libc::close(fd) };
        }
    }
    if open_fds().is_none_or(|fds| fds.iter().any(|&fd| fd > 2)) {
        return Err("inherited-fds-not-closed");
    }

    // `<mach/exception_types.h>`: `EXC_MASK_ALL` (bits 1-9, 11 and 12), `EXC_MASK_CRASH` (bit 10)
    // and `EXC_MASK_CORPSE_NOTIFY` (bit 13). `EXC_MASK_ALL` leaves out the last two.
    const EXC_MASK_ALL: u32 = 0x1bfe;
    const EXC_MASK_CRASH: u32 = 0x400;
    const EXC_MASK_CORPSE_NOTIFY: u32 = 0x2000;
    const EXCEPTION_DEFAULT: i32 = 1;
    const KERN_INVALID_ARGUMENT: libc::kern_return_t = 4;
    #[cfg(target_arch = "aarch64")]
    const THREAD_STATE_NONE: i32 = 5;
    #[cfg(not(target_arch = "aarch64"))]
    const THREAD_STATE_NONE: i32 = 13;
    extern "C" {
        fn task_set_exception_ports(
            task: libc::mach_port_t,
            mask: u32,
            port: libc::mach_port_t,
            behavior: i32,
            flavor: i32,
        ) -> libc::kern_return_t;
        fn thread_set_exception_ports(
            thread: libc::mach_port_t,
            mask: u32,
            port: libc::mach_port_t,
            behavior: i32,
            flavor: i32,
        ) -> libc::kern_return_t;
        /// This task's own port (what `mach_task_self()` reads).
        static mach_task_self_: libc::mach_port_t;
        fn mach_thread_self() -> libc::mach_port_t;
        fn mach_port_deallocate(
            task: libc::mach_port_t,
            name: libc::mach_port_t,
        ) -> libc::kern_return_t;
    }
    let null = libc::MACH_PORT_NULL as libc::mach_port_t;
    // SAFETY: sets this task's and this thread's exception ports to none; the ports passed are
    // this task's own, valid for the calls.
    let (thread, task, corpse) = unsafe {
        let task_self = mach_task_self_;
        let me = mach_thread_self();
        // The thread call takes `EXC_MASK_ALL` only (`EXC_CRASH` is a task-level exception).
        let thread = thread_set_exception_ports(
            me,
            EXC_MASK_ALL,
            null,
            EXCEPTION_DEFAULT,
            THREAD_STATE_NONE,
        );
        mach_port_deallocate(task_self, me);
        // The task call takes `EXC_CRASH` too: a crash port set by the pre-exec image would get
        // the server's corpse (and its memory) after a crash it can cause.
        let task = task_set_exception_ports(
            task_self,
            EXC_MASK_ALL | EXC_MASK_CRASH,
            null,
            EXCEPTION_DEFAULT,
            THREAD_STATE_NONE,
        );
        let corpse = task_set_exception_ports(
            task_self,
            EXC_MASK_CORPSE_NOTIFY,
            null,
            EXCEPTION_DEFAULT,
            THREAD_STATE_NONE,
        );
        (thread, task, corpse)
    };
    if thread != 0 || task != 0 {
        return Err("exception-ports-not-reset");
    }
    let mut notes = vec!["exception-ports-reset"];
    match corpse {
        0 => notes.push("corpse-notify-reset"),
        // The only error that may be ignored: the kernel doesn't take this mask from a task.
        KERN_INVALID_ARGUMENT => notes.push("corpse-notify-refused-by-the-kernel"),
        _ => return Err("corpse-notify-not-reset"),
    }
    Ok(notes)
}

/// This process's open fds, listed by the kernel (`PROC_PIDLISTFDS`), or `None` if it can't say.
fn open_fds() -> Option<Vec<i32>> {
    let size = std::mem::size_of::<libc::proc_fdinfo>();
    // Size the buffer from the kernel's own hint (a call with no buffer), so a kernel that lists
    // by fd index can't miss a high fd: start from at least that.
    // SAFETY: a null buffer of size 0 asks only for the size.
    let hint = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDLISTFDS,
            0,
            std::ptr::null_mut(),
            0,
        )
    };
    let mut room = usize::try_from(hint)
        .ok()
        .map_or(64, |bytes| (bytes / size).max(64));
    loop {
        // SAFETY: an all-zero `proc_fdinfo` is a valid value.
        let mut infos: Vec<libc::proc_fdinfo> = vec![unsafe { std::mem::zeroed() }; room];
        let bytes = i32::try_from(room * size).ok()?;
        // SAFETY: a buffer of exactly `bytes` bytes, which outlives the call; it writes at most
        // that many.
        let n = unsafe {
            libc::proc_pidinfo(
                libc::getpid(),
                libc::PROC_PIDLISTFDS,
                0,
                infos.as_mut_ptr().cast(),
                bytes,
            )
        };
        if n <= 0 {
            return None;
        }
        let count = usize::try_from(n).ok()? / size;
        if count < room {
            return Some(infos[..count].iter().map(|info| info.proc_fd).collect());
        }
        room *= 4;
    }
}

/// Whether fds 0, 1 and 2 are pipes, by `fstat` alone: no read, write or other call that would
/// make this process the last user of a socket handed to it there.
fn stdio_pipes() -> [bool; 3] {
    [0, 1, 2].map(|fd| {
        // SAFETY: an all-zero `stat` is a valid out-value; fstat writes at most its size.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: fstat on an fd number, with a valid out-pointer; fails harmlessly if closed.
        let ok = unsafe { libc::fstat(fd, &mut st) } == 0;
        ok && st.st_mode & libc::S_IFMT == libc::S_IFIFO
    })
}

/// The contract's answer to a failed fd hygiene (an fd 0, 1 or 2 that isn't a pipe, or a later
/// step that failed): no I/O at all on any inherited fd, no `failed` line, just
/// `_exit(EXIT_LAUNCH_DESCRIPTORS_REFUSED)`. The host learns why from the exit status alone. (The
/// record goes to `fake.log`, a file this process opens itself.)
fn refuse_descriptors(home: Option<&Path>, why: &str) -> ! {
    if let Some(home) = home {
        record(
            home,
            &format!("fd-hygiene-refused {} {why}", std::process::id()),
        );
    }
    // SAFETY: ends the process at once, writing nothing anywhere else and flushing nothing.
    unsafe { libc::_exit(EXIT_LAUNCH_DESCRIPTORS_REFUSED) }
}

/// The fake's `hand_off: impostor` process: a same-user process that replaces the key socket
/// with its own (0600, like the host's) and serves a forged hand-off line to whoever connects.
fn impostor(home: &Path, path: &Path) -> ! {
    let _ = fs::remove_file(path);
    let Ok(listener) = UnixListener::bind(path) else {
        exit(1)
    };
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    // Ready: the fake waits for this byte before it connects.
    let _ = io::stdout().write_all(b"r");
    let _ = io::stdout().flush();
    if let Ok((mut stream, _)) = listener.accept() {
        // Recorded before anything is written, so a test can tell it was reached even when the
        // fake refused (and closed) before reading.
        record(home, &format!("impostor-accepted {}", std::process::id()));
        let token = AppToken::from_bytes([0x42; 32]);
        let key = DbKey::from_bytes([0x42; 32]);
        if org_office_launch::write_hand_off(&mut stream, &token, Some(&key)).is_ok() {
            record(home, &format!("impostor-served {}", std::process::id()));
        }
        // Written: the fake checks its peer only now, so it is sure to find the line waiting.
        let _ = io::stdout().write_all(b"w");
        let _ = io::stdout().flush();
        // Kept open until the fake closes its end.
        let mut sink = [0u8; 1];
        let _ = stream.read(&mut sink);
    }
    let _ = fs::remove_file(path);
    exit(0);
}

/// The fake's stdio probe, for the tests: runs the fake (`rest`) with a socket of a socket pair
/// in place of fd `which` (1 or 2), keeps the other end, and after the fake exits records whether
/// the fake touched the socket: `stdio-probe <which> <probe pid> <the socket's last user's pid>
/// <bytes the fake sent>`. A fake that never touched it leaves the probe as the last user.
fn stdio_probe(home: &Path, which: &str, rest: &[String]) -> ! {
    if which != "1" && which != "2" {
        eprintln!("--stdio-probe takes 1 or 2");
        exit(2);
    }
    let Ok((given, kept)) = UnixStream::pair() else {
        exit(1)
    };
    let Ok(exe) = std::env::current_exe() else {
        exit(1)
    };
    let mut fake = std::process::Command::new(exe);
    fake.args(rest);
    let given = std::process::Stdio::from(std::os::fd::OwnedFd::from(given));
    if which == "1" {
        fake.stdout(given);
    } else {
        fake.stderr(given);
    }
    let status = fake.status();
    let code = status.ok().and_then(|s| s.code()).unwrap_or(-1);
    let last = peer_pid(&kept).map_or_else(|| "unknown".to_owned(), |pid| pid.to_string());
    let mut waiting: libc::c_int = 0;
    // SAFETY: a valid socket fd, and an out-value of the size FIONREAD writes.
    let sent = if unsafe { libc::ioctl(kept.as_raw_fd(), libc::FIONREAD, &mut waiting) } == 0 {
        waiting.to_string()
    } else {
        "unknown".to_owned()
    };
    record(
        home,
        &format!(
            "stdio-probe {which} {} {last} {sent} {code}",
            std::process::id()
        ),
    );
    // The fake's own status, so the host sees what the fake meant.
    exit(code);
}

/// The pid that last used the peer end of `stream` (`LOCAL_PEERPID`).
fn peer_pid(stream: &UnixStream) -> Option<u32> {
    let mut pid: libc::pid_t = 0;
    let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: a valid socket fd, and an out-value of `len` bytes that outlives the call.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut len,
        )
    };
    (rc == 0).then(|| u32::try_from(pid).ok()).flatten()
}

/// The debug rule for the host end of the hand-off: the peer of the connection is this process's
/// parent (the host spawned it), running as this user. (A release server checks the peer's code
/// against the host's signing requirement instead.)
fn host_is_peer(stream: &UnixStream) -> bool {
    // SAFETY: getppid has no preconditions and can't fail.
    let parent = u32::try_from(unsafe { libc::getppid() }).ok();
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: a valid socket fd and two valid out-pointers.
    let same_user = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } == 0
        // SAFETY: geteuid has no preconditions and can't fail.
        && uid == unsafe { libc::geteuid() };
    parent.is_some() && peer_pid(stream) == parent && same_user
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

/// The first stdin line, recorded as it was read (it holds no secret; if it ever did, the tests'
/// secret checks would find it in `fake.log`). An unreadable one is refused here; the caller
/// checks that its op matches the flag. EOF before it means the host is gone: exit 0 without a
/// line.
fn first_line(home: &Path, key: &str, stdin: &mut impl BufRead) -> HostMessage {
    let mut raw = Vec::new();
    let read = stdin
        .by_ref()
        .take(org_proto::MAX_FRAME_BYTES as u64 + 1)
        .read_until(b'\n', &mut raw);
    if read.is_err() {
        refuse_first_line(home, key, FailCode::StorageError);
    }
    if !raw.is_empty() {
        let shown = String::from_utf8_lossy(&raw).trim_end().replace(' ', "_");
        record(
            home,
            &format!("first-line {key} {} {shown}", std::process::id()),
        );
    }
    match read_host_message(&mut raw.as_slice()) {
        Ok(Some(message)) => message,
        Ok(None) => {
            record(
                home,
                &format!("eof-before-first-line {key} {}", std::process::id()),
            );
            exit(0);
        }
        Err(e) => refuse_first_line(home, key, e.fail_code()),
    }
}

fn refuse_first_line(home: &Path, key: &str, code: FailCode) -> ! {
    record(
        home,
        &format!("first-line-refused {key} {}", std::process::id()),
    );
    fail(code)
}

/// The key hand-off, right after the first line: see the module docs and the `hand_off` step.
fn take_hand_off(
    home: &Path,
    key: &str,
    step: &Step,
    id: &KeySocketId,
    first: FirstLine,
    stdin: &mut impl BufRead,
) -> HandOff {
    let pid = std::process::id();
    let path = key_socket_path(home, id);
    let Ok(meta) = fs::symlink_metadata(&path) else {
        record(home, &format!("key-socket {key} {pid} missing not-ours"));
        fail(FailCode::InvalidRequest);
    };
    // SAFETY: geteuid has no preconditions and can't fail.
    let ours = meta.uid() == unsafe { libc::geteuid() };
    record(
        home,
        &format!(
            "key-socket {key} {pid} {:o} {}",
            meta.mode(),
            if ours { "ours" } else { "not-ours" }
        ),
    );
    if !meta.file_type().is_socket() || !ours || meta.permissions().mode() & 0o777 != 0o600 {
        fail(FailCode::InvalidRequest);
    }
    let mut impostor: Option<std::process::ChildStdout> = None;
    match step.hand_off.as_str() {
        "none" => {
            record(home, &format!("no-connect {key} {pid}"));
            wait_for_eof(step, home, key, stdin);
            exit(0);
        }
        "helper" => {
            // Waits for the helper: it is refused (or not) before the fake goes on.
            let _ = std::process::Command::new(
                std::env::current_exe().unwrap_or_else(|_| fail(FailCode::StorageError)),
            )
            .arg(KEY_HELPER_FLAG)
            .arg(key)
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
            wait_for_eof(step, home, key, stdin);
            exit(0);
        }
        "impostor" => {
            // Another process replaces the socket before the fake connects; waits until it has.
            let spawned = std::process::Command::new(
                std::env::current_exe().unwrap_or_else(|_| fail(FailCode::StorageError)),
            )
            .arg(IMPOSTOR_FLAG)
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn();
            let mut ready = [0u8; 1];
            let mut out = spawned
                .ok()
                .and_then(|mut child| child.stdout.take())
                .unwrap_or_else(|| fail(FailCode::StorageError));
            if out.read_exact(&mut ready).is_err() {
                fail(FailCode::StorageError);
            }
            impostor = Some(out);
        }
        _ => {}
    }
    let stream = match step.hand_off.as_str() {
        how @ ("dup" | "fork" | "on_stderr") => connect_held_badly(&path, how),
        _ => connect(&path),
    };
    if let Some(mut out) = impostor {
        // For the test: the impostor has written its line before this process checks its peer.
        let mut written = [0u8; 1];
        if out.read_exact(&mut written).is_err() {
            fail(FailCode::StorageError);
        }
    }
    // A server descheduled before its check, for the tests: the host must still be there.
    thread::sleep(step.peer_check_delay);
    // Who is at the other end, checked right after the connect, before a byte is read: a
    // same-user process could have replaced the socket (`lstat` guards only against other users).
    // The host keeps its end open until this process closes its own, so it can still be asked.
    if !host_is_peer(&stream) {
        // Refused before a byte is read: nothing from this connection is used. Recorded with the
        // bytes still waiting on it, unread (`FIONREAD`).
        let mut waiting: libc::c_int = 0;
        // SAFETY: a valid socket fd, and an out-value of the size FIONREAD writes.
        let rc = unsafe { libc::ioctl(stream.as_raw_fd(), libc::FIONREAD, &mut waiting) };
        let waiting = if rc == 0 {
            waiting.to_string()
        } else {
            "unknown".to_owned()
        };
        drop(stream);
        record(
            home,
            &format!("hand-off-peer-refused {key} {pid} {waiting}"),
        );
        fail(FailCode::InvalidRequest);
    }
    let second = (step.hand_off == "twice").then(|| UnixStream::connect(&path));
    let hand_off = read_hand_off(&mut BufReader::new(&stream), first);
    drop(stream);
    if let Some(second) = second {
        let got = match second {
            Ok(second) => read_to_end_count(second),
            Err(_) => "connect-failed".to_owned(),
        };
        record(home, &format!("other-connection {key} {pid} second {got}"));
    }
    match hand_off {
        Ok(hand_off) => hand_off,
        Err(e) if e.is_host_gone() => {
            record(home, &format!("hand-off-eof {key} {pid}"));
            exit(0);
        }
        Err(e) => fail(e.fail_code()),
    }
}

/// A connection to the key socket, with the contract's deadline on its reads.
fn connect(path: &Path) -> UnixStream {
    let stream = UnixStream::connect(path).unwrap_or_else(|_| fail(FailCode::StorageError));
    // macOS refuses (EINVAL) a timeout on a socket whose peer has already closed. The host keeps
    // its end open until this process closes its own, so that shouldn't happen here; if it does,
    // the peer check that follows fails closed.
    let _ = stream.set_read_timeout(Some(HAND_OFF_WAIT));
    stream
}

/// A connection to the key socket held the way the contract forbids (`how`: `dup`, `fork` or
/// `on_stderr`, see the module docs), arranged before the connect.
fn connect_held_badly(path: &Path, how: &str) -> UnixStream {
    use std::os::fd::FromRawFd;
    // SAFETY: plain socket creation; checked before use.
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    // SAFETY: sets only the close-on-exec flag of the fd just made.
    if fd < 0 || unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
        fail(FailCode::StorageError);
    }
    let fd = match how {
        "dup" => {
            // SAFETY: duplicates the fd just made; the copy stays open until the fake exits.
            if unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) } < 0 {
                fail(FailCode::StorageError);
            }
            fd
        }
        "fork" => {
            hold_in_a_forked_child(fd);
            fd
        }
        _ => {
            // SAFETY: moves the socket onto fd 2 (`dup2` leaves it without close-on-exec) and
            // closes the original, so fd 2 is its only fd.
            unsafe {
                if libc::dup2(fd, 2) != 2 {
                    fail(FailCode::StorageError);
                }
                libc::close(fd);
            }
            2
        }
    };
    if connect_fd(fd, path).is_err() {
        fail(FailCode::StorageError);
    }
    // SAFETY: a connected socket this process owns, handed over whole.
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    let _ = stream.set_read_timeout(Some(HAND_OFF_WAIT));
    stream
}

/// Forks a child that holds `fd` (and nothing else of the fake's) until the fake exits.
fn hold_in_a_forked_child(fd: i32) {
    let mut ends = [0; 2];
    // SAFETY: an out-array of two fds.
    if unsafe { libc::pipe(ends.as_mut_ptr()) } != 0 {
        fail(FailCode::StorageError);
    }
    // SAFETY: the fake is single-threaded here; the child only closes fds, reads and exits.
    match unsafe { libc::fork() } {
        0 => {
            // SAFETY: async-signal-safe calls only; `_exit` never returns.
            unsafe {
                for other in 0..1024 {
                    if other != fd && other != ends[0] {
                        libc::close(other);
                    }
                }
                let mut byte = 0u8;
                // EOF once the fake (the pipe's only writer) is gone.
                while libc::read(ends[0], (&mut byte as *mut u8).cast(), 1) > 0 {}
                libc::_exit(0)
            }
        }
        -1 => fail(FailCode::StorageError),
        _ => {
            // SAFETY: closes our copy of the read end; the write end stays open (close-on-exec)
            // until the fake exits.
            unsafe {
                libc::close(ends[0]);
                libc::fcntl(ends[1], libc::F_SETFD, libc::FD_CLOEXEC);
            }
        }
    }
}

/// `connect(2)` of socket `fd` to `path`.
fn connect_fd(fd: i32, path: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    // SAFETY: an all-zero `sockaddr_un` is a valid value.
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    let bytes = path.as_os_str().as_bytes();
    if bytes.len() >= addr.sun_path.len() {
        return Err(io::Error::other("path too long"));
    }
    for (slot, &b) in addr.sun_path.iter_mut().zip(bytes) {
        *slot = b as libc::c_char;
    }
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    let len = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
    addr.sun_len = len as u8;
    // SAFETY: a valid `sockaddr_un` of `len` bytes that outlives the call.
    if unsafe { libc::connect(fd, (&addr as *const libc::sockaddr_un).cast(), len) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The early resumer (`--early-resumer <args>`): see the module docs. Runs as the pre-exec image
/// the host spawned and resumed; never returns.
fn early_resumer(home: &Path, args: &[String]) -> ! {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let key = match args {
        [_, id] => id.clone(),
        _ => "create".to_owned(),
    };
    // SAFETY: getppid has no preconditions: the host, which spawned this process.
    let host = unsafe { libc::getppid() };
    // SAFETY: ignored across the exec too (no POSIX_SPAWN_SETSIGDEF below), so the server's
    // failed pre-`main` write to fd 2 doesn't kill it.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) };
    // The socket the helper keeps, and the server will have on fd 2: no close-on-exec.
    // SAFETY: plain socket creation; checked.
    let socket = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    // Its write end closes when this process execs: the helper's signal that it has.
    let mut execed = [0; 2];
    // SAFETY: an out-array of two fds; then sets only their close-on-exec flags.
    let piped = unsafe {
        libc::pipe(execed.as_mut_ptr()) == 0
            && libc::fcntl(execed[0], libc::F_SETFD, libc::FD_CLOEXEC) == 0
            && libc::fcntl(execed[1], libc::F_SETFD, libc::FD_CLOEXEC) == 0
    };
    if socket < 0 || !piped {
        exit(1);
    }
    // SAFETY: this process is single-threaded; the child runs ordinary code, then exits.
    match unsafe { libc::fork() } {
        0 => {
            // SAFETY: closes the helper's copy of the write end.
            unsafe { libc::close(execed[1]) };
            early_resumer_helper(home, &key, host, socket, execed[0])
        }
        -1 => exit(1),
        _ => {}
    }
    let Ok(exe) = std::env::current_exe() else {
        exit(1)
    };
    let c = |bytes: &[u8]| CString::new(bytes).unwrap_or_else(|_| exit(1));
    let program = c(exe.as_os_str().as_bytes());
    let mut argv_c = vec![program.clone()];
    argv_c.extend(args.iter().map(|arg| c(arg.as_bytes())));
    let mut home_pair = format!("{OFFICE_HOME_ENV}=").into_bytes();
    home_pair.extend_from_slice(home.as_os_str().as_bytes());
    let env_c = [
        home_pair,
        b"OBJC_HELP=YES".to_vec(),
        format!("{STOP_AT_MAIN_ENV}=1").into_bytes(),
    ]
    .map(|pair| c(&pair));
    let mut argv: Vec<*mut libc::c_char> = argv_c.iter().map(|s| s.as_ptr().cast_mut()).collect();
    argv.push(std::ptr::null_mut());
    let mut envp: Vec<*mut libc::c_char> = env_c.iter().map(|s| s.as_ptr().cast_mut()).collect();
    envp.push(std::ptr::null_mut());
    let mut actions: libc::posix_spawn_file_actions_t = std::ptr::null_mut();
    let mut attr: libc::posix_spawnattr_t = std::ptr::null_mut();
    // SAFETY: initialized before use; on success the exec replaces this process, so nothing here
    // is used again; the pointers outlive the call.
    let rc = unsafe {
        libc::posix_spawn_file_actions_init(&mut actions);
        libc::posix_spawnattr_init(&mut attr);
        libc::posix_spawn_file_actions_adddup2(&mut actions, socket, 2);
        libc::posix_spawn_file_actions_addclose(&mut actions, socket);
        let flags = libc::POSIX_SPAWN_SETEXEC | libc::POSIX_SPAWN_START_SUSPENDED;
        libc::posix_spawnattr_setflags(&mut attr, flags as libc::c_short);
        let mut pid = 0;
        libc::posix_spawn(
            &mut pid,
            program.as_ptr(),
            &actions,
            &attr,
            argv.as_ptr(),
            envp.as_ptr(),
        )
    };
    record(home, &format!("early-resumer-exec-failed {key} {rc}"));
    exit(1);
}

/// The early resumer's helper: see the module docs. `socket` is the server's fd 2; `execed`
/// reads EOF once the server image is in place.
fn early_resumer_helper(home: &Path, key: &str, host: libc::pid_t, socket: i32, execed: i32) -> ! {
    use std::os::fd::FromRawFd;
    // Off the host's pipes, so a server's exit is seen as soon as it happens.
    if let Ok(null) = File::open("/dev/null") {
        for fd in 0..=2 {
            // SAFETY: points 0-2 at /dev/null; `null` stays open for the call.
            unsafe { libc::dup2(null.as_raw_fd(), fd) };
        }
    }
    // SAFETY: getppid has no preconditions: the server's pid (an exec keeps it).
    let server = unsafe { libc::getppid() };
    let mut byte = 0u8;
    // SAFETY: reads into a valid one-byte buffer until EOF.
    while unsafe { libc::read(execed, (&mut byte as *mut u8).cast(), 1) } > 0 {}
    let run = home.join("run");
    let deadline = Instant::now() + HAND_OFF_WAIT;
    let key_socket = loop {
        let found = fs::read_dir(&run).ok().and_then(|entries| {
            entries.flatten().map(|e| e.path()).find(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("k-") && n.ends_with(".sock"))
            })
        });
        if let Some(path) = found {
            break path;
        }
        if Instant::now() > deadline {
            record(home, &format!("early-resumer-no-key-socket {key}"));
            exit(1);
        }
        thread::sleep(Duration::from_millis(1));
    };
    /// Resumes the host however the helper ends.
    struct Resume(libc::pid_t);
    impl Drop for Resume {
        fn drop(&mut self) {
            // SAFETY: signals the host; harmless if it is gone.
            unsafe { libc::kill(self.0, libc::SIGCONT) };
        }
    }
    // SAFETY: stops the host (same user) so it can't accept before the server's write.
    unsafe { libc::kill(host, libc::SIGSTOP) };
    let resume = Resume(host);
    // SAFETY: shuts down the sending side of the socket the server and the helper share.
    let ready = connect_fd(socket, &key_socket).is_ok()
        && unsafe { libc::shutdown(socket, libc::SHUT_WR) } == 0;
    if ready {
        // SAFETY: resumes the server, suspended since its exec.
        unsafe { libc::kill(server, libc::SIGCONT) };
        // Until it has stopped in `main`: suspended since its exec, it looked stopped before.
        let marker = format!("stopped-at-main {server}");
        let deadline = Instant::now() + HAND_OFF_WAIT;
        while Instant::now() < deadline
            && !(stopped(server)
                && fs::read_to_string(home.join("fake.log"))
                    .is_ok_and(|log| log.lines().any(|line| line == marker)))
        {
            thread::sleep(Duration::from_millis(1));
        }
    }
    drop(resume);
    // Leaves the socket alone while the host decides (it accepts within milliseconds of
    // resuming): a read, and even a `poll` or `select`, would make the helper its last user.
    thread::sleep(Duration::from_secs(1));
    // SAFETY: a socket this process owns, handed over whole.
    let got = read_to_end_count(unsafe { UnixStream::from_raw_fd(socket) });
    record(
        home,
        &format!(
            "early-resumer {key} {} {server} {}",
            std::process::id(),
            if ready {
                got
            } else {
                "connect-failed".to_owned()
            }
        ),
    );
    exit(0);
}

/// Whether process `pid` is stopped (`SSTOP`).
fn stopped(pid: libc::pid_t) -> bool {
    // SAFETY: an all-zero `proc_bsdinfo` is a valid value.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: a buffer of exactly `size` bytes that outlives the call.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    n == size && info.pbi_status == libc::SSTOP
}

/// What a connection that isn't the server's own got: the number of bytes until EOF (the bytes
/// themselves are never kept), or `connect-failed`.
fn other_connection(path: &Path) -> String {
    match UnixStream::connect(path) {
        Ok(stream) => read_to_end_count(stream),
        Err(_) => "connect-failed".to_owned(),
    }
}

fn read_to_end_count(mut stream: UnixStream) -> String {
    let _ = stream.set_read_timeout(Some(HAND_OFF_WAIT));
    let mut got = Vec::new();
    let read = stream.read_to_end(&mut got);
    let n = got.len();
    for b in got.iter_mut() {
        // SAFETY: `b` is a valid, aligned, exclusive reference to a u8.
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    match read {
        Ok(_) => n.to_string(),
        Err(_) => format!("{n}-then-error"),
    }
}

/// Records the token's fingerprint (never the token) in `fake.log`, and on stderr unless the
/// step writes its own stderr bytes.
fn note_token(home: &Path, key: &str, step: &Step, token: &AppToken) {
    let fingerprint = fingerprint(token);
    record(
        home,
        &format!("token {key} {} {fingerprint}", std::process::id()),
    );
    if step.stderr_bytes == 0 {
        let _ = writeln!(
            io::stderr(),
            "fake office server {key}: got its hand-off, token fingerprint {fingerprint}"
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
    let socket_id = match first_line(home, key, stdin) {
        HostMessage::Discard(discard) => discard.key_socket_id,
        _ => refuse_first_line(home, key, FailCode::InvalidRequest),
    };
    let db_key = take_hand_off(home, key, step, &socket_id, FirstLine::Discard, stdin).db_key;
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

/// Blocks until stdin closes, then returns, unless the step ignores EOF.
fn wait_for_eof(step: &Step, home: &Path, key: &str, stdin: &mut impl BufRead) {
    let mut sink = Vec::new();
    let _ = stdin.read_to_end(&mut sink);
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

fn emit(event: &ServerEvent) {
    let mut out = io::stdout().lock();
    if write_server_event(&mut out, event).is_err() {
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
        hand_off: text("hand_off", "connect"),
        peer_check_delay: Duration::from_millis(
            entry
                .get("peer_check_delay_ms")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        ),
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
