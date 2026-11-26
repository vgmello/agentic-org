//! The systemd adapter: what it asks `systemctl --user` and `loginctl`, and how register,
//! start, stop, status and unregister act on the unit under the instance's name.

use std::{
    ffi::{OsStr, OsString},
    fs, io,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::service::{
    valid_instance_id, validate, Lingering, Placement, Registration, ServiceError, ServiceManager,
    ServiceStatus, Unit,
};

use super::directory::UnitDirectory;
use super::show::{
    applies_by_name, canonical, effective_differs, no_manager, parse_show, property, recent_enough,
    resolved, said, Shown,
};
use super::unit_file::{definition, render, unit_file_safe};
use super::{Commands, Outcome, Program, PROPERTIES, UNLOAD_WAIT};

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
            // `name` is the placement's prefix, the instance's ID and `.service`.
            let instance_id = name
                .strip_prefix(self.placement.prefix.as_str())
                .and_then(|rest| rest.strip_suffix(".service"))
                .unwrap_or_default();
            let defined = self
                .read_definition(name)?
                .and_then(|bytes| definition(name, instance_id, &bytes));
            let Some(defined) = defined else {
                return Err(ServiceError::Conflict(format!(
                    "{} is not a definition this adapter wrote; systemd would run what it says",
                    file.display()
                )));
            };
            if let Some(property) = effective_differs(&defined, &shown) {
                // A manager too old for `Type=exec` reports `simple` without any drop-in: it
                // is named as that, not as a drop-in that changes the type.
                if property == "Type" {
                    match self.systemctl(&["show".as_ref(), "--property=Version".as_ref()]) {
                        Ok(out) if out.code == 0 => recent_enough(&out)?,
                        _ => {}
                    }
                }
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

    /// The definition `name` in the placement's directory, read relative to the held
    /// directory without following a link. `None` when the directory or the file is missing.
    fn read_definition(&self, name: &str) -> Result<Option<Vec<u8>>, ServiceError> {
        match self.directory(false)? {
            Some(directory) => directory.read(name),
            None => Ok(None),
        }
    }

    /// The placement's directory, held open and judged ([`UnitDirectory`]). `create`: made
    /// first where it is missing (mode 700, whatever the process's mask leaves). `None`
    /// when it does not exist and is not to be made.
    pub(super) fn directory(&self, create: bool) -> Result<Option<UnitDirectory>, ServiceError> {
        let directory = &self.placement.directory;
        let system = |e: io::Error| {
            ServiceError::System(format!("cannot open {}: {e}", directory.display()))
        };
        if create {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(directory)
                .map_err(system)?;
        }
        let file = match fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(directory)
        {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound && !create => return Ok(None),
            Err(e) => return Err(system(e)),
        };
        let found = file.metadata().map_err(system)?;
        // SAFETY: geteuid has no preconditions and cannot fail.
        let user = unsafe { libc::geteuid() };
        let rule = if found.uid() != user {
            "it belongs to another user"
        } else if found.mode() & 0o002 != 0 {
            "it is writable by other users"
        } else {
            return Ok(Some(UnitDirectory {
                file,
                path: directory.clone(),
            }));
        };
        Err(ServiceError::Conflict(format!(
            "{} cannot hold this user's units: {rule}",
            directory.display()
        )))
    }

    /// Whether systemd's user manager reads unit files from the placement's directory: its
    /// `UnitPath` names it. A manager whose `XDG_CONFIG_HOME` is another folder reads
    /// another directory, and a unit written to this one would never be loaded (X5 review,
    /// fourth round, P2-4). Asked before anything is written.
    fn reads_placement(&self) -> Result<(), ServiceError> {
        let out = self.systemctl(&["show".as_ref(), "--property=UnitPath".as_ref()])?;
        if out.code != 0 {
            return Err(ServiceError::System(format!(
                "systemctl --user show exited with {} ({})",
                out.code,
                said(&out)
            )));
        }
        let listed = property(&out.stdout, "UnitPath").ok_or_else(|| {
            ServiceError::System("systemd's user manager did not report its unit path".into())
        })?;
        let wanted = &self.placement.directory;
        // The entries are separated by spaces; a directory with a space in its name is
        // found as a whole entry.
        let named = format!(" {listed} ").contains(&format!(" {} ", wanted.display()))
            || listed
                .split(' ')
                .any(|entry| resolved(Path::new(entry)) == resolved(wanted));
        if named {
            return Ok(());
        }
        Err(ServiceError::System(format!(
            "systemd's user manager does not read unit files from {}, so a unit written there \
             would never be loaded (its unit path is {}; its XDG_CONFIG_HOME names another \
             folder); nothing was changed",
            wanted.display(),
            listed
                .chars()
                .take(400)
                .flat_map(char::escape_default)
                .collect::<String>()
        )))
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
        // The earlier rendering this call replaces, if any.
        let earlier = match self.read_definition(&name)? {
            // An earlier rendering of a unit under this name, which holds the instance's
            // ID: what this adapter wrote for the same instance with another program,
            // arguments or environment (setup run again with what the service lacked, X5
            // review V-b). It is replaced, never edited: what systemd runs from it stops and
            // loses its enablement first. Anything else is never taken over.
            Some(existing) if existing != bytes => {
                if definition(&name, &unit.instance_id, &existing).is_none() {
                    return Err(ServiceError::Conflict(format!(
                        "{} already defines {name} differently; it is left untouched",
                        file.display()
                    )));
                }
                Some(existing)
            }
            Some(_) if shown.as_ref().is_some_and(Shown::settled) => {
                return Ok(Registration::Unchanged)
            }
            Some(_) => return self.load(&name, &file).map(|()| Registration::Loaded),
            None if shown.is_some() => {
                return Err(ServiceError::Conflict(format!(
                    "systemd still has {name} from a definition that is gone"
                )))
            }
            None => None,
        };
        // Nothing is written for a manager that would never read it.
        self.reads_placement()?;
        let directory = self
            .directory(true)?
            .expect("a directory this call makes exists");
        // Published first: a write that fails leaves the earlier rendering as it was, and
        // running. Then what systemd runs from the earlier one stops and loses its
        // enablement, and the new one is loaded as a first registration's would be.
        directory.publish(&name, &bytes, earlier.is_some())?;
        if let (Some(earlier), Some(_)) = (&earlier, &shown) {
            if let Err(refused) =
                self.expect_ok(&["disable".as_ref(), "--now".as_ref(), name.as_ref()])
            {
                // The earlier rendering may still be running: it is put back, so the file
                // on disk is again what systemd runs from, and the next register replaces
                // it again from the start. Left as the new rendering, that register would
                // find it current and only enable it, which never restarts the earlier
                // program (X5 final review).
                return Err(match directory.publish(&name, earlier, true) {
                    Ok(()) => refused,
                    Err(restore) => ServiceError::System(format!(
                        "{refused}; the earlier definition could not be put back: {restore}"
                    )),
                });
            }
        }
        match self.load(&name, &file) {
            Ok(()) => Ok(Registration::Loaded),
            Err(refused) => {
                // What this call wrote is not left behind for the next one to find: the
                // file goes (its enablement went in `load`), systemd is told so, and a
                // start that failed is not kept as a failed unit. A definition this call
                // replaced is not put back: it was this instance's, and it did not serve.
                let _ = directory.remove(&name);
                let _ = self.systemctl(&["daemon-reload".as_ref()]);
                let _ = self.systemctl(&["reset-failed".as_ref(), name.as_ref()]);
                Err(refused)
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
            None if self.read_definition(&name)?.is_some() => Ok(ServiceStatus::Unloaded),
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
        let removed = match self.directory(false)? {
            Some(directory) => directory.remove(&name),
            None => Ok(()),
        };
        match removed {
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

    fn definition(&self, instance_id: &str) -> Result<Option<Unit>, ServiceError> {
        let name = self.name(instance_id)?;
        // A drop-in or another file under the name is a conflict, as for what runs it.
        let shown = self.show_ours(&name, true)?;
        match self.read_definition(&name)? {
            Some(bytes) => match definition(&name, instance_id, &bytes) {
                Some(unit) => Ok(Some(unit)),
                None => Err(ServiceError::Conflict(format!(
                    "{} is not a definition this adapter wrote (it was changed by hand); it \
                     is left untouched",
                    self.file(&name).display()
                ))),
            },
            None if shown.is_some() => Err(ServiceError::Conflict(format!(
                "systemd still has {name} from a definition that is gone"
            ))),
            None => Ok(None),
        }
    }

    fn redefine(&self, unit: &Unit) -> Result<Registration, ServiceError> {
        validate(unit)?;
        unit_file_safe(unit)?;
        let name = self.name(&unit.instance_id)?;
        let file = self.file(&name);
        let bytes = render(&name, unit);
        let shown = self.show_ours(&name, true)?;
        // As in stop: also a start systemd has only scheduled (`activating`, `MainPID=0`,
        // an automatic restart after a crash) would run the program being replaced.
        if shown
            .as_ref()
            .is_some_and(|s| !matches!(s.active.as_str(), "inactive" | "failed"))
        {
            return Err(ServiceError::Conflict(format!(
                "{name} is running; stop it before its definition is replaced"
            )));
        }
        // Only an exact earlier rendering, which holds the instance's ID, is replaced, as in
        // register; its enablement stays (the file keeps its name) and nothing is started.
        let earlier = match self.read_definition(&name)? {
            Some(existing) if existing == bytes => return Ok(Registration::Unchanged),
            Some(existing) if definition(&name, &unit.instance_id, &existing).is_some() => existing,
            Some(_) => {
                return Err(ServiceError::Conflict(format!(
                    "{} already defines {name} differently; it is left untouched",
                    file.display()
                )))
            }
            None if shown.is_some() => {
                return Err(ServiceError::Conflict(format!(
                    "systemd still has {name} from a definition that is gone"
                )))
            }
            None => return Err(ServiceError::Invalid("the service is not registered")),
        };
        let directory = self.directory(false)?.ok_or_else(|| {
            ServiceError::System(format!("{} is gone", self.placement.directory.display()))
        })?;
        directory.publish(&name, &bytes, true)?;
        if let Err(refused) = self.expect_ok(&["daemon-reload".as_ref()]) {
            // The earlier rendering is put back, so the file on disk stays what systemd
            // runs from and the next call replaces it from the start.
            return Err(match directory.publish(&name, &earlier, true) {
                Ok(()) => refused,
                Err(restore) => ServiceError::System(format!(
                    "{refused}; the earlier definition could not be put back: {restore}"
                )),
            });
        }
        Ok(Registration::Loaded)
    }

    fn available(&self) -> Result<(), ServiceError> {
        let out = self.systemctl(&["show".as_ref(), "--property=Version".as_ref()])?;
        if out.code != 0 {
            return Err(ServiceError::Unavailable(format!(
                "systemd's user manager does not answer ({})",
                said(&out)
            )));
        }
        recent_enough(&out)?;
        self.reads_placement()
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

/// The account name of `uid`, or the number where the account database has none
/// (`loginctl` takes either).
pub(super) fn user_name(uid: u32) -> String {
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
