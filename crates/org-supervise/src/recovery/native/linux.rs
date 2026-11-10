//! Linux process observations and individual signals bound to held pidfds.

use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::time::{Duration, Instant};

use super::{
    native_mark, sealed, valid_pid, MarkEvidence, ProcessIdentity, ProcessObservation,
    RecoveryContext, RecoveryProcesses, RecoverySignal, SignalOutcome, TargetEvidence,
    VerifiedTarget,
};

const PROC_RECORD_LIMIT: usize = 64 * 1024;
const ENVIRONMENT_LIMIT: usize = 2 * 1024 * 1024;

/// Native Linux recovery operations. A snapshot retains pidfds for its live processes.
/// Each signal is sent through the matching descriptor after revalidating the target.
pub struct NativeRecovery {
    procfs: File,
    context: RecoveryContext,
    pins: BTreeMap<u32, Pin>,
}

impl NativeRecovery {
    /// Open the native adapter, checking the procfs namespace and kernel pidfd support.
    pub fn new() -> io::Result<Self> {
        crate::platform::validate()?;
        let procfs = File::open("/proc")?;
        let mut filesystem = std::mem::MaybeUninit::<libc::statfs>::uninit();
        // SAFETY: an open file descriptor and correctly sized output slot.
        if unsafe { libc::fstatfs(procfs.as_raw_fd(), filesystem.as_mut_ptr()) } < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful fstatfs filled the slot.
        if unsafe { filesystem.assume_init() }.f_type != libc::PROC_SUPER_MAGIC as libc::c_long {
            return Err(invalid("/proc is not procfs"));
        }
        // SAFETY: these getters have no pointer arguments or side effects.
        let (pid, uid) = unsafe { (libc::getpid() as u32, libc::getuid()) };
        let deadline = Instant::now() + Duration::from_secs(1);
        let fd = pidfd(pid).map_err(pidfd_startup_error)?;
        send(&fd, 0).map_err(pidfd_startup_error)?;
        let info_path = CString::new(format!("self/fdinfo/{}", fd.as_raw_fd())).unwrap();
        let info = read_at(&procfs, &info_path, PROC_RECORD_LIMIT, Some(deadline))?;
        let info = std::str::from_utf8(&info).map_err(|_| invalid("invalid pidfd metadata"))?;
        if field(info, "Pid:")?.trim().parse::<u32>() != Ok(pid)
            || !field(info, "NSpid:")?
                .split_ascii_whitespace()
                .map(str::parse::<u32>)
                .eq([Ok(pid)])
        {
            return Err(invalid(
                "procfs does not match the supervisor PID namespace",
            ));
        }
        let boot = read_at(&procfs, c"sys/kernel/random/boot_id", 128, Some(deadline))?;
        let boot = std::str::from_utf8(&boot)
            .map_err(|_| invalid("invalid procfs boot identity"))?
            .trim();
        if boot.is_empty() {
            return Err(invalid("empty procfs boot identity"));
        }
        Ok(Self {
            procfs,
            context: RecoveryContext {
                uid,
                pid,
                boot: boot.into(),
            },
            pins: BTreeMap::new(),
        })
    }

    fn identity(&self, pid: u32, stat: Stat) -> ProcessIdentity {
        ProcessIdentity {
            pid,
            started: stat.started.to_string(),
            boot: self.context.boot.clone(),
        }
    }

    fn sample(&self, pid: u32, deadline: Instant) -> io::Result<Option<(ProcessObservation, Pin)>> {
        let Some(pin) = self.pin(pid, Some(deadline))? else {
            return Ok(None);
        };
        let credentials = match credentials(&pin.directory, deadline) {
            Ok(credentials) => credentials,
            Err(err) if vanished(&err) && exited(&pin.fd, Instant::now())? => return Ok(None),
            Err(err) => return Err(err),
        };
        if credentials.uid[0] != self.context.uid {
            return Ok(None);
        }
        let credentials_safe = credentials.safe(self.context.uid)
            && pin.directory.metadata()?.uid() == self.context.uid
            && ordinary_exec(&pin.directory, deadline)?;
        let mark = if pin.stat.zombie || !credentials_safe || pid == self.context.pid {
            MarkEvidence::Unreadable
        } else {
            read_mark(&pin.directory, deadline)?
        };
        if !pin.stat.zombie && exited(&pin.fd, Instant::now())? {
            return Ok(None);
        }
        Ok(Some((
            ProcessObservation {
                pid,
                identity: Some(self.identity(pid, pin.stat)),
                parent: None,
                group: pin.stat.group,
                session: pin.stat.session,
                uid: credentials.uid[0],
                credentials_safe,
                zombie: pin.stat.zombie,
                mark,
            },
            pin,
        )))
    }

    fn pin(&self, pid: u32, deadline: Option<Instant>) -> io::Result<Option<Pin>> {
        if let Some(deadline) = deadline {
            check_time(deadline)?;
        }
        let fd = match pidfd(pid) {
            Ok(fd) => fd,
            Err(err) if vanished(&err) => return Ok(None),
            Err(err) => return Err(err),
        };
        let name = CString::new(pid.to_string()).unwrap();
        let capture = open_at(&self.procfs, &name, libc::O_DIRECTORY).and_then(|directory| {
            let stat = read_stat(&directory, pid, deadline)?;
            Ok((directory, stat))
        });
        match capture {
            Ok((directory, stat)) => {
                if exited(&fd, Instant::now())? && !stat.zombie {
                    return Ok(None);
                }
                Ok(Some(Pin {
                    fd,
                    directory,
                    stat,
                }))
            }
            Err(err) if vanished(&err) && exited(&fd, Instant::now())? => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl sealed::Processes for NativeRecovery {}

impl RecoveryProcesses for NativeRecovery {
    fn context(&mut self) -> io::Result<RecoveryContext> {
        Ok(self.context.clone())
    }

    fn snapshot(&mut self, deadline: Instant) -> io::Result<Vec<ProcessObservation>> {
        self.pins.clear();
        let mut rows = Vec::new();
        let mut pins = BTreeMap::new();
        // Open the already validated procfs root through its held descriptor.
        let entries = fs::read_dir(format!("/proc/self/fd/{}", self.procfs.as_raw_fd()))?;
        for entry in entries {
            check_time(deadline)?;
            let entry = entry?;
            let Ok(pid) = number::<u32>(entry.file_name().as_bytes()) else {
                continue;
            };
            if !valid_pid(pid) || pid == self.context.pid {
                continue;
            }
            if let Some((row, pin)) = self.sample(pid, deadline)? {
                rows.push(row);
                pins.insert(pid, pin);
            }
        }
        for row in &mut rows {
            check_time(deadline)?;
            let child = &pins[&row.pid];
            let Some(parent) = pins.get(&child.stat.parent) else {
                continue;
            };
            if child.stat.zombie || parent.stat.zombie {
                continue;
            }
            // Read PPID after both descriptors were acquired. A live parent fd at
            // the end excludes reuse during the observation of the child's PPID.
            let current = match read_stat(&child.directory, row.pid, Some(deadline)) {
                Ok(stat) => stat,
                Err(err) if vanished(&err) && exited(&child.fd, Instant::now())? => continue,
                Err(err) => return Err(err),
            };
            if current.started == child.stat.started
                && current.parent == child.stat.parent
                && !current.zombie
                && !exited(&child.fd, Instant::now())?
                && !exited(&parent.fd, Instant::now())?
            {
                row.parent = Some(self.identity(current.parent, parent.stat));
            }
        }
        check_time(deadline)?;
        self.pins = pins;
        Ok(rows)
    }

    fn signal_checked(
        &mut self,
        target: &VerifiedTarget,
        signal: RecoverySignal,
        deadline: Instant,
    ) -> io::Result<SignalOutcome> {
        check_time(deadline)?;
        let identity = target.identity();
        if !valid_pid(identity.pid)
            || identity.pid == self.context.pid
            || identity.boot != self.context.boot
        {
            return Err(invalid("invalid pidfd signal target"));
        }
        let pin = self
            .pins
            .get(&identity.pid)
            .filter(|p| p.stat.started.to_string() == identity.started)
            .ok_or_else(|| invalid("signal target is not pinned by the current snapshot"))?;
        let verified = (|| {
            let stat = read_stat(&pin.directory, identity.pid, Some(deadline))?;
            if stat.started != pin.stat.started {
                return Err(invalid("pinned process start identity changed"));
            }
            if stat.zombie {
                return Ok(false);
            }
            let mark = read_mark(&pin.directory, deadline)?;
            let direct = matches!(
                target.evidence(),
                TargetEvidence::Marked | TargetEvidence::RecordedLeader
            );
            if mark == MarkEvidence::Unreadable
                || (direct && mark != MarkEvidence::Present(target.mark().into()))
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "recovery mark changed or is unreadable",
                ));
            }
            if !credentials(&pin.directory, deadline)?.safe(self.context.uid)
                || pin.directory.metadata()?.uid() != self.context.uid
                || !ordinary_exec(&pin.directory, deadline)?
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "recovery target credentials are unsafe",
                ));
            }
            Ok(true)
        })();
        if exited(&pin.fd, Instant::now())? || !verified? {
            return Ok(SignalOutcome::Gone);
        }
        check_time(deadline)?;
        match send(
            &pin.fd,
            match signal {
                RecoverySignal::Terminate => libc::SIGTERM,
                RecoverySignal::Kill => libc::SIGKILL,
            },
        ) {
            Ok(()) => Ok(SignalOutcome::Signalled),
            Err(err) if err.raw_os_error() == Some(libc::ESRCH) => Ok(SignalOutcome::Gone),
            Err(err) => Err(err),
        }
    }

    fn wait_exited(&mut self, identity: &ProcessIdentity, deadline: Instant) -> io::Result<bool> {
        if identity.boot != self.context.boot {
            return Ok(true);
        }
        if !valid_pid(identity.pid) {
            return Err(invalid("invalid process wait identity"));
        }
        if let Some(pin) = self
            .pins
            .get(&identity.pid)
            .filter(|p| p.stat.started.to_string() == identity.started)
        {
            return exited(&pin.fd, deadline);
        }
        // The policy may request an observation with an already-expired deadline.
        // Take one bounded procfs reading rather than interpreting the deadline as exit.
        let read_deadline = (Instant::now() < deadline).then_some(deadline);
        let Some(pin) = self.pin(identity.pid, read_deadline)? else {
            return Ok(true);
        };
        if pin.stat.started.to_string() != identity.started || pin.stat.zombie {
            return Ok(true);
        }
        exited(&pin.fd, deadline)
    }

    fn now(&mut self) -> Instant {
        Instant::now()
    }

    fn wait_until(&mut self, deadline: Instant) -> io::Result<()> {
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        Ok(())
    }
}

struct Pin {
    fd: OwnedFd,
    directory: File,
    stat: Stat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stat {
    parent: u32,
    group: u32,
    session: u32,
    started: u64,
    zombie: bool,
}

fn read_stat(directory: &File, pid: u32, deadline: Option<Instant>) -> io::Result<Stat> {
    let bytes = read_at(directory, c"stat", PROC_RECORD_LIMIT, deadline)?;
    parse_stat(&bytes, pid)
}

fn parse_stat(bytes: &[u8], pid: u32) -> io::Result<Stat> {
    let open = bytes
        .iter()
        .position(|b| *b == b'(')
        .ok_or_else(|| invalid("missing proc stat comm"))?;
    let close = bytes
        .iter()
        .rposition(|b| *b == b')')
        .filter(|close| *close > open)
        .ok_or_else(|| invalid("unterminated proc stat comm"))?;
    let prefix = bytes[..open]
        .strip_suffix(b" ")
        .ok_or_else(|| invalid("invalid proc stat PID"))?;
    if number::<u32>(prefix)? != pid {
        return Err(invalid("proc stat PID mismatch"));
    }
    let mut fields = bytes[close + 1..]
        .split(|b| b.is_ascii_whitespace())
        .filter(|f| !f.is_empty());
    let mut next = || {
        fields
            .next()
            .ok_or_else(|| invalid("short proc stat record"))
    };
    let state = next()?;
    if state.len() != 1 {
        return Err(invalid("invalid proc stat state"));
    }
    let parent = number(next()?)?;
    let group = number(next()?)?;
    let session = number(next()?)?;
    let started = number(
        fields
            .nth(15)
            .ok_or_else(|| invalid("missing proc stat start time"))?,
    )?;
    Ok(Stat {
        parent,
        group,
        session,
        started,
        zombie: matches!(state, b"Z" | b"X" | b"x"),
    })
}

struct Credentials {
    uid: [u32; 4],
    gid: [u32; 4],
    capabilities: u64,
}
impl Credentials {
    fn safe(&self, uid: u32) -> bool {
        uid != 0 && self.uid == [uid; 4] && self.gid == [self.gid[0]; 4] && self.capabilities == 0
    }
}

fn credentials(directory: &File, deadline: Instant) -> io::Result<Credentials> {
    let bytes = read_at(directory, c"status", PROC_RECORD_LIMIT, Some(deadline))?;
    // A comm field can contain non-UTF-8 bytes. Parse only the exact numeric rows.
    let value = |key: &[u8]| -> io::Result<&str> {
        let mut rows = bytes
            .split(|b| *b == b'\n')
            .filter_map(|line| line.strip_prefix(key));
        let row = rows
            .next()
            .ok_or_else(|| invalid("missing proc credential row"))?;
        if rows.next().is_some() {
            return Err(invalid("duplicate proc credential row"));
        }
        std::str::from_utf8(row).map_err(|_| invalid("invalid proc credential row"))
    };
    let ids = |key: &[u8]| -> io::Result<[u32; 4]> {
        let mut words = value(key)?.split_ascii_whitespace();
        let mut ids = [0; 4];
        for id in &mut ids {
            *id = words
                .next()
                .and_then(|word| word.parse().ok())
                .ok_or_else(|| invalid("invalid proc credential IDs"))?;
        }
        if words.next().is_some() {
            return Err(invalid("extra proc credential ID"));
        }
        Ok(ids)
    };
    let caps = |key| {
        u64::from_str_radix(value(key)?.trim(), 16)
            .map_err(|_| invalid("invalid proc capabilities"))
    };
    Ok(Credentials {
        uid: ids(b"Uid:")?,
        gid: ids(b"Gid:")?,
        capabilities: caps(b"CapPrm:")? | caps(b"CapEff:")?,
    })
}

// AT_SECURE distinguishes a privileged exec even when the process has since
// reset its UID/GID values. Unknown layouts or unreadable evidence fail closed.
fn ordinary_exec(directory: &File, deadline: Instant) -> io::Result<bool> {
    let bytes = match read_at(directory, c"auxv", PROC_RECORD_LIMIT, Some(deadline)) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::TimedOut => return Err(err),
        Err(_) => return Ok(false),
    };
    let word = std::mem::size_of::<usize>();
    let mut entries = bytes.chunks_exact(2 * word);
    if !entries.remainder().is_empty() {
        return Ok(false);
    }
    let mut ordinary = None;
    for entry in &mut entries {
        let kind = usize::from_ne_bytes(entry[..word].try_into().unwrap());
        let value = usize::from_ne_bytes(entry[word..].try_into().unwrap());
        if kind == libc::AT_NULL as usize {
            return Ok(value == 0 && entries.len() == 0 && ordinary == Some(true));
        }
        if kind == libc::AT_SECURE as usize && ordinary.replace(value == 0).is_some() {
            return Ok(false);
        }
    }
    Ok(false)
}

fn read_mark(directory: &File, deadline: Instant) -> io::Result<MarkEvidence> {
    match read_at(directory, c"environ", ENVIRONMENT_LIMIT, Some(deadline)) {
        Ok(bytes) => Ok(native_mark(&bytes)),
        Err(err) if err.kind() == io::ErrorKind::TimedOut => Err(err),
        Err(_) => Ok(MarkEvidence::Unreadable),
    }
}

fn number<T: std::str::FromStr>(bytes: &[u8]) -> io::Result<T> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| invalid("invalid proc number"))
}

fn field<'a>(info: &'a str, key: &str) -> io::Result<&'a str> {
    info.lines()
        .find_map(|line| line.strip_prefix(key))
        .ok_or_else(|| invalid("missing pidfd namespace field"))
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn check_time(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "native recovery deadline elapsed",
        ))
    } else {
        Ok(())
    }
}

fn open_at(directory: &File, name: &CStr, flags: libc::c_int) -> io::Result<File> {
    // SAFETY: directory and NUL-terminated path stay live. No create flag is supplied.
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat returned a new descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn read_at(
    directory: &File,
    name: &CStr,
    limit: usize,
    deadline: Option<Instant>,
) -> io::Result<Vec<u8>> {
    if let Some(deadline) = deadline {
        check_time(deadline)?;
    }
    let mut file = open_at(directory, name, 0)?.take((limit + 1) as u64);
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        if let Some(deadline) = deadline {
            check_time(deadline)?;
        }
        match file.read(&mut buffer) {
            Ok(0) => return Ok(bytes),
            Ok(read) => {
                if bytes.len() + read > limit {
                    return Err(invalid("proc record exceeds its size limit"));
                }
                bytes.extend_from_slice(&buffer[..read]);
            }
            Err(err) if deadline.is_some() && err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        }
    }
}

fn vanished(err: &io::Error) -> bool {
    matches!(err.raw_os_error(), Some(libc::ENOENT | libc::ESRCH))
}

fn pidfd_startup_error(err: io::Error) -> io::Error {
    match err.raw_os_error() {
        Some(libc::ENOSYS | libc::EPERM) => io::Error::new(
            io::ErrorKind::Unsupported,
            "doctor: Linux recovery requires pidfd_open and pidfd_send_signal permissions",
        ),
        _ => err,
    }
}

fn pidfd(pid: u32) -> io::Result<OwnedFd> {
    // SAFETY: validated PID in the caller's namespace, no flags or pointers.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0u32) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful pidfd_open returned a new close-on-exec descriptor.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as libc::c_int) })
}

fn send(fd: &OwnedFd, signal: libc::c_int) -> io::Result<()> {
    // SAFETY: a live owned pidfd, individual signal, null siginfo and zero flags.
    if unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            fd.as_raw_fd(),
            signal,
            std::ptr::null::<libc::siginfo_t>(),
            0u32,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn exited(fd: &OwnedFd, deadline: Instant) -> io::Result<bool> {
    let mut slot = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let millis = remaining.as_millis().min(i32::MAX as u128) as libc::c_int;
        // SAFETY: one valid poll slot for a held pidfd and a finite timeout.
        let result = unsafe { libc::poll(&mut slot, 1, millis) };
        if result < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted && Instant::now() < deadline {
                continue;
            }
            return Err(err);
        }
        if result > 0 {
            if slot.revents & (libc::POLLIN | libc::POLLHUP) != 0 {
                return Ok(true);
            }
            return Err(io::Error::other("unexpected pidfd poll event"));
        }
        if remaining < Duration::from_millis(1) || Instant::now() >= deadline {
            return Ok(false);
        }
    }
}
