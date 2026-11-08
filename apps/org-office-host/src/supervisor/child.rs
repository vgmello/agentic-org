//! One `org-office-server` process: spawning it with its pipes, reading its control lines,
//! copying its stderr to the office's log, and stopping it.

use std::io::{self, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use org_office_launch::{
    read_server_event, write_host_line, CreateOffice, HostLine, ServerEvent, CREATE_FLAG,
    DISCARD_FLAG, OFFICE_FLAG,
};
use org_proto::location::OFFICE_HOME_ENV;
use org_proto::{AppToken, DbKey, APP_TOKEN_BYTES, DB_KEY_BYTES};

use crate::cloexec::SPAWN_LOCK;
use crate::host_log;
use crate::logs::RotatingLog;
use crate::sync::lock;

/// How often a stop polls whether the child has exited.
const EXIT_POLL: Duration = Duration::from_millis(10);

/// What an office's supervision thread hears: its child's control lines, the end of its
/// stdout, or a stop request. Child messages carry the child's generation, so a late line from
/// a child that was already replaced is ignored.
#[derive(Debug)]
pub(crate) enum Signal {
    Event {
        generation: u64,
        event: ServerEvent,
    },
    /// The child closed its stdout (it exited) or wrote something unreadable.
    Closed {
        generation: u64,
    },
    /// The host is shutting down: stop the child by this deadline.
    Stop {
        deadline: Instant,
    },
    /// `office.restart`: start the office again now, if it isn't running.
    Restart,
}

/// The environment a server gets, besides `ORG_OFFICE_HOME`: only these of the host's own, so
/// nothing else in the login session reaches it (the development key folder included: servers
/// never touch a key store, they get their key on the first line). SQLCipher needs only the
/// user's identity and home; `TMPDIR` is the user's temporary folder.
const PASSED_ENV: [&str; 6] = ["HOME", "USER", "LOGNAME", "PATH", "TMPDIR", "LANG"];

/// How to start a server: the program and the office home it is given.
#[derive(Debug, Clone)]
pub(crate) struct Spawner {
    pub(crate) program: PathBuf,
    pub(crate) home: PathBuf,
}

/// Which server to start, with what its first line carries. Every first line is written with
/// `write_host_line`, which borrows the secrets (never copies them) and zeroes its encoded line.
#[derive(Clone, Copy)]
pub(crate) enum Mode<'a> {
    /// Create an office: its first line is this create message (the new office's app token and
    /// database key).
    Create(&'a CreateLine),
    /// Serve the office with this id: its first line is a start with the office's app token for
    /// this host run and its database key from the key store.
    Office(&'a str, &'a AppToken, &'a DbKey),
    /// Remove an unfinished office, then exit: its first line is a discard with the office's
    /// database key, or `null` when the host has none.
    Discard(&'a str, Option<&'a DbKey>),
}

/// The `--create` first line: the new office's create message, holding its app token for this
/// host run and its database key.
///
/// It is kept as the message itself, built once, because the message owns its secrets and
/// neither [`AppToken`] nor [`DbKey`] is `Clone`: writing it never copies them, and dropping it
/// zeroes them. It lives on the heap (a `Box`), so moving it moves a pointer, not the secrets.
/// Building it still leaves the usual stack copies behind (see `AppToken`'s docs).
pub(crate) struct CreateLine(Box<CreateOffice>);

impl CreateLine {
    pub(crate) fn new(create: CreateOffice) -> Self {
        Self(Box::new(create))
    }

    /// The id of the office it creates.
    pub(crate) fn office_id(&self) -> &str {
        &self.0.office_id
    }

    /// The new office's database key, for the key store. Never copy it.
    pub(crate) fn db_key(&self) -> &DbKey {
        &self.0.db_key
    }

    /// Zeroes the database key in place, once the first line is written (or can't be): from then
    /// on only the key store holds it.
    pub(crate) fn drop_key(&mut self) {
        // Replaced in place, leaving zeros; the old key is zeroed as it drops.
        drop(std::mem::replace(
            &mut self.0.db_key,
            DbKey::from_bytes([0; DB_KEY_BYTES]),
        ));
    }

    /// The created office's app token, so its restarts get the token it was created with.
    pub(crate) fn into_token(mut self) -> Arc<AppToken> {
        self.drop_key();
        // Taken out in place, leaving zeros: the box's memory never keeps a copy.
        Arc::new(std::mem::replace(
            &mut self.0.app_token,
            AppToken::from_bytes([0; APP_TOKEN_BYTES]),
        ))
    }
}

/// A running server process. Dropping it kills and reaps the process if it's still running,
/// so a server is never left behind unsupervised.
pub(crate) struct RunningChild {
    child: Child,
    stdin: Option<ChildStdin>,
    pub(crate) generation: u64,
    pub(crate) started: Instant,
    reaped: bool,
}

impl Spawner {
    /// Starts a server. Its control lines go to `signals` tagged with `generation`; its stderr
    /// goes to `log`. Its first line (per [`Mode`]) is written at once.
    pub(crate) fn spawn(
        &self,
        mode: Mode<'_>,
        generation: u64,
        signals: Sender<Signal>,
        log: Arc<Mutex<RotatingLog>>,
    ) -> io::Result<RunningChild> {
        let mut command = Command::new(&self.program);
        match mode {
            Mode::Create(_) => command.arg(CREATE_FLAG),
            Mode::Office(id, ..) => command.arg(OFFICE_FLAG).arg(id),
            Mode::Discard(id, _) => command.arg(DISCARD_FLAG).arg(id),
        };
        command.env_clear();
        for name in PASSED_ENV {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command
            .env(OFFICE_HOME_ENV, &self.home)
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = {
            // See `cloexec`: no fd of ours may be half-made while a child is spawned.
            let _one_at_a_time = lock(&SPAWN_LOCK);
            command.spawn()?
        };
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        let mut running = RunningChild {
            child,
            stdin,
            generation,
            started: Instant::now(),
            reaped: false,
        };
        let pid = running.child.id();
        thread::Builder::new()
            .name(format!("office-{pid}-out"))
            .spawn(move || read_events(stdout, generation, &signals))?;
        thread::Builder::new()
            .name(format!("office-{pid}-err"))
            .spawn(move || copy_stderr(stderr, &log))?;

        running.write_first_line(mode);
        Ok(running)
    }
}

impl RunningChild {
    /// The first line on a new server's control pipe, the one place that writes it: `--create`
    /// gets the create message, `--office` the start message (the app token and the database
    /// key), `--discard` the discard message (the database key or `null`). `write_host_line`
    /// zeroes its encoded copy of the line.
    fn write_first_line(&mut self, mode: Mode<'_>) {
        let pid = self.child.id();
        let Some(stdin) = self.stdin.as_mut() else {
            return;
        };
        let line = match mode {
            Mode::Create(CreateLine(create)) => HostLine::Create(create),
            Mode::Office(_, app_token, db_key) => HostLine::Start { app_token, db_key },
            Mode::Discard(_, db_key) => HostLine::Discard { db_key },
        };
        let written = write_host_line(stdin, &line);
        // A child that dies at once makes this fail with a broken pipe; its stdout then closes
        // and that is reported as a crash, so the error itself adds nothing. The error never
        // holds a secret: it is an I/O error or a fixed message.
        if let Err(e) = written {
            host_log::log(&format!("couldn't write to office server {pid}: {e}"));
        }
    }

    /// Closes the control pipe: the server shuts down cleanly.
    pub(crate) fn close_stdin(&mut self) {
        self.stdin = None;
    }

    /// Asks the server to stop (closes its stdin), waits for it until `deadline`, then
    /// SIGKILLs it. Always reaps it.
    pub(crate) fn stop(&mut self, deadline: Instant) {
        self.close_stdin();
        if !self.wait_until(deadline) {
            self.kill();
        }
    }

    #[cfg(test)]
    pub(crate) fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Whether the process has exited (it is reaped if so). Doesn't wait.
    pub(crate) fn has_exited(&mut self) -> bool {
        if !self.reaped && matches!(self.child.try_wait(), Ok(Some(_))) {
            self.reaped = true;
        }
        self.reaped
    }

    /// SIGKILLs and reaps the process.
    pub(crate) fn kill(&mut self) {
        self.close_stdin();
        if !self.reaped {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.reaped = true;
        }
    }

    /// Waits for the process to exit until `deadline`; reaps it if it did.
    fn wait_until(&mut self, deadline: Instant) -> bool {
        loop {
            if self.reaped {
                return true;
            }
            match self.child.try_wait() {
                Ok(Some(_)) => self.reaped = true,
                Ok(None) if Instant::now() < deadline => thread::sleep(EXIT_POLL),
                Ok(None) => return false,
                Err(_) => return false,
            }
        }
    }
}

impl Drop for RunningChild {
    fn drop(&mut self) {
        self.kill();
    }
}

/// The stdout reader: forwards control lines until EOF or an error that leaves the pipe out of
/// step. Lines it can't use (bad JSON, another version) are logged and skipped.
fn read_events(stdout: impl Read, generation: u64, signals: &Sender<Signal>) {
    let mut reader = BufReader::new(stdout);
    loop {
        match read_server_event(&mut reader) {
            Ok(Some(event)) => {
                if signals.send(Signal::Event { generation, event }).is_err() {
                    return;
                }
            }
            Ok(None) => break,
            Err(e) if e.closes_stream() => {
                host_log::log(&format!("an office server's control pipe failed: {e}"));
                break;
            }
            Err(e) => host_log::log(&format!("ignored a line from an office server: {e}")),
        }
    }
    let _ = signals.send(Signal::Closed { generation });
}

/// The stderr copier: drains stderr to the end (so the server never blocks on a full pipe),
/// writing what it can to the log.
fn copy_stderr(mut stderr: impl Read, log: &Mutex<RotatingLog>) {
    let mut buf = [0u8; 8192];
    let mut warned = false;
    loop {
        match stderr.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => {
                if let Err(e) = lock(log).write(&buf[..n]) {
                    if !warned {
                        host_log::log(&format!("couldn't write an office server's log: {e}"));
                        warned = true;
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}
