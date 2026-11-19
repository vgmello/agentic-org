//! Tests only (`test-support`): starting a program by its path, for a program the
//! adapter itself cannot admit (a `#!` script). Nothing of the image is verified
//! here; no production composition can reach this module.
//!
//! The child gets what a child of the adapter gets: its three pipes on descriptors 0
//! to 2 and no other descriptor, exactly the given environment, `/` as its working
//! directory, an empty signal mask and `SIGPIPE` at its default. It is held by a
//! pidfd opened for it while it is this process's unreaped child.

use std::ffi::CString;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::Mutex;

use super::spawn::{Child, Pipes, Spawned, Strings};
use super::sys::{self, failed};

/// A `posix_spawn*` result: 0, or an error number.
fn check(call: &'static str, rc: libc::c_int) -> io::Result<()> {
    if rc == 0 {
        Ok(())
    } else {
        Err(failed(call, rc))
    }
}

/// Starts `program` by its path with `posix_spawn`.
pub(super) fn spawn_by_path(
    program: &Path,
    strings: &Strings,
    fence: &Mutex<()>,
) -> io::Result<Spawned> {
    let path = CString::new(program.as_os_str().as_bytes())
        .map_err(|_| io::Error::other("the path holds a NUL byte"))?;
    let _one_at_a_time = fence
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let pipes = Pipes::new()?;
    let mut actions = std::mem::MaybeUninit::<libc::posix_spawn_file_actions_t>::uninit();
    let mut attributes = std::mem::MaybeUninit::<libc::posix_spawnattr_t>::uninit();
    // SAFETY: a valid slot to initialize; destroyed below on every path.
    check("posix_spawn_file_actions_init", unsafe {
        libc::posix_spawn_file_actions_init(actions.as_mut_ptr())
    })?;
    let spawned = (|| {
        // SAFETY: a valid slot to initialize; destroyed below once initialized.
        check("posix_spawnattr_init", unsafe {
            libc::posix_spawnattr_init(attributes.as_mut_ptr())
        })?;
        // SAFETY: `actions` and `attributes` are initialized; the descriptors, the
        // strings and the signal sets outlive the spawn call that reads them.
        let spawned: io::Result<libc::pid_t> = (|| unsafe {
            let (actions, attributes) = (actions.as_mut_ptr(), attributes.as_mut_ptr());
            for (source, target) in pipes.child.iter().zip(0..) {
                check(
                    "posix_spawn_file_actions_adddup2",
                    libc::posix_spawn_file_actions_adddup2(actions, source.as_raw_fd(), target),
                )?;
            }
            check(
                "posix_spawn_file_actions_addchdir_np",
                libc::posix_spawn_file_actions_addchdir_np(actions, c"/".as_ptr()),
            )?;
            check(
                "posix_spawn_file_actions_addclosefrom_np",
                libc::posix_spawn_file_actions_addclosefrom_np(actions, 3),
            )?;
            let mut no_signals: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut no_signals);
            let mut defaults: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut defaults);
            libc::sigaddset(&mut defaults, libc::SIGPIPE);
            check(
                "posix_spawnattr_setsigmask",
                libc::posix_spawnattr_setsigmask(attributes, &no_signals),
            )?;
            check(
                "posix_spawnattr_setsigdefault",
                libc::posix_spawnattr_setsigdefault(attributes, &defaults),
            )?;
            let flags = libc::POSIX_SPAWN_SETSIGMASK | libc::POSIX_SPAWN_SETSIGDEF;
            check(
                "posix_spawnattr_setflags",
                libc::posix_spawnattr_setflags(attributes, flags as libc::c_short),
            )?;
            let mut pid: libc::pid_t = 0;
            check(
                "posix_spawn",
                libc::posix_spawn(
                    &mut pid,
                    path.as_ptr(),
                    actions,
                    attributes,
                    strings.argv().cast(),
                    strings.envp().cast(),
                ),
            )?;
            Ok(pid)
        })();
        // SAFETY: initialized above.
        unsafe { libc::posix_spawnattr_destroy(attributes.as_mut_ptr()) };
        spawned
    })();
    // SAFETY: initialized above, whatever became of the attributes and the spawn.
    unsafe { libc::posix_spawn_file_actions_destroy(actions.as_mut_ptr()) };
    let pid: libc::pid_t = spawned?;
    // This process's own unreaped child: its PID still names it.
    let pidfd = sys::pidfd_open(pid).inspect_err(|_| {
        // SAFETY: signals and reaps exactly the child `posix_spawn` just returned.
        unsafe {
            libc::kill(pid, libc::SIGKILL);
            libc::waitpid(pid, std::ptr::null_mut(), 0);
        }
    })?;
    Ok(Spawned {
        child: Child::started(pid, pidfd),
        stdin: pipes.stdin,
        stdout: pipes.stdout,
        stderr: pipes.stderr,
    })
}
