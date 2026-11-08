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
    read_server_event, write_host_message, CreateOffice, HostMessage, ServerEvent, StartOffice,
    CREATE_FLAG, DISCARD_FLAG, OFFICE_FLAG,
};
use org_proto::location::OFFICE_HOME_ENV;
use org_proto::AppToken;

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
/// nothing else in the login session reaches it. The Keychain and SQLCipher need only the user's
/// identity and home; `TMPDIR` is the user's temporary folder.
const PASSED_ENV: [&str; 6] = ["HOME", "USER", "LOGNAME", "PATH", "TMPDIR", "LANG"];

/// The key folder a `dev-keys` build of org-office-server reads (org-office-server's keys.rs).
/// Passed through in debug builds only, so a release host never hands it on.
#[cfg(debug_assertions)]
const DEV_KEY_DIR_ENV: &str = "ORG_OFFICE_DEV_KEY_DIR";

/// How to start a server: the program and the office home it is given.
#[derive(Debug, Clone)]
pub(crate) struct Spawner {
    pub(crate) program: PathBuf,
    pub(crate) home: PathBuf,
}

/// Which server to start.
#[derive(Clone, Copy)]
pub(crate) enum Mode<'a> {
    /// Create an office: its first line is this create message.
    Create(&'a CreateLine),
    /// Serve the office with this id: its first line is this start message.
    Office(&'a str, &'a StartLine),
    /// Remove an unfinished office (its folder and any key it left), then exit. No first line.
    Discard(&'a str),
}

/// The `--create` first line, holding the new office's app token for this host run.
///
/// The lines are kept as the messages themselves, built once, because a [`HostMessage`] owns
/// its token and [`AppToken`] isn't `Clone`: writing a kept line never copies the token, and
/// dropping it zeroes the token. They live on the heap (a `Box`, or the `Arc` a start line is
/// shared through), so moving one moves a pointer, not the token. Building one still leaves the
/// usual stack copies of the token behind (see `AppToken`'s docs).
pub(crate) struct CreateLine(Box<HostMessage>);

/// The `--office` first line, holding the office's app token for this host run. Written, as is,
/// on every start of the office within the run, and shared (through an `Arc`) with the
/// supervisor, which hands the token to the app. See [`CreateLine`].
pub(crate) struct StartLine(HostMessage);

impl CreateLine {
    pub(crate) fn new(create: CreateOffice) -> Self {
        Self(Box::new(HostMessage::Create(create)))
    }

    /// The start line for the office this line created: the same token, so its restarts get
    /// the token it was created with.
    pub(crate) fn into_start_line(mut self) -> Arc<StartLine> {
        match &mut *self.0 {
            // Taken out in place, leaving zeros: the box's memory never keeps a copy.
            HostMessage::Create(create) => Arc::new(StartLine::new(std::mem::replace(
                &mut create.app_token,
                AppToken::from_bytes([0; 32]),
            ))),
            _ => unreachable!("a create line holds a create message"),
        }
    }
}

impl StartLine {
    pub(crate) fn new(app_token: AppToken) -> Self {
        Self(HostMessage::Start(StartOffice { app_token }))
    }

    /// The office's app token. Never clone it into a string or log it.
    pub(crate) fn app_token(&self) -> &AppToken {
        match &self.0 {
            HostMessage::Start(start) => &start.app_token,
            _ => unreachable!("a start line holds a start message"),
        }
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
            Mode::Office(id, _) => command.arg(OFFICE_FLAG).arg(id),
            Mode::Discard(id) => command.arg(DISCARD_FLAG).arg(id),
        };
        command.env_clear();
        for name in PASSED_ENV {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        // A dev-keys build of org-office-server needs its key folder; development only.
        #[cfg(debug_assertions)]
        if let Some(value) = std::env::var_os(DEV_KEY_DIR_ENV) {
            command.env(DEV_KEY_DIR_ENV, value);
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
    /// gets the create message, `--office` the start message (both hold the app token),
    /// `--discard` none. `write_host_message` zeroes its encoded copy of the line.
    fn write_first_line(&mut self, mode: Mode<'_>) {
        let pid = self.child.id();
        let Some(stdin) = self.stdin.as_mut() else {
            return;
        };
        let written = match mode {
            Mode::Create(CreateLine(message)) => write_host_message(stdin, message),
            Mode::Office(_, StartLine(message)) => write_host_message(stdin, message),
            Mode::Discard(_) => Ok(()),
        };
        // A child that dies at once makes this fail with a broken pipe; its stdout then closes
        // and that is reported as a crash, so the error itself adds nothing. The error never
        // holds the token: it is an I/O error or a fixed message.
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
