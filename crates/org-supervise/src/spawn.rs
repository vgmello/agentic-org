use std::ffi::{CString, OsString};
use std::fs::File;
use std::io;
use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
use std::path::PathBuf;

use org_platform::process;

use crate::{invalid, ChildIdentity, ChildStdin, Environment, OwnedChild, Spawned};

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
        process::validate()?;
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
        process::validate()?;
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
        let (pgid, started, boot) = process::identity(pid)?;
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
        child.watch = Some(process::Watch::new(pid)?);
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
    process::spawn_piped(&program, &args, &env, &cwd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;

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
            // Queries a live owned parent pipe end without altering it.
            let flags = org_platform::posix::descriptor_flags(fd).unwrap_or(-1);
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
            .env_clear()
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

/// Where a signal to a group of only exited members is refused (macOS), the exception
/// the owner's cleanup verifies before it reaps. Elsewhere there is no such refusal.
#[cfg(test)]
mod zombie_group_tests {
    use super::*;
    use std::io::BufRead;
    use std::time::{Duration, Instant};

    fn exited_group(script: &str) -> (OwnedChild, File, File) {
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
        let (pid, stdin, stdout, _stderr) = spawn_piped(&spec).unwrap();
        let mut child = OwnedChild::new(pid);
        child.group = true;
        child.watch = Some(process::Watch::new(pid).unwrap());
        child.release_after_record().unwrap();
        assert_eq!(
            child
                .wait_until(Instant::now() + Duration::from_secs(5))
                .unwrap()
                .unwrap()
                .code(),
            Some(17)
        );
        (child, stdout, stdin)
    }

    #[test]
    fn zombie_only_group_eperm_is_verified_before_explicit_cleanup_reaps() {
        if !process::EXITED_GROUP_SIGNAL_IS_DENIED {
            return;
        }
        let (mut child, _stdout, _hold_input) = exited_group("exit 17");
        assert!(process::group_contains_only_leader(child.pid).unwrap());
        // The held leader is still our unreaped child, reserving this group ID.
        let denied = org_platform::posix::kill_group(child.pid, libc::SIGKILL).unwrap_err();
        assert_eq!(denied.raw_os_error(), Some(libc::EPERM));
        assert_eq!(child.cleanup_and_reap(&[]).unwrap().status.code(), Some(17));
    }

    #[test]
    fn an_exited_leader_with_a_live_group_member_is_not_the_zombie_only_exception() {
        if !process::EXITED_GROUP_SIGNAL_IS_DENIED {
            return;
        }
        let (mut child, stdout, _hold_input) =
            exited_group("exec 3<&0; cat <&3 & echo $!; exit 17");
        let mut line = String::new();
        std::io::BufReader::new(stdout)
            .read_line(&mut line)
            .unwrap();
        let member: i32 = line.trim().parse().unwrap();
        // Observation only of the gated fixture member in the held child group.
        assert_eq!(
            org_platform::posix::process_group(member).unwrap(),
            child.pid
        );
        assert!(!process::group_contains_only_leader(child.pid).unwrap());
        assert_eq!(child.cleanup_and_reap(&[]).unwrap().status.code(), Some(17));
    }
}
