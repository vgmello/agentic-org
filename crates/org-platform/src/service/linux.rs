//! A systemd user unit, `~/.config/systemd/user/agentic-org-<instance-id>.service`.
//!
//! The unit is `Type=exec` with `Restart=on-failure`, enabled for `default.target`: the
//! user's manager starts the host when the manager starts, restarts it after a crash, and
//! an explicit stop (`SIGTERM`, a clean exit) stays stopped. `KillMode=mixed` sends
//! `SIGTERM` to the host alone and, after `TimeoutStopSec=15`, `SIGKILL` to what is left of
//! its group; the host itself stops within 8 s.
//!
//! The launchd adapter's guarantees, in systemd's terms:
//!
//! - **Nothing under the name that is someone else's is ever taken over.** A unit systemd
//!   holds under the name from any file other than our definition (its `FragmentPath`: a
//!   unit in another directory, a transient or a masked one) is a conflict for register,
//!   start, stop, status and unregister. A definition file with different bytes than the
//!   rendered unit is a conflict for register, which never replaces, reloads or enables it.
//! - **An identical, settled registration is left alone**: the file has the rendered bytes,
//!   systemd loaded it from that file, nothing changed on disk since, and it is enabled.
//!   A healthy service is not restarted.
//! - **The definition is published complete and never replaces a file that appeared
//!   meanwhile**: a temporary file, flushed, then a hard link.
//! - **`start` never loads a definition.** systemd reads a unit file when it is first
//!   asked about it, so "loaded" means here what register established: enabled, and
//!   unchanged on disk since systemd read it. A unit that is not enabled, or whose file
//!   changed since (`NeedDaemonReload`), is `Unloaded`: `start` refuses it and the caller
//!   runs `register` again, which compares the bytes. Only register runs `daemon-reload`
//!   and `enable`.
//! - **`unregister` waits** until systemd no longer knows the unit.
//!
//! systemd is asked through `systemctl --user` and logind through `loginctl`, run as
//! programs by absolute path (the first of `/usr/bin` and `/bin` that has them), with an
//! explicit environment and no shell: this crate links nothing but the C library. Without
//! `XDG_RUNTIME_DIR`, without `systemctl`, or when the user manager does not answer,
//! every operation answers [`ServiceError::Unavailable`] and changes nothing.
//!
//! What the unit-file grammar cannot carry safely is refused before anything is written
//! ([`unit_file_safe`]); every other value is quoted and escaped ([`quote`]).
use super::{
    valid_instance_id, validate, Lingering, Placement, Registration, ServiceError, ServiceManager,
    ServiceStatus, Unit,
};
use crate::Refused;
use std::{
    ffi::{OsStr, OsString},
    fs,
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// Where the two programs are looked for, in order. Nothing else is searched.
const SEARCH: [&str; 2] = ["/usr/bin", "/bin"];
/// How long unregister waits for systemd to forget the unit after `disable --now`, which
/// itself returns only when the unit has stopped (`TimeoutStopSec=15`).
const UNLOAD_WAIT: Duration = Duration::from_secs(20);
pub(super) const USER_PREFIX: &str = "agentic-org-";
/// What the adapter reads of a unit, in one `systemctl show`.
const PROPERTIES: &str =
    "--property=LoadState,ActiveState,MainPID,FragmentPath,UnitFileState,NeedDaemonReload";

pub(super) fn user_placement(home: &Path) -> Result<Placement, Refused> {
    Ok(Placement {
        prefix: USER_PREFIX.into(),
        directory: home.join(".config/systemd/user"),
    })
}

pub(super) fn registered(placement: Placement) -> Result<Box<dyn ServiceManager>, Refused> {
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    Ok(Box::new(Systemd::new(
        placement,
        uid,
        Box::new(System::from_environment()),
    )))
}

/// The outcome of one run of `systemctl` or `loginctl`.
pub(super) struct Outcome {
    pub code: i32,
    pub stdout: String,
    /// The program's own words for a failure, shown to the person after escaping.
    pub stderr: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Program {
    Systemctl,
    Loginctl,
}

impl Program {
    fn name(self) -> &'static str {
        match self {
            Program::Systemctl => "systemctl",
            Program::Loginctl => "loginctl",
        }
    }
}

/// Runs the two programs; tests substitute a recording fake.
pub(super) trait Commands: Send + Sync {
    /// The user's runtime directory (`XDG_RUNTIME_DIR`), where the user manager listens.
    fn runtime_dir(&self) -> Option<PathBuf>;
    /// `NotFound` when the program is not installed.
    fn run(&self, program: Program, args: &[OsString]) -> io::Result<Outcome>;
}

struct System {
    /// The directories the two programs are looked for in, in order.
    search: Vec<PathBuf>,
    runtime_dir: Option<PathBuf>,
}

impl System {
    /// Reads `XDG_RUNTIME_DIR` once; only an absolute path counts.
    fn from_environment() -> Self {
        Self {
            search: SEARCH.iter().map(PathBuf::from).collect(),
            runtime_dir: std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .filter(|dir| dir.is_absolute()),
        }
    }
}

/// The adapter as [`registered`] makes it, except that its two programs are looked for in
/// `programs` alone and the runtime directory is given: consumer tests drive the real
/// adapter and its real runner against stand-in programs, never against systemd.
#[cfg(any(test, feature = "test-support"))]
pub(super) fn registered_with_programs(
    placement: Placement,
    programs: &Path,
    runtime_dir: Option<PathBuf>,
) -> Result<Box<dyn ServiceManager>, Refused> {
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    let system = System {
        search: vec![programs.to_owned()],
        runtime_dir: runtime_dir.filter(|dir| dir.is_absolute()),
    };
    Ok(Box::new(Systemd::new(placement, uid, Box::new(system))))
}

impl Commands for System {
    fn runtime_dir(&self) -> Option<PathBuf> {
        self.runtime_dir.clone()
    }

    fn run(&self, program: Program, args: &[OsString]) -> io::Result<Outcome> {
        let path = self
            .search
            .iter()
            .map(|dir| dir.join(program.name()))
            .find(|path| path.is_file())
            .ok_or_else(|| {
                let searched: Vec<_> = self
                    .search
                    .iter()
                    .map(|d| d.display().to_string())
                    .collect();
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("{} is not in {}", program.name(), searched.join(" or ")),
                )
            })?;
        let mut command = Command::new(path);
        command
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .stdin(Stdio::null());
        if let Some(dir) = &self.runtime_dir {
            command.env("XDG_RUNTIME_DIR", dir);
        }
        let out = command.output()?;
        Ok(Outcome {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}

/// What `systemctl show` says of a unit.
#[derive(Debug, Default, PartialEq, Eq)]
struct Shown {
    load: String,
    active: String,
    pid: u32,
    fragment: String,
    file_state: String,
    needs_reload: bool,
}

impl Shown {
    /// The main process, while the unit has one that is not just scheduled to start.
    fn running(&self) -> Option<u32> {
        let live = matches!(
            self.active.as_str(),
            "active" | "reloading" | "activating" | "deactivating"
        );
        (live && self.pid != 0).then_some(self.pid)
    }

    /// What register established and nothing has changed since: systemd read our file as
    /// it is now, and the unit is enabled.
    fn settled(&self) -> bool {
        self.load == "loaded" && self.file_state == "enabled" && !self.needs_reload
    }
}

pub(super) struct Systemd {
    placement: Placement,
    uid: u32,
    commands: Box<dyn Commands>,
}

impl Systemd {
    pub(super) fn new(placement: Placement, uid: u32, commands: Box<dyn Commands>) -> Self {
        Self {
            placement,
            uid,
            commands,
        }
    }

    fn name(&self, instance_id: &str) -> Result<String, ServiceError> {
        valid_instance_id(instance_id)?;
        Ok(format!("{}{instance_id}.service", self.placement.prefix))
    }

    fn file(&self, name: &str) -> PathBuf {
        self.placement.directory.join(name)
    }

    /// One `systemctl --user` run. No runtime directory and no `systemctl` both mean that
    /// there is no user manager to ask.
    fn systemctl(&self, args: &[&OsStr]) -> Result<Outcome, ServiceError> {
        if self.commands.runtime_dir().is_none() {
            return Err(ServiceError::Unavailable(
                "XDG_RUNTIME_DIR is not set, so this session has no systemd user manager".into(),
            ));
        }
        let mut all: Vec<OsString> = vec!["--user".into()];
        all.extend(args.iter().map(|a| (*a).to_owned()));
        match self.commands.run(Program::Systemctl, &all) {
            Ok(out) if out.code != 0 && no_manager(&out.stderr) => Err(ServiceError::Unavailable(
                format!("systemd's user manager does not answer ({})", said(&out)),
            )),
            Ok(out) => Ok(out),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(ServiceError::Unavailable(
                format!("{e}: systemd is not installed"),
            )),
            Err(e) => Err(ServiceError::System(format!(
                "systemctl could not run: {e}"
            ))),
        }
    }

    fn expect_ok(&self, args: &[&OsStr]) -> Result<(), ServiceError> {
        let out = self.systemctl(args)?;
        if out.code != 0 {
            return Err(ServiceError::System(format!(
                "systemctl --user {} exited with {} ({})",
                args.first()
                    .map(|a| a.to_string_lossy())
                    .unwrap_or_default(),
                out.code,
                said(&out)
            )));
        }
        Ok(())
    }

    fn show(&self, name: &str) -> Result<Shown, ServiceError> {
        let out = self.systemctl(&["show".as_ref(), name.as_ref(), PROPERTIES.as_ref()])?;
        if out.code != 0 {
            return Err(ServiceError::System(format!(
                "systemctl --user show exited with {} ({})",
                out.code,
                said(&out)
            )));
        }
        parse_show(&out.stdout).ok_or_else(|| {
            ServiceError::System("systemctl --user show did not report the unit's state".into())
        })
    }

    /// What systemd holds under the name, `None` when it knows no such unit, or a conflict
    /// when it holds one from anything but our definition file.
    fn show_ours(&self, name: &str) -> Result<Option<Shown>, ServiceError> {
        let shown = self.show(name)?;
        if shown.fragment.is_empty() {
            let idle = matches!(shown.active.as_str(), "inactive" | "failed");
            if shown.load == "not-found" && idle {
                return Ok(None);
            }
            // Something runs or is kept under the name without a definition file.
            return Err(ServiceError::Conflict(format!(
                "systemd holds {name} without a definition file (it is {})",
                shown.active.escape_default()
            )));
        }
        let file = self.file(name);
        let loaded = Path::new(&shown.fragment);
        let ours = loaded == file
            || canonical(&file).is_some_and(|f| canonical(loaded).as_ref() == Some(&f));
        if !ours {
            return Err(ServiceError::Conflict(format!(
                "systemd has {name} from a definition other than {}",
                file.display()
            )));
        }
        Ok(Some(shown))
    }

    fn read_definition(&self, file: &Path) -> Result<Option<Vec<u8>>, ServiceError> {
        match fs::symlink_metadata(file) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(ServiceError::System(format!(
                    "cannot inspect {}: {e}",
                    file.display()
                )))
            }
            Ok(m) if !m.file_type().is_file() => {
                return Err(ServiceError::Conflict(format!(
                    "{} is not a regular file",
                    file.display()
                )))
            }
            Ok(_) => {}
        }
        fs::read(file)
            .map(Some)
            .map_err(|e| ServiceError::System(format!("cannot read {}: {e}", file.display())))
    }

    fn write_definition(&self, file: &Path, bytes: &[u8]) -> Result<(), ServiceError> {
        let system =
            |e: io::Error| ServiceError::System(format!("cannot write {}: {e}", file.display()));
        fs::create_dir_all(&self.placement.directory).map_err(system)?;
        let temporary = file.with_extension(format!("service.{}.tmp", std::process::id()));
        let conflict = |e: io::Error| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                ServiceError::Conflict(format!(
                    "{} appeared while registering; it is left untouched",
                    file.display()
                ))
            } else {
                system(e)
            }
        };
        let _ = fs::remove_file(&temporary);
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)
            .map_err(system)?;
        let written = f
            .write_all(bytes)
            .and_then(|()| f.sync_all())
            .and_then(|()| fs::set_permissions(&temporary, fs::Permissions::from_mode(0o644)));
        // A hard link publishes the complete file and, unlike rename, never replaces a
        // definition another writer placed meanwhile.
        let published = written
            .map_err(system)
            .and_then(|()| fs::hard_link(&temporary, file).map_err(conflict));
        let _ = fs::remove_file(&temporary);
        published
    }

    /// Makes systemd read the definition at `file`, whose bytes the caller compared with
    /// the unit, and enables and starts it. A running unit is not restarted.
    fn load(&self, name: &str, file: &Path) -> Result<(), ServiceError> {
        self.expect_ok(&["daemon-reload".as_ref()])?;
        match self.show_ours(name)? {
            Some(shown) if shown.load == "loaded" => {}
            Some(shown) => {
                return Err(ServiceError::System(format!(
                    "systemd cannot use {} (its load state is {})",
                    file.display(),
                    shown.load.escape_default()
                )))
            }
            None => {
                return Err(ServiceError::System(format!(
                    "systemd's user manager does not read {}; the definition {} is not loaded",
                    self.placement.directory.display(),
                    file.display()
                )))
            }
        }
        self.expect_ok(&["enable".as_ref(), "--now".as_ref(), name.as_ref()])
    }
}

impl ServiceManager for Systemd {
    fn register(&self, unit: &Unit) -> Result<Registration, ServiceError> {
        validate(unit)?;
        unit_file_safe(unit)?;
        let name = self.name(&unit.instance_id)?;
        let file = self.file(&name);
        let bytes = render(&name, unit);
        let shown = self.show_ours(&name)?;
        match self.read_definition(&file)? {
            Some(existing) if existing != bytes => Err(ServiceError::Conflict(format!(
                "{} already defines {name} differently; it is left untouched",
                file.display()
            ))),
            Some(_) if shown.as_ref().is_some_and(Shown::settled) => Ok(Registration::Unchanged),
            Some(_) => self.load(&name, &file).map(|()| Registration::Loaded),
            None if shown.is_some() => Err(ServiceError::Conflict(format!(
                "systemd still has {name} from a definition that is gone"
            ))),
            None => {
                self.write_definition(&file, &bytes)?;
                self.load(&name, &file).map(|()| Registration::Loaded)
            }
        }
    }

    fn start(&self, instance_id: &str) -> Result<(), ServiceError> {
        let name = self.name(instance_id)?;
        match self.status(instance_id)? {
            ServiceStatus::Running { .. } => Ok(()),
            ServiceStatus::NotRegistered => {
                Err(ServiceError::Invalid("the service is not registered"))
            }
            // The file at the definition path is not known to be ours until register has
            // compared it with the unit; start never enables or reloads it unchecked.
            ServiceStatus::Unloaded => Err(ServiceError::Invalid(
                "the service is not loaded; register it again",
            )),
            ServiceStatus::Stopped => self.expect_ok(&["start".as_ref(), name.as_ref()]),
        }
    }

    fn stop(&self, instance_id: &str) -> Result<(), ServiceError> {
        let name = self.name(instance_id)?;
        match self.show_ours(&name)? {
            // Also a start systemd has only scheduled (after a crash): it is cancelled.
            Some(shown) if !matches!(shown.active.as_str(), "inactive" | "failed") => {
                self.expect_ok(&["stop".as_ref(), name.as_ref()])
            }
            _ => Ok(()),
        }
    }

    fn status(&self, instance_id: &str) -> Result<ServiceStatus, ServiceError> {
        let name = self.name(instance_id)?;
        // A unit under the name from another file is reported as a conflict, so start and
        // stop never act on it.
        match self.show_ours(&name)? {
            Some(shown) => Ok(match shown.running() {
                Some(pid) => ServiceStatus::Running { pid },
                None if shown.settled() => ServiceStatus::Stopped,
                None => ServiceStatus::Unloaded,
            }),
            None if self.read_definition(&self.file(&name))?.is_some() => {
                Ok(ServiceStatus::Unloaded)
            }
            None => Ok(ServiceStatus::NotRegistered),
        }
    }

    fn unregister(&self, instance_id: &str) -> Result<(), ServiceError> {
        let name = self.name(instance_id)?;
        let file = self.file(&name);
        let known = self.show_ours(&name)?.is_some();
        if known {
            // Stops the unit (and waits for it) and removes its enablement links.
            self.expect_ok(&["disable".as_ref(), "--now".as_ref(), name.as_ref()])?;
        }
        match fs::remove_file(&file) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                return Err(ServiceError::System(format!(
                    "cannot remove {}: {e}",
                    file.display()
                )))
            }
            _ => {}
        }
        if known {
            self.expect_ok(&["daemon-reload".as_ref()])?;
            // A unit that failed is kept in memory until this; one that did not fail has
            // nothing to reset, which is not an error worth reporting.
            let _ = self.systemctl(&["reset-failed".as_ref(), name.as_ref()]);
            let deadline = Instant::now() + UNLOAD_WAIT;
            loop {
                let shown = self.show(&name)?;
                if shown.load == "not-found" && shown.running().is_none() {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(ServiceError::System(format!(
                        "systemd still knows {name} {} s after it was disabled",
                        UNLOAD_WAIT.as_secs()
                    )));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        Ok(())
    }

    fn available(&self) -> Result<(), ServiceError> {
        let out = self.systemctl(&["show".as_ref(), "--property=Version".as_ref()])?;
        if out.code != 0 {
            return Err(ServiceError::Unavailable(format!(
                "systemd's user manager does not answer ({})",
                said(&out)
            )));
        }
        Ok(())
    }

    fn lingering(&self) -> Lingering {
        let args: [OsString; 3] = [
            "show-user".into(),
            self.uid.to_string().into(),
            "--property=Linger".into(),
        ];
        match self.commands.run(Program::Loginctl, &args) {
            Ok(out) if out.code == 0 => match property(&out.stdout, "Linger") {
                Some("yes") => Lingering::On,
                Some("no") => Lingering::Off {
                    enable: format!("loginctl enable-linger {}", user_name(self.uid)),
                },
                _ => Lingering::Unknown("loginctl did not report Linger".into()),
            },
            Ok(out) => Lingering::Unknown(format!(
                "loginctl exited with {} ({})",
                out.code,
                said(&out)
            )),
            Err(e) => Lingering::Unknown(format!("loginctl could not run: {e}")),
        }
    }
}

/// `systemctl`'s own words when no user manager answers it.
fn no_manager(stderr: &str) -> bool {
    stderr.contains("Failed to connect to bus")
        || stderr.contains("Failed to connect to user scope bus")
        || stderr.contains("Failed to get D-Bus connection")
}

/// The first line a program wrote about its failure, for an error text: escaped, and
/// shortened so that a program's output never floods a terminal.
fn said(out: &Outcome) -> String {
    let line = out
        .stderr
        .lines()
        .chain(out.stdout.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("it printed nothing");
    line.chars()
        .take(200)
        .flat_map(char::escape_default)
        .collect()
}

/// The value of `key` in `Key=Value` lines.
fn property<'a>(output: &'a str, key: &str) -> Option<&'a str> {
    output.lines().find_map(|line| {
        line.strip_prefix(key)
            .and_then(|rest| rest.strip_prefix('='))
    })
}

/// `systemctl show`'s `Key=Value` lines for [`PROPERTIES`]. A unit systemd does not know
/// is answered too (`LoadState=not-found`); output without the load and active state is
/// not an answer.
fn parse_show(output: &str) -> Option<Shown> {
    Some(Shown {
        load: property(output, "LoadState")?.to_owned(),
        active: property(output, "ActiveState")?.to_owned(),
        pid: property(output, "MainPID")
            .and_then(|pid| pid.parse().ok())
            .unwrap_or(0),
        fragment: property(output, "FragmentPath").unwrap_or("").to_owned(),
        file_state: property(output, "UnitFileState").unwrap_or("").to_owned(),
        needs_reload: property(output, "NeedDaemonReload") == Some("yes"),
    })
}

fn canonical(path: &Path) -> Option<PathBuf> {
    match path.parent() {
        Some(parent) => Some(fs::canonicalize(parent).ok()?.join(path.file_name()?)),
        None => None,
    }
}

/// The account name of `uid`, or the number where the account database has none
/// (`loginctl` takes either).
fn user_name(uid: u32) -> String {
    let mut buffer = vec![0u8; 4096];
    // SAFETY: passwd is plain data that getpwuid_r fills in; zeroed is a valid start.
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: `entry`, `buffer` and `found` are valid for the call; the name points into
    // `buffer`, which outlives its use below.
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            &mut entry,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut found,
        )
    };
    if status == 0 && !found.is_null() && !entry.pw_name.is_null() {
        // SAFETY: on success pw_name is a NUL-terminated string inside `buffer`.
        let name = unsafe { std::ffi::CStr::from_ptr(entry.pw_name) };
        if let Ok(name) = name.to_str() {
            let plain = !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
            if plain {
                return name.to_owned();
            }
        }
    }
    uid.to_string()
}

/// What the unit-file grammar cannot carry safely, beyond the contract's `validate`:
///
/// - an environment name that is not an identifier (`[A-Za-z_][A-Za-z0-9_]*`): systemd
///   drops such an assignment with a warning, and the service would start without it;
/// - a carriage return or DEL in an environment value: some systemd versions drop the
///   whole assignment for a control character other than tab and line feed;
/// - a `$` in the program path: systemd expands variables in arguments, where `$$` writes a
///   literal `$`, but not in the path it executes, so no one spelling is right for both.
fn unit_file_safe(unit: &Unit) -> Result<(), ServiceError> {
    if unit.program.to_string_lossy().contains('$') {
        return Err(ServiceError::Invalid(
            "a systemd unit cannot name a program whose path contains a dollar sign",
        ));
    }
    for (key, value) in &unit.env {
        let key = key.to_string_lossy();
        let identifier = key
            .bytes()
            .enumerate()
            .all(|(at, b)| b == b'_' || b.is_ascii_alphabetic() || (at > 0 && b.is_ascii_digit()));
        if key.is_empty() || !identifier {
            return Err(ServiceError::Invalid(
                "a systemd unit cannot set an environment variable whose name is not an \
                 identifier",
            ));
        }
        if value.to_string_lossy().contains(['\r', '\u{7f}']) {
            return Err(ServiceError::Invalid(
                "a systemd unit cannot set an environment value with a carriage return or DEL",
            ));
        }
    }
    Ok(())
}

/// One double-quoted word of a unit file. Inside the quotes systemd undoes C escapes, so
/// backslash, quote and the three line and field characters are written as escapes and no
/// line ever ends in a backslash; `%` begins a specifier, so it is doubled; and in
/// `ExecStart=` (`exec`) `$` begins a variable, so it is doubled too. In `Environment=` a
/// `$` is literal. A quoted `;` is an argument, not a command separator.
fn quote(text: &str, exec: bool) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '%' => out.push_str("%%"),
            '$' if exec => out.push_str("$$"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The unit file. Inputs were validated as UTF-8 by `validate` and by [`unit_file_safe`].
fn render(name: &str, unit: &Unit) -> Vec<u8> {
    let text = |s: &OsStr, exec: bool| quote(&s.to_string_lossy(), exec);
    let mut u = String::from(
        "# Written by agentic-org setup. A changed file is refused, never overwritten.\n[Unit]\n",
    );
    u.push_str(&format!(
        "Description=Agentic Org instance ({})\n\n",
        name.replace('%', "%%")
    ));
    u.push_str("[Service]\nType=exec\nExecStart=");
    u.push_str(&text(unit.program.as_os_str(), true));
    for arg in &unit.args {
        u.push(' ');
        u.push_str(&text(arg, true));
    }
    u.push('\n');
    for (key, value) in &unit.env {
        let assignment = format!("{}={}", key.to_string_lossy(), value.to_string_lossy());
        u.push_str(&format!("Environment={}\n", quote(&assignment, false)));
    }
    u.push_str(concat!(
        "KillMode=mixed\n",
        "TimeoutStopSec=15\n",
        "Restart=on-failure\n",
        "UMask=0077\n",
        "NoNewPrivileges=yes\n",
        "\n[Install]\nWantedBy=default.target\n",
    ));
    u.into_bytes()
}

#[cfg(test)]
mod tests;
