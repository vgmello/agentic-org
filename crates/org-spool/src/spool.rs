//! Maildir-style spool operations encapsulated behind `Spool`.
//!
//! All filesystem paths (`outbox/{tmp,new}`, `inbox/<agent>/{tmp,new,cur,wake}`, `dead/`,
//! `relay.sock`, `relay.lock`) are private to this crate.

use std::env;
use std::fmt;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use crate::envelope::{validate_agent_id, Envelope, MAX_ENVELOPE_BYTES};
use crate::safe_read::{create_new_nofollow, read_spool_file, SpoolFile};
use crate::time::{check_wait_timeout, format_iso8601_utc};
use crate::ulid::{generate_ulid, is_valid_ulid};

const RETENTION_SECS: u64 = 30 * 24 * 60 * 60;
const ASK_WAIT_BACKSTOP: Duration = Duration::from_millis(2);
/// `msg wait` rescans on every wake-socket nudge; this backstop only catches snoozes coming due
/// (and a relay that can't reach the socket), so it can be long: a busy `--urgent-only` watcher
/// otherwise re-reads the whole inbox many times a second.
const INBOX_WAIT_BACKSTOP: Duration = Duration::from_millis(1000);
/// Without a bound socket (bind failed), there's nothing to wake us: poll faster.
const INBOX_WAIT_NO_SOCKET: Duration = Duration::from_millis(100);

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Async-signal-safe interrupt flag setter so `wait_for_reply` / `wait_for_inbox` unwind
/// cleanly and run `WakeListener::drop` on the main thread.
pub fn request_interrupt() {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

pub fn is_interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

const POLLIN: i16 = 0x0001;

extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn fsync(fd: i32) -> i32;
    fn poll(fds: *mut PollFd, nfds: u32, timeout: i32) -> i32;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WakeEvents {
    pub any: bool,
    pub urgent: bool,
}

/// Wait on a `UnixListener` for up to `timeout` using POSIX `poll(2)` (0% CPU while idle),
/// inspecting `'U'` (urgent) vs `'N'` (normal) wake tags.
pub fn poll_unix_listener_events(listener: &UnixListener, timeout: Duration) -> WakeEvents {
    let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    let mut pfd = PollFd {
        fd: listener.as_raw_fd(),
        events: POLLIN,
        revents: 0,
    };
    let rc = unsafe { poll(&mut pfd as *mut PollFd, 1, ms) };
    let mut events = WakeEvents::default();
    if rc > 0 {
        while let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 16];
            if let Ok(n) = stream.read(&mut buf) {
                if buf[..n].contains(&b'U') {
                    events.urgent = true;
                }
            }
            events.any = true;
        }
    }
    events
}

/// Wait on a `UnixListener` for up to `timeout` using POSIX `poll(2)` (0% CPU while idle).
pub fn poll_unix_listener(listener: &UnixListener, timeout: Duration) -> bool {
    poll_unix_listener_events(listener, timeout).any
}

/// Read a message file (see [`read_spool_file`]): `None` if it's not there (any more), and an
/// envelope, or why it isn't one (not a regular file, too large, not a valid envelope).
fn read_envelope(path: &Path) -> Result<Option<Result<Envelope, String>>, SpoolError> {
    Ok(match read_spool_file(path, MAX_ENVELOPE_BYTES)? {
        SpoolFile::Missing => None,
        SpoolFile::Bytes(bytes) => Some(Envelope::from_json_bytes(&bytes)),
        SpoolFile::Rejected(why) => Some(Err(why)),
    })
}

/// `now + timeout` for a wait: at most [`crate::MAX_WAIT_TIMEOUT`], and never a panic on
/// overflow (`Instant + Duration` panics).
fn wait_deadline(timeout: Duration) -> Result<Instant, SpoolError> {
    check_wait_timeout(timeout).map_err(SpoolError::InvalidInput)?;
    Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| SpoolError::InvalidInput("--timeout is out of range".into()))
}

fn is_pid_dead(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return true;
    }
    let rc = unsafe { kill(pid as i32, 0) };
    if rc == 0 {
        return false;
    }
    io::Error::last_os_error().raw_os_error() == Some(3)
}

fn socket_owner_pid_is_dead(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let Some((pid_str, _)) = name.split_once('-') else {
        return false;
    };
    let Ok(pid) = pid_str.parse::<u32>() else {
        return false;
    };
    is_pid_dead(pid)
}

#[derive(Debug)]
pub enum SpoolError {
    InvalidInput(String),
    UnknownRecipient(String),
    NotFound(String),
    Timeout(String),
    Interrupted,
    Io(io::Error),
}

impl fmt::Display for SpoolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(msg) => write!(f, "{msg}"),
            Self::UnknownRecipient(agent) => write!(
                f,
                "unknown recipient {agent} (register it with: org-agent msg register {agent})"
            ),
            Self::NotFound(msg) => write!(f, "{msg}"),
            Self::Timeout(msg) => write!(f, "{msg}"),
            Self::Interrupted => write!(f, "interrupted by signal"),
            Self::Io(err) => write!(f, "I/O error: {err}"),
        }
    }
}

impl From<io::Error> for SpoolError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliverOutcome {
    Delivered {
        id: String,
        to: String,
        urgent: bool,
    },
    Dead {
        id: String,
        reason: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeliverSummary {
    pub delivered: usize,
    pub dead: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpoolStatus {
    pub root: PathBuf,
    pub outbox_new: usize,
    pub dead: usize,
    pub registered_agents: Vec<String>,
}

/// Listener bound to `inbox/<agent>/wake/<pid>-<ulid>.sock` that unlinks on `Drop`.
pub struct WakeListener {
    listener: Option<UnixListener>,
    sock_path: PathBuf,
}

impl WakeListener {
    pub fn socket_path(&self) -> &Path {
        &self.sock_path
    }

    /// Whether the socket is bound (binding fails e.g. on an over-long path).
    pub fn is_bound(&self) -> bool {
        self.listener.is_some()
    }

    pub fn wait_slice(&self, max_wait: Duration) -> bool {
        self.wait_slice_with_cap(max_wait, ASK_WAIT_BACKSTOP).any
    }

    pub fn wait_slice_with_cap(&self, max_wait: Duration, cap: Duration) -> WakeEvents {
        let slice = max_wait.min(cap);
        if let Some(listener) = &self.listener {
            poll_unix_listener_events(listener, slice)
        } else {
            thread::sleep(slice);
            WakeEvents::default()
        }
    }
}

impl Drop for WakeListener {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.sock_path);
    }
}

/// Encapsulates the `$ORG_HOME/spool` layout and all atomic file operations.
#[derive(Debug, Clone)]
pub struct Spool {
    root: PathBuf,
}

impl Spool {
    /// Resolve `$ORG_HOME` or default to `$HOME/.agency`.
    pub fn resolve_default_root() -> Result<PathBuf, SpoolError> {
        if let Ok(val) = env::var("ORG_HOME") {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Ok(PathBuf::from(trimmed));
            }
        }
        let home = env::var("HOME")
            .map_err(|_| SpoolError::InvalidInput("neither ORG_HOME nor HOME is set".into()))?;
        Ok(PathBuf::from(home).join(".agency"))
    }

    pub fn open_from_env() -> Result<Self, SpoolError> {
        Self::open(Self::resolve_default_root()?)
    }

    pub fn open(root: impl Into<PathBuf>) -> Result<Self, SpoolError> {
        let root = root.into();
        let spool = Self { root };
        spool.ensure_dir(&spool.outbox_tmp_dir())?;
        spool.ensure_dir(&spool.outbox_new_dir())?;
        spool.ensure_dir(&spool.inbox_root_dir())?;
        spool.ensure_dir(&spool.dead_dir())?;
        Ok(spool)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn relay_socket_path(&self) -> PathBuf {
        self.spool_dir().join("relay.sock")
    }

    pub fn relay_lock_path(&self) -> PathBuf {
        self.root.join("relay.lock")
    }

    pub(crate) fn spool_dir(&self) -> PathBuf {
        self.root.join("spool")
    }

    pub(crate) fn outbox_tmp_dir(&self) -> PathBuf {
        self.spool_dir().join("outbox").join("tmp")
    }

    pub(crate) fn outbox_new_dir(&self) -> PathBuf {
        self.spool_dir().join("outbox").join("new")
    }

    pub(crate) fn inbox_root_dir(&self) -> PathBuf {
        self.spool_dir().join("inbox")
    }

    pub(crate) fn agent_inbox_dir(&self, agent: &str) -> PathBuf {
        self.inbox_root_dir().join(agent)
    }

    pub(crate) fn agent_tmp_dir(&self, agent: &str) -> PathBuf {
        self.agent_inbox_dir(agent).join("tmp")
    }

    pub(crate) fn agent_new_dir(&self, agent: &str) -> PathBuf {
        self.agent_inbox_dir(agent).join("new")
    }

    pub(crate) fn agent_cur_dir(&self, agent: &str) -> PathBuf {
        self.agent_inbox_dir(agent).join("cur")
    }

    pub(crate) fn agent_wake_dir(&self, agent: &str) -> PathBuf {
        self.agent_inbox_dir(agent).join("wake")
    }

    pub(crate) fn dead_dir(&self) -> PathBuf {
        self.spool_dir().join("dead")
    }

    pub(crate) fn ensure_dir(&self, path: &Path) -> Result<(), SpoolError> {
        let mut builder = DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(path)?;
        Ok(())
    }

    pub(crate) fn write_file_0600(path: &Path, bytes: &[u8], sync: bool) -> Result<(), SpoolError> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(bytes)?;
        if sync {
            let rc = unsafe { fsync(file.as_raw_fd()) };
            if rc != 0 {
                return Err(SpoolError::Io(io::Error::last_os_error()));
            }
        }
        Ok(())
    }

    /// The `*.json` entries of `dir` (not dot-files), sorted. Anything but a directory is
    /// listed, symlinks and FIFOs included (without following them), so readers treat those
    /// like corrupt files (dead-lettered or skipped) instead of leaving them behind.
    pub(crate) fn sorted_json_entries(dir: &Path) -> Result<Vec<PathBuf>, SpoolError> {
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut files = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if !name.starts_with('.')
                    && name.ends_with(".json")
                    && !entry.file_type().is_ok_and(|t| t.is_dir())
                {
                    files.push(path);
                }
            }
        }
        files.sort();
        Ok(files)
    }

    /// Register an agent's inbox (`inbox/<agent>/{tmp,new,cur,wake}`).
    pub fn register(&self, agent: &str) -> Result<(), SpoolError> {
        validate_agent_id(agent, "register").map_err(SpoolError::InvalidInput)?;
        self.ensure_dir(&self.agent_tmp_dir(agent))?;
        self.ensure_dir(&self.agent_new_dir(agent))?;
        self.ensure_dir(&self.agent_cur_dir(agent))?;
        self.ensure_dir(&self.agent_wake_dir(agent))?;
        Ok(())
    }

    /// Remove an agent's inbox registration.
    pub fn deregister(&self, agent: &str) -> Result<(), SpoolError> {
        validate_agent_id(agent, "deregister").map_err(SpoolError::InvalidInput)?;
        let dir = self.agent_inbox_dir(agent);
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        Ok(())
    }

    /// Check whether `<agent>` has a registered inbox.
    pub fn is_registered(&self, agent: &str) -> bool {
        if validate_agent_id(agent, "to").is_err() {
            return false;
        }
        self.agent_new_dir(agent).is_dir() && self.agent_cur_dir(agent).is_dir()
    }

    /// Atomically write an envelope into `outbox/tmp/<id>.json`, `fsync` it,
    /// and rename it into `outbox/new/<id>.json`.
    pub fn enqueue(&self, env: &Envelope) -> Result<PathBuf, SpoolError> {
        let bytes = env.to_json_bytes().map_err(SpoolError::InvalidInput)?;
        let filename = format!("{}.json", env.id);
        let tmp_path = self.outbox_tmp_dir().join(&filename);
        let new_path = self.outbox_new_dir().join(&filename);

        Self::write_file_0600(&tmp_path, &bytes, true)?;
        fs::rename(&tmp_path, &new_path)?;
        Ok(new_path)
    }

    /// Non-blocking 1-byte nudge to `spool/relay.sock` (ignored if relay is down).
    pub fn nudge_relay(&self) {
        let sock = self.relay_socket_path();
        if let Ok(stream) = UnixStream::connect(&sock) {
            let _ = stream.set_nonblocking(true);
            let _ = (&stream).write_all(b"1");
        }
    }

    /// Bind a per-listener wake socket at `inbox/<agent>/wake/<pid>-<suffix>.sock`.
    ///
    /// The suffix is the last 10 characters of a fresh (monotonic) ULID rather than the whole
    /// ULID: macOS limits a socket path to 103 bytes, and with the full ULID the path of an
    /// agent with a long id under `~/.agency` (e.g. `ac-software-engineer-river`) hit 104, so
    /// `bind` failed and that agent's waits silently fell back to sleep-polling.
    pub fn bind_wake_listener(&self, agent: &str) -> Result<WakeListener, SpoolError> {
        validate_agent_id(agent, "agent").map_err(SpoolError::InvalidInput)?;
        let wake_dir = self.agent_wake_dir(agent);
        self.ensure_dir(&wake_dir)?;
        let ulid = generate_ulid();
        let sock_name = format!("{}-{}.sock", process::id(), &ulid[16..]);
        let sock_path = wake_dir.join(sock_name);
        let _ = fs::remove_file(&sock_path);

        let listener = match UnixListener::bind(&sock_path) {
            Ok(l) => {
                let _ = fs::set_permissions(&sock_path, fs::Permissions::from_mode(0o600));
                let _ = l.set_nonblocking(true);
                Some(l)
            }
            Err(_) => None,
        };

        Ok(WakeListener {
            listener,
            sock_path,
        })
    }

    pub(crate) fn has_live_wake_listener(&self, agent: &str) -> bool {
        let wake_dir = self.agent_wake_dir(agent);
        let Ok(entries) = fs::read_dir(&wake_dir) else {
            return false;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("sock") {
                continue;
            }
            if !socket_owner_pid_is_dead(&path) {
                return true;
            }
        }
        false
    }

    /// Notify all bound wake sockets in `inbox/<agent>/wake/*.sock` with a 1-byte tag:
    /// `b"U"` for urgent messages, `b"N"` for normal messages.
    pub fn notify_agent_wake_sockets(&self, agent: &str, tag: &[u8; 1]) {
        let wake_dir = self.agent_wake_dir(agent);
        let Ok(entries) = fs::read_dir(&wake_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("sock") {
                continue;
            }
            // Only real sockets (the entry's own type, never a symlink's target): a link
            // planted here must not make the relay connect elsewhere.
            if !entry.file_type().is_ok_and(|t| t.is_socket()) {
                continue;
            }
            match UnixStream::connect(&path) {
                Ok(stream) => {
                    let _ = stream.set_nonblocking(true);
                    let _ = (&stream).write_all(tag);
                }
                Err(e) => {
                    if matches!(e.kind(), ErrorKind::ConnectionRefused | ErrorKind::NotFound)
                        || socket_owner_pid_is_dead(&path)
                    {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
        }
    }

    fn safe_dead_stem(&self, candidate: &str) -> String {
        let valid_chars = !candidate.is_empty()
            && candidate.len() <= 64
            && !candidate.starts_with('.')
            && candidate
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'));
        let base = if valid_chars {
            candidate.to_string()
        } else {
            generate_ulid()
        };
        // Taken if either file exists in any form (`symlink_metadata`: a dangling symlink
        // counts too).
        let taken = |stem: &str| {
            ["json", "reason"].iter().any(|ext| {
                fs::symlink_metadata(self.dead_dir().join(format!("{stem}.{ext}"))).is_ok()
            })
        };
        if !taken(&base) {
            base
        } else {
            format!("{base}-{}", generate_ulid())
        }
    }

    pub(crate) fn move_to_dead(
        &self,
        source_file: &Path,
        id_stem: &str,
        reason: &str,
    ) -> Result<DeliverOutcome, SpoolError> {
        let safe_stem = self.safe_dead_stem(id_stem);
        let dead_json = self.dead_dir().join(format!("{safe_stem}.json"));
        let dead_reason = self.dead_dir().join(format!("{safe_stem}.reason"));
        fs::rename(source_file, &dead_json)?;
        create_new_nofollow(&dead_reason, format!("{reason}\n").as_bytes())?;
        Ok(DeliverOutcome::Dead {
            id: safe_stem,
            reason: reason.to_string(),
        })
    }

    /// Route and deliver a single file from `outbox/new/<id>.json` into `inbox/<to>/new/<id>.json`
    /// or `dead/<id>.json` + `dead/<id>.reason`.
    pub fn deliver_one(&self, outbox_file: &Path) -> Result<DeliverOutcome, SpoolError> {
        let file_name = outbox_file
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| SpoolError::InvalidInput("invalid outbox file name".into()))?;
        let id_stem = file_name.strip_suffix(".json").unwrap_or(file_name);

        let parsed = match read_envelope(outbox_file) {
            Ok(Some(parsed)) => parsed,
            Ok(None) => {
                return Err(SpoolError::NotFound(format!(
                    "outbox file {} already moved",
                    outbox_file.display()
                )));
            }
            // A file we can't read (e.g. mode 000) would otherwise block every delivery
            // behind it, for good.
            Err(err) => {
                return self.move_to_dead(outbox_file, id_stem, &format!("unreadable: {err}"));
            }
        };

        let env = match parsed {
            Ok(env) => env,
            Err(err) => {
                return self.move_to_dead(
                    outbox_file,
                    id_stem,
                    &format!("invalid_envelope: {err}"),
                );
            }
        };

        if env.id != id_stem {
            return self.move_to_dead(
                outbox_file,
                id_stem,
                &format!("id_filename_mismatch: file={id_stem} env.id={}", env.id),
            );
        }

        let now_iso = format_iso8601_utc(SystemTime::now());
        if env.is_expired_at(&now_iso) {
            let exp = env.expires_at.as_deref().unwrap_or("unknown");
            return self.move_to_dead(outbox_file, &env.id, &format!("expired: expires_at={exp}"));
        }

        if !self.is_registered(&env.to) {
            return self.move_to_dead(
                outbox_file,
                &env.id,
                &format!("unknown_recipient: {}", env.to),
            );
        }

        let urgent = env.is_urgent();
        let tag = if urgent { b"U" } else { b"N" };
        let dest_path = self.agent_new_dir(&env.to).join(format!("{}.json", env.id));
        fs::rename(outbox_file, &dest_path)?;
        self.notify_agent_wake_sockets(&env.to, tag);

        Ok(DeliverOutcome::Delivered {
            id: env.id,
            to: env.to,
            urgent,
        })
    }

    /// Returns `true` if `outbox/new` contains at least one `.json` entry that isn't a
    /// directory (the same entries [`Spool::deliver_all`] handles, so a `*.json` directory
    /// can't keep the relay loop busy).
    pub fn has_pending_outbox(&self) -> bool {
        let Ok(entries) = fs::read_dir(self.outbox_new_dir()) else {
            return false;
        };
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if !name.starts_with('.')
                    && name.ends_with(".json")
                    && !entry.file_type().is_ok_and(|t| t.is_dir())
                {
                    return true;
                }
            }
        }
        false
    }

    /// Deliver all pending messages currently in `outbox/new/`. One file failing (e.g. an
    /// inbox it can't be moved into) doesn't stop the others: every file is tried, then the
    /// first error is returned.
    pub fn deliver_all(&self) -> Result<DeliverSummary, SpoolError> {
        let mut summary = DeliverSummary::default();
        let mut first_error = None;
        for path in Self::sorted_json_entries(&self.outbox_new_dir())? {
            match self.deliver_one(&path) {
                Ok(DeliverOutcome::Delivered { .. }) => summary.delivered += 1,
                Ok(DeliverOutcome::Dead { .. }) => summary.dead += 1,
                Err(SpoolError::NotFound(_)) => continue,
                Err(e) => {
                    first_error.get_or_insert(e);
                }
            }
        }
        match first_error {
            Some(e) => Err(e),
            None => Ok(summary),
        }
    }

    /// List unread messages in `inbox/<agent>/new/` (and `cur/` when `include_cur` is true)
    /// without claiming them. Corrupt files in `new/` are quarantined to `dead/`.
    pub fn list_inbox(&self, agent: &str, include_cur: bool) -> Result<Vec<Envelope>, SpoolError> {
        validate_agent_id(agent, "agent").map_err(SpoolError::InvalidInput)?;
        let mut out = Vec::new();
        for path in Self::sorted_json_entries(&self.agent_new_dir(agent))? {
            let Ok(Some(parsed)) = read_envelope(&path) else {
                continue;
            };
            match parsed {
                Ok(env) => out.push(env),
                Err(err) => {
                    let stem = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("corrupt");
                    let _ = self.move_to_dead(&path, stem, &format!("corrupt_inbox_file: {err}"));
                }
            }
        }
        if include_cur {
            for path in Self::sorted_json_entries(&self.agent_cur_dir(agent))? {
                if let Ok(Some(Ok(env))) = read_envelope(&path) {
                    out.push(env);
                }
            }
            out.sort_by(|a, b| a.id.cmp(&b.id));
        }
        Ok(out)
    }

    /// Claim a specific message by ULID (`inbox/<agent>/new/<id>.json` -> `cur/<id>.json`).
    pub fn claim(&self, agent: &str, id: &str) -> Result<Envelope, SpoolError> {
        validate_agent_id(agent, "agent").map_err(SpoolError::InvalidInput)?;
        if !is_valid_ulid(id) {
            return Err(SpoolError::InvalidInput(format!(
                "invalid message id '{id}'"
            )));
        }
        let fname = format!("{id}.json");
        let new_path = self.agent_new_dir(agent).join(&fname);
        let cur_path = self.agent_cur_dir(agent).join(&fname);

        // `defer` may move the file between new/ and cur/ while we look (it holds the meta lock;
        // we don't, to keep reads cheap), so a miss in both places is retried once.
        for _ in 0..2 {
            if let Some(parsed) = read_envelope(&new_path)? {
                let env = match parsed {
                    Ok(e) => e,
                    Err(err) => {
                        let _ =
                            self.move_to_dead(&new_path, id, &format!("corrupt_inbox_file: {err}"));
                        return Err(SpoolError::InvalidInput(format!(
                            "corrupt message '{id}' moved to dead/: {err}"
                        )));
                    }
                };
                self.ensure_dir(&self.agent_cur_dir(agent))?;
                if self.claim_into_cur(agent, id, &new_path, &cur_path)? {
                    return Ok(env);
                }
            }
            if let Some(parsed) = read_envelope(&cur_path)? {
                return parsed.map_err(SpoolError::InvalidInput);
            }
        }

        Err(SpoolError::NotFound(format!(
            "message '{id}' not found in inbox for '{agent}'"
        )))
    }

    /// Claim the oldest unsnoozed unread message in `inbox/<agent>/new/` (`new` -> `cur`), skipping
    /// currently snoozed messages and dead-lettering any corrupted file so `read --next` never wedges.
    pub fn claim_next(&self, agent: &str) -> Result<Option<Envelope>, SpoolError> {
        validate_agent_id(agent, "agent").map_err(SpoolError::InvalidInput)?;
        let triage = self.release_due_snoozes(agent, false)?;
        for path in Self::sorted_json_entries(&self.agent_new_dir(agent))? {
            let Some(stem) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".json"))
            else {
                continue;
            };
            if !is_valid_ulid(stem) {
                let _ = self.move_to_dead(&path, stem, "invalid_inbox_filename");
                continue;
            }
            if let Some(entry) = triage.messages.get(stem) {
                if entry.snoozed_until.is_some() || entry.done_at.is_some() {
                    continue;
                }
            }
            match self.claim(agent, stem) {
                Ok(env) => return Ok(Some(env)),
                Err(SpoolError::NotFound(_)) | Err(SpoolError::InvalidInput(_)) => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(None)
    }

    /// Find a message in `inbox/<agent>/new/` or `cur/` without changing its read state
    /// (used by `org-agent msg reply <id>`).
    pub fn find_for_reply(&self, agent: &str, id: &str) -> Result<Envelope, SpoolError> {
        validate_agent_id(agent, "agent").map_err(SpoolError::InvalidInput)?;
        if !is_valid_ulid(id) {
            return Err(SpoolError::InvalidInput(format!(
                "invalid message id '{id}'"
            )));
        }
        let fname = format!("{id}.json");
        // Retried once: a concurrent `defer` or `read` may be moving it between new/ and cur/.
        for _ in 0..2 {
            for dir in [self.agent_new_dir(agent), self.agent_cur_dir(agent)] {
                if let Some(parsed) = read_envelope(&dir.join(&fname))? {
                    return parsed.map_err(SpoolError::InvalidInput);
                }
            }
        }
        Err(SpoolError::NotFound(format!(
            "message '{id}' not found in inbox for '{agent}'"
        )))
    }

    /// Scan `inbox/<agent>/new/` for a reply with `reply_to == Some(req_id)` and claim ONLY
    /// that message, leaving all other unread messages in `new/`.
    pub fn claim_reply_to(
        &self,
        agent: &str,
        req_id: &str,
    ) -> Result<Option<Envelope>, SpoolError> {
        validate_agent_id(agent, "agent").map_err(SpoolError::InvalidInput)?;
        for path in Self::sorted_json_entries(&self.agent_new_dir(agent))? {
            let Ok(Some(parsed)) = read_envelope(&path) else {
                continue;
            };
            let env = match parsed {
                Ok(env) => env,
                Err(err) => {
                    let stem = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("corrupt");
                    let _ = self.move_to_dead(&path, stem, &format!("corrupt_inbox_file: {err}"));
                    continue;
                }
            };
            if env.reply_to.as_deref() == Some(req_id) {
                let cur_path = self.agent_cur_dir(agent).join(format!("{}.json", env.id));
                // Same locked claim as `msg read`, so a concurrent `defer` can't interleave.
                if self.claim_into_cur(agent, &env.id, &path, &cur_path)? {
                    return Ok(Some(env));
                }
            }
        }
        Ok(None)
    }

    /// Wait using a pre-bound `WakeListener` until a reply with `reply_to == req_id` arrives
    /// in `inbox/<agent>/new/`, claiming only that reply and leaving other messages unread.
    pub fn wait_for_reply_with_listener(
        &self,
        agent: &str,
        req_id: &str,
        listener: &WakeListener,
        timeout: Duration,
    ) -> Result<Envelope, SpoolError> {
        let deadline = wait_deadline(timeout)?;
        loop {
            if is_interrupted() {
                return Err(SpoolError::Interrupted);
            }
            if let Some(reply) = self.claim_reply_to(agent, req_id)? {
                return Ok(reply);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(SpoolError::Timeout(format!(
                    "timed out after {}ms waiting for reply to {req_id}",
                    timeout.as_millis()
                )));
            }
            let remaining = deadline.saturating_duration_since(now);
            listener.wait_slice(remaining);
        }
    }

    /// Wait until an unsnoozed unread message (optionally matching `from_filter` and `urgent_only`)
    /// is present in `inbox/<agent>/new/` and return it without marking it read.
    pub fn wait_for_inbox_filtered(
        &self,
        agent: &str,
        from_filter: Option<&str>,
        urgent_only: bool,
        timeout: Duration,
    ) -> Result<Envelope, SpoolError> {
        validate_agent_id(agent, "agent").map_err(SpoolError::InvalidInput)?;
        if let Some(f) = from_filter {
            validate_agent_id(f, "from").map_err(SpoolError::InvalidInput)?;
        }
        let deadline = wait_deadline(timeout)?;
        self.register(agent)?;
        let listener = self.bind_wake_listener(agent)?;

        loop {
            if is_interrupted() {
                return Err(SpoolError::Interrupted);
            }
            let triage = self.release_due_snoozes(agent, false)?;
            for env in self.list_inbox(agent, false)? {
                if let Some(entry) = triage.messages.get(&env.id) {
                    if entry.snoozed_until.is_some() || entry.done_at.is_some() {
                        continue;
                    }
                }
                if urgent_only && !env.is_urgent_at(&format_iso8601_utc(SystemTime::now())) {
                    continue;
                }
                if from_filter.map(|f| env.from == f).unwrap_or(true) {
                    return Ok(env);
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(SpoolError::Timeout(format!(
                    "timed out after {}ms waiting for message in {agent}'s inbox",
                    timeout.as_millis()
                )));
            }
            let remaining = deadline.saturating_duration_since(now);
            let cap = if listener.is_bound() {
                INBOX_WAIT_BACKSTOP
            } else {
                INBOX_WAIT_NO_SOCKET
            };
            listener.wait_slice_with_cap(remaining, cap);
        }
    }

    /// Wait until an unsnoozed unread message (optionally matching `from_filter`) is present in
    /// `inbox/<agent>/new/` and return it.
    pub fn wait_for_inbox(
        &self,
        agent: &str,
        from_filter: Option<&str>,
        timeout: Duration,
    ) -> Result<Envelope, SpoolError> {
        self.wait_for_inbox_filtered(agent, from_filter, false, timeout)
    }

    /// Prune messages in `inbox/*/cur/` older than 30 days.
    pub fn prune_old_cur(&self) -> Result<usize, SpoolError> {
        let inbox_root = self.inbox_root_dir();
        if !inbox_root.is_dir() {
            return Ok(0);
        }
        let cutoff = SystemTime::now()
            .checked_sub(Duration::from_secs(RETENTION_SECS))
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let mut pruned = 0usize;

        for agent_entry in fs::read_dir(&inbox_root)? {
            let agent_entry = agent_entry?;
            let cur_dir = agent_entry.path().join("cur");
            for file_path in Self::sorted_json_entries(&cur_dir)? {
                if let Ok(meta) = fs::symlink_metadata(&file_path) {
                    if let Ok(modified) = meta.modified() {
                        if modified < cutoff && fs::remove_file(&file_path).is_ok() {
                            pruned += 1;
                        }
                    }
                }
            }
        }
        Ok(pruned)
    }

    /// Return current spool statistics (`outbox_new`, `dead`, `registered_agents`).
    pub fn status(&self) -> Result<SpoolStatus, SpoolError> {
        let outbox_new = Self::sorted_json_entries(&self.outbox_new_dir())?.len();
        let dead = Self::sorted_json_entries(&self.dead_dir())?.len();
        let mut registered_agents = Vec::new();
        if let Ok(entries) = fs::read_dir(self.inbox_root_dir()) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if !name.starts_with('.') && self.is_registered(name) {
                        registered_agents.push(name.to_string());
                    }
                }
            }
        }
        registered_agents.sort();
        Ok(SpoolStatus {
            root: self.root.clone(),
            outbox_new,
            dead,
            registered_agents,
        })
    }
}
