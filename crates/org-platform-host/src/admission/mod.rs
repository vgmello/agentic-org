//! Process admission: starting a child that is the image the host meant to start, and
//! knowing who is at the other end of a connection.
//!
//! Linux host design section 3.1. The host (`org-host`) is the only consumer. It keeps
//! the policy: whether a child may be started at all, which signing identifier and team
//! a release image must carry, which entitlements refuse it, when a development build
//! skips the publisher check, and every text it logs. It passes that policy here as
//! plain data ([`ExpectedImage`], [`SigningRule`]). This module asks the system and
//! applies the data; it names no identifier, team or product.
//!
//! # What the contract promises
//!
//! - [`Admission::spawn`] returns only a child that has passed the system's check of
//!   its image. A child that fails is killed and reaped before the error is returned,
//!   and the error carries the reason ([`SpawnRefused`]).
//! - Nothing lets a child run before its check. [`Admission::start`] is the same
//!   start split in two, for a caller that checks once more before the child runs:
//!   it returns the checked child as [`Held`], and [`Held::release`] lets it run.
//! - [`Admission::recheck`] and [`Admission::readmit`] repeat the check on a child
//!   that already runs; [`Admission::verify_reader`] requires that the process at the
//!   other end of an accepted connection is that child.
//! - A [`Child`] is the host's own child: it is killed and reaped when dropped.
//! - [`ChildEnv`] refuses, on every system, a variable that changes how the loader
//!   runs the image.
//!
//! # Systems
//!
//! - macOS: the child is started suspended (`posix_spawn` with
//!   `POSIX_SPAWN_START_SUSPENDED`), identified by its audit token, checked through
//!   the Security framework against the requirement and the signing rule, optionally
//!   pinned to a CodeDirectory hash, and only then continued. A native client is
//!   identified by the audit token of its connection.
//! - GNU/Linux: the image is checked before any child exists. When the admission is
//!   built, the pinned executable is copied once into a sealed, execute-only memory
//!   file and its SHA-256 compared with the expected digest; every child is started
//!   from that copy (`execveat`), is held by a pidfd, and is recognized on a
//!   connection by `SO_PEERCRED`. There is no suspended child: a held child runs
//!   already, from the verified bytes. A native client is identified by its user and
//!   its PID only; no system fact names its image. The host's own image is checked by
//!   the digest of its executable. Code signatures do not exist: no peer satisfies a
//!   requirement and no static image check is made.
//! - Every other Unix target: every entry refuses.

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::unix::net::UnixStream;
#[cfg(debug_assertions)]
use std::path::PathBuf;
use std::sync::Mutex;

use org_platform::Refused;

mod env;
mod errors;
mod policy;

pub use self::env::{is_loader_variable, ChildEnv};
pub use self::errors::{AdmissionError, ImageRefusal, NotTheChild, OwnImageError, SpawnRefused};
pub use self::policy::{ExpectedImage, Signing, SigningRule};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as native;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod linux;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use linux as native;

// Unused in a build for macOS or GNU/Linux, where only its tests call it and the
// Linux adapter takes from it the entries that refuse there too.
#[cfg_attr(
    any(target_os = "macos", all(target_os = "linux", target_env = "gnu")),
    allow(
        dead_code,
        reason = "refusing fallback module is used on unsupported targets and in unit tests"
    )
)]
mod refused;
#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
use refused as native;

// The decisions of an adapter that identifies a child after it was started (macOS),
// as pure functions over injected probes. Compiled for that adapter, and for the
// tests of every target so that the decisions are tested wherever the crate is.
#[cfg(any(target_os = "macos", test))]
mod decision;

#[cfg(test)]
mod tests;

/// The host's own child. Dropping it kills it (running or not) and reaps it, unless
/// it was reaped already.
pub struct Child(native::Child);

impl Child {
    /// Its PID. It stays this child's until the child is reaped here.
    pub fn id(&self) -> u32 {
        self.0.id()
    }

    /// Whether it has exited (it is reaped if so). Doesn't wait.
    pub fn try_wait(&mut self) -> io::Result<bool> {
        self.0.try_wait()
    }

    /// How it ended once reaped here, as fixed text: its exit code or signal.
    /// `None` while it runs, or when it was reaped elsewhere.
    pub fn ended(&self) -> Option<String> {
        self.0.ended()
    }

    /// Kills it (running or not) and reaps it.
    pub fn kill(&mut self) {
        self.0.kill()
    }
}

/// A child that passed its check and runs, with the host's ends of its standard
/// input, output and error.
pub struct Started {
    /// The child.
    pub child: Child,
    /// The write end of its standard input.
    pub stdin: File,
    /// The read end of its standard output.
    pub stdout: File,
    /// The read end of its standard error.
    pub stderr: File,
}

/// A child that passed its check and has not been let run yet. On macOS it is still
/// suspended: not one instruction of its image has run unless another process of the
/// same user continued it. On GNU/Linux the check was made before the child existed,
/// so the child runs already, from the verified bytes: nothing unchecked can be met
/// between the start and the release. Dropping it kills and reaps the child.
pub struct Held(Started);

impl Held {
    /// The child, for a further check before it runs ([`Admission::recheck`]).
    pub fn child(&self) -> &Child {
        &self.0.child
    }

    /// Let the child run (on GNU/Linux it does already: nothing is done). On an error
    /// the child is killed and reaped.
    pub fn release(self) -> io::Result<Started> {
        self.0.child.0.release()?;
        Ok(self.0)
    }
}

/// Starts and checks the children that must be one [`ExpectedImage`].
pub struct Admission(native::Admission);

/// The admission for `expected`, or why there is none. Nothing is started.
///
/// macOS reads nothing here: a child's image is checked when the child is started. A
/// system that executes verified bytes (GNU/Linux) reads and checks the image here,
/// once: every child of the admission then runs exactly the bytes read now.
#[allow(
    clippy::useless_conversion,
    reason = "conversion is identity on adapters returning this error directly and needed on others"
)]
pub fn admission(expected: ExpectedImage) -> Result<Admission, AdmissionError> {
    native::Admission::new(expected)
        .map(Admission)
        .map_err(AdmissionError::from)
}

impl Admission {
    /// The SHA-256 of the bytes every child of this admission executes, on a system
    /// that executes verified bytes: the adapter read exactly these bytes when the
    /// admission was built and starts nothing else. When the expected image named a
    /// digest, this is that digest. When it named none (a development image), this is
    /// the digest the adapter recorded, and no publisher vouches for it. `None` where
    /// the system admits a child by other means (a code signature checked on the
    /// loaded image).
    pub fn image_sha256(&self) -> Option<[u8; 32]> {
        self.0.image_sha256()
    }

    /// Start the expected program with `args`, exactly the environment `env` and `/`
    /// as its working directory, with its standard input, output and error on new
    /// pipes and no other descriptor of this process; check its image; let it run.
    ///
    /// `fence` is the lock the caller holds across each of its own two-step
    /// descriptor creations (a system without `pipe2` or `SOCK_CLOEXEC`). The adapter
    /// holds it only while it creates the pipes and the process, never during the
    /// check; a poisoned fence is taken all the same.
    ///
    /// A child that fails its check is killed and reaped, and the error's source is
    /// a [`SpawnRefused`]. Any other error is the system's: nothing runs after it.
    pub fn spawn(&self, args: &[&OsStr], env: &ChildEnv, fence: &Mutex<()>) -> io::Result<Started> {
        self.start(args, env, fence)?.release()
    }

    /// [`Self::spawn`] without its last step: the child is started and checked, and
    /// held until [`Held::release`].
    pub fn start(&self, args: &[&OsStr], env: &ChildEnv, fence: &Mutex<()>) -> io::Result<Held> {
        self.0.start(args, env, fence).map(Held)
    }

    /// Whether `child` is still the image that was admitted, and still the pinned
    /// one. A child admitted without an identity (a development image) passes unless
    /// the image is pinned.
    ///
    /// GNU/Linux: whether the child has not exited and the sealed image is still the
    /// object that was sealed. The kernel does not report whether a process replaced
    /// its own image, so this does not show that it has not; the verified image is
    /// trusted not to.
    pub fn recheck(&self, child: &Child) -> Result<(), ImageRefusal> {
        self.0.recheck(&child.0)
    }

    /// Check `child`'s image again from the start, as when it was admitted, and
    /// require the same identity ([`ImageRefusal::Readmitted`] otherwise). On
    /// GNU/Linux the bytes were verified before the child existed and cannot change,
    /// so this is [`Self::recheck`].
    pub fn readmit(&self, child: &Child) -> Result<(), ImageRefusal> {
        self.0.readmit(&child.0)
    }

    /// Whether the process at the other end of `stream` is `child`, as the image that
    /// was admitted. A mismatch is an error whose source is [`NotTheChild`]; a failed
    /// query is the system's error. On GNU/Linux a child that has exited is a
    /// mismatch, reaped or not.
    pub fn verify_reader(&self, child: &Child, stream: &UnixStream) -> io::Result<()> {
        self.0.verify_reader(&child.0, stream)
    }
}

/// The process at the other end of a connected socket, as the system reports it at
/// one moment. Plain facts: compare two of them, or ask what code one names.
///
/// Each system fills the facts it has. Build one with the constructor of the system
/// the facts come from, never field by field, so that a new fact does not change every
/// place that builds one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Peer {
    /// macOS: the audit token (`audit_token_t`) of the process that last touched the
    /// socket. It names one process image for its whole life and is never reused; an
    /// `exec` changes it. All zero on a system without audit tokens, where it names no
    /// process.
    pub macos_audit_token: [u8; 32],
    /// Linux: the effective user ID the peer had when it connected (`SO_PEERCRED`).
    /// `None` on a system that identifies a peer by its audit token.
    pub uid: Option<u32>,
    /// Linux: the process ID the peer had when it connected (`SO_PEERCRED`). A PID
    /// names a process only while that process is not reaped, and says nothing about
    /// the image it runs. `None` on a system that identifies a peer by its audit token.
    pub pid: Option<u32>,
}

impl Peer {
    /// The peer a macOS audit token names.
    pub fn from_macos_audit_token(token: [u8; 32]) -> Self {
        Self {
            macos_audit_token: token,
            uid: None,
            pid: None,
        }
    }

    /// The peer Linux socket credentials (`SO_PEERCRED`) name: its effective user ID
    /// and its process ID.
    pub fn from_linux_credentials(uid: u32, pid: u32) -> Self {
        Self {
            macos_audit_token: [0; 32],
            uid: Some(uid),
            pid: Some(pid),
        }
    }

    /// Whether the process this names satisfies the code requirement `requirement`
    /// and the flags and rejected entitlements of `rule`, all checked on one code
    /// object. Fails closed: any error, an exited process and a system without code
    /// signatures are `false`.
    pub fn satisfies(&self, requirement: &str, rule: &SigningRule) -> bool {
        native::peer_satisfies(self, requirement, rule)
    }
}

/// Who is at the other end of `stream` (on GNU/Linux: who connected it), or the
/// `peer identity` refusal (as an [`io::ErrorKind::Unsupported`] error) on a system
/// that cannot identify a peer.
pub fn peer(stream: &UnixStream) -> io::Result<Peer> {
    native::peer(stream)
}

/// What identifies an image to this system's admission: the fact a consumer's policy
/// needs to know which of its data ([`ExpectedImage`]) can vouch for a publisher here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageIdentity {
    /// The code signature of the loaded image (macOS). A child is started first, held
    /// suspended, and checked against the requirement, the signing rule and the pinned
    /// CodeDirectory hash before it runs. A release image therefore needs a requirement:
    /// without one nothing names its publisher. A native client is identified by its
    /// image as well.
    CodeSignature,
    /// The bytes themselves (GNU/Linux). The image is read and its SHA-256 compared with
    /// the expected digest before any child exists, and a child executes exactly those
    /// bytes: no suspended child, no code signature, no requirement. A release image's
    /// publisher is whoever signed the digest it is pinned to. A native client is
    /// identified by its user alone.
    VerifiedBytes,
}

/// What identifies an image to this system's admission, or the `process admission`
/// refusal on a system without an adapter.
pub fn image_identity() -> Result<ImageIdentity, Refused> {
    native::image_identity()
}

/// Whether this system identifies a native client at all ([`peer`]): by its image
/// on macOS, by its user and PID only on GNU/Linux, where [`Peer::satisfies`] is
/// always `false`. A consumer refuses to serve native clients where this refuses.
pub fn native_peers() -> Result<(), Refused> {
    native::native_peers()
}

/// The path of the executable the process at the other end of `stream` runs, found
/// through its PID. A PID can be reused and a path proves nothing about the image:
/// this is for a debug build's development fallback only, and is not compiled into a
/// release build.
#[cfg(debug_assertions)]
pub fn peer_executable(stream: &UnixStream) -> io::Result<PathBuf> {
    native::peer_executable(stream)
}

/// Whether this running process is the image `expected` pins. macOS compares the
/// kernel's CodeDirectory hash of the loaded image with `macos_cdhash`; GNU/Linux
/// compares the SHA-256 of this process's executable with `sha256`. An image with no
/// pin is a mismatch on every system, never a pass, development image or not.
pub fn own_image(expected: &ExpectedImage) -> Result<(), OwnImageError> {
    native::own_image(expected)
}

/// This process's own signing information: its flags and its entitlements. `None`
/// when it can't be read, and on every system without code signatures: nothing is
/// trusted then.
pub fn own_signing() -> Option<Signing> {
    native::own_signing()
}

/// What a static check of an executable file's code signature found.
#[derive(Debug)]
pub struct StaticSignature {
    /// The file's signing information.
    pub signing: Signing,
    /// The file's CodeDirectory hash.
    pub cdhash: [u8; 20],
}

/// Why a file's static signature was not established.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaticImageError {
    /// The held file could not be addressed.
    Path,
    /// The requirement is not well formed, the file's signature is missing or
    /// invalid, or it does not satisfy the requirement.
    Refused,
}

/// Static checks of executable files that are not running: what a release payload is
/// before it is published. Never evidence of the image a process loaded.
pub struct StaticImages(native::StaticImages);

/// The static check of this system, or the `process admission` refusal where an
/// executable's publisher cannot be established from its file.
pub fn static_images() -> Result<StaticImages, Refused> {
    native::StaticImages::new().map(StaticImages)
}

impl StaticImages {
    /// The signature of the held `file`, which must be valid and satisfy the code
    /// requirement `requirement`. The file is addressed through its descriptor, never
    /// through a path that could be replaced.
    pub fn signature(
        &self,
        file: &File,
        requirement: &str,
    ) -> Result<StaticSignature, StaticImageError> {
        self.0.signature(file, requirement)
    }
}

/// What every refusing entry of process admission answers.
fn refusal() -> Refused {
    Refused::Unsupported {
        capability: "process admission",
        os: std::env::consts::OS,
    }
}

/// What the refusing entry of the own-image check answers: the refusal of
/// `org_platform::image`'s loaded-image queries.
fn image_refusal() -> Refused {
    Refused::Unsupported {
        capability: "image identity",
        os: std::env::consts::OS,
    }
}

/// What every refusing entry of peer identification answers.
fn peer_refusal() -> Refused {
    Refused::Unsupported {
        capability: "peer identity",
        os: std::env::consts::OS,
    }
}

/// Hooks for the host's tests (`test-support`), re-exported from `crate::testing`.
#[cfg(any(test, feature = "test-support"))]
pub(crate) mod test_support {
    use super::{native, Admission, AdmissionError, Child, ExpectedImage, Held, Peer, Started};
    use std::io;
    use std::sync::Arc;

    /// Which step a scripted admission is asked for.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Step {
        /// The admission of a started child, and each later re-admission: the
        /// identity it is admitted with (`None`: not identified, as a development
        /// image), or a refusal.
        Admit,
        /// A re-check: the child's identity now. `None` refuses.
        Still,
    }

    /// A test's stand-in for the system's check: called with the step and the
    /// child's PID.
    pub type Script = Arc<dyn Fn(Step, u32) -> Result<Option<Peer>, String> + Send + Sync>;

    /// An admission whose image check is answered by `script` instead of the system.
    /// It still starts real children as the adapter does (held until released), and a
    /// pin in `expected` is still checked against the real loaded image.
    #[allow(
        clippy::useless_conversion,
        reason = "conversion is identity on adapters returning this error directly and needed on others"
    )]
    pub fn scripted_admission(
        expected: ExpectedImage,
        script: Script,
    ) -> Result<Admission, AdmissionError> {
        native::Admission::scripted(expected, script)
            .map(Admission)
            .map_err(AdmissionError::from)
    }

    /// An admission that starts `expected.program` by its path and checks nothing of
    /// its image, so that a test can start a program the production adapter of its
    /// system would not admit (on Linux, a `#!` script).
    ///
    /// - macOS: the production adapter, which already starts by path. The child is
    ///   started suspended and checked as `expected` says; a development image is not
    ///   checked.
    /// - GNU/Linux: `posix_spawn` of the path. No bytes are verified, nothing is
    ///   sealed and the pinned file's owner and mode are not checked: `sha256` is not
    ///   read and [`Admission::image_sha256`] is `None`. The child still gets exactly
    ///   its three pipes, the given environment and `/` as its working directory, is
    ///   held by a pidfd, and [`Admission::verify_reader`] makes the production
    ///   adapter's check of the connection (the child's PID, this user, still alive).
    ///
    /// Never part of a production composition: it exists only with `test-support`.
    #[allow(
        clippy::useless_conversion,
        reason = "conversion is identity on adapters returning this error directly and needed on others"
    )]
    pub fn path_admission(expected: ExpectedImage) -> Result<Admission, AdmissionError> {
        native::Admission::by_path(expected)
            .map(Admission)
            .map_err(AdmissionError::from)
    }

    /// The system's identity of this process's own unreaped child `pid`.
    pub fn child_peer(pid: u32) -> io::Result<Peer> {
        native::child_peer(pid)
    }

    /// A handle for a process the test does not own, admitted as `admitted`: it is
    /// never signalled and never waited for, and dropping it does nothing. For
    /// [`Admission::verify_reader`] and [`Admission::recheck`] against a process the
    /// test chose (itself, usually).
    #[allow(
        clippy::useless_conversion,
        reason = "conversion is identity on adapters returning this error directly and needed on others"
    )]
    pub fn unowned_child(pid: u32, admitted: Option<Peer>) -> io::Result<Child> {
        native::Child::unowned(pid, admitted)
            .map(Child)
            .map_err(io::Error::from)
    }

    /// The child of `held`, still not let run, with its pipes: for a test that needs
    /// a child that stays alive without running anything.
    pub fn unreleased(held: Held) -> Started {
        held.0
    }
}
