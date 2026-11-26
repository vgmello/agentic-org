//! A process started with exactly the arguments and environment a test gives, through the
//! same `posix_spawn` the macOS and GNU/Linux adapters use (`test-support`).

use std::ffi::{CStr, CString};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

fn check(rc: libc::c_int) -> io::Result<()> {
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(rc))
    }
}

struct Attributes(libc::posix_spawnattr_t);

impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: an Attributes exists only after posix_spawnattr_init succeeded.
        unsafe { libc::posix_spawnattr_destroy(&mut self.0) };
    }
}

struct Actions(libc::posix_spawn_file_actions_t);

impl Drop for Actions {
    fn drop(&mut self) {
        // SAFETY: an Actions exists only after posix_spawn_file_actions_init succeeded.
        unsafe { libc::posix_spawn_file_actions_destroy(&mut self.0) };
    }
}

pub(super) fn spawn_raw(
    directory: &Path,
    program: &Path,
    arguments: &[&CStr],
    environment: &[&CStr],
    new_session: bool,
) -> io::Result<(libc::pid_t, io::PipeReader)> {
    let invalid = |_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in a path");
    let cwd = CString::new(directory.as_os_str().as_bytes()).map_err(invalid)?;
    let program = CString::new(program.as_os_str().as_bytes()).map_err(invalid)?;
    let strings = |items: &[&CStr]| {
        items
            .iter()
            .map(|item| item.as_ptr().cast_mut())
            .chain(std::iter::once(std::ptr::null_mut()))
            .collect::<Vec<_>>()
    };
    let argv = strings(arguments);
    let envp = strings(environment);
    let (stdout, writer) = io::pipe()?;
    let mut raw = std::mem::MaybeUninit::<libc::posix_spawnattr_t>::zeroed();
    // SAFETY: valid, exclusively borrowed attribute storage; Attributes destroys it.
    check(unsafe { libc::posix_spawnattr_init(raw.as_mut_ptr()) })?;
    // SAFETY: posix_spawnattr_init succeeded just above.
    let mut attributes = Attributes(unsafe { raw.assume_init() });
    if new_session {
        // SAFETY: `attributes.0` is live and initialized.
        check(unsafe {
            libc::posix_spawnattr_setflags(&mut attributes.0, super::native::posix_spawn_setsid()?)
        })?;
    }
    let mut raw = std::mem::MaybeUninit::<libc::posix_spawn_file_actions_t>::zeroed();
    // SAFETY: valid, exclusively borrowed action storage; Actions destroys it.
    check(unsafe { libc::posix_spawn_file_actions_init(raw.as_mut_ptr()) })?;
    // SAFETY: posix_spawn_file_actions_init succeeded just above.
    let mut actions = Actions(unsafe { raw.assume_init() });
    // SAFETY: `actions.0` is live and initialized and `cwd` is NUL-terminated; the call
    // copies the path.
    check(unsafe { libc::posix_spawn_file_actions_addchdir_np(&mut actions.0, cwd.as_ptr()) })?;
    // SAFETY: `actions.0` is live and initialized; the descriptors are plain numbers.
    check(unsafe {
        libc::posix_spawn_file_actions_adddup2(
            &mut actions.0,
            writer.as_raw_fd(),
            libc::STDOUT_FILENO,
        )
    })?;
    let mut pid = 0;
    // SAFETY: the actions and attributes are initialized; `program`, every string `argv`
    // and `envp` point at (the caller's, borrowed for this call) and both null-terminated
    // arrays are live for the call, which only reads them; `pid` is a live int it writes.
    check(unsafe {
        libc::posix_spawn(
            &mut pid,
            program.as_ptr(),
            &actions.0,
            &attributes.0,
            argv.as_ptr(),
            envp.as_ptr(),
        )
    })?;
    Ok((pid, stdout))
}
