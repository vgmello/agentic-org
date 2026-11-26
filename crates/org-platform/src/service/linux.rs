//! A systemd user unit, `~/.config/systemd/user/agentic-org-<instance-id>.service`.
//!
//! The unit is `Type=exec` with `Restart=on-failure`, enabled for `default.target`: the
//! user's manager starts the host when the manager starts, restarts it after a crash, and
//! an explicit stop (`SIGTERM`, a clean exit) stays stopped. `KillMode=mixed` sends
//! `SIGTERM` to the host alone and, after `TimeoutStopSec=15`, `SIGKILL` to what is left of
//! its group; the host itself stops within 2.5 s. `Type=exec` needs systemd 240 or later
//! ([`MINIMUM_VERSION`]); an older user manager is refused as unavailable, by its version.
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
//!   meanwhile**: a temporary file, flushed, then a hard link, all relative to the unit
//!   directory held open, which must be this user's and not writable by other users. The
//!   one file register replaces is an exact earlier rendering of a unit under the same
//!   name, which holds the instance's ID: its unit is disabled and stopped first, and
//!   when that fails the earlier rendering is put back, so the file on disk is still what
//!   systemd runs from.
//! - **Nothing is written for a manager that would not read it**: the manager's
//!   `UnitPath` must name the placement's directory (`available` and register ask).
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
//!
//! [`ServiceError::Unavailable`]: super::ServiceError::Unavailable
//! [`unit_file_safe`]: unit_file::unit_file_safe
//! [`quote`]: unit_file::quote
use super::{Placement, ServiceManager};
use crate::Refused;
use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

mod directory;
mod show;
mod systemd;
mod unit_file;

use systemd::Systemd;

/// Where the two programs are looked for, in order. Nothing else is searched.
const SEARCH: [&str; 2] = ["/usr/bin", "/bin"];
/// How long unregister waits for systemd to forget the unit after `disable --now`, which
/// itself returns only when the unit has stopped (`TimeoutStopSec=15`).
const UNLOAD_WAIT: Duration = Duration::from_secs(20);
pub(super) const USER_PREFIX: &str = "agentic-org-";
/// The first systemd that knows `Type=exec`. An older one ignores the line, runs the unit as
/// `Type=simple`, and so would run something other than what the definition says.
const MINIMUM_VERSION: u32 = 240;
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

#[cfg(test)]
mod tests;
