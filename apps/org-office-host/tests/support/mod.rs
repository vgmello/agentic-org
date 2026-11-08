//! Shared helpers for the host's integration tests: a short temporary office home (never the
//! real one) with its own development key folder (never the Keychain), the fake office server
//! and its script and log, and a small org-proto client.

#![allow(dead_code)] // each test file uses a different part

use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use org_office_host::{Config, Host, HostError, Timings};
use org_office_launch::OfficeFile;
use org_proto::{ApiError, Call, Event, OfficeRole, Request, Response, ServerMessage};
use serde_json::Value;

/// The fake office server, built for the tests only.
pub fn fake_server() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake-office-server"))
}

/// Timings short enough for tests, long enough not to be flaky.
pub fn fast_timings() -> Timings {
    Timings {
        ready_timeout: Duration::from_secs(10),
        create_timeout: Duration::from_secs(10),
        backoff_initial: Duration::from_millis(100),
        backoff_max: Duration::from_millis(400),
        backoff_reset_after: Duration::from_secs(60),
        give_up_after: 10,
        give_up_window: Duration::from_secs(30 * 60),
        shutdown_grace: Duration::from_secs(5),
        discard_timeout: Duration::from_secs(10),
        create_wait: Duration::from_secs(10),
        exit_poll: Duration::from_millis(100),
    }
}

/// A temporary office home under /tmp (socket paths must stay under 103 bytes), with its own
/// development key folder next to it (outside the home, so no key is ever in the home or its
/// logs).
pub struct TestHome {
    dir: tempfile::TempDir,
}

impl TestHome {
    pub fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("oh")
            .tempdir_in("/tmp")
            .unwrap();
        Self { dir }
    }

    /// The office home itself, a folder inside the temp dir (so the host makes it).
    pub fn path(&self) -> PathBuf {
        self.dir.path().join("h")
    }

    /// The host's development key folder (`<id>.key` files), outside the office home.
    pub fn keys_dir(&self) -> PathBuf {
        self.dir.path().join("k")
    }

    pub fn config(&self) -> Config {
        let mut config = Config::new(self.path(), fake_server());
        config.timings = fast_timings();
        #[cfg(debug_assertions)]
        {
            config.dev_key_dir = Some(self.keys_dir());
        }
        config
    }

    /// Writes the fake server's script (see `testing/fake_office_server.rs`).
    pub fn script(&self, script: Value) {
        fs::create_dir_all(self.path()).unwrap();
        fs::write(
            self.path().join("fake-script.json"),
            serde_json::to_vec(&script).unwrap(),
        )
        .unwrap();
    }

    /// Stores an office as if the host had created it: its `office.json`, and a key in the key
    /// folder (unless it has one).
    pub fn write_office(&self, office: &OfficeFile) {
        let dir = self.office_dir(&office.office_id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("office.json"), office.to_vec().unwrap()).unwrap();
        if !self.has_key(&office.office_id) {
            self.write_key(&office.office_id);
        }
    }

    fn key_path(&self, office_id: &str) -> PathBuf {
        self.keys_dir().join(format!("{office_id}.key"))
    }

    /// Writes a new random key for `office_id` in the development store's format (64 hex
    /// digits, owner-only), replacing any.
    pub fn write_key(&self, office_id: &str) {
        let mut bytes = [0u8; 32];
        fs::File::open("/dev/urandom")
            .unwrap()
            .read_exact(&mut bytes)
            .unwrap();
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        self.write_key_text(office_id, &hex);
    }

    /// Writes `text` as `office_id`'s key file, replacing any (e.g. one that isn't a key).
    pub fn write_key_text(&self, office_id: &str, text: &str) {
        std::os::unix::fs::DirBuilderExt::mode(fs::DirBuilder::new().recursive(true), 0o700)
            .create(self.keys_dir())
            .unwrap();
        let _ = fs::remove_file(self.key_path(office_id));
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.key_path(office_id))
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }

    pub fn remove_key(&self, office_id: &str) {
        fs::remove_file(self.key_path(office_id)).unwrap();
    }

    pub fn has_key(&self, office_id: &str) -> bool {
        self.key_path(office_id).exists()
    }

    /// The raw text of `office_id`'s key file.
    pub fn key_text(&self, office_id: &str) -> Option<String> {
        fs::read_to_string(self.key_path(office_id)).ok()
    }

    /// The fingerprint of `office_id`'s stored key, as the fake records the keys it gets.
    pub fn stored_key_fingerprint(&self, office_id: &str) -> Option<String> {
        self.key_text(office_id)
            .map(|hex| fingerprint_hex(hex.trim()))
    }

    /// The office ids that have a key in the key folder.
    pub fn key_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = fs::read_dir(self.keys_dir())
            .map(|entries| {
                entries
                    .map(|e| e.unwrap().file_name().into_string().unwrap())
                    .filter_map(|name| name.strip_suffix(".key").map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        ids.sort();
        ids
    }

    pub fn office_dir(&self, office_id: &str) -> PathBuf {
        self.path().join("offices").join(office_id)
    }

    /// The names of the folders under `offices/`.
    pub fn office_folders(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.path().join("offices"))
            .map(|entries| {
                entries
                    .map(|e| e.unwrap().file_name().into_string().unwrap())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    /// The names of the folders under `offices/.refused/`, where refused creates are moved.
    pub fn refused_folders(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.path().join("offices/.refused"))
            .map(|entries| {
                entries
                    .map(|e| e.unwrap().file_name().into_string().unwrap())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    /// How many times the fake ran for `key` ("create" or an office id).
    pub fn runs(&self, key: &str) -> u64 {
        fs::metadata(self.path().join("fake-runs").join(key))
            .map(|m| m.len())
            .unwrap_or(0)
    }

    fn log_fields(&self, kind: &str, key: &str) -> Vec<Vec<String>> {
        fs::read_to_string(self.path().join("fake.log"))
            .unwrap_or_default()
            .lines()
            .map(|l| l.split(' ').map(str::to_owned).collect::<Vec<_>>())
            .filter(|f| f[0] == kind && f[1] == key)
            .collect()
    }

    /// The pids of the fake's runs for `key`, in order.
    pub fn pids(&self, key: &str) -> Vec<u32> {
        self.log_fields("start", key)
            .iter()
            .map(|f| f[2].parse().unwrap())
            .collect()
    }

    /// When each run for `key` started (Unix ms).
    pub fn start_times(&self, key: &str) -> Vec<u128> {
        self.log_fields("start", key)
            .iter()
            .map(|f| f[4].parse().unwrap())
            .collect()
    }

    /// The pids that exited cleanly on stdin EOF.
    pub fn eof_exits(&self, key: &str) -> Vec<u32> {
        self.log_fields("eof-exit", key)
            .iter()
            .map(|f| f[2].parse().unwrap())
            .collect()
    }

    /// The pids that saw stdin EOF and ignored it.
    pub fn eof_ignored(&self, key: &str) -> Vec<u32> {
        self.log_fields("eof-ignored", key)
            .iter()
            .map(|f| f[2].parse().unwrap())
            .collect()
    }

    /// The fingerprints of the app tokens the fake's runs for `key` got, in order (one per run
    /// that accepted its first line).
    pub fn token_fingerprints(&self, key: &str) -> Vec<String> {
        self.log_fields("token", key)
            .iter()
            .map(|f| f[3].clone())
            .collect()
    }

    /// The fingerprints of the database keys the fake's runs for `key` ("create" or an office
    /// id) got, in order (`null` for none).
    pub fn db_key_fingerprints(&self, key: &str) -> Vec<String> {
        self.log_fields("dbkey", key)
            .iter()
            .map(|f| f[3].clone())
            .collect()
    }

    /// The `--discard` runs that read their first line, in order: the office id and the
    /// fingerprint of the key the line carried (`null` for none).
    pub fn discard_keys(&self) -> Vec<(String, String)> {
        self.log_fields("dbkey", "discard")
            .iter()
            .map(|f| (f[4].clone(), f[3].clone()))
            .collect()
    }

    /// Kills the `sleep`s the fake left holding its stdout (`orphan_stdout_ms`).
    pub fn kill_orphans(&self) {
        for fields in fs::read_to_string(self.path().join("fake.log"))
            .unwrap_or_default()
            .lines()
            .map(|l| l.split(' ').collect::<Vec<_>>())
            .filter(|f| f[0] == "orphan")
        {
            if let Ok(pid) = fields[3].parse::<libc::pid_t>() {
                // SAFETY: signals a process the fake started for this test.
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
        }
    }

    /// The pids whose stdin closed before their first line (they exited 0).
    pub fn eof_before_first_line(&self, key: &str) -> Vec<u32> {
        self.log_fields("eof-before-first-line", key)
            .iter()
            .map(|f| f[2].parse().unwrap())
            .collect()
    }

    /// The environment variable names each of the fake's runs for `key` got.
    pub fn env_names(&self, key: &str) -> Vec<Vec<String>> {
        self.log_fields("env", key)
            .iter()
            .map(|f| f[3].split(',').map(str::to_owned).collect())
            .collect()
    }

    /// The pids that refused their first line.
    pub fn first_line_refusals(&self, key: &str) -> Vec<u32> {
        self.log_fields("first-line-refused", key)
            .iter()
            .map(|f| f[2].parse().unwrap())
            .collect()
    }

    /// Writes an executable `sh` script next to the office home (not in it) and returns its
    /// path, to use as the server program. `$FAKE` in `body` is the fake server.
    pub fn wrapper(&self, name: &str, body: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        let script = format!(
            "#!/bin/sh\nFAKE='{}'\n{body}\n",
            fake_server().to_str().unwrap()
        );
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    /// The text of every office log (`<home>/logs/*`) and of `fake.log`, with the file names.
    pub fn logs(&self) -> Vec<(PathBuf, String)> {
        let mut files: Vec<PathBuf> = fs::read_dir(self.path().join("logs"))
            .map(|entries| entries.map(|e| e.unwrap().path()).collect())
            .unwrap_or_default();
        files.push(self.path().join("fake.log"));
        files
            .into_iter()
            .filter_map(|path| {
                let bytes = fs::read(&path).ok()?;
                Some((path, String::from_utf8_lossy(&bytes).into_owned()))
            })
            .collect()
    }

    /// Asserts no log holds an app token's wire form: 64 lowercase hex digits in a row. Tests
    /// never see the tokens, so this looks for any such run.
    pub fn assert_no_token_in_logs(&self) {
        // An environment variable that already looks like a token in the tests' own environment
        // (e.g. a SHA-256 digest set by CI) reached the fake through inheritance, not the host.
        let inherited = |line: &str| {
            line.rsplit(' ')
                .next()
                .and_then(|place| place.strip_prefix("env:"))
                .is_some_and(|name| std::env::var(name).is_ok_and(|value| has_hex_run(&value, 64)))
        };
        let leaks: Vec<_> = fs::read_to_string(self.path().join("fake.log"))
            .unwrap_or_default()
            .lines()
            .filter(|l| l.starts_with("leak ") && !inherited(l))
            .map(str::to_owned)
            .collect();
        assert!(leaks.is_empty(), "a token reached the fake: {leaks:?}");
        for (path, text) in self.logs() {
            assert!(
                !has_hex_run(&text, 64),
                "{} holds 64 hex digits in a row",
                path.display()
            );
        }
    }

    /// Asserts no secret (an app token or a database key: 64 lowercase hex digits in a row) is
    /// in any file in the office home (its logs, the offices' folders, `.refused/`, the fake's
    /// files), and no stored key's hex is either: keys live only in the key folder, outside the
    /// home. Also checks the logs as [`TestHome::assert_no_token_in_logs`] does.
    pub fn assert_no_secret_in_home(&self) {
        self.assert_no_token_in_logs();
        let keys: Vec<String> = self
            .key_ids()
            .iter()
            .filter_map(|id| self.key_text(id))
            .map(|hex| hex.trim().to_lowercase())
            .collect();
        let mut folders = vec![self.path()];
        while let Some(folder) = folders.pop() {
            let Ok(entries) = fs::read_dir(&folder) else {
                continue;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                let meta = fs::symlink_metadata(&path).unwrap();
                if meta.is_dir() {
                    folders.push(path);
                } else if meta.is_file() {
                    let text = String::from_utf8_lossy(&fs::read(&path).unwrap()).into_owned();
                    assert!(
                        !has_hex_run(&text, 64),
                        "{} holds 64 hex digits in a row",
                        path.display()
                    );
                    for key in &keys {
                        assert!(
                            !text.contains(key.as_str()),
                            "{} holds a key",
                            path.display()
                        );
                    }
                }
            }
        }
    }
}

/// Whether `text` has `len` or more lowercase hex digits in a row.
pub fn has_hex_run(text: &str, len: usize) -> bool {
    let mut run = 0;
    for b in text.bytes() {
        run = if matches!(b, b'0'..=b'9' | b'a'..=b'f') {
            run + 1
        } else {
            0
        };
        if run >= len {
            return true;
        }
    }
    false
}

/// The fingerprint the fake records for a token (64-bit FNV-1a of its 32 bytes, as hex), so a
/// test can match a token the host hands out with the one a server got.
pub fn fingerprint(token: &org_proto::AppToken) -> String {
    let json = serde_json::to_string(token).unwrap();
    fingerprint_hex(json.trim_matches('"'))
}

/// The fingerprint of a secret given as 64 hex digits (see [`fingerprint`]).
pub fn fingerprint_hex(hex: &str) -> String {
    let hex = hex.to_lowercase();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for i in (0..hex.len()).step_by(2) {
        let byte = u8::from_str_radix(&hex[i..i + 2], 16).unwrap();
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// `at` as RFC 3339 UTC with milliseconds (the contract's timestamps sort as strings).
pub fn rfc3339(at: std::time::SystemTime) -> String {
    let since = at.duration_since(std::time::UNIX_EPOCH).unwrap();
    let (secs, millis) = (since.as_secs(), since.subsec_millis());
    let (days, rem) = (secs / 86_400, secs % 86_400);
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

/// The office `office_id` as `office.list` shows it now.
pub fn office_info(socket: &Path, office_id: &str) -> Option<org_proto::OfficeInfo> {
    Client::connect(socket)
        .call::<org_proto::OfficeList>(&org_proto::Empty {})
        .unwrap()
        .offices
        .into_iter()
        .find(|o| o.id == office_id)
}

/// An HQ office file.
pub fn hq(office_id: &str, org_id: &str, name: &str, created_at: &str) -> OfficeFile {
    OfficeFile::new(office_id, "HQ", OfficeRole::Hq, org_id, name, created_at)
}

/// Starts a host on a home another in-process host just shut down. Retries `AlreadyRunning` for
/// a while: a child that another test spawns at that moment holds a copy of every fd (the old
/// `host.lock` too) until its `exec` completes (usually a few ms). A real host restart is a new
/// process, which can't hit this.
pub fn restart_host(config: &Config) -> Host {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match Host::start(config.clone()) {
            Ok(host) => return host,
            Err(HostError::AlreadyRunning { .. }) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => panic!("{e}"),
        }
    }
}

/// Polls `check` until it holds, or panics after `timeout`.
pub fn eventually(timeout: Duration, what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// Whether a process exists (a reaped child doesn't).
pub fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 only checks the pid.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// A small blocking org-proto client.
pub struct Client {
    out: UnixStream,
    reader: BufReader<UnixStream>,
    next_id: u64,
    events: VecDeque<Event>,
}

impl Client {
    pub fn connect(path: &Path) -> Self {
        let stream = UnixStream::connect(path).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        Self {
            out: stream.try_clone().unwrap(),
            reader: BufReader::new(stream),
            next_id: 1,
            events: VecDeque::new(),
        }
    }

    /// Calls `C` and waits for its answer (events that arrive meanwhile are kept).
    pub fn call<C: Call>(&mut self, params: &C::Params) -> Result<C::Result, ApiError> {
        let id = self.next_id;
        self.next_id += 1;
        let request = Request::new::<C>(id, params).unwrap();
        let mut line = serde_json::to_vec(&request).unwrap();
        line.push(b'\n');
        self.out.write_all(&line).unwrap();
        let response = self.response();
        assert_eq!(response.id, id);
        response.into_result::<C>().map_err(|e| match e {
            org_proto::CallError::Api(e) => e,
            other => panic!("{other}"),
        })
    }

    /// Sends a raw line and returns the next response.
    pub fn raw(&mut self, line: &str) -> Response {
        self.out.write_all(line.as_bytes()).unwrap();
        self.out.write_all(b"\n").unwrap();
        self.response()
    }

    /// The next event, or `None` if none arrives within `timeout`.
    pub fn next_event(&mut self, timeout: Duration) -> Option<Event> {
        if let Some(event) = self.events.pop_front() {
            return Some(event);
        }
        // macOS refuses (EINVAL) to set a timeout on a socket whose peer already closed: that
        // is EOF.
        if self
            .reader
            .get_ref()
            .set_read_timeout(Some(timeout))
            .is_err()
        {
            return None;
        }
        let message = self.read();
        let _ = self
            .reader
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(20)));
        match message? {
            ServerMessage::Event(event) => Some(event),
            other => panic!("expected an event, got {other:?}"),
        }
    }

    /// Every event that arrives within `timeout` (fewer if the connection closes).
    pub fn events_within(&mut self, timeout: Duration) -> Vec<Event> {
        let deadline = Instant::now() + timeout;
        let mut events = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return events;
            }
            match self.next_event(left) {
                Some(event) => events.push(event),
                None => return events,
            }
        }
    }

    /// The next event named `name` (others are skipped), or `None` within `timeout`.
    pub fn next_event_named(&mut self, name: &str, timeout: Duration) -> Option<Event> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            let event = self.next_event(left)?;
            if event.event == name {
                return Some(event);
            }
        }
    }

    fn response(&mut self) -> Response {
        loop {
            match self.read().expect("the host answered") {
                ServerMessage::Response(response) => return response,
                ServerMessage::Event(event) => self.events.push_back(event),
                ServerMessage::Unknown(v) => panic!("unexpected {v}"),
            }
        }
    }

    fn read(&mut self) -> Option<ServerMessage> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(serde_json::from_str(&line).unwrap()),
        }
    }
}
