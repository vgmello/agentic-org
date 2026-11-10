use std::ffi::{CString, OsString};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
use std::path::PathBuf;

use crate::{invalid, platform, ChildIdentity, ChildStdin, Environment, OwnedChild, Spawned};

/// Explicit input for a new piped session. Additional group policies/PTY support are deferred.
#[derive(Debug)]
pub struct SpawnSpec {
    /// Existing absolute executable path, resolved and checked before spawn.
    pub program: PathBuf,
    /// Literal argv entries, never shell text. Do not put secrets here.
    pub args: Vec<OsString>,
    /// Existing absolute working directory.
    pub cwd: PathBuf,
    /// Supplied captured/context data; the server environment is never consulted.
    pub environment: Environment,
}

/// Non-secret mark supplied by the session owner, random per session and persisted before
/// spawn. Reusing a mark concurrently is forbidden; do not use a fixed mark in tests.
#[derive(Debug, Clone)]
pub struct SessionMark(String);

impl SessionMark {
    /// Validate a nonempty opaque mark (up to 128 ASCII alphanumeric/hyphen characters).
    pub fn new(value: String) -> io::Result<Self> {
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(invalid("invalid session mark"));
        }
        Ok(Self(value))
    }
}

/// A launch that cannot spawn until its mark is durably recorded by the caller.
pub struct PreparedSpawn {
    spec: SpawnSpec,
    mark: SessionMark,
    cleanup_dir: Option<crate::files::PrivateDirectory>,
}

impl PreparedSpawn {
    /// Validate inputs. This performs no spawn and reads no ambient environment.
    pub fn new(mut spec: SpawnSpec, mark: SessionMark) -> io::Result<Self> {
        platform::validate()?;
        if !spec.program.is_absolute() || !spec.cwd.is_absolute() {
            return Err(invalid("executable and cwd must be absolute"));
        }
        spec.program = spec.program.canonicalize()?;
        spec.cwd = spec.cwd.canonicalize()?;
        let metadata = spec.program.metadata()?;
        if !metadata.is_file() || metadata.mode() & 0o6000 != 0 || metadata.mode() & 0o111 == 0 {
            return Err(invalid(
                "executable must be a regular executable without setuid/setgid bits",
            ));
        }
        if !spec.cwd.is_dir() {
            return Err(invalid("cwd must be a directory"));
        }
        spec.environment
            .insert("ORG_SESSION_MARK".into(), mark.0.clone().into())?;
        Ok(Self {
            spec,
            mark,
            cleanup_dir: None,
        })
    }

    pub(crate) fn with_cleanup_directory(
        mut self,
        directory: crate::files::PrivateDirectory,
    ) -> Self {
        self.cleanup_dir = Some(directory);
        self
    }

    /// The callback must durably persist this mark before returning success. Failure cannot
    /// spawn a child. This separate step makes the crash window explicit in caller code.
    pub fn record_mark(
        self,
        persist: impl FnOnce(&str) -> io::Result<()>,
    ) -> io::Result<RecordedSpawn> {
        persist(&self.mark.0)?;
        Ok(RecordedSpawn(self))
    }
}

/// A launch whose mark persistence succeeded; it may now spawn.
pub struct RecordedSpawn(PreparedSpawn);

impl RecordedSpawn {
    /// Start with pipes, capture process identity, and keep stdin unavailable until recorded.
    pub fn spawn(self) -> io::Result<PendingChild> {
        platform::validate()?;
        let PreparedSpawn {
            spec,
            mark,
            cleanup_dir,
        } = self.0;
        let metadata = spec.program.metadata()?;
        if metadata.mode() & 0o6000 != 0 {
            return Err(invalid("setuid/setgid executable refused"));
        }
        let (pid, stdin, stdout, stderr) = spawn_piped(&spec)?;
        let mut child = OwnedChild::new(pid);
        child.cleanup_dir = cleanup_dir.map(|mut directory| {
            directory.attach();
            directory
        });
        // Successful posix_spawn applied SETSID before exec; guard the group even if a
        // subsequent identity/watch query fails.
        child.group = true;
        let (pgid, started, boot) = platform::identity(pid)?;
        if pgid != pid as u32 {
            return Err(io::Error::other("spawn did not create a private session"));
        }
        child.identity = Some(ChildIdentity {
            pid: pid as u32,
            pgid,
            started,
            boot,
            mark: mark.0,
        });
        child.watch = Some(platform::Watch::new(pid)?);
        Ok(PendingChild {
            child,
            stdin,
            stdout,
            stderr,
        })
    }
}

/// Child identity available for persistence, stdin still private. Drop kills/reaps the child.
pub struct PendingChild {
    child: OwnedChild,
    stdin: File,
    stdout: File,
    stderr: File,
}

impl PendingChild {
    /// Inspect what must be recorded before the first protocol byte.
    pub fn identity(&self) -> &ChildIdentity {
        self.child.identity()
    }

    /// Persist the identity and release all pipes to the pump once, on success only.
    /// On macOS this resumes the suspended child after persistence. This orders startup;
    /// it is not admission control, since another same-user process can send SIGCONT.
    pub fn record_identity(
        mut self,
        persist: impl FnOnce(&ChildIdentity) -> io::Result<()>,
    ) -> io::Result<Spawned> {
        persist(self.child.identity())?;
        self.child.release_after_record()?;
        Ok(Spawned {
            child: self.child,
            stdin: ChildStdin::new(self.stdin),
            stdout: self.stdout,
            stderr: self.stderr,
        })
    }
}

fn c(bytes: &[u8]) -> io::Result<CString> {
    CString::new(bytes).map_err(|_| invalid("NUL in process argument"))
}

fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut pair = [-1; 2];
    #[cfg(target_os = "linux")]
    // SAFETY: valid two-element out buffer; CLOEXEC is set atomically with creation.
    let rc = unsafe { libc::pipe2(pair.as_mut_ptr(), libc::O_CLOEXEC) };
    #[cfg(target_os = "macos")]
    // SAFETY: valid out buffer. Darwin has no pipe2: other embedder spawn paths must
    // also close descriptors by default, or serialize their spawn with this entire call.
    let rc = unsafe { libc::pipe(pair.as_mut_ptr()) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: pipe returned two distinct owned descriptors.
    let original = unsafe { [OwnedFd::from_raw_fd(pair[0]), OwnedFd::from_raw_fd(pair[1])] };
    let duplicate = |fd: OwnedFd| -> io::Result<OwnedFd> {
        #[cfg(target_os = "linux")]
        if fd.as_raw_fd() >= 3 {
            // Already CLOEXEC from pipe2; transfer it without another duplication.
            return Ok(fd);
        }
        // SAFETY: valid owned descriptor. Keep spawn source fds above2 even if stdio is closed.
        let new = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
        if new < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful duplication returns a new owned descriptor.
        Ok(unsafe { OwnedFd::from_raw_fd(new) })
    };
    let [read, write] = original;
    Ok((duplicate(read)?, duplicate(write)?))
}

fn spawn_piped(spec: &SpawnSpec) -> io::Result<(libc::pid_t, File, File, File)> {
    let program = c(spec.program.as_os_str().as_bytes())?;
    let cwd = c(spec.cwd.as_os_str().as_bytes())?;
    let mut args = vec![program.clone()];
    for arg in &spec.args {
        args.push(c(arg.as_bytes())?);
    }
    let env = spec
        .environment
        .0
        .iter()
        .map(|(name, value)| {
            let mut bytes = name.as_bytes().to_vec();
            bytes.push(b'=');
            bytes.extend_from_slice(value.as_bytes());
            c(&bytes)
        })
        .collect::<io::Result<Vec<_>>>()?;
    let pointers = |strings: &[CString]| {
        let mut result: Vec<_> = strings.iter().map(|s| s.as_ptr().cast_mut()).collect();
        result.push(std::ptr::null_mut());
        result
    };
    let argv = pointers(&args);
    let envp = pointers(&env);
    let (input_child, input) = pipe()?;
    // SAFETY: our pipe's write end; O_NONBLOCK prevents a writer holding the stdin mutex
    // from blocking close/deadline handling when the child stops reading.
    if unsafe { libc::fcntl(input.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let (output, output_child) = pipe()?;
    let (error, error_child) = pipe()?;
    let mut actions = std::mem::MaybeUninit::<libc::posix_spawn_file_actions_t>::uninit();
    let mut attrs = std::mem::MaybeUninit::<libc::posix_spawnattr_t>::uninit();
    // SAFETY: valid init slots; every successful initialization is destroyed below.
    platform::check(unsafe { libc::posix_spawn_file_actions_init(actions.as_mut_ptr()) })?;
    let result = (|| {
        // SAFETY: actions initialized above, attrs initialized here and destroyed on every
        // closure result. Strings, fds, sigsets and pointers outlive the spawn call.
        platform::check(unsafe { libc::posix_spawnattr_init(attrs.as_mut_ptr()) })?;
        let result: io::Result<libc::pid_t> = (|| unsafe {
            let actions = actions.as_mut_ptr();
            let attrs = attrs.as_mut_ptr();
            for (source, target) in [
                (input_child.as_raw_fd(), 0),
                (output_child.as_raw_fd(), 1),
                (error_child.as_raw_fd(), 2),
            ] {
                platform::check(libc::posix_spawn_file_actions_adddup2(
                    actions, source, target,
                ))?;
            }
            platform::check(libc::posix_spawn_file_actions_addchdir_np(
                actions,
                cwd.as_ptr(),
            ))?;
            let mut mask = std::mem::zeroed();
            let mut defaults = std::mem::zeroed();
            libc::sigemptyset(&mut mask);
            libc::sigfillset(&mut defaults);
            libc::sigdelset(&mut defaults, libc::SIGKILL);
            libc::sigdelset(&mut defaults, libc::SIGSTOP);
            platform::check(libc::posix_spawnattr_setsigmask(attrs, &mask))?;
            platform::check(libc::posix_spawnattr_setsigdefault(attrs, &defaults))?;
            #[cfg(target_os = "macos")]
            // Darwin spawn.h defines SETSID=0x0400; libc doesn't export this flag.
            let flags = libc::POSIX_SPAWN_CLOEXEC_DEFAULT
                | libc::POSIX_SPAWN_START_SUSPENDED
                | 0x0400
                | libc::POSIX_SPAWN_SETSIGMASK
                | libc::POSIX_SPAWN_SETSIGDEF;
            #[cfg(target_os = "linux")]
            let flags = {
                platform::check(libc::posix_spawn_file_actions_addclosefrom_np(actions, 3))?;
                i32::from(libc::POSIX_SPAWN_SETSID)
                    | libc::POSIX_SPAWN_SETSIGMASK
                    | libc::POSIX_SPAWN_SETSIGDEF
            };
            platform::check(libc::posix_spawnattr_setflags(attrs, flags as _))?;
            let mut pid = 0;
            platform::check(libc::posix_spawn(
                &mut pid,
                program.as_ptr(),
                actions,
                attrs,
                argv.as_ptr(),
                envp.as_ptr(),
            ))?;
            Ok(pid)
        })();
        // SAFETY: attrs was successfully initialized.
        unsafe { libc::posix_spawnattr_destroy(attrs.as_mut_ptr()) };
        result
    })();
    // SAFETY: actions was successfully initialized, independently of attrs/result.
    unsafe { libc::posix_spawn_file_actions_destroy(actions.as_mut_ptr()) };
    Ok((result?, input.into(), output.into(), error.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_parent_pipe_is_cloexec_and_stays_out_of_an_unrelated_exec() {
        let temp = tempfile::tempdir().unwrap();
        let spec = SpawnSpec {
            program: "/bin/cat".into(),
            args: Vec::new(),
            cwd: temp.path().canonicalize().unwrap(),
            environment: Environment::new(
                crate::SearchPath::new(
                    std::path::Path::new("/usr/bin"),
                    "/usr/bin:/bin".as_ref(),
                    &[],
                )
                .unwrap(),
                &[],
            )
            .unwrap(),
        };
        let (pid, stdin, stdout, stderr) = spawn_piped(&spec).unwrap();
        let mut child = OwnedChild::new(pid);
        child.group = true;
        let descriptors = [stdin.as_raw_fd(), stdout.as_raw_fd(), stderr.as_raw_fd()];
        for fd in descriptors {
            // SAFETY: queries a live owned parent pipe end without altering it.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            assert!(
                flags >= 0 && flags & libc::FD_CLOEXEC != 0,
                "parent fd {fd} lacks CLOEXEC"
            );
        }
        // A normal exec relies on CLOEXEC, unlike org-supervise's close-all spawn actions.
        // This isolated shell only tests these exact fd numbers; it opens no test files.
        #[expect(
            clippy::disallowed_methods,
            reason = "test-only exec must rely on CLOEXEC rather than the supervisor's close-all actions"
        )]
        let status = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                "for fd in \"$@\"; do if test -p \"/dev/fd/$fd\"; then exit 1; fi; done",
                "check-parent-pipes",
            ])
            .args(descriptors.map(|fd| fd.to_string()))
            .status()
            .unwrap();
        assert!(
            status.success(),
            "parent pipe leaked through unrelated exec"
        );
        drop(child);
    }
}

#[cfg(all(test, target_os = "macos"))]
mod zombie_group_tests {
    use super::*;
    use std::io::BufRead;
    use std::time::{Duration, Instant};

    fn exited_group(script: &str) -> (OwnedChild, File) {
        let temp = tempfile::tempdir().unwrap();
        let spec = SpawnSpec {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: temp.path().canonicalize().unwrap(),
            environment: Environment::new(
                crate::SearchPath::new(
                    std::path::Path::new("/usr/bin"),
                    "/usr/bin:/bin".as_ref(),
                    &[],
                )
                .unwrap(),
                &[],
            )
            .unwrap(),
        };
        let (pid, _stdin, stdout, _stderr) = spawn_piped(&spec).unwrap();
        let mut child = OwnedChild::new(pid);
        child.group = true;
        child.watch = Some(platform::Watch::new(pid).unwrap());
        child.release_after_record().unwrap();
        assert_eq!(
            child
                .wait_until(Instant::now() + Duration::from_secs(5))
                .unwrap()
                .unwrap()
                .code(),
            Some(17)
        );
        (child, stdout)
    }

    #[test]
    fn zombie_only_group_eperm_is_verified_before_explicit_cleanup_reaps() {
        let (mut child, _stdout) = exited_group("exit 17");
        assert!(platform::group_contains_only_leader(child.pid).unwrap());
        assert_eq!(unsafe { libc::killpg(child.pid, libc::SIGKILL) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EPERM));
        assert_eq!(child.cleanup_and_reap(&[]).unwrap().status.code(), Some(17));
    }

    #[test]
    fn an_exited_leader_with_a_live_group_member_is_not_the_zombie_only_exception() {
        let (mut child, stdout) = exited_group("sleep 8 & echo $!; exit 17");
        let mut line = String::new();
        std::io::BufReader::new(stdout)
            .read_line(&mut line)
            .unwrap();
        let member: i32 = line.trim().parse().unwrap();
        assert_eq!(unsafe { libc::getpgid(member) }, child.pid);
        assert!(!platform::group_contains_only_leader(child.pid).unwrap());
        assert_eq!(child.cleanup_and_reap(&[]).unwrap().status.code(), Some(17));
    }
}
