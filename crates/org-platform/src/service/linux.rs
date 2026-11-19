//! A systemd user unit, `~/.config/systemd/user/agentic-org-<instance-id>.service`.
//!
//! The unit is `Type=exec` with `Restart=on-failure`, enabled for `default.target`: the
//! user's manager starts the host when the manager starts, restarts it after a crash, and
//! an explicit stop (`SIGTERM`, a clean exit) stays stopped. `KillMode=mixed` sends
//! `SIGTERM` to the host alone and, after `TimeoutStopSec=15`, `SIGKILL` to what is left of
//! its group; the host itself stops within 2.5 s.
//!
//! The launchd adapter's guarantees, in systemd's terms:
//!
//! - **Nothing under the name that is someone else's is ever taken over.** A unit systemd
//!   holds under the name from any file other than our definition (its `FragmentPath`: a
//!   unit in another directory, a transient or a masked one) is a conflict for register,
//!   start, stop, status and unregister. So is a unit of ours with a drop-in that applies
//!   to it by name (`<name>.d/*.conf`, or in the folder of a prefix of the name that ends
//!   at a dash, such as `agentic-org-.service.d/`, in any unit directory): a drop-in can
//!   replace `ExecStart` or add to the environment, so what systemd would run is no longer
//!   the definition. A definition file with different bytes than the
//!   rendered unit is a conflict for register, which never replaces, reloads or enables it.
//! - **What systemd would run is what the definition says.** A drop-in for every service of
//!   the user (`service.d/`) is accepted, and distributions ship some, but it can still
//!   change `ExecStart`, the environment or `KillMode` while `FragmentPath` is our file. So
//!   whenever systemd has our file loaded as it is on disk, the values the service depends
//!   on, as systemd reports them ([`EFFECTIVE`]), are compared with what our definition
//!   says (read back from the file, which must be exactly a rendering of a unit), and any
//!   difference is a conflict for register, start and status. Stop and unregister still
//!   end and remove our own unit: refusing them would only leave it running.
//! - **An identical, settled registration is left alone**: the file has the rendered bytes,
//!   systemd loaded it from that file, nothing changed on disk since, and it is enabled.
//!   A healthy service is not restarted.
//! - **The definition is published complete and never replaces a file that appeared
//!   meanwhile**: a temporary file, flushed, then a hard link.
//! - **The location variables the unit does not set are unset** (`UnsetEnvironment=`, for
//!   every name of `crate::dirs::location_variables`): the user manager's own environment
//!   cannot move the instance's locations away from the ones setup resolved.
//! - **A failed registration leaves nothing behind**: when `enable --now` enabled the unit
//!   and then failed to start it, the enablement that call made is removed
//!   (`disable --now`; `disable` alone for a unit that was already running) before the
//!   definition this call wrote is removed.
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
/// What the adapter reads of a unit, in one `systemctl show`: its state, and the values
/// the service depends on ([`EFFECTIVE`]).
const PROPERTIES: &str = "--property=LoadState,ActiveState,MainPID,FragmentPath,UnitFileState,\
     NeedDaemonReload,DropInPaths,ExecStart,ExecStartPre,ExecStartPost,ExecCondition,ExecStop,\
     ExecStopPost,Environment,EnvironmentFiles,PassEnvironment,UnsetEnvironment,Type,KillMode,\
     KillSignal,SendSIGKILL,TimeoutStopUSec,Restart,UMask,NoNewPrivileges";
/// The properties of [`PROPERTIES`] that say what systemd would run and how: each is
/// compared with what the definition says (`effective_differs`).
const EFFECTIVE: [&str; 18] = [
    "ExecStart",
    "ExecStartPre",
    "ExecStartPost",
    "ExecCondition",
    "ExecStop",
    "ExecStopPost",
    "Environment",
    "EnvironmentFiles",
    "PassEnvironment",
    "UnsetEnvironment",
    "Type",
    "KillMode",
    "KillSignal",
    "SendSIGKILL",
    "TimeoutStopUSec",
    "Restart",
    "UMask",
    "NoNewPrivileges",
];

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
    /// The drop-in files systemd applies to the unit, as it lists them.
    drop_ins: String,
    /// The [`EFFECTIVE`] properties systemd printed, by name, as it printed them.
    effective: Vec<(&'static str, String)>,
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

    /// The value systemd printed for the [`EFFECTIVE`] property `name`; a property it did
    /// not print is empty (`systemctl show` leaves out an empty list).
    fn effective(&self, name: &str) -> &str {
        self.effective
            .iter()
            .find(|(key, _)| *key == name)
            .map_or("", |(_, value)| value.as_str())
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
    /// when it holds one from anything but our definition file. `runs`: the caller may make
    /// systemd run the unit (register, start, and status, which start asks), so what systemd
    /// would run must also be what the file says. Stop and unregister only end our own unit:
    /// they act on it whatever a drop-in for every service changed, and whatever an earlier
    /// rendering of the file says.
    fn show_ours(&self, name: &str, runs: bool) -> Result<Option<Shown>, ServiceError> {
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
        // A drop-in that systemd applies to this unit by its name changes what it runs
        // under our name.
        if let Some(drop_in) = shown
            .drop_ins
            .split_whitespace()
            .find(|path| applies_by_name(name, path))
        {
            return Err(ServiceError::Conflict(format!(
                "systemd applies a drop-in to {name} ({}); the definition is not the only \
                 thing it would run",
                drop_in.escape_default()
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
        // What systemd holds is our file as it is on disk: what it would run must be what
        // the file says, whatever a drop-in for every service changed. A file that changed
        // since systemd read it is not compared: it is `Unloaded`, nothing starts it, and
        // register compares its bytes and reloads it, which compares again.
        if runs && shown.load == "loaded" && !shown.needs_reload {
            let defined = self
                .read_definition(&file)?
                .and_then(|bytes| definition(name, &bytes));
            let Some(defined) = defined else {
                return Err(ServiceError::Conflict(format!(
                    "{} is not a definition this adapter wrote; systemd would run what it says",
                    file.display()
                )));
            };
            if let Some(property) = effective_differs(&defined, &shown) {
                return Err(ServiceError::Conflict(format!(
                    "systemd's {property} for {name} is not what {} says (a drop-in changes \
                     it: {}); the definition is not the only thing it would run",
                    file.display(),
                    if shown.drop_ins.is_empty() {
                        "none is listed".into()
                    } else {
                        shown.drop_ins.escape_default().to_string()
                    }
                )));
            }
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
        published?;
        // The new name is in the directory: flush the directory, so the definition is
        // still there after a power loss.
        fs::File::open(&self.placement.directory)
            .and_then(|directory| directory.sync_all())
            .map_err(system)
    }

    /// Makes systemd read the definition at `file`, whose bytes the caller compared with
    /// the unit, and enables and starts it. A running unit is not restarted. When `enable
    /// --now` fails after it enabled a unit that was not enabled (its start failed), that
    /// enablement is removed again, and a restart systemd scheduled is cancelled.
    fn load(&self, name: &str, file: &Path) -> Result<(), ServiceError> {
        self.expect_ok(&["daemon-reload".as_ref()])?;
        let (was_enabled, was_running) = match self.show_ours(name, true)? {
            Some(shown) if shown.load == "loaded" => {
                (shown.file_state == "enabled", shown.running().is_some())
            }
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
        };
        let enabled = self.expect_ok(&["enable".as_ref(), "--now".as_ref(), name.as_ref()]);
        if enabled.is_err() && !was_enabled {
            // Best effort: the error the caller reports is the enable's. A unit that was
            // already running keeps running: this call did not start it.
            let disable: &[&OsStr] = if was_running {
                &["disable".as_ref(), name.as_ref()]
            } else {
                &["disable".as_ref(), "--now".as_ref(), name.as_ref()]
            };
            let _ = self.systemctl(disable);
        }
        enabled
    }
}

impl ServiceManager for Systemd {
    fn register(&self, unit: &Unit) -> Result<Registration, ServiceError> {
        validate(unit)?;
        unit_file_safe(unit)?;
        let name = self.name(&unit.instance_id)?;
        let file = self.file(&name);
        let bytes = render(&name, unit);
        let shown = self.show_ours(&name, true)?;
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
                match self.load(&name, &file) {
                    Ok(()) => Ok(Registration::Loaded),
                    Err(refused) => {
                        // What this call wrote is not left behind for the next one to
                        // find: the file goes (its enablement went in `load`), systemd is
                        // told so, and a start that failed is not kept as a failed unit.
                        let _ = fs::remove_file(&file);
                        let _ = self.systemctl(&["daemon-reload".as_ref()]);
                        let _ = self.systemctl(&["reset-failed".as_ref(), name.as_ref()]);
                        Err(refused)
                    }
                }
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
        match self.show_ours(&name, false)? {
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
        match self.show_ours(&name, true)? {
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
        let known = self.show_ours(&name, false)?.is_some();
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
        drop_ins: property(output, "DropInPaths").unwrap_or("").to_owned(),
        effective: EFFECTIVE
            .iter()
            .filter_map(|name| Some((*name, property(output, name)?.to_owned())))
            .collect(),
    })
}

/// What a definition this adapter wrote says: the unit it is a rendering of. `None` when
/// the bytes are not exactly the rendering of a unit under `name`: a file someone wrote or
/// changed, whose meaning the adapter does not claim to know. The parse only has to find
/// the values; the comparison of the bytes with a new rendering is what makes it exact.
fn definition(name: &str, bytes: &[u8]) -> Option<Unit> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut exec = None;
    let mut env = Vec::new();
    for line in text.lines() {
        if let Some(words) = line.strip_prefix("ExecStart=") {
            exec = Some(unquoted_words(words, true)?);
        } else if let Some(word) = line.strip_prefix("Environment=") {
            let mut words = unquoted_words(word, false)?.into_iter();
            let assignment = words.next().filter(|_| words.next().is_none())?;
            let (key, value) = assignment.split_once('=')?;
            env.push((key.into(), value.into()));
        }
    }
    let mut exec = exec?.into_iter();
    let unit = Unit {
        instance_id: String::new(),
        program: exec.next()?.into(),
        args: exec.map(OsString::from).collect(),
        env,
    };
    (render(name, &unit) == bytes).then_some(unit)
}

/// The words of a line written by [`quote`], with its escapes undone: `%%` is `%` and, in
/// `ExecStart=` (`exec`), `$$` is `$`. `None` for anything [`quote`] does not write.
fn unquoted_words(line: &str, exec: bool) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut chars = line.chars();
    loop {
        if chars.next()? != '"' {
            return None;
        }
        let mut word = String::new();
        loop {
            match chars.next()? {
                '"' => break,
                '\\' => word.push(match chars.next()? {
                    '\\' => '\\',
                    '"' => '"',
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    _ => return None,
                }),
                '%' => (chars.next()? == '%').then(|| word.push('%'))?,
                '$' if exec => (chars.next()? == '$').then(|| word.push('$'))?,
                c => word.push(c),
            }
        }
        words.push(word);
        match chars.next() {
            None => return Some(words),
            Some(' ') => {}
            Some(_) => return None,
        }
    }
}

/// The first of the [`EFFECTIVE`] properties for which what systemd reports differs from
/// what the unit `defined` says, or `None`. systemd 257 prints `ExecStart` as
/// `{ path=<program> ; argv[]=<words> ; ignore_errors=no ; … }`, where the words are the
/// program and its arguments joined by spaces, unquoted, each `$` still written `$$`; and
/// `Environment=` and `UnsetEnvironment=` as words separated by spaces, double-quoted where
/// they hold a space or a character a shell treats specially ([`shell_words`]). So
/// `ExecStart` compares the joined text: a change that only regroups words around a space
/// is not seen. Units hold no control character ([`unit_file_safe`]), which systemd would
/// print raw in an argument and break the line. A systemd that quotes differently makes a
/// value with a space or such a character differ, and the unit is refused: never accepted
/// on doubt. The command must also stop at our words: an extra argument after an argument
/// `;` shows as more text before `ignore_errors=`.
fn effective_differs(defined: &Unit, shown: &Shown) -> Option<&'static str> {
    let exec = std::iter::once(defined.program.as_os_str())
        .chain(defined.args.iter().map(OsString::as_os_str))
        .map(|word| word.to_string_lossy().replace('$', "$$"))
        .collect::<Vec<_>>()
        .join(" ");
    let start = shown.effective("ExecStart");
    // One command, our words and nothing after them: the field that follows the arguments
    // comes next, and appears once (an argument `;` would otherwise hide more words).
    let one_command = start
        .strip_prefix(&format!(
            "{{ path={} ; argv[]={exec} ; ignore_errors=no ; ",
            defined.program.display()
        ))
        .is_some_and(|rest| rest.ends_with(" }") && !rest.contains("{ path="))
        && start.matches("ignore_errors=").count() == 1;
    if !one_command {
        return Some("ExecStart");
    }
    let sorted = |mut words: Vec<String>| {
        words.sort();
        words
    };
    let environment: Vec<String> = defined
        .env
        .iter()
        .map(|(key, value)| format!("{}={}", key.to_string_lossy(), value.to_string_lossy()))
        .collect();
    if shell_words(shown.effective("Environment")).map(sorted) != Some(sorted(environment)) {
        return Some("Environment");
    }
    let unset: Vec<String> = unset_variables(defined)
        .into_iter()
        .map(str::to_owned)
        .collect();
    if shell_words(shown.effective("UnsetEnvironment")).map(sorted) != Some(sorted(unset)) {
        return Some("UnsetEnvironment");
    }
    let expected = [
        ("ExecStartPre", ""),
        ("ExecStartPost", ""),
        ("ExecCondition", ""),
        ("ExecStop", ""),
        ("ExecStopPost", ""),
        ("EnvironmentFiles", ""),
        ("PassEnvironment", ""),
        ("Type", "exec"),
        ("KillMode", "mixed"),
        ("KillSignal", "15"),
        ("SendSIGKILL", "yes"),
        ("TimeoutStopUSec", "15s"),
        ("Restart", "on-failure"),
        ("UMask", "0077"),
        ("NoNewPrivileges", "yes"),
    ];
    expected
        .into_iter()
        .find(|(name, value)| shown.effective(name) != *value)
        .map(|(name, _)| name)
}

/// The words of a list `systemctl show` prints, such as `Environment=`: separated by
/// spaces; a word, or part of one, in double quotes may hold spaces, and inside the quotes
/// a backslash makes the next `\`, `"`, `$` or `` ` `` literal. `None` for anything else
/// (an unfinished quote, another escape): that is not compared, it differs.
fn shell_words(text: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut quoted = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            ' ' if !quoted => words.extend(word.take()),
            '"' => {
                quoted = !quoted;
                word.get_or_insert_with(String::new);
            }
            '\\' if quoted => match chars.next()? {
                escaped @ ('\\' | '"' | '$' | '`') => {
                    word.get_or_insert_with(String::new).push(escaped)
                }
                _ => return None,
            },
            '\\' => return None,
            c => word.get_or_insert_with(String::new).push(c),
        }
    }
    if quoted {
        return None;
    }
    words.extend(word);
    Some(words)
}

/// The location variables (`crate::dirs::location_variables`) that `unit` does not set:
/// its definition unsets them, so the user manager's own values never reach the service.
fn unset_variables(unit: &Unit) -> Vec<&'static str> {
    crate::dirs::location_variables()
        .unwrap_or(&[])
        .iter()
        .copied()
        .filter(|name| !unit.env.iter().any(|(key, _)| key == OsStr::new(name)))
        .collect()
}

/// Whether the drop-in file `path` is one systemd applies to the unit `name` by its name:
/// it is in `<name>.d/`, or in the folder of a prefix of the name that ends at a dash
/// (`agentic-org-.service.d/` for `agentic-org-<id>.service`, and every longer one up to
/// the last dash of the instance ID), as systemd.unit(5) has it. Such a drop-in reaches
/// only units named like ours, and can replace `ExecStart` while the unit's file is still
/// our definition. The folder for every unit of the type (`service.d/`) is not one: it
/// applies to every service of the user, and distributions ship drop-ins there.
fn applies_by_name(name: &str, path: &str) -> bool {
    let Some(folder) = Path::new(path)
        .parent()
        .and_then(Path::file_name)
        .and_then(OsStr::to_str)
    else {
        return false;
    };
    let Some(stem) = name.strip_suffix(".service") else {
        return folder == format!("{name}.d");
    };
    folder == format!("{name}.d")
        || stem
            .match_indices('-')
            .any(|(dash, _)| folder == format!("{}.service.d", &stem[..=dash]))
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

/// What the unit-file grammar cannot carry safely, or what `systemctl show` cannot report
/// back for the comparison of what systemd would run, beyond the contract's `validate`:
///
/// - an environment name that is not an identifier (`[A-Za-z_][A-Za-z0-9_]*`): systemd
///   drops such an assignment with a warning, and the service would start without it;
/// - a control character (tab, line feed and carriage return included) in the program,
///   an argument or an environment value: systemd prints an argument raw, which breaks its
///   report into lines, and some systemd versions drop an assignment that holds one;
/// - a `$` in the program path: systemd expands variables in arguments, where `$$` writes a
///   literal `$`, but not in the path it executes, so no one spelling is right for both.
fn unit_file_safe(unit: &Unit) -> Result<(), ServiceError> {
    if unit.program.to_string_lossy().contains('$') {
        return Err(ServiceError::Invalid(
            "a systemd unit cannot name a program whose path contains a dollar sign",
        ));
    }
    let control = |text: &OsStr| text.to_string_lossy().chars().any(char::is_control);
    if control(unit.program.as_os_str()) || unit.args.iter().any(|arg| control(arg)) {
        return Err(ServiceError::Invalid(
            "a systemd unit cannot run a program or argument with a control character",
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
        if control(value) {
            return Err(ServiceError::Invalid(
                "a systemd unit cannot set an environment value with a control character",
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
    let unset = unset_variables(unit);
    if !unset.is_empty() {
        u.push_str(&format!("UnsetEnvironment={}\n", unset.join(" ")));
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
