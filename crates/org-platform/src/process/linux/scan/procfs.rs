//! The held procfs instance: its PID namespace, mount and boot identity, its directory
//! listing, and bounded reads relative to a held directory that never cross a mount.

use std::ffi::CStr;
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
use std::os::unix::fs::MetadataExt;
use std::time::Instant;

use crate::process::check_time;

use super::pidfd::descriptor_error;
use super::{invalid, MOUNTINFO_LIMIT};

fn field<'a>(info: &'a str, key: &str) -> io::Result<&'a str> {
    info.lines()
        .find_map(|line| line.strip_prefix(key))
        .ok_or_else(|| invalid("missing pidfd namespace field"))
}

pub(super) fn pidfd_namespace_matches(info: &str, pid: u32) -> io::Result<bool> {
    Ok(field(info, "Pid:")?.trim().parse::<u32>() == Ok(pid)
        && field(info, "NSpid:")?
            .split_ascii_whitespace()
            .map(str::parse::<u32>)
            .eq([Ok(pid)]))
}

pub(super) fn boot_bind_ok(root: &[u8], source_dev: u64, procfs_dev: u64) -> bool {
    root == b"/sys" && source_dev == procfs_dev
}

pub(super) fn mount_id(file: &File) -> io::Result<u64> {
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

pub(super) fn check_mount(procfs: &File, mount_id: u64, deadline: Instant) -> io::Result<()> {
    let mountinfo = read_at(procfs, c"self/mountinfo", MOUNTINFO_LIMIT, Some(deadline))?;
    validate_mountinfo(&mountinfo, mount_id)
}

pub(super) fn validate_mountinfo(mountinfo: &[u8], mount_id: u64) -> io::Result<()> {
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

pub(super) fn boot_identity(procfs: &File, deadline: Instant) -> io::Result<String> {
    let resolve = libc::RESOLVE_NO_XDEV | libc::RESOLVE_NO_SYMLINKS;
    let boot = match open_at(procfs, c"sys/kernel/random/boot_id", 0, resolve) {
        Ok(boot) => boot,
        Err(err) if err.raw_os_error() == Some(libc::EXDEV) => {
            // Container runtimes may bind the same procfs /sys subtree read-only.
            // Verify that subtree's origin and forbid further mount crossings.
            let sys = open_at(procfs, c"sys", libc::O_DIRECTORY, libc::RESOLVE_NO_SYMLINKS)?;
            let mountinfo = read_at(procfs, c"self/mountinfo", MOUNTINFO_LIMIT, Some(deadline))?;
            let (root, _) = proc_mount_record(&mountinfo, mount_id(&sys)?)?;
            if !boot_bind_ok(root, sys.metadata()?.dev(), procfs.metadata()?.dev()) {
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

pub(super) struct ProcDirectory(std::ptr::NonNull<libc::DIR>);

impl ProcDirectory {
    pub(super) fn open(procfs: &File) -> io::Result<Self> {
        let directory = open_at(procfs, c".", libc::O_DIRECTORY, libc::RESOLVE_NO_XDEV)?;
        // SAFETY: valid, separately opened directory fd. fdopendir takes it on success.
        let stream = unsafe { libc::fdopendir(directory.as_raw_fd()) };
        let stream = std::ptr::NonNull::new(stream)
            .ok_or_else(|| descriptor_error(io::Error::last_os_error()))?;
        let _ = directory.into_raw_fd();
        Ok(Self(stream))
    }

    pub(super) fn next(&mut self, deadline: Instant) -> io::Result<Option<&CStr>> {
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

pub(super) fn open_at(
    directory: &File,
    name: &CStr,
    flags: libc::c_int,
    resolve: u64,
) -> io::Result<File> {
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
        return Err(descriptor_error(io::Error::last_os_error()));
    }
    // SAFETY: openat2 returned a new descriptor.
    Ok(unsafe { File::from_raw_fd(fd as libc::c_int) })
}

pub(super) fn read_at(
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
