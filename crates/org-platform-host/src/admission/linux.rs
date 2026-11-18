//! The GNU/Linux admission adapter: a child runs the bytes the host verified, not a
//! path (design section 3.1; Linux 5.9 and glibc 2.34 or later).
//!
//! 1. When the admission is built, the pinned executable is read once into a sealed,
//!    execute-only memory file while its SHA-256 is computed (`image`).
//! 2. The digest must be the expected one. An image that names none is admitted only
//!    as a development image, and its digest is recorded for the host to log.
//! 3. A child is started from that memory file with `clone` and `execveat`, with
//!    exactly its three pipes (`spawn`). The start returns once the child executes
//!    the image.
//! 4. The child is held by its pidfd: exit is observed there, signals go there, and
//!    nothing else reaps it.
//! 5. A connection's peer is the child when its `SO_PEERCRED` PID is the child's, its
//!    user is this process's, and the pidfd says the child has not exited.
//!
//! There is no suspended child and nothing to resume: the bytes are fixed before the
//! `exec`. A [`crate::admission::Held`] child therefore runs already, and releasing
//! it does nothing; it was started from the verified bytes, so nothing a caller does
//! between the two steps can meet an unchecked process.
//!
//! # What a re-check can and cannot show
//!
//! [`Admission::recheck`] and [`Admission::readmit`] show that the held pidfd's
//! process has not exited, and that the sealed image is still the object that was
//! sealed. They cannot show that the process has not replaced its own image since:
//! the kernel reports no such thing for a non-dumpable process without tracing it.
//! The design rests on the verified image never calling `exec` on itself. Nor do
//! they say anything of the loader and the shared libraries the image mapped, which
//! are trusted because root owns them.
//!
//! No code signature exists on Linux: no peer satisfies a requirement, no signing
//! information is read and no static image check is made.

use std::ffi::OsStr;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use std::{fs, io};

use org_platform::Refused;

use super::{
    AdmissionError, ChildEnv, ExpectedImage, ImageRefusal, NotTheChild, OwnImageError, Peer,
    Signing, SigningRule, Started,
};

mod image;
#[cfg(any(test, feature = "test-support"))]
mod path;
mod spawn;
mod sys;

#[cfg(test)]
mod tests;

#[cfg(debug_assertions)]
pub(super) use super::refused::peer_executable;
pub(super) use super::refused::StaticImages;
pub(super) use spawn::Child;

/// How an admission starts its children.
enum Start {
    /// From the sealed copy of the verified bytes: the adapter.
    Sealed(image::SealedImage),
    /// Tests only: by path, nothing verified.
    #[cfg(any(test, feature = "test-support"))]
    ByPath,
}

/// Starts the expected program from the bytes that were verified when it was built.
pub(super) struct Admission {
    expected: ExpectedImage,
    start: Start,
    /// Tests only: a stand-in for a check of the started child, answered by the test.
    #[cfg(any(test, feature = "test-support"))]
    script: Option<super::test_support::Script>,
}

impl Admission {
    /// Reads, checks and seals the image (steps 1 and 2). Nothing is started.
    pub(super) fn new(expected: ExpectedImage) -> Result<Self, AdmissionError> {
        let start = Start::Sealed(image::SealedImage::of(&expected)?);
        Ok(Self {
            expected,
            start,
            #[cfg(any(test, feature = "test-support"))]
            script: None,
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn scripted(
        expected: ExpectedImage,
        script: super::test_support::Script,
    ) -> Result<Self, AdmissionError> {
        let mut admission = Self::new(expected)?;
        admission.script = Some(script);
        Ok(admission)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn by_path(expected: ExpectedImage) -> Result<Self, AdmissionError> {
        Ok(Self {
            expected,
            start: Start::ByPath,
            script: None,
        })
    }

    pub(super) fn image_sha256(&self) -> Option<[u8; 32]> {
        match &self.start {
            Start::Sealed(image) => Some(image.sha256()),
            #[cfg(any(test, feature = "test-support"))]
            Start::ByPath => None,
        }
    }

    /// Starts a child from the sealed image (step 3). The pinned path is the child's
    /// first argument and is not opened again.
    pub(super) fn start(
        &self,
        args: &[&OsStr],
        env: &ChildEnv,
        fence: &Mutex<()>,
    ) -> io::Result<Started> {
        let strings = spawn::Strings::new(&self.expected.program, args, env.pairs())?;
        let spawned = match &self.start {
            Start::Sealed(image) => spawn::spawn_sealed(image.file(), &strings, fence)?,
            #[cfg(any(test, feature = "test-support"))]
            Start::ByPath => path::spawn_by_path(&self.expected.program, &strings, fence)?,
        };
        let spawn::Spawned {
            child,
            stdin,
            stdout,
            stderr,
        } = spawned;
        #[cfg(any(test, feature = "test-support"))]
        let child = self.scripted_admit(child)?;
        Ok(Started {
            child: super::Child(child),
            stdin,
            stdout,
            stderr,
        })
    }

    /// Tests only: the script's answer for a started child. A refused child is killed
    /// and reaped.
    #[cfg(any(test, feature = "test-support"))]
    fn scripted_admit(&self, mut child: Child) -> io::Result<Child> {
        let Some(script) = &self.script else {
            return Ok(child);
        };
        let pid = child.id();
        match script(super::test_support::Step::Admit, pid) {
            Ok(admitted) => {
                child.admitted = admitted;
                Ok(child)
            }
            Err(why) => {
                child.kill();
                Err(io::Error::other(super::SpawnRefused {
                    pid,
                    refusal: ImageRefusal::Scripted(why),
                }))
            }
        }
    }

    /// What the system can still say of a running child: the sealed image is the
    /// object that was sealed, and the held process has not exited.
    fn standing(&self, child: &Child) -> Result<(), ImageRefusal> {
        match &self.start {
            Start::Sealed(image) => image.intact().map_err(ImageRefusal::Identity)?,
            #[cfg(any(test, feature = "test-support"))]
            Start::ByPath => {}
        }
        match child.alive() {
            Ok(true) => Ok(()),
            Ok(false) => Err(ImageRefusal::Identity("the child has exited".into())),
            Err(error) => Err(ImageRefusal::Identity(error.to_string())),
        }
    }

    /// Whether `child` still stands as it was admitted (see the module documentation
    /// for what that does not show).
    pub(super) fn recheck(&self, child: &Child) -> Result<(), ImageRefusal> {
        self.standing(child)?;
        #[cfg(any(test, feature = "test-support"))]
        if let (Some(script), Some(admitted)) = (&self.script, child.admitted) {
            return match script(super::test_support::Step::Still, child.id()) {
                Ok(Some(now)) if now == admitted => Ok(()),
                Ok(Some(_)) => Err(ImageRefusal::Changed),
                Ok(None) => Err(ImageRefusal::Identity("no identity".into())),
                Err(why) => Err(ImageRefusal::Identity(why)),
            };
        }
        Ok(())
    }

    /// The admission of `child` once more. The bytes were verified before the child
    /// existed and cannot have changed, so this is the re-check: there is no second
    /// identity to compare with the first.
    pub(super) fn readmit(&self, child: &Child) -> Result<(), ImageRefusal> {
        self.standing(child)?;
        #[cfg(any(test, feature = "test-support"))]
        if let Some(script) = &self.script {
            let again = script(super::test_support::Step::Admit, child.id())
                .map_err(ImageRefusal::Scripted)?;
            if again != child.admitted {
                return Err(ImageRefusal::Readmitted);
            }
        }
        Ok(())
    }

    /// Whether the process that connected the other end of `stream` is `child`
    /// (step 5): the PID the kernel recorded for the connection is the child's, its
    /// user is this process's, and the child has not exited. While the child is
    /// neither reaped nor exited its PID names no other process, so a matching PID is
    /// that process.
    pub(super) fn verify_reader(&self, child: &Child, stream: &UnixStream) -> io::Result<()> {
        let peer = sys::peer_credentials(stream.as_fd())?;
        if u32::try_from(peer.pid) != Ok(child.id())
            || peer.uid != sys::effective_uid()
            || !child.alive()?
        {
            return Err(io::Error::other(NotTheChild));
        }
        Ok(())
    }
}

/// The identity of this process's own unreaped child `pid`, as [`peer`] reports it
/// for a connection the child makes: its effective user and its PID.
#[cfg(any(test, feature = "test-support"))]
pub(super) fn child_peer(pid: u32) -> io::Result<Peer> {
    let status = fs::read_to_string(format!("/proc/{pid}/status"))?;
    let field = |name: &str, index: usize| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|values| values.split_whitespace().nth(index))
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or_else(|| io::Error::other("the process status has no such field"))
    };
    if field("PPid:", 0)? != std::process::id() {
        return Err(io::Error::other("not a child of this process"));
    }
    // `Uid:` lists the real, effective, saved and filesystem user IDs.
    Ok(Peer::from_linux_credentials(field("Uid:", 1)?, pid))
}

/// Who connected the other end of `stream`: the user and the PID the kernel recorded
/// then (`SO_PEERCRED`). Nothing identifies the peer's image.
pub(super) fn peer(stream: &UnixStream) -> io::Result<Peer> {
    let peer = sys::peer_credentials(stream.as_fd())?;
    let pid = u32::try_from(peer.pid).map_err(|_| io::Error::other("pid out of range"))?;
    Ok(Peer::from_linux_credentials(peer.uid, pid))
}

/// A native client is identified, by its user and its PID.
pub(super) fn native_peers() -> Result<(), Refused> {
    Ok(())
}

/// The bytes that are executed, by their SHA-256.
pub(super) fn image_identity() -> Result<super::ImageIdentity, Refused> {
    Ok(super::ImageIdentity::VerifiedBytes)
}

/// No code signature exists to satisfy a requirement.
pub(super) fn peer_satisfies(_peer: &Peer, _requirement: &str, _rule: &SigningRule) -> bool {
    false
}

/// No signing information exists.
pub(super) fn own_signing() -> Option<Signing> {
    None
}

/// Whether this process's executable is the image `expected` pins: the SHA-256 of
/// `/proc/self/exe`, opened once and read through that descriptor, must be `sha256`.
/// The link names the file this process was started from even when the path it was
/// started by names another file now. An image with no digest is a mismatch.
pub(super) fn own_image(expected: &ExpectedImage) -> Result<(), OwnImageError> {
    let Some(pin) = expected.sha256 else {
        return Err(OwnImageError::Mismatch);
    };
    let executable = fs::File::open("/proc/self/exe")
        .map_err(|error| OwnImageError::Unavailable(sys::named("open", error)))?;
    if image::digest(executable).map_err(OwnImageError::Unavailable)? != pin {
        return Err(OwnImageError::Mismatch);
    }
    Ok(())
}
