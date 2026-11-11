//! Linux process observations and individual signals bound to held pidfds.

use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::time::{Duration, Instant};

use super::{
    native_mark, sealed, valid_pid, MarkEvidence, ProcessIdentity, ProcessObservation,
    RecoveryContext, RecoveryProcesses, RecoverySignal, SignalOutcome, TargetEvidence,
    VerifiedTarget,
};

const PROC_RECORD_LIMIT: usize = 64 * 1024;
const MOUNTINFO_LIMIT: usize = 2 * 1024 * 1024;
const ENVIRONMENT_LIMIT: usize = 2 * 1024 * 1024;

/// Native Linux recovery operations. A snapshot retains pidfds for its live processes.
/// Each signal is sent through the matching descriptor after revalidating the target.
pub struct NativeRecovery {
    procfs: File,
    procfs_mount_id: u64,
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
        let procfs_mount_id = mount_id(&procfs)?;
        check_mount(&procfs, procfs_mount_id, deadline)?;
        let boot = boot_identity(&procfs, deadline)?;
        Ok(Self {
            procfs,
            procfs_mount_id,
            context: RecoveryContext { uid, pid, boot },
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
            Err(err) if vanished(&err) && exited(&pin.fd, None)? => return Ok(None),
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
            read_mark(&pin.directory, pid, pin.stat, deadline)?
        };
        // A zombie thread-group leader can still have live workers. Only the
        // process pidfd certifies exit of the whole group.
        let exited = exited(&pin.fd, None)?;
        if exited && !pin.stat.zombie {
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
                zombie: exited,
                mark,
            },
            pin,
        )))
    }

    fn pin(&self, pid: u32, deadline: Option<Instant>) -> io::Result<Option<Pin>> {
        let name = CString::new(pid.to_string()).unwrap();
        for _ in 0..2 {
            if let Some(deadline) = deadline {
                check_time(deadline)?;
            }
            // Open the identity-bound procfs directory before the numeric pidfd
            // lookup. Reading stat through that directory afterward proves the
            // old PID could not have been reaped and reassigned between opens.
            let directory = open_at(
                &self.procfs,
                &name,
                libc::O_DIRECTORY,
                libc::RESOLVE_NO_XDEV,
            );
            let fd = match pidfd(pid) {
                Ok(fd) => fd,
                Err(err) if vanished(&err) => return Ok(None),
                Err(err) => return Err(err),
            };
            let capture = directory.and_then(|directory| {
                let stat = read_stat(&directory, pid, deadline)?;
                Ok((directory, stat))
            });
            match capture {
                Ok((directory, stat)) => {
                    return Ok(Some(Pin {
                        fd,
                        directory,
                        stat,
                    }));
                }
                // Retry a vanished lookup. A hidden procfs row alone is never
                // proof of exit while a pidfd lookup succeeds.
                Err(err) if vanished(&err) => continue,
                Err(err) => return Err(err),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "process identity could not be pinned to its procfs directory",
        ))
    }
}

impl sealed::Processes for NativeRecovery {}

impl RecoveryProcesses for NativeRecovery {
    fn context(&mut self) -> io::Result<RecoveryContext> {
        Ok(self.context.clone())
    }

    fn snapshot(&mut self, deadline: Instant) -> io::Result<Vec<ProcessObservation>> {
        self.pins.clear();
        check_mount(&self.procfs, self.procfs_mount_id, deadline)?;
        let mut rows = Vec::new();
        let mut pins = BTreeMap::new();
        let mut directory = ProcDirectory::open(&self.procfs)?;
        while let Some(name) = directory.next(deadline)? {
            let Ok(pid) = number::<u32>(name.to_bytes()) else {
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
                Err(err) if vanished(&err) && exited(&child.fd, None)? => continue,
                Err(err) => return Err(err),
            };
            if current.started == child.stat.started
                && current.parent == child.stat.parent
                && !current.zombie
                && !exited(&child.fd, None)?
                && !exited(&parent.fd, None)?
            {
                row.parent = Some(self.identity(current.parent, parent.stat));
            }
        }
        check_mount(&self.procfs, self.procfs_mount_id, deadline)?;
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
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "recovery cannot verify credentials and mark for an exited thread-group leader",
                ));
            }
            let mark = read_mark(&pin.directory, identity.pid, stat, deadline)?;
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
            Ok(())
        })();
        if exited(&pin.fd, None)? {
            return Ok(SignalOutcome::Gone);
        }
        verified?;
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
        // An already-expired deadline requests one nonblocking observation.
        let deadline = (Instant::now() < deadline).then_some(deadline);
        if let Some(pin) = self
            .pins
            .get(&identity.pid)
            .filter(|p| p.stat.started.to_string() == identity.started)
        {
            return exited(&pin.fd, deadline);
        }
        let pin = self.pin(identity.pid, deadline)?;
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Ok(false);
        }
        let Some(pin) = pin else {
            return Ok(true);
        };
        if pin.stat.started.to_string() != identity.started {
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
    environment: (u64, u64),
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
    // Field 22 was start time. Fields 50 and 51 bound the environment.
    let environment = (
        number(
            fields
                .nth(27)
                .ok_or_else(|| invalid("missing proc stat env_start"))?,
        )?,
        number(
            fields
                .next()
                .ok_or_else(|| invalid("missing proc stat env_end"))?,
        )?,
    );
    Ok(Stat {
        parent,
        group,
        session,
        started,
        environment,
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

fn read_mark(
    directory: &File,
    pid: u32,
    stat: Stat,
    deadline: Instant,
) -> io::Result<MarkEvidence> {
    let (start, end) = stat.environment;
    let Some(length) = end
        .checked_sub(start)
        .filter(|length| start != 0 && *length <= ENVIRONMENT_LIMIT as u64)
    else {
        return Ok(MarkEvidence::Unreadable);
    };
    let capture: io::Result<MarkEvidence> = (|| {
        let bytes = read_at(directory, c"environ", length as usize, Some(deadline))?;
        // A hole in the target's memory can produce an early EOF even after a
        // complete NUL-terminated entry. Require the entire advertised range.
        if bytes.len() != length as usize {
            return Ok(MarkEvidence::Unreadable);
        }
        let current = read_stat(directory, pid, Some(deadline))?;
        if current.started != stat.started
            || current.zombie
            || current.environment != stat.environment
        {
            return Ok(MarkEvidence::Unreadable);
        }
        Ok(native_mark(&bytes))
    })();
    match capture {
        Ok(mark) => Ok(mark),
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

fn mount_id(file: &File) -> io::Result<u64> {
    let mut info = std::mem::MaybeUninit::<libc::statx>::uninit();
    // SAFETY: AT_EMPTY_PATH queries the open descriptor into a correctly sized slot.
    if unsafe {
        libc::statx(
            file.as_raw_fd(),
            c"".as_ptr(),
            libc::AT_EMPTY_PATH | libc::AT_STATX_DONT_SYNC,
            libc::STATX_MNT_ID,
            info.as_mut_ptr(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful statx initialized the output.
    let info = unsafe { info.assume_init() };
    if info.stx_mask & libc::STATX_MNT_ID == 0 {
        return Err(invalid("missing procfs mount ID"));
    }
    Ok(info.stx_mnt_id)
}

fn check_mount(procfs: &File, mount_id: u64, deadline: Instant) -> io::Result<()> {
    let mountinfo = read_at(procfs, c"self/mountinfo", MOUNTINFO_LIMIT, Some(deadline))?;
    validate_mountinfo(&mountinfo, mount_id)
}

fn validate_mountinfo(mountinfo: &[u8], mount_id: u64) -> io::Result<()> {
    let (root, options) = proc_mount_record(mountinfo, mount_id)?;
    if root != b"/" {
        return Err(invalid("recovery requires the procfs root directory"));
    }
    if options
        .split(|byte| *byte == b',')
        .filter_map(|option| option.strip_prefix(b"hidepid="))
        .any(|value| value != b"off" && value != b"0")
    {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Linux recovery requires an unfiltered procfs mount (hidepid=off)",
        ));
    }
    Ok(())
}

fn proc_mount_record(mountinfo: &[u8], mount_id: u64) -> io::Result<(&[u8], &[u8])> {
    let id = mount_id.to_string();
    for row in mountinfo.split(|byte| *byte == b'\n') {
        let mut fields = row.split(|byte| *byte == b' ');
        if fields.next() != Some(id.as_bytes()) {
            continue;
        }
        let root = fields
            .nth(2)
            .ok_or_else(|| invalid("missing procfs mount root"))?;
        if !fields.any(|field| field == b"-") || fields.next() != Some(b"proc") {
            return Err(invalid("invalid procfs mount record"));
        }
        let options = fields
            .nth(1)
            .ok_or_else(|| invalid("missing procfs mount options"))?;
        return Ok((root, options));
    }
    Err(invalid("procfs mount ID is absent from mountinfo"))
}

fn boot_identity(procfs: &File, deadline: Instant) -> io::Result<String> {
    let resolve = libc::RESOLVE_NO_XDEV | libc::RESOLVE_NO_SYMLINKS;
    let boot = match open_at(procfs, c"sys/kernel/random/boot_id", 0, resolve) {
        Ok(boot) => boot,
        Err(err) if err.raw_os_error() == Some(libc::EXDEV) => {
            // Container runtimes may bind the same procfs /sys subtree read-only.
            // Verify that subtree's origin and forbid further mount crossings.
            let sys = open_at(procfs, c"sys", libc::O_DIRECTORY, libc::RESOLVE_NO_SYMLINKS)?;
            let mountinfo = read_at(procfs, c"self/mountinfo", MOUNTINFO_LIMIT, Some(deadline))?;
            let (root, _) = proc_mount_record(&mountinfo, mount_id(&sys)?)?;
            if root != b"/sys" || sys.metadata()?.dev() != procfs.metadata()?.dev() {
                return Err(invalid("boot identity is outside the held procfs instance"));
            }
            open_at(&sys, c"kernel/random/boot_id", 0, resolve)?
        }
        Err(err) => return Err(err),
    };
    let boot = read_bounded(boot, 128, Some(deadline))?;
    let boot = std::str::from_utf8(&boot)
        .map_err(|_| invalid("invalid procfs boot identity"))?
        .trim();
    if boot.len() != 36
        || !boot.bytes().enumerate().all(|(i, byte)| match i {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
    {
        return Err(invalid("invalid procfs boot identity"));
    }
    Ok(boot.into())
}

struct ProcDirectory(std::ptr::NonNull<libc::DIR>);

impl ProcDirectory {
    fn open(procfs: &File) -> io::Result<Self> {
        let directory = open_at(procfs, c".", libc::O_DIRECTORY, libc::RESOLVE_NO_XDEV)?;
        // SAFETY: valid, separately opened directory fd. fdopendir takes it on success.
        let stream = unsafe { libc::fdopendir(directory.as_raw_fd()) };
        let stream = std::ptr::NonNull::new(stream).ok_or_else(io::Error::last_os_error)?;
        let _ = directory.into_raw_fd();
        Ok(Self(stream))
    }

    fn next(&mut self, deadline: Instant) -> io::Result<Option<&CStr>> {
        check_time(deadline)?;
        // SAFETY: errno is thread-local and the stream is exclusively borrowed.
        let entry = unsafe {
            *libc::__errno_location() = 0;
            libc::readdir(self.0.as_ptr())
        };
        if entry.is_null() {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(0) {
                Ok(None)
            } else {
                Err(error)
            };
        }
        // SAFETY: readdir returned a valid NUL-terminated name. Borrowing the
        // stream prevents another read or close while the result is in use.
        Ok(Some(unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }))
    }
}

impl Drop for ProcDirectory {
    fn drop(&mut self) {
        // SAFETY: the stream exclusively owns its fd and is closed exactly once.
        unsafe { libc::closedir(self.0.as_ptr()) };
    }
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

fn open_at(directory: &File, name: &CStr, flags: libc::c_int, resolve: u64) -> io::Result<File> {
    // SAFETY: open_how contains only integer fields. Zero initializes mode.
    let mut how: libc::open_how = unsafe { std::mem::zeroed() };
    how.flags =
        (flags | libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK) as u64;
    how.resolve = resolve;
    // SAFETY: live dirfd, NUL-terminated path and correctly sized open_how.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            directory.as_raw_fd(),
            name.as_ptr(),
            &how,
            std::mem::size_of_val(&how),
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat2 returned a new descriptor.
    Ok(unsafe { File::from_raw_fd(fd as libc::c_int) })
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
    // Process records and descriptor metadata must be from the held procfs
    // instance. A bind mount can otherwise substitute another process's data.
    let file = open_at(directory, name, 0, libc::RESOLVE_NO_XDEV)?;
    read_bounded(file, limit, deadline)
}

fn read_bounded(file: File, limit: usize, deadline: Option<Instant>) -> io::Result<Vec<u8>> {
    let mut file = file.take((limit + 1) as u64);
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

fn exited(fd: &OwnedFd, deadline: Option<Instant>) -> io::Result<bool> {
    let mut slot = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    poll_until(deadline, Instant::now, |remaining| {
        let timeout = libc::timespec {
            tv_sec: remaining.as_secs().min(libc::time_t::MAX as u64) as libc::time_t,
            tv_nsec: remaining.subsec_nanos() as _,
        };
        // SAFETY: one valid slot for a held pidfd, a finite timeout and no signal mask.
        match unsafe { libc::ppoll(&mut slot, 1, &timeout, std::ptr::null()) } {
            result if result < 0 => Err(io::Error::last_os_error()),
            0 => Ok(false),
            _ if slot.revents & (libc::POLLIN | libc::POLLHUP) != 0 => Ok(true),
            _ => Err(io::Error::other("unexpected pidfd poll event")),
        }
    })
}

fn poll_until(
    deadline: Option<Instant>,
    mut now: impl FnMut() -> Instant,
    mut poll: impl FnMut(Duration) -> io::Result<bool>,
) -> io::Result<bool> {
    loop {
        let current = now();
        let remaining = match deadline {
            Some(deadline) if current >= deadline => return Ok(false),
            Some(deadline) => deadline.duration_since(current),
            None => Duration::ZERO,
        };
        let ready = poll(remaining);
        // ppoll can observe readiness on a rescan after its timer expires.
        if deadline.is_some_and(|deadline| now() >= deadline) {
            return Ok(false);
        }
        match ready {
            Ok(false) if deadline.is_some() => continue,
            Err(err) if deadline.is_some() && err.kind() == io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{open_at, parse_stat, poll_until, validate_mountinfo, Stat};
    use std::cell::Cell;
    use std::fs::File;
    use std::time::{Duration, Instant};

    #[test]
    fn process_record_open_rejects_crossing_a_mount() {
        let root = File::open("/").unwrap();
        let err = open_at(&root, c"proc", libc::O_DIRECTORY, libc::RESOLVE_NO_XDEV).unwrap_err();
        assert_eq!(err.raw_os_error(), Some(libc::EXDEV));
    }

    #[test]
    fn pidfd_readiness_after_the_deadline_is_not_completion() {
        let start = Instant::now();
        let deadline = start + Duration::from_millis(30);
        let clock = Cell::new(start);
        let gone = poll_until(
            Some(deadline),
            || clock.get(),
            |_| {
                clock.set(deadline);
                Ok(true)
            },
        )
        .unwrap();
        assert!(!gone);
        assert!(poll_until(
            None,
            || clock.get(),
            |remaining| {
                assert!(remaining.is_zero());
                Ok(true)
            }
        )
        .unwrap());
    }

    #[test]
    fn checks_the_held_procfs_mount_for_filtering() {
        let unfiltered = b"42 20 0:21 / /proc rw - proc proc rw\n";
        assert!(validate_mountinfo(unfiltered, 42).is_ok());
        assert!(validate_mountinfo(unfiltered, 44).is_err());
        let another_mount = b"41 20 0:22 / /proc-alt rw - proc proc rw,hidepid=off\n";
        let filtered = b"42 20 0:21 / /proc rw master:2 - proc proc rw,hidepid=ptraceable\n";
        let mountinfo = [another_mount.as_slice(), filtered.as_slice()].concat();
        assert!(validate_mountinfo(&mountinfo, 41).is_ok());
        assert!(validate_mountinfo(&mountinfo, 42).is_err());
        let bind = b"42 20 0:21 /2 /proc rw - proc proc rw\n";
        assert!(validate_mountinfo(bind, 42).is_err());
    }

    #[test]
    fn stat_uses_the_last_parenthesis_and_only_decodes_numeric_fields() {
        let mut bytes =
            b"42 (\xff) a process) S 13 41 44 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 789".to_vec();
        for _ in 23..50 {
            bytes.extend_from_slice(b" 0");
        }
        bytes.extend_from_slice(b" 4096 4128 0\n");
        assert_eq!(
            parse_stat(&bytes, 42).unwrap(),
            Stat {
                parent: 13,
                group: 41,
                session: 44,
                started: 789,
                environment: (4096, 4128),
                zombie: false,
            }
        );
        assert!(parse_stat(&bytes, 43).is_err());
    }
}
