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

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::File;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Mutex;

use org_platform::Refused;

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
    allow(dead_code)
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

/// The image a child must be: the host's policy, as plain data.
///
/// The host builds one per program it starts (the office worker, the web process) and
/// one for itself ([`own_image`]). An adapter uses the fields its system has a check
/// for and ignores the others; it never invents a missing one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpectedImage {
    /// The pinned path of the executable.
    pub program: PathBuf,
    /// The SHA-256 of the executable's bytes, from the verified release manifest.
    /// The Linux rule: the bytes a child executes must have it, and the host's own
    /// executable too ([`own_image`]). `None` is admitted only with `development`,
    /// and never for the host's own image. macOS does not read it.
    pub sha256: Option<[u8; 32]>,
    /// macOS: the CodeDirectory hash the loaded image must have, from the verified
    /// release manifest. `None` pins nothing.
    pub macos_cdhash: Option<[u8; 20]>,
    /// macOS: the code requirement text the image's code object must satisfy (a
    /// Developer ID requirement). `None` is allowed only with `development`.
    pub macos_requirement: Option<String>,
    /// macOS: what the image's signing information must show, checked on the same
    /// code object as the requirement. Read only when `macos_requirement` is set.
    pub macos_signing: SigningRule,
    /// A debug build's own build output: the publisher check is skipped when no
    /// requirement (macOS) or digest (Linux) is given. A pin still applies, and
    /// refuses an image the system cannot identify.
    pub development: bool,
}

/// What signing information must show before its code is trusted, as plain data.
///
/// The names and bits are the consumer's policy. [`SigningRule::allows`] is the whole
/// meaning of a rule, on every system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SigningRule {
    /// Code-signature flag bits that must all be set. Signing information without
    /// flags is refused, whatever this is.
    pub required_flags: u32,
    /// Entitlements that refuse the code when present with any value but `false`.
    pub rejected_entitlements: &'static [&'static str],
    /// Entitlements that refuse the code when present at all, whatever their value.
    pub absent_entitlements: &'static [&'static str],
}

impl SigningRule {
    /// Whether `signing` shows every required flag and none of the rejected
    /// entitlements (one that is present with any value but `false` counts). Missing
    /// flags fail closed.
    pub fn flags_and_rejections_pass(&self, signing: &Signing) -> bool {
        let Some(flags) = signing.flags else {
            return false;
        };
        if flags & self.required_flags != self.required_flags {
            return false;
        }
        !signing.entitlements.iter().flatten().any(|(key, value)| {
            self.rejected_entitlements.contains(&key.as_str()) && *value != Some(false)
        })
    }

    /// Whether `signing` holds none of the entitlements that must be absent.
    pub fn absences_pass(&self, signing: &Signing) -> bool {
        !signing
            .entitlements
            .iter()
            .flatten()
            .any(|(key, _)| self.absent_entitlements.contains(&key.as_str()))
    }

    /// The whole rule: [`Self::flags_and_rejections_pass`] and [`Self::absences_pass`].
    pub fn allows(&self, signing: &Signing) -> bool {
        self.flags_and_rejections_pass(signing) && self.absences_pass(signing)
    }
}

/// What the system reports about a program's code signature. Plain facts: the
/// consumer's [`SigningRule`] decides what they allow.
#[derive(Debug, Default)]
pub struct Signing {
    /// The code-signature flags (`kSecCodeInfoFlags`), if present.
    pub flags: Option<u32>,
    /// The entitlements (`kSecCodeInfoEntitlementsDict`): each key with its value when it's a
    /// boolean (`None` for any other kind of value). `None` when the code has no entitlements.
    pub entitlements: Option<Vec<(String, Option<bool>)>>,
    /// The `keychain-access-groups` entitlement: its groups, when it's an array of strings.
    /// `None` when it's missing or anything else.
    pub keychain_access_groups: Option<Vec<String>>,
}

/// Why an image was not admitted. The consumer words it for its own log.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageRefusal {
    /// The system's identity of the child could not be read. The text is the
    /// system's error, never a path or a secret.
    Identity(String),
    /// The child's code does not satisfy the requirement or the signing rule.
    NotTheImage,
    /// The image is pinned, but the system has no identity to read its hash by (a
    /// development image admitted without a check).
    Unidentified,
    /// The loaded image's hash could not be read.
    DigestUnavailable,
    /// The loaded image is not the pinned one.
    DigestMismatch,
    /// The child is no longer the image that was admitted: it replaced itself.
    Changed,
    /// A second admission of the same child gave another identity than the first.
    Readmitted,
    /// The image names no digest to check its bytes against, and it is not a
    /// development image (a system that executes verified bytes).
    Unpinned,
    /// The pinned file is not one whose bytes may be executed: the fixed reason (its
    /// kind, its owner or its mode). Never a path.
    UnsafeFile(&'static str),
    /// A scripted check refused, in the test's own words (`test-support`).
    #[cfg(any(test, feature = "test-support"))]
    Scripted(String),
}

impl fmt::Display for ImageRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identity(why) => write!(f, "the child's identity is unavailable: {why}"),
            Self::NotTheImage => {
                f.write_str("the child's code does not satisfy the requirement or the signing rule")
            }
            Self::Unidentified => {
                f.write_str("a pinned image needs an identity the system can read")
            }
            Self::DigestUnavailable => f.write_str("the loaded image's hash is unavailable"),
            Self::DigestMismatch => f.write_str("the loaded image is not the pinned image"),
            Self::Changed => f.write_str("the child replaced its image after it was checked"),
            Self::Readmitted => f.write_str("the child's identity changed between two admissions"),
            Self::Unpinned => f.write_str("a release image needs a SHA-256 digest"),
            Self::UnsafeFile(why) => write!(f, "the pinned image file {why}"),
            #[cfg(any(test, feature = "test-support"))]
            Self::Scripted(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for ImageRefusal {}

/// The source of the error [`Admission::spawn`] and [`Admission::start`] return when
/// the child was started and then refused. The child is already killed and reaped. A
/// consumer classifies the failure by this type ([`SpawnRefused::of`]), never by the
/// message.
#[derive(Debug)]
pub struct SpawnRefused {
    /// The PID the refused child had. It names nothing any more.
    pub pid: u32,
    /// Why the child was refused.
    pub refusal: ImageRefusal,
}

impl SpawnRefused {
    /// The refusal inside `error`, if that is what it is.
    pub fn of(error: &io::Error) -> Option<&Self> {
        error.get_ref().and_then(|source| source.downcast_ref())
    }
}

impl fmt::Display for SpawnRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "child {} was refused: {}", self.pid, self.refusal)
    }
}

impl std::error::Error for SpawnRefused {}

/// The source of the error [`Admission::verify_reader`] returns when the connection's
/// peer is another process than the child, or another image of it than the admitted
/// one. Any other error of that function is the system's.
#[derive(Debug)]
pub struct NotTheChild;

impl NotTheChild {
    /// Whether `error` is this mismatch.
    pub fn is(error: &io::Error) -> bool {
        error
            .get_ref()
            .is_some_and(|source| source.is::<NotTheChild>())
    }
}

impl fmt::Display for NotTheChild {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the connection's peer is not the admitted child")
    }
}

impl std::error::Error for NotTheChild {}

/// Whether `name` is a variable that makes a child load code its image does not name:
/// any name beginning `LD_` or `DYLD_` (the loaders), `GLIBC_TUNABLES`, and
/// `GCONV_PATH` (glibc loads character-set converters, shared objects, from there).
/// The same on every system, so that no caller depends on which loader the child gets.
pub fn is_loader_variable(name: &OsStr) -> bool {
    let name = name.as_bytes();
    name.starts_with(b"LD_")
        || name.starts_with(b"DYLD_")
        || name == b"GLIBC_TUNABLES"
        || name == b"GCONV_PATH"
}

/// The whole environment of a child: exactly these variables, nothing of the host's.
/// It holds no loader variable ([`is_loader_variable`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChildEnv(Vec<(OsString, OsString)>);

impl ChildEnv {
    /// An empty environment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `name=value`. A loader variable is refused with
    /// [`io::ErrorKind::InvalidInput`] and nothing is added; the error names the rule,
    /// never the variable's value.
    pub fn set(&mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> io::Result<()> {
        let name = name.into();
        if is_loader_variable(&name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a child's environment may not set a loader variable",
            ));
        }
        self.0.push((name, value.into()));
        Ok(())
    }

    /// The variables, in the order they were set.
    pub fn pairs(&self) -> &[(OsString, OsString)] {
        &self.0
    }
}

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

/// Why no admission was made ([`admission`]). Nothing was started.
#[derive(Debug)]
pub enum AdmissionError {
    /// This system has no admission adapter.
    Unsupported(Refused),
    /// A system that executes verified bytes read the image and will not start it.
    Image(ImageRefusal),
    /// A step of preparing the image failed. The error names the system call.
    System(io::Error),
}

impl fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(refused) => refused.fmt(f),
            Self::Image(refusal) => write!(f, "the image was refused: {refusal}"),
            Self::System(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for AdmissionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unsupported(refused) => Some(refused),
            Self::Image(refusal) => Some(refusal),
            Self::System(error) => Some(error),
        }
    }
}

impl From<Refused> for AdmissionError {
    fn from(refused: Refused) -> Self {
        Self::Unsupported(refused)
    }
}

impl From<ImageRefusal> for AdmissionError {
    fn from(refusal: ImageRefusal) -> Self {
        Self::Image(refusal)
    }
}

impl From<io::Error> for AdmissionError {
    fn from(error: io::Error) -> Self {
        Self::System(error)
    }
}

/// For a caller whose own errors are `io::Error`s: a missing adapter stays an
/// [`io::ErrorKind::Unsupported`] error whose source is the [`Refused`], a refused image
/// is an error whose source is the [`ImageRefusal`], and the system's error is itself.
impl From<AdmissionError> for io::Error {
    fn from(error: AdmissionError) -> Self {
        match error {
            AdmissionError::Unsupported(refused) => refused.into(),
            AdmissionError::Image(refusal) => io::Error::other(refusal),
            AdmissionError::System(error) => error,
        }
    }
}

/// The admission for `expected`, or why there is none. Nothing is started.
///
/// macOS reads nothing here: a child's image is checked when the child is started. A
/// system that executes verified bytes (GNU/Linux) reads and checks the image here,
/// once: every child of the admission then runs exactly the bytes read now.
// The conversion is the identity on a system whose adapter reports this error itself.
#[allow(clippy::useless_conversion)]
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

/// Why the host's own image is not the expected one ([`own_image`]).
#[derive(Debug)]
pub enum OwnImageError {
    /// This system has no loaded-image query.
    Unsupported(Refused),
    /// The system's query failed.
    Unavailable(io::Error),
    /// The loaded image is not the expected one, or nothing was expected of it.
    Mismatch,
}

impl fmt::Display for OwnImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(refused) => refused.fmt(f),
            Self::Unavailable(error) => write!(f, "the loaded image is unavailable: {error}"),
            Self::Mismatch => f.write_str("the loaded image is not the expected image"),
        }
    }
}

impl std::error::Error for OwnImageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unsupported(refused) => Some(refused),
            Self::Unavailable(error) => Some(error),
            Self::Mismatch => None,
        }
    }
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
    // The conversion is the identity on a system whose adapter reports this error itself.
    #[allow(clippy::useless_conversion)]
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
    // The conversion is the identity on a system whose adapter reports this error itself.
    #[allow(clippy::useless_conversion)]
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
    // The conversion is the identity on a system whose adapter reports this error itself.
    #[allow(clippy::useless_conversion)]
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
