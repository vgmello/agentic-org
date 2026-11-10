//! Starting an office server **suspended**, the FFI the host's server check needs, and the
//! key socket's owner-only listener (`handoff`): the one module with this unsafe code.
//!
//! Why suspended: the host must check a server before the server can run any code of its own
//! (`server_check`). An image the host didn't vet could otherwise, in its first instructions,
//! start a helper that inherits stdin, then `exec` the genuine signed server: the check would see
//! the genuine one. So the host spawns every server with `POSIX_SPAWN_START_SUSPENDED`: the exec
//! is done (the audit token names the image), but not one instruction of it has run. The host
//! takes its audit token (`task_name_for_pid`, then `TASK_AUDIT_TOKEN`), checks the code it
//! names, and only then resumes it (`SIGCONT`); a child that fails is killed while still
//! suspended, so it never ran at all.
//!
//! **Suspending isn't enough on its own, so no secret goes on stdin.** A suspended child is
//! resumed by any `SIGCONT`, and the kernel doesn't check who sends it. A same-user process that
//! resumes the child before the host has taken its token (stopping or starving the host makes
//! that easy) can let its image fork a helper that keeps stdin and turn itself into the genuine
//! server (`posix_spawn` with `POSIX_SPAWN_SETEXEC | POSIX_SPAWN_START_SUSPENDED`): the host's
//! check then sees the genuine server, and no check of the child can tell this from a clean
//! spawn. So the helper must find nothing to read: the first line on stdin carries no secret
//! (launch version 3), and the app token and the key are handed over only on a connection the
//! admitted server opens itself, after its exec, to a one-shot socket whose peer must be the
//! admitted image (`handoff`, `server_check::PeerRule`). That socket is bound only after the
//! admission and the re-check, so the pre-exec image can't have connected to it and handed the
//! connection to its helper (a connection's peer token follows the pid that last used it, not
//! the image that made it: see `handoff`). Suspending still means a refused image never runs at
//! all.
//!
//! A host that dies between the spawn and the check leaves the child stopped (it holds no key and
//! never ran; launchd adopts it). That's a leaked process, not a leak of anything secret.
//!
//! It also gives the child nothing but its three pipes (`POSIX_SPAWN_CLOEXEC_DEFAULT`), an empty
//! signal mask and default `SIGPIPE` (as `std::process::Command` does), the environment it's
//! given, and `/` as its working directory. [`Process`] is the child's handle: it reaps it, and
//! kills it if dropped first.

use std::ffi::{CString, OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::unix::net::UnixListener;
use std::path::Path;

use crate::peer::AuditToken;

/// A child started by [`spawn_suspended`], with the host's ends of its stdin, stdout and stderr.
pub(crate) struct Spawned {
    pub(crate) process: Process,
    pub(crate) stdin: File,
    pub(crate) stdout: File,
    pub(crate) stderr: File,
}

/// A child process the host started: killed and reaped when dropped, if it hasn't been reaped.
pub(crate) struct Process {
    pid: libc::pid_t,
    reaped: bool,
    /// Its exit code, once reaped by this handle after a normal exit (not a signal).
    exit_code: Option<i32>,
}

impl Process {
    pub(crate) fn id(&self) -> u32 {
        self.pid as u32
    }

    /// Its exit code, if this handle reaped it after it exited normally.
    pub(crate) fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// Lets a suspended child run.
    pub(crate) fn resume(&self) -> io::Result<()> {
        // SAFETY: signals our own child, which isn't reaped, so the pid is still its.
        if unsafe { libc::kill(self.pid, libc::SIGCONT) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Whether it has exited (it is reaped if so). Doesn't wait.
    pub(crate) fn try_wait(&mut self) -> io::Result<bool> {
        if !self.reaped {
            self.reaped = match self.wait_pid(libc::WNOHANG) {
                Ok(reaped) => reaped,
                // Reaped elsewhere: gone, and never to be signalled again.
                Err(e) if e.raw_os_error() == Some(libc::ECHILD) => true,
                Err(e) => return Err(e),
            };
        }
        Ok(self.reaped)
    }

    /// SIGKILLs it (suspended or not) and reaps it.
    pub(crate) fn kill(&mut self) {
        if self.reaped {
            return;
        }
        // SAFETY: as in `resume`.
        unsafe { libc::kill(self.pid, libc::SIGKILL) };
        while !self.reaped {
            match self.wait_pid(0) {
                Ok(reaped) => self.reaped = reaped,
                // Not ours to reap any more (reaped elsewhere): never signal that pid again.
                Err(e) if e.raw_os_error() == Some(libc::ECHILD) => self.reaped = true,
                Err(_) => break,
            }
        }
    }

    /// `waitpid` with `options`, retried on `EINTR`: whether it reaped the child (noting its exit
    /// code).
    fn wait_pid(&mut self, options: libc::c_int) -> io::Result<bool> {
        let status = self.wait_status(options)?;
        if let Some(status) = status {
            if libc::WIFEXITED(status) {
                self.exit_code = Some(libc::WEXITSTATUS(status));
            }
        }
        Ok(status.is_some())
    }

    /// `waitpid` with `options`, retried on `EINTR`: the status if it reaped the child.
    fn wait_status(&self, options: libc::c_int) -> io::Result<Option<libc::c_int>> {
        loop {
            let mut status = 0;
            // SAFETY: our own unreaped child, and a valid out-pointer.
            let rc = unsafe { libc::waitpid(self.pid, &mut status, options) };
            if rc == self.pid {
                return Ok(Some(status));
            }
            if rc == 0 {
                return Ok(None);
            }
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Starts `program` with `args`, exactly the environment `env` and `/` as its working directory,
/// **suspended** (see the module docs), with its stdin, stdout and stderr on new pipes and no other
/// descriptor of ours. Resume it with [`Process::resume`].
#[cfg(target_os = "macos")]
pub(crate) fn spawn_suspended(
    program: &Path,
    args: &[&OsStr],
    env: &[(OsString, OsString)],
) -> io::Result<Spawned> {
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;

    let c = |bytes: &[u8]| {
        CString::new(bytes).map_err(|_| io::Error::other("an argument holds a NUL byte"))
    };
    let program_c = c(program.as_os_str().as_bytes())?;
    let mut argv_c = vec![program_c.clone()];
    for arg in args {
        argv_c.push(c(arg.as_bytes())?);
    }
    let mut env_c = Vec::new();
    for (name, value) in env {
        let mut pair = name.as_bytes().to_vec();
        pair.push(b'=');
        pair.extend_from_slice(value.as_bytes());
        env_c.push(c(&pair)?);
    }
    let mut argv: Vec<*mut libc::c_char> = argv_c.iter().map(|s| s.as_ptr().cast_mut()).collect();
    argv.push(std::ptr::null_mut());
    let mut envp: Vec<*mut libc::c_char> = env_c.iter().map(|s| s.as_ptr().cast_mut()).collect();
    envp.push(std::ptr::null_mut());

    let (stdin_child, stdin_ours) = pipe()?;
    let (stdout_ours, stdout_child) = pipe()?;
    let (stderr_ours, stderr_child) = pipe()?;
    let root = c(b"/")?;

    let mut actions: libc::posix_spawn_file_actions_t = std::ptr::null_mut();
    let mut attr: libc::posix_spawnattr_t = std::ptr::null_mut();
    // SAFETY: each init gets a valid out-pointer and is paired with its destroy below.
    check(unsafe { libc::posix_spawn_file_actions_init(&mut actions) })?;
    let result = (|| -> io::Result<libc::pid_t> {
        // SAFETY: as above.
        check(unsafe { libc::posix_spawnattr_init(&mut attr) })?;
        // SAFETY: `actions` and `attr` are initialized; the fds are open for the call; `root`
        // outlives it; the signal sets are initialized before use.
        unsafe {
            check(libc::posix_spawn_file_actions_adddup2(
                &mut actions,
                raw(&stdin_child),
                0,
            ))?;
            check(libc::posix_spawn_file_actions_adddup2(
                &mut actions,
                raw(&stdout_child),
                1,
            ))?;
            check(libc::posix_spawn_file_actions_adddup2(
                &mut actions,
                raw(&stderr_child),
                2,
            ))?;
            check(libc::posix_spawn_file_actions_addchdir_np(
                &mut actions,
                root.as_ptr(),
            ))?;
            let flags = libc::POSIX_SPAWN_START_SUSPENDED
                | libc::POSIX_SPAWN_CLOEXEC_DEFAULT
                | libc::POSIX_SPAWN_SETSIGMASK
                | libc::POSIX_SPAWN_SETSIGDEF;
            check(libc::posix_spawnattr_setflags(
                &mut attr,
                flags as libc::c_short,
            ))?;
            let mut empty: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut empty);
            check(libc::posix_spawnattr_setsigmask(&mut attr, &empty))?;
            let mut defaults: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut defaults);
            libc::sigaddset(&mut defaults, libc::SIGPIPE);
            check(libc::posix_spawnattr_setsigdefault(&mut attr, &defaults))?;
            let mut pid: libc::pid_t = 0;
            check(libc::posix_spawn(
                &mut pid,
                program_c.as_ptr(),
                &actions,
                &attr,
                argv.as_ptr(),
                envp.as_ptr(),
            ))?;
            Ok(pid)
        }
    })();
    // SAFETY: both were initialized above (`attr` only if its init ran: destroying a null
    // attribute is skipped).
    unsafe {
        libc::posix_spawn_file_actions_destroy(&mut actions);
        if !attr.is_null() {
            libc::posix_spawnattr_destroy(&mut attr);
        }
    }
    let pid = result?;
    // The child's ends are its own now; ours close as they drop.
    drop((stdin_child, stdout_child, stderr_child));
    let file = |fd: OwnedFd| {
        // SAFETY: a valid descriptor we own, handed over whole.
        unsafe { File::from_raw_fd(std::os::fd::IntoRawFd::into_raw_fd(fd)) }
    };
    Ok(Spawned {
        process: Process {
            pid,
            reaped: false,
            exit_code: None,
        },
        stdin: file(stdin_ours),
        stdout: file(stdout_ours),
        stderr: file(stderr_ours),
    })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn spawn_suspended(
    _program: &Path,
    _args: &[&OsStr],
    _env: &[(OsString, OsString)],
) -> io::Result<Spawned> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "office servers run only on macOS",
    ))
}

/// A pipe, both ends close-on-exec: (read end, write end).
#[cfg(target_os = "macos")]
fn pipe() -> io::Result<(std::os::fd::OwnedFd, std::os::fd::OwnedFd)> {
    use std::os::fd::{FromRawFd, OwnedFd};
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: an out-array of two descriptors.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: two fresh descriptors we now own.
    let ends = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    for fd in [&ends.0, &ends.1] {
        // SAFETY: a valid descriptor; sets only its close-on-exec flag.
        if unsafe { libc::fcntl(raw(fd), libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(ends)
}

#[cfg(target_os = "macos")]
fn raw(fd: &std::os::fd::OwnedFd) -> libc::c_int {
    std::os::fd::AsRawFd::as_raw_fd(fd)
}

/// A `posix_spawn*` result: 0, or an errno.
#[cfg(target_os = "macos")]
fn check(rc: libc::c_int) -> io::Result<()> {
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(rc))
    }
}

/// A listening Unix socket at `path`, owner-only (0600) before anyone can connect: it is bound,
/// then its file is set to 0600, and only then does it listen (a socket that isn't listening
/// refuses every connection). Its fd is close-on-exec; it is made in two steps, so the caller
/// holds `cloexec::SPAWN_LOCK`. `path` must not exist and must fit `sockaddr_un` (103 bytes,
/// `org_proto::location::check_socket_path`). On an error after the bind, the file is removed if
/// it is still the node that was bound (`lstat`ed right after the bind: a socket of ours, the same
/// `(dev, ino)`); anything else is left for the startup sweep.
pub(crate) fn listen_owner_only(path: &Path, backlog: u8) -> io::Result<UnixListener> {
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;

    // SAFETY: an all-zero `sockaddr_un` is a valid value (an empty path).
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    let bytes = path.as_os_str().as_bytes();
    // Room for the terminating NUL, which the zeroed array already holds.
    if bytes.is_empty() || bytes.len() >= addr.sun_path.len() || bytes.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a socket path must be 1 to 103 bytes, without NUL",
        ));
    }
    for (slot, &b) in addr.sun_path.iter_mut().zip(bytes) {
        *slot = b as libc::c_char;
    }
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    let len = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
    #[cfg(target_os = "macos")]
    {
        addr.sun_len = len as u8;
    }
    // SAFETY: plain socket creation; the result is checked before use.
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a fresh descriptor we now own (closed when `owned` drops).
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    // SAFETY: a valid descriptor; sets only its close-on-exec flag.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `addr` is a valid `sockaddr_un` of `len` bytes that outlives the call.
    let rc = unsafe { libc::bind(fd, (&addr as *const libc::sockaddr_un).cast(), len) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    // The node just bound, to remove only that one if a later step fails. (`fstat` of the socket
    // can't say: it describes the socket, not the path's node.) A same-user swap before this
    // `lstat` isn't detected, as in `handoff`.
    let bound = own_socket_node(path);
    let listening = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .and_then(|()| {
            // SAFETY: a valid, bound socket descriptor.
            if unsafe { libc::listen(fd, libc::c_int::from(backlog)) } == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        });
    if let Err(e) = listening {
        // Only the node it bound, still a socket of ours: anything else is left for the startup
        // sweep (`handoff::remove_stale_key_sockets`) or its owner.
        if bound.is_some() && own_socket_node(path) == bound {
            let _ = std::fs::remove_file(path);
        }
        return Err(e);
    }
    Ok(UnixListener::from(owned))
}

/// `path`'s `(dev, ino)` if it is a socket owned by us (`lstat`, so never through a symlink).
fn own_socket_node(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let meta = std::fs::symlink_metadata(path).ok()?;
    (meta.file_type().is_socket() && meta.uid() == crate::home::euid())
        .then(|| (meta.dev(), meta.ino()))
}

/// `TASK_AUDIT_TOKEN` (`<mach/task_info.h>`).
#[cfg(target_os = "macos")]
const TASK_AUDIT_TOKEN: libc::task_flavor_t = 15;
/// `audit_token_t` is 8 `natural_t`s.
#[cfg(target_os = "macos")]
const AUDIT_TOKEN_WORDS: usize = 8;
#[cfg(target_os = "macos")]
const KERN_SUCCESS: libc::kern_return_t = 0;

#[cfg(target_os = "macos")]
extern "C" {
    /// This task's own port (what `mach_task_self()` reads).
    static mach_task_self_: libc::mach_port_t;
    fn task_name_for_pid(
        target_tport: libc::mach_port_t,
        pid: libc::c_int,
        task_name: *mut libc::mach_port_t,
    ) -> libc::kern_return_t;
    fn task_info(
        task: libc::mach_port_t,
        flavor: libc::task_flavor_t,
        info: *mut libc::integer_t,
        count: *mut libc::mach_msg_type_number_t,
    ) -> libc::kern_return_t;
    fn mach_port_deallocate(
        task: libc::mach_port_t,
        name: libc::mach_port_t,
    ) -> libc::kern_return_t;
}

/// The audit token of our child `pid` (unreaped, so the pid can't have been reused), as the raw
/// `audit_token_t` bytes the Security framework takes (the layout `LOCAL_PEERTOKEN` gives,
/// `peer::audit_token`). It names one process image: an `exec` changes it (its pid version).
#[cfg(target_os = "macos")]
pub(crate) fn audit_token_of_child(pid: u32) -> io::Result<AuditToken> {
    let pid = libc::c_int::try_from(pid).map_err(|_| io::Error::other("pid out of range"))?;
    // SAFETY: `mach_task_self_` is set up by the system before `main` and never changes.
    let me = unsafe { mach_task_self_ };
    let mut name: libc::mach_port_t = 0;
    // SAFETY: `name` is a valid out-pointer for the call.
    let kr = unsafe { task_name_for_pid(me, pid, &mut name) };
    if kr != KERN_SUCCESS {
        return Err(io::Error::other(format!("task_name_for_pid failed ({kr})")));
    }
    let mut words = [0 as libc::natural_t; AUDIT_TOKEN_WORDS];
    let mut count = AUDIT_TOKEN_WORDS as libc::mach_msg_type_number_t;
    // SAFETY: `name` is a task name port we hold; `words` has room for `count` words, which the
    // call may lower, never raise.
    let kr = unsafe {
        task_info(
            name,
            TASK_AUDIT_TOKEN,
            words.as_mut_ptr().cast::<libc::integer_t>(),
            &mut count,
        )
    };
    // SAFETY: releases the send right `task_name_for_pid` gave us, once.
    unsafe { mach_port_deallocate(me, name) };
    if kr != KERN_SUCCESS || count as usize != AUDIT_TOKEN_WORDS {
        return Err(io::Error::other(format!("task_info failed ({kr})")));
    }
    let mut token = [0u8; std::mem::size_of::<AuditToken>()];
    for (bytes, word) in token.as_chunks_mut::<4>().0.iter_mut().zip(words) {
        bytes.copy_from_slice(&word.to_ne_bytes());
    }
    Ok(token)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::io::Read;

    fn sh(script: &str) -> Spawned {
        let args = [OsStr::new("-c"), OsStr::new(script)];
        spawn_suspended(Path::new("/bin/sh"), &args, &[]).unwrap()
    }

    /// The `audit_token_t` word at `index` (1: euid, 5: pid).
    fn word(token: &AuditToken, index: usize) -> u32 {
        u32::from_ne_bytes(token[4 * index..4 * index + 4].try_into().unwrap())
    }

    /// A suspended child runs nothing until it's resumed; one killed while suspended never ran.
    #[test]
    fn a_suspended_child_runs_nothing_until_it_is_resumed() {
        let dir = tempfile::tempdir().unwrap();
        let (resumed, killed) = (dir.path().join("resumed"), dir.path().join("killed"));
        let mut a = sh(&format!("touch '{}'", resumed.display()));
        let mut b = sh(&format!("touch '{}'", killed.display()));
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(!resumed.exists() && !killed.exists(), "ran while suspended");
        // Its token can be taken while it's suspended: it names the child.
        let token = audit_token_of_child(a.process.id()).unwrap();
        assert_eq!(word(&token, 5), a.process.id());
        assert_eq!(word(&token, 1), crate::home::euid());
        a.process.resume().unwrap();
        b.process.kill();
        while !a.process.try_wait().unwrap() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(resumed.exists());
        assert!(!killed.exists(), "a child killed while suspended ran");
    }

    /// The child gets its pipes as 0, 1 and 2, exactly the environment given, `/` as its
    /// working directory, and no other descriptor.
    #[test]
    fn a_child_gets_its_pipes_its_environment_and_nothing_else() {
        let env = [(OsString::from("ONLY"), OsString::from("this"))];
        let script = "read line; echo \"$line|$ONLY|$(pwd)|$(env | grep -cv '^ONLY=\\|^PWD=\\|^SHLVL=\\|^_=')\"; \
                      echo err 1>&2";
        let args = [OsStr::new("-c"), OsStr::new(script)];
        let mut child = spawn_suspended(Path::new("/bin/sh"), &args, &env).unwrap();
        child.process.resume().unwrap();
        std::io::Write::write_all(&mut child.stdin, b"hi\n").unwrap();
        drop(child.stdin);
        let (mut out, mut err) = (String::new(), String::new());
        child.stdout.read_to_string(&mut out).unwrap();
        child.stderr.read_to_string(&mut err).unwrap();
        assert_eq!(out, "hi|this|/|0\n");
        assert_eq!(err, "err\n");
        child.process.kill();

        // `ls -l /dev/fd`: 0, 1 and 2 are the child's pipes, and anything above is a directory
        // `ls` opened itself; nothing of the host's (which holds many descriptors: these tests'
        // pipes among them) reached it.
        // One descriptor of ours that isn't close-on-exec: only `CLOEXEC_DEFAULT` keeps it out.
        let open = File::open("/dev/null").unwrap();
        let leaky = std::os::fd::AsRawFd::as_raw_fd(&open);
        // SAFETY: a valid descriptor of ours; clears only its close-on-exec flag.
        assert_eq!(unsafe { libc::fcntl(leaky, libc::F_SETFD, 0) }, 0);
        let args = [OsStr::new("-l"), OsStr::new("/dev/fd")];
        let mut ls = spawn_suspended(Path::new("/bin/ls"), &args, &[]).unwrap();
        ls.process.resume().unwrap();
        drop(ls.stdin);
        let mut listed = String::new();
        ls.stdout.read_to_string(&mut listed).unwrap();
        let entries: Vec<(&str, &str)> = listed
            .lines()
            .filter_map(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                (fields.len() > 2).then(|| (fields[0], *fields.last().unwrap()))
            })
            .collect();
        let fds: Vec<&str> = entries.iter().map(|(_, fd)| *fd).collect();
        assert_eq!(&fds[..3], ["0", "1", "2"], "{listed}");
        for (mode, fd) in &entries {
            let pipe_or_dir = if fd.parse::<u32>().unwrap() <= 2 {
                'p'
            } else {
                'd'
            };
            assert!(mode.starts_with(pipe_or_dir), "fd {fd}: {listed}");
        }
        // Not as `/dev/null` (a character device) under any number.
        assert!(
            !entries.iter().any(|(mode, _)| mode.starts_with('c')),
            "{listed}"
        );
        drop(open);
        ls.process.kill();
    }

    /// The listener is 0600 from the moment it can be connected to, close-on-exec, and accepts.
    #[test]
    fn an_owner_only_listener_is_0600_and_close_on_exec() {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::{FileTypeExt, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("k.sock");
        let listener = listen_owner_only(&path, 4).unwrap();
        let meta = std::fs::symlink_metadata(&path).unwrap();
        assert!(meta.file_type().is_socket());
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        // SAFETY: F_GETFD only reads the flags of an fd this test owns.
        let flags = unsafe { libc::fcntl(listener.as_raw_fd(), libc::F_GETFD) };
        assert!(flags >= 0 && flags & libc::FD_CLOEXEC != 0, "{flags}");
        let _client = std::os::unix::net::UnixStream::connect(&path).unwrap();
        listener.accept().unwrap();
        // An existing path, or one too long, is refused.
        assert!(listen_owner_only(&path, 4).is_err());
        let long = dir.path().join("x".repeat(110));
        assert!(listen_owner_only(&long, 4).is_err());
    }

    #[test]
    fn another_child_has_another_token_and_a_reaped_one_none() {
        let mut a = sh("exit 0");
        let mut b = sh("exit 0");
        let (ta, tb) = (
            audit_token_of_child(a.process.id()).unwrap(),
            audit_token_of_child(b.process.id()).unwrap(),
        );
        assert_ne!(ta, tb);
        let pid = a.process.id();
        a.process.kill();
        b.process.kill();
        assert!(audit_token_of_child(pid).is_err());
    }
}
