//! The macOS admission adapter: a child started suspended and identified by its audit token,
//! code signatures through the Security framework, and a connection's peer by its audit token.

use std::ffi::OsStr;
use std::fs::File;
use std::io;
#[cfg(debug_assertions)]
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
#[cfg(debug_assertions)]
use std::path::PathBuf;
use std::sync::Mutex;

use org_platform::Refused;

use super::{
    decision, ChildEnv, ExpectedImage, ImageRefusal, NotTheChild, Peer, Signing, SigningRule,
    SpawnRefused, Started, StaticImageError, StaticSignature,
};

mod code;
mod spawn;

#[cfg(test)]
mod tests;

pub(super) use spawn::Child;

/// An `audit_token_t`: eight 32-bit words.
const AUDIT_TOKEN_BYTES: usize = 32;

/// A process's audit token (`audit_token_t`).
type AuditToken = [u8; AUDIT_TOKEN_BYTES];

/// Starts the expected program suspended and checks the image the suspended child loaded.
pub(super) struct Admission {
    expected: ExpectedImage,
    /// Tests only: the check before the pin is answered by the test.
    #[cfg(any(test, feature = "test-support"))]
    script: Option<super::test_support::Script>,
}

impl Admission {
    pub(super) fn new(expected: ExpectedImage) -> Result<Self, Refused> {
        Ok(Self {
            expected,
            #[cfg(any(test, feature = "test-support"))]
            script: None,
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn scripted(
        expected: ExpectedImage,
        script: super::test_support::Script,
    ) -> Result<Self, Refused> {
        Ok(Self {
            expected,
            script: Some(script),
        })
    }

    /// Starts the child suspended (`spawn::spawn_suspended`), so not one instruction of its
    /// image runs before the check unless another process resumes it (the residual risk in
    /// `spawn`'s docs): a child that fails is killed still suspended, having got nothing.
    pub(super) fn start(
        &self,
        args: &[&OsStr],
        env: &ChildEnv,
        fence: &Mutex<()>,
    ) -> io::Result<Started> {
        if self.expected.macos_requirement.is_none() && !self.expected.development {
            // Nothing to check a release image against: nothing is started.
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a release image needs a code requirement",
            ));
        }
        let spawned = {
            // No descriptor of the caller's may be half-made while a child is spawned.
            let _one_at_a_time = fence
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            spawn::spawn_suspended(&self.expected.program, args, env.pairs())?
        };
        let spawn::Spawned {
            mut process,
            stdin,
            stdout,
            stderr,
        } = spawned;
        let pid = process.id();
        // Suspended: check the image, before any of it has run.
        match self.admit(pid) {
            Ok(admitted) => process.admitted = admitted,
            Err(refusal) => {
                process.kill();
                return Err(io::Error::other(SpawnRefused { pid, refusal }));
            }
        }
        Ok(Started {
            child: super::Child(process),
            stdin,
            stdout,
            stderr,
        })
    }

    /// After the spawn of child `pid`: its audit token if it may be admitted (`None`: not
    /// checked, a development image), or why not.
    fn admit(&self, pid: u32) -> Result<Option<AuditToken>, ImageRefusal> {
        decision::pinned(
            &self.expected,
            self.admit_unpinned(pid),
            org_platform::image::loaded_cdhash_for_audit_token,
        )
    }

    fn admit_unpinned(&self, pid: u32) -> Result<Option<AuditToken>, ImageRefusal> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(script) = &self.script {
            return script(super::test_support::Step::Admit, pid)
                .map(|peer| peer.map(|peer| peer.macos_audit_token))
                .map_err(ImageRefusal::Scripted);
        }
        decision::admit(
            &self.expected,
            || token_of(pid),
            code::satisfies_hardened,
            code::without_entitlements,
        )
    }

    /// Whether `child` is still the image it was admitted as, and still the pinned one.
    pub(super) fn recheck(&self, child: &Child) -> Result<(), ImageRefusal> {
        let pid = child.id();
        let now = || {
            #[cfg(any(test, feature = "test-support"))]
            if let Some(script) = &self.script {
                return script(super::test_support::Step::Still, pid).and_then(|peer| {
                    peer.map(|peer| peer.macos_audit_token)
                        .ok_or_else(|| "no token".to_owned())
                });
            }
            token_of(pid)
        };
        decision::still(
            &self.expected,
            child.admitted,
            now,
            org_platform::image::loaded_cdhash_for_audit_token,
        )
    }

    /// The admission of `child` once more: it must give the token the child was admitted with.
    pub(super) fn readmit(&self, child: &Child) -> Result<(), ImageRefusal> {
        if self.admit(child.id())? != child.admitted {
            return Err(ImageRefusal::Readmitted);
        }
        Ok(())
    }

    /// Authenticate the socket's actual reader, not merely a process that was admitted before
    /// it ran: the peer's audit token must name the child's pid and, when the child was
    /// identified, be exactly the admitted token.
    pub(super) fn verify_reader(&self, child: &Child, stream: &UnixStream) -> io::Result<()> {
        let token = audit_token(stream)?;
        let peer_pid = u32::from_ne_bytes(token[20..24].try_into().map_err(not_the_child)?);
        if child.admitted.is_some_and(|expected| expected != token) || peer_pid != child.id() {
            return Err(not_the_child(()));
        }
        Ok(())
    }
}

fn not_the_child<T>(_: T) -> io::Error {
    io::Error::other(NotTheChild)
}

fn token_of(pid: u32) -> Result<AuditToken, String> {
    spawn::audit_token_of_child(pid).map_err(|e| e.to_string())
}

/// The system's identity of this process's own unreaped child `pid`.
#[cfg(any(test, feature = "test-support"))]
pub(super) fn child_peer(pid: u32) -> io::Result<Peer> {
    spawn::audit_token_of_child(pid).map(|macos_audit_token| Peer { macos_audit_token })
}

pub(super) fn peer(stream: &UnixStream) -> io::Result<Peer> {
    audit_token(stream).map(|macos_audit_token| Peer { macos_audit_token })
}

pub(super) fn native_peers() -> Result<(), Refused> {
    Ok(())
}

pub(super) fn peer_satisfies(peer: &Peer, requirement: &str, rule: &SigningRule) -> bool {
    let token = &peer.macos_audit_token;
    // The requirement and the hardening checked on the same code object.
    code::satisfies_hardened(token, requirement, rule)
        && (rule.absent_entitlements.is_empty()
            || code::without_entitlements(token, rule.absent_entitlements))
}

#[cfg(debug_assertions)]
pub(super) fn peer_executable(stream: &UnixStream) -> io::Result<PathBuf> {
    pid(stream).and_then(executable)
}

/// This process's own signing information (through `SecCodeCopySelf`).
pub(super) fn own_signing() -> Option<Signing> {
    code::own_signing()
}

/// Static signed-file checks through the Security framework.
pub(super) struct StaticImages;

impl StaticImages {
    pub(super) fn new() -> Result<Self, Refused> {
        Ok(Self)
    }

    pub(super) fn signature(
        &self,
        file: &File,
        requirement: &str,
    ) -> Result<StaticSignature, StaticImageError> {
        use security_framework::os::macos::code_signing::{Flags, SecRequirement};
        use std::str::FromStr;
        let requirement =
            SecRequirement::from_str(requirement).map_err(|_| StaticImageError::Refused)?;
        let code = code::static_code(file)?;
        code.check_validity(Flags::NONE, &requirement)
            .map_err(|_| StaticImageError::Refused)?;
        let (signing, cdhash) = code::static_signing(&code).ok_or(StaticImageError::Refused)?;
        Ok(StaticSignature { signing, cdhash })
    }
}

/// The peer's audit token, which the code-signing check identifies the caller by. Unlike a pid,
/// it names one process for its whole life and is never reused.
pub(crate) fn audit_token(stream: &UnixStream) -> io::Result<[u8; AUDIT_TOKEN_BYTES]> {
    let mut token = [0u8; AUDIT_TOKEN_BYTES];
    let mut len = AUDIT_TOKEN_BYTES as libc::socklen_t;
    // SAFETY: a valid socket fd, and an out-buffer of `len` bytes that outlives the call.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERTOKEN,
            token.as_mut_ptr().cast(),
            &mut len,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    if len as usize != AUDIT_TOKEN_BYTES {
        return Err(io::Error::other(
            "the peer's audit token has an unexpected size",
        ));
    }
    Ok(token)
}

/// The peer's pid (for the debug-only fallback; a pid can be reused, an audit token can't).
#[cfg(debug_assertions)]
fn pid(stream: &UnixStream) -> io::Result<libc::pid_t> {
    let mut pid: libc::pid_t = 0;
    let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: a valid socket fd, and an out-value of `len` bytes that outlives the call.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut len,
        )
    };
    if rc == 0 {
        Ok(pid)
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The path of the executable process `pid` runs (for the debug-only fallback).
#[cfg(debug_assertions)]
fn executable(pid: libc::pid_t) -> io::Result<PathBuf> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: a buffer of exactly the size passed, which outlives the call.
    let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    if n <= 0 {
        return Err(io::Error::last_os_error());
    }
    buf.truncate(n as usize);
    Ok(PathBuf::from(OsStr::from_bytes(&buf)))
}
