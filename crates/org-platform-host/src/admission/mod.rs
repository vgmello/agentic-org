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
//! - GNU/Linux: no adapter yet (increment X2 writes it: a sealed copy of the verified
//!   bytes, a pidfd and `SO_PEERCRED`). Every entry refuses, so no child starts and no
//!   client is identified.
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
pub(crate) mod macos;
#[cfg(target_os = "macos")]
use macos as native;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod linux;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use linux as native;

// Unused in a build for macOS, where only its tests call it. The Linux adapter is
// this one until increment X2 writes its own.
#[cfg_attr(target_os = "macos", allow(dead_code))]
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
    /// The Linux rule (increment X2); macOS does not read it.
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
    /// requirement is given. A pin still applies, and refuses an image the system
    /// cannot identify.
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

/// Whether `name` is a variable that changes how the loader runs an image: any name
/// beginning `LD_` or `DYLD_`, and `GLIBC_TUNABLES`. The same on every system, so that
/// no caller depends on which loader the child gets.
pub fn is_loader_variable(name: &OsStr) -> bool {
    let name = name.as_bytes();
    name.starts_with(b"LD_") || name.starts_with(b"DYLD_") || name == b"GLIBC_TUNABLES"
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
/// same user continued it. Dropping it kills and reaps the child.
pub struct Held(Started);

impl Held {
    /// The child, for a further check before it runs ([`Admission::recheck`]).
    pub fn child(&self) -> &Child {
        &self.0.child
    }

    /// Let the child run. On an error the child is killed and reaped.
    pub fn release(self) -> io::Result<Started> {
        self.0.child.0.release()?;
        Ok(self.0)
    }
}

/// Starts and checks the children that must be one [`ExpectedImage`].
pub struct Admission(native::Admission);

/// The admission for `expected`, or the `process admission` refusal on a system
/// without an adapter. Nothing is started and nothing is read.
pub fn admission(expected: ExpectedImage) -> Result<Admission, Refused> {
    native::Admission::new(expected).map(Admission)
}

impl Admission {
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
    pub fn recheck(&self, child: &Child) -> Result<(), ImageRefusal> {
        self.0.recheck(&child.0)
    }

    /// Check `child`'s image again from the start, as when it was admitted, and
    /// require the same identity ([`ImageRefusal::Readmitted`] otherwise).
    pub fn readmit(&self, child: &Child) -> Result<(), ImageRefusal> {
        self.0.readmit(&child.0)
    }

    /// Whether the process at the other end of `stream` is `child`, as the image that
    /// was admitted. A mismatch is an error whose source is [`NotTheChild`]; a failed
    /// query is the system's error.
    pub fn verify_reader(&self, child: &Child, stream: &UnixStream) -> io::Result<()> {
        self.0.verify_reader(&child.0, stream)
    }
}

/// The process at the other end of a connected socket, as the system reports it at
/// one moment. Plain facts: compare two of them, or ask what code one names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peer {
    /// macOS: the audit token (`audit_token_t`) of the process that last touched the
    /// socket. It names one process image for its whole life and is never reused; an
    /// `exec` changes it.
    pub macos_audit_token: [u8; 32],
}

impl Peer {
    /// Whether the process this names satisfies the code requirement `requirement`
    /// and the flags and rejected entitlements of `rule`, all checked on one code
    /// object. Fails closed: any error, an exited process and a system without code
    /// signatures are `false`.
    pub fn satisfies(&self, requirement: &str, rule: &SigningRule) -> bool {
        native::peer_satisfies(self, requirement, rule)
    }
}

/// Who is at the other end of `stream` now, or the `peer identity` refusal (as an
/// [`io::ErrorKind::Unsupported`] error) on a system that cannot identify a peer's
/// image.
pub fn peer(stream: &UnixStream) -> io::Result<Peer> {
    native::peer(stream)
}

/// Whether this system can identify the image of a native client at all ([`peer`]).
/// A consumer that admits clients only by their image refuses to serve them where
/// this refuses.
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
/// kernel's CodeDirectory hash of the loaded image with `macos_cdhash`; an image
/// with no pin is a mismatch, never a pass.
pub fn own_image(expected: &ExpectedImage) -> Result<(), OwnImageError> {
    let loaded = org_platform::image::own_loaded_cdhash().map_err(|error| {
        match error
            .get_ref()
            .and_then(|source| source.downcast_ref::<Refused>())
        {
            Some(refused) => OwnImageError::Unsupported(refused.clone()),
            None => OwnImageError::Unavailable(error),
        }
    })?;
    if expected.macos_cdhash != Some(loaded) {
        return Err(OwnImageError::Mismatch);
    }
    Ok(())
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
    use super::{native, Admission, Child, ExpectedImage, Held, Peer, Started};
    use org_platform::Refused;
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
    pub fn scripted_admission(
        expected: ExpectedImage,
        script: Script,
    ) -> Result<Admission, Refused> {
        native::Admission::scripted(expected, script).map(Admission)
    }

    /// The system's identity of this process's own unreaped child `pid`.
    pub fn child_peer(pid: u32) -> io::Result<Peer> {
        native::child_peer(pid)
    }

    /// A handle for a process the test does not own, admitted as `admitted`: it is
    /// never signalled and never waited for, and dropping it does nothing. For
    /// [`Admission::verify_reader`] and [`Admission::recheck`] against a process the
    /// test chose (itself, usually).
    pub fn unowned_child(pid: u32, admitted: Option<Peer>) -> Result<Child, Refused> {
        native::Child::unowned(pid, admitted).map(Child)
    }

    /// The child of `held`, still not let run, with its pipes: for a test that needs
    /// a child that stays alive without running anything.
    pub fn unreleased(held: Held) -> Started {
        held.0
    }
}
