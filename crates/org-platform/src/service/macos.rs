//! A per-user launchd agent in the `gui/<uid>` domain.
//!
//! The job runs `KeepAlive { SuccessfulExit = false }` and `RunAtLoad`: launchd restarts
//! the host after a crash and starts it at login, and an explicit stop (`SIGTERM`, a clean
//! exit) stays stopped. Nothing under the label that is someone else's is ever taken over.
//! A job loaded under the label from any file other than our definition (the top-level
//! `path` of `launchctl print`) is a conflict for register, start, stop, status and
//! unregister. A definition file with the same label but different bytes than the rendered
//! unit is a conflict for register, which never replaces or loads it.
use super::{
    valid_instance_id, validate, Placement, Registration, ServiceError, ServiceManager,
    ServiceStatus, Unit,
};
use crate::Refused;
use std::{
    ffi::OsString,
    fs,
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const LAUNCHCTL: &str = "/bin/launchctl";
/// `launchctl print` and `bootout` for a label launchd does not know.
const NO_SUCH_SERVICE: i32 = 113;
/// How long unregister waits for launchd to drop a job after `bootout`; the host exits
/// within 8 s of `SIGTERM`.
const UNLOAD_WAIT: std::time::Duration = std::time::Duration::from_secs(15);
pub(super) const USER_PREFIX: &str = "com.ellosoft.agentic-org.instance.";

pub(super) fn user_placement(home: &Path) -> Result<Placement, Refused> {
    Ok(Placement {
        prefix: USER_PREFIX.into(),
        directory: home.join("Library/LaunchAgents"),
    })
}

pub(super) fn registered(placement: Placement) -> Result<Box<dyn ServiceManager>, Refused> {
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    Ok(Box::new(Launchd::new(placement, uid, Box::new(System))))
}

/// The outcome of one `launchctl` run.
pub(super) struct Outcome {
    pub code: i32,
    pub stdout: String,
}

/// Runs `launchctl`; tests substitute a recording fake.
pub(super) trait Launchctl: Send + Sync {
    fn run(&self, args: &[OsString]) -> io::Result<Outcome>;
}

struct System;

impl Launchctl for System {
    fn run(&self, args: &[OsString]) -> io::Result<Outcome> {
        let out = Command::new(LAUNCHCTL)
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()?;
        Ok(Outcome {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        })
    }
}

pub(super) struct Launchd {
    placement: Placement,
    domain: String,
    launchctl: Box<dyn Launchctl>,
}

impl Launchd {
    pub(super) fn new(placement: Placement, uid: u32, launchctl: Box<dyn Launchctl>) -> Self {
        Self {
            placement,
            domain: format!("gui/{uid}"),
            launchctl,
        }
    }

    fn label(&self, instance_id: &str) -> Result<String, ServiceError> {
        valid_instance_id(instance_id)?;
        Ok(format!("{}{instance_id}", self.placement.prefix))
    }

    fn file(&self, label: &str) -> PathBuf {
        self.placement.directory.join(format!("{label}.plist"))
    }

    fn target(&self, label: &str) -> OsString {
        format!("{}/{label}", self.domain).into()
    }

    fn run(&self, args: &[OsString]) -> Result<Outcome, ServiceError> {
        self.launchctl
            .run(args)
            .map_err(|e| ServiceError::System(format!("launchctl could not run: {e}")))
    }

    fn expect_ok(&self, args: &[OsString]) -> Result<Outcome, ServiceError> {
        let out = self.run(args)?;
        if out.code != 0 {
            return Err(ServiceError::System(format!(
                "launchctl {} exited with {}",
                args.first()
                    .map(|a| a.to_string_lossy())
                    .unwrap_or_default(),
                out.code
            )));
        }
        Ok(out)
    }

    /// `None` when launchd does not know the label.
    fn print(&self, label: &str) -> Result<Option<String>, ServiceError> {
        let out = self.run(&["print".into(), self.target(label)])?;
        match out.code {
            0 => Ok(Some(out.stdout)),
            NO_SUCH_SERVICE => Ok(None),
            code => Err(ServiceError::System(format!(
                "launchctl print exited with {code}"
            ))),
        }
    }

    /// The loaded job's `launchctl print`, `None` when nothing is loaded under the label,
    /// or a conflict when the job was loaded from a file other than our definition.
    fn print_ours(&self, label: &str) -> Result<Option<String>, ServiceError> {
        let Some(printed) = self.print(label)? else {
            return Ok(None);
        };
        let file = self.file(label);
        let ours = match parse_path(&printed) {
            Some(loaded) => {
                let loaded = Path::new(&loaded);
                loaded == file
                    || canonical(&file).is_some_and(|f| canonical(loaded).as_ref() == Some(&f))
            }
            None => false,
        };
        if !ours {
            return Err(ServiceError::Conflict(format!(
                "launchd runs {label} from a definition other than {}",
                file.display()
            )));
        }
        Ok(Some(printed))
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
        let temporary = file.with_extension(format!("plist.{}.tmp", std::process::id()));
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

    fn bootstrap(&self, file: &Path) -> Result<(), ServiceError> {
        self.expect_ok(&["bootstrap".into(), self.domain.clone().into(), file.into()])
            .map(drop)
    }
}

impl ServiceManager for Launchd {
    fn register(&self, unit: &Unit) -> Result<Registration, ServiceError> {
        validate(unit)?;
        let label = self.label(&unit.instance_id)?;
        let file = self.file(&label);
        let bytes = render(&label, unit);
        let loaded = self.print_ours(&label)?.is_some();
        match self.read_definition(&file)? {
            Some(existing) if existing != bytes => Err(ServiceError::Conflict(format!(
                "{} already defines {label} differently; it is left untouched",
                file.display()
            ))),
            Some(_) if loaded => Ok(Registration::Unchanged),
            Some(_) => self.bootstrap(&file).map(|()| Registration::Loaded),
            None if loaded => Err(ServiceError::Conflict(format!(
                "launchd already runs {label} from another definition"
            ))),
            None => {
                self.write_definition(&file, &bytes)?;
                self.bootstrap(&file).map(|()| Registration::Loaded)
            }
        }
    }

    fn start(&self, instance_id: &str) -> Result<(), ServiceError> {
        let label = self.label(instance_id)?;
        match self.status(instance_id)? {
            ServiceStatus::Running { .. } => Ok(()),
            ServiceStatus::NotRegistered => {
                Err(ServiceError::Invalid("the service is not registered"))
            }
            // The file at the definition path is not known to be ours until register has
            // compared it with the unit; start never loads it unchecked.
            ServiceStatus::Unloaded => Err(ServiceError::Invalid(
                "the service is not loaded; register it again",
            )),
            // Without -k: kickstart never restarts a running job.
            ServiceStatus::Stopped => self
                .expect_ok(&["kickstart".into(), self.target(&label)])
                .map(drop),
        }
    }

    fn stop(&self, instance_id: &str) -> Result<(), ServiceError> {
        let label = self.label(instance_id)?;
        match self.status(instance_id)? {
            ServiceStatus::Running { .. } => self
                .expect_ok(&["kill".into(), "SIGTERM".into(), self.target(&label)])
                .map(drop),
            _ => Ok(()),
        }
    }

    fn status(&self, instance_id: &str) -> Result<ServiceStatus, ServiceError> {
        let label = self.label(instance_id)?;
        // A job under the label loaded from another file is reported as a conflict, so
        // start and stop never act on it.
        match self.print_ours(&label)? {
            Some(printed) => Ok(parse_print(&printed)),
            None if self.read_definition(&self.file(&label))?.is_some() => {
                Ok(ServiceStatus::Unloaded)
            }
            None => Ok(ServiceStatus::NotRegistered),
        }
    }

    fn unregister(&self, instance_id: &str) -> Result<(), ServiceError> {
        let label = self.label(instance_id)?;
        let file = self.file(&label);
        if self.print_ours(&label)?.is_some() {
            let out = self.run(&["bootout".into(), self.target(&label)])?;
            if out.code != 0 && out.code != NO_SUCH_SERVICE {
                return Err(ServiceError::System(format!(
                    "launchctl bootout exited with {}",
                    out.code
                )));
            }
            // bootout can return while launchd still tears the job down.
            let deadline = std::time::Instant::now() + UNLOAD_WAIT;
            while self.print(&label)?.is_some() {
                if std::time::Instant::now() >= deadline {
                    return Err(ServiceError::System(format!(
                        "launchd still lists {label} {} s after bootout",
                        UNLOAD_WAIT.as_secs()
                    )));
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
        match fs::remove_file(&file) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(ServiceError::System(format!(
                "cannot remove {}: {e}",
                file.display()
            ))),
            _ => Ok(()),
        }
    }
}

/// `state = running` and `pid = N` from `launchctl print`'s top-level block.
fn parse_print(printed: &str) -> ServiceStatus {
    let mut running = false;
    let mut pid = None;
    for line in printed.lines() {
        // Top-level properties are indented by exactly one tab.
        let Some(line) = line.strip_prefix('\t').filter(|l| !l.starts_with('\t')) else {
            continue;
        };
        if let Some(state) = line.strip_prefix("state = ") {
            running = state.trim() == "running";
        } else if let Some(value) = line.strip_prefix("pid = ") {
            pid = value.trim().parse::<u32>().ok();
        }
    }
    match (running, pid) {
        (true, Some(pid)) => ServiceStatus::Running { pid },
        _ => ServiceStatus::Stopped,
    }
}

/// The top-level `path = ...`: the file launchd loaded the job from.
fn parse_path(printed: &str) -> Option<String> {
    printed.lines().find_map(|line| {
        line.strip_prefix('\t')
            .filter(|l| !l.starts_with('\t'))
            .and_then(|l| l.strip_prefix("path = "))
            .map(|p| p.trim_end().to_owned())
    })
}

fn canonical(path: &Path) -> Option<PathBuf> {
    match path.parent() {
        Some(parent) => Some(fs::canonicalize(parent).ok()?.join(path.file_name()?)),
        None => None,
    }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // A literal carriage return would be read back as a line feed.
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
    out
}

/// The launchd definition. Inputs were validated as UTF-8 by `validate`.
fn render(label: &str, unit: &Unit) -> Vec<u8> {
    let text = |s: &std::ffi::OsStr| escape(&s.to_string_lossy());
    let mut p = String::from(concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
        "<plist version=\"1.0\">\n<dict>\n",
    ));
    p.push_str(&format!(
        "\t<key>Label</key>\n\t<string>{}</string>\n",
        escape(label)
    ));
    p.push_str("\t<key>ProgramArguments</key>\n\t<array>\n");
    p.push_str(&format!(
        "\t\t<string>{}</string>\n",
        text(unit.program.as_os_str())
    ));
    for arg in &unit.args {
        p.push_str(&format!("\t\t<string>{}</string>\n", text(arg)));
    }
    p.push_str("\t</array>\n\t<key>EnvironmentVariables</key>\n\t<dict>\n");
    for (key, value) in &unit.env {
        p.push_str(&format!(
            "\t\t<key>{}</key>\n\t\t<string>{}</string>\n",
            text(key),
            text(value)
        ));
    }
    p.push_str(concat!(
        "\t</dict>\n",
        "\t<key>RunAtLoad</key>\n\t<true/>\n",
        "\t<key>KeepAlive</key>\n\t<dict>\n\t\t<key>SuccessfulExit</key>\n\t\t<false/>\n\t</dict>\n",
        "\t<key>Umask</key>\n\t<integer>63</integer>\n",
        "</dict>\n</plist>\n",
    ));
    p.into_bytes()
}

#[cfg(test)]
mod tests;
