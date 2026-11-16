//! Per-user launchd reconciliation. Labels are immutable instance IDs; an
//! existing different registration is refused, never unloaded or overwritten.
use super::*;
use std::{
    ffi::CString,
    fs::File,
    os::fd::{AsRawFd, FromRawFd},
};
fn xml(s: &str) -> Result<String, String> {
    if s.len() > 4096 || s.chars().any(char::is_control) {
        return Err("invalid service string".into());
    }
    Ok(s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;"))
}
fn description(
    label: &str,
    program: &Path,
    args: &[String],
    account: &Path,
) -> Result<Vec<u8>, String> {
    let mut elements = String::new();
    for arg in std::iter::once(program.to_str().ok_or("non-UTF8 service program")?)
        .chain(args.iter().map(String::as_str))
    {
        elements.push_str(&format!("<string>{}</string>", xml(arg)?));
    }
    Ok(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>{}</string><key>ProgramArguments</key><array>{elements}</array><key>WorkingDirectory</key><string>/</string><key>EnvironmentVariables</key><dict><key>PATH</key><string>/usr/bin:/bin</string><key>HOME</key><string>{}</string></dict><key>RunAtLoad</key><true/><key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict><key>ProcessType</key><string>Background</string></dict></plist>\n",xml(label)?,xml(account.to_str().ok_or("non-UTF8 account home")?)?).into_bytes())
}
fn launchctl(args: &[&std::ffi::OsStr]) -> Result<std::process::Output, String> {
    Command::new("/bin/launchctl")
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .map_err(|_| {
            "launchd is unavailable; use agentic-org serve --name NAME in the foreground".into()
        })
}
fn registration_matches(output: &[u8], unit: &Path, program: &Path, args: &[String]) -> bool {
    if output.len() > 64 * 1024 {
        return false;
    }
    let Ok(text) = std::str::from_utf8(output) else {
        return false;
    };
    let fields: Vec<_> = text.lines().map(str::trim).collect();
    if !fields.contains(&format!("path = {}", unit.display()).as_str())
        || !fields.contains(&format!("program = {}", program.display()).as_str())
    {
        return false;
    }
    let Some(start) = fields.iter().position(|line| *line == "arguments = {") else {
        return false;
    };
    let actual: Vec<_> = fields[start + 1..]
        .iter()
        .copied()
        .take_while(|line| *line != "}")
        .collect();
    let mut expected = vec![program.to_string_lossy().into_owned()];
    expected.extend_from_slice(args);
    actual == expected
}
pub(super) fn register(
    journal: &Journal,
    r: &Record,
    name: Option<&str>,
    program: &Path,
) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("user service manager is unavailable on this target; retain setup and run agentic-org serve --name NAME in the foreground".into());
    }
    let instance = r.instance_id.as_deref().ok_or("setup instance missing")?;
    if org_proto::ulid_time_ms(instance).is_none() || !r.exited {
        return Err("service registration requires completed original HQ setup".into());
    }
    let label = format!("com.ellosoft.agentic-org.{instance}");
    let name = Name::parse(name).map_err(error)?;
    let args = vec!["serve".into(), "--name".into(), name.as_str().into()];
    let account = crate::home::account_dir().ok_or("account home unavailable")?;
    let unit = journal
        .service_file(&description(&label, program, &args, &account)?)
        .map_err(error)?;
    reconcile(&account, &label, &unit, program, &args)
}
fn reconcile(
    account: &Path,
    label: &str,
    unit: &Path,
    program: &Path,
    args: &[String],
) -> Result<(), String> {
    let parent = AgentDirectory::open(&account.join("Library/LaunchAgents"))?;
    let link = format!("{label}.plist");
    parent.ensure_link(&link, unit)?;
    let domain = format!("gui/{}", org_socket::effective_uid());
    let job = format!("{domain}/{label}");
    let printed = launchctl(&["print".as_ref(), job.as_ref()])?;
    if printed.status.success() {
        if !registration_matches(&printed.stdout, unit, program, args) {
            return Err(
                "launchd label belongs to a different registration; nothing was replaced".into(),
            );
        }
    } else {
        let loaded = launchctl(&["bootstrap".as_ref(), domain.as_ref(), unit.as_os_str()])?;
        if !loaded.status.success() {
            return Err("launchd bootstrap failed; setup and data retained; check the GUI login domain or use foreground serve".into());
        }
    }
    let printed = launchctl(&["print".as_ref(), job.as_ref()])?;
    if !printed.status.success() || !registration_matches(&printed.stdout, unit, program, args) {
        return Err("launchd registration could not be verified; retained for repair".into());
    }
    let started = launchctl(&["kickstart".as_ref(), job.as_ref()])?;
    if !started.status.success() {
        return Err("launchd start failed; original registration/data retained".into());
    }
    Ok(())
}
// LaunchAgents itself may be 0755. This adapter permits only root/own safe
// ancestors and an own nonwritable final directory, then performs create-new
// link operations through its retained descriptor. Private state adapters keep
// their stricter 0700 policy unchanged.
struct AgentDirectory {
    file: File,
    path: PathBuf,
}
impl AgentDirectory {
    fn open(path: &Path) -> Result<Self, String> {
        Self::open_with(path, true)
    }
    fn open_with(path: &Path, create: bool) -> Result<Self, String> {
        use std::path::Component;
        if !path.is_absolute() || path.as_os_str().len() > 4096 {
            return Err("invalid LaunchAgents path".into());
        }
        let mut current = File::open("/").map_err(error)?;
        let mut parts = path.components().peekable();
        while let Some(part) = parts.next() {
            match part {
                Component::RootDir => {}
                Component::Normal(name) => {
                    use std::os::unix::ffi::OsStrExt;
                    let name = CString::new(name.as_bytes()).map_err(error)?;
                    let final_part = parts.peek().is_none();
                    let opened = open_agent_component(&current, &name, final_part, create)?;
                    let m = opened.metadata().map_err(error)?;
                    validate_agent_component(&m, final_part)?;
                    current = opened;
                }
                _ => return Err("invalid LaunchAgents component".into()),
            }
        }
        Ok(Self {
            file: current,
            path: path.to_owned(),
        })
    }
    fn verify(&self) -> Result<(), String> {
        let now = Self::open_with(&self.path, false)?;
        let before = self.file.metadata().map_err(error)?;
        let current = now.file.metadata().map_err(error)?;
        if before.dev() != current.dev() || before.ino() != current.ino() {
            return Err("LaunchAgents directory changed; registration retained for repair".into());
        }
        Ok(())
    }
    fn ensure_link(&self, name: &str, target: &Path) -> Result<(), String> {
        self.verify()?;
        use std::os::unix::ffi::OsStrExt;
        if name.len() > 128
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c))
        {
            return Err("invalid service link".into());
        }
        let name = CString::new(name).map_err(error)?;
        let target = CString::new(target.as_os_str().as_bytes()).map_err(error)?;
        // SAFETY: module-owned names and retained directory, create-new only.
        if unsafe { libc::symlinkat(target.as_ptr(), self.file.as_raw_fd(), name.as_ptr()) } != 0
            && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(error(()));
        }
        let mut m = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: valid writable stat and no-follow final entry.
        if unsafe {
            libc::fstatat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                m.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(error(()));
        }
        // SAFETY: successful fstatat initialized metadata.
        let m = unsafe { m.assume_init() };
        if m.st_mode & libc::S_IFMT != libc::S_IFLNK
            || m.st_uid != org_socket::effective_uid()
            || m.st_nlink != 1
        {
            return Err(
                "service autostart entry belongs to different state; nothing replaced".into(),
            );
        }
        let mut bytes = vec![0_u8; 4097];
        // SAFETY: live dirfd/name and initialized bounded writable buffer.
        let n = unsafe {
            libc::readlinkat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
            )
        };
        if n < 0 || n as usize >= bytes.len() || &bytes[..n as usize] != target.as_bytes() {
            return Err("service autostart link differs; nothing replaced".into());
        }
        self.file.sync_all().map_err(error)?;
        self.verify()
    }
}

fn open_agent_component(
    parent: &File,
    name: &std::ffi::CStr,
    final_part: bool,
    create: bool,
) -> Result<File, String> {
    // SAFETY: live retained dirfd, initialized leaf, no-follow flags.
    let mut fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0
        && final_part
        && create
        && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound
    {
        // SAFETY: create only the known final leaf under held parent.
        if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
            return Err(error(()));
        }
        parent.sync_all().map_err(error)?;
        // SAFETY: same retained parent and validated leaf.
        fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
    }
    if fd < 0 {
        return Err("unsafe or unavailable LaunchAgents directory".into());
    }
    // SAFETY: successful openat transferred this new owned fd.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn validate_agent_component(m: &fs::Metadata, final_part: bool) -> Result<(), String> {
    if !m.is_dir()
        || (m.uid() != 0 && m.uid() != org_socket::effective_uid())
        || (m.mode() & 0o022 != 0 && (final_part || m.mode() & 0o1000 == 0))
        || final_part && m.uid() != org_socket::effective_uid()
    {
        return Err("unsafe LaunchAgents ownership or permissions".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
