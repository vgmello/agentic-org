//! Confinement of the web process.
//!
//! Linux host design section 3.3 (the `confinement` contract of ADR-0017); the macOS
//! requirements are ADR-0008. Narrow OS process-entry confinement: no protocol,
//! storage or domain policy. The web process and the host are the consumers; they
//! keep the wire protocol and decide what a failure means.
//!
//! In the web process: [`disable_core_dumps`], then [`confine_web`]. Call both before
//! runtime/thread creation and before any secret or network input. The host spawn
//! contract closes all inherited descriptors except stdio. We verify that closure
//! rather than closing descriptors owned by safe Rust code.
//!
//! In the host: [`web_confinement_supported`] before any bind, child or secret;
//! [`verify_ready_report`] on the report of the child's ready line; [`verify_web`]
//! after the child's image and peer checks and before any descriptor or secret is
//! sent.
//!
//! # Systems
//!
//! - macOS: the Seatbelt profile of ADR-0008 and the host's `sandbox_check` probes.
//! - GNU/Linux (x86-64 and aarch64, Linux 6.2 or later with Landlock enabled): a
//!   Landlock domain that allows no path and a seccomp filter that kills the process
//!   on every system call outside a fixed list; the host reads the child's
//!   no-new-privileges and seccomp state from `/proc`. Where the kernel or a
//!   container's profile lacks or refuses either mechanism, [`confine_web`] and
//!   [`web_confinement_supported`] refuse and name the call and the cause
//!   (ADR-0017 LNX-3).
//! - Every other Unix target: each adapter function returns the refusal
//!   ([`crate::Refused`], as an [`io::ErrorKind::Unsupported`] error where the
//!   function returns `io::Result`).
//!
//! [`disable_core_dumps`] is the same call on every system and is not an adapter
//! function. [`fill_entropy`] is the same `getentropy` loop on macOS and GNU/Linux and
//! refuses elsewhere.
//!
//! # Refusals
//!
//! An adapter's own refusal (a precondition that does not hold, a mechanism that is
//! unavailable, a child that is not confined) is an error without an OS error number
//! whose text is `gateway confinement unavailable`. The Linux adapter appends a colon
//! and the cause. A failed connection to the rendezvous keeps its OS error number.

#![deny(unsafe_op_in_unsafe_fn)]

use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;

use crate::Refused;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as native;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod linux;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use linux as native;

// Unused in a build for macOS and for GNU/Linux, where only its tests call it.
#[cfg_attr(
    any(target_os = "macos", all(target_os = "linux", target_env = "gnu")),
    allow(dead_code)
)]
mod refused;
#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
use refused as native;

#[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
mod shared;

#[cfg(all(
    test,
    any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))
))]
mod adapter_tests;

#[cfg(any(test, feature = "test-support"))]
pub mod testing;

#[cfg(test)]
mod tests;

/// What the web process reports about the confinement it applied to itself: the
/// evidence of its ready line. The host trusts it as far as it trusts the image it
/// admitted.
///
/// macOS: no Landlock and no seccomp filter (Seatbelt is observed by the host
/// itself, in [`verify_web`]).
///
/// GNU/Linux: the Landlock ABI and `seccomp: true`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Enforcement {
    /// The Landlock ABI the process restricted itself under, in full, where it used
    /// Landlock: every right that ABI knows is handled and no rule allows anything.
    /// On a kernel with a newer ABI than the adapter knows, this is the newest one
    /// the adapter knows. An adapter that cannot enforce its whole ruleset fails; it
    /// reports nothing.
    pub landlock_abi: Option<u32>,
    /// Whether the process installed its seccomp filter. Where it did, a forbidden
    /// system call kills the process; it never returns an error. The ready line has
    /// no field for this: the host observes the filter on the process itself, and
    /// the Linux [`verify_ready_report`] and [`verify_web`] do not read this field.
    pub seccomp: bool,
}

/// The fixed text of this module's own refusals. An adapter that can say more
/// appends a colon and the cause.
const REFUSAL: &str = "gateway confinement unavailable";

fn refused() -> io::Error {
    io::Error::other(REFUSAL)
}

/// Disable core dumps before loading any potentially sensitive process state.
pub fn disable_core_dumps() -> io::Result<()> {
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: a valid initialized input, no descriptors or memory retained by libc.
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } != 0 {
        return Err(refused());
    }
    Ok(())
}

/// Confine this process, the web process, and return its connection to the launch
/// rendezvous together with what it may report in its ready line. The stream is
/// returned only after confinement is in force, so it cannot be used unconfined.
///
/// macOS: apply the reviewed policy, only with verified main/never-threaded entry
/// and exactly the host's three stdio descriptors, then connect to `rendezvous`,
/// the one connection the policy allows. Failure must terminate the caller before
/// readiness. Unsupported platforms never return success.
///
/// GNU/Linux, in this order: limit the C library's allocator to one arena; disable
/// core dumps and same-user tracing; require exactly one thread now (a thread that
/// has ended does not count) and exactly descriptors 0 to 2; connect to
/// `rendezvous`, an absolute path, the last socket the process creates; set
/// no-new-privileges; restrict the process with Landlock (ABI 3 or higher, every
/// right of the ABI handled, no rule); install the seccomp
/// filter. Any failure returns an error that names the call and the cause, and the
/// connected stream is dropped: the caller never holds a stream unconfined. After a
/// failure of the last two steps the process is partly confined and must still exit.
/// After it returns, opening a file, creating, connecting or binding a socket and
/// starting a program each kill the process. The host therefore starts the web
/// process without `RUST_BACKTRACE`: a backtrace opens files.
///
/// The allocator limit is there because glibc opens
/// `/proc/sys/vm/overcommit_memory` the first time the heap of an arena other than
/// the main one shrinks, and the filter kills that open. If the limit cannot be set,
/// the call refuses before it has changed anything. The limit holds only when this
/// is called before the process ever had a second thread: an earlier thread, even
/// one that was joined, may have left a second arena, and then a later thread's
/// allocator can make that open and the process is killed. That is the filter
/// failing closed, never an unconfined process. The cost is one allocator lock for
/// all threads; each thread keeps its own cache of small blocks.
pub fn confine_web(rendezvous: &Path) -> io::Result<(UnixStream, Enforcement)> {
    native::confine_web(rendezvous)
}

/// Whether this platform has a web process confinement adapter. The host asks before
/// any bind, child or secret handoff. macOS: supported. Every other target refuses
/// with the reason until its adapter exists (ADR-0017 LNX-3/LNX-5).
///
/// GNU/Linux: answered from what the asking process can observe without confining
/// itself: the architecture has a system call list, the Landlock version query
/// answers ABI 3 or higher, and the kernel can kill a process from a seccomp filter.
/// A refusal's `capability` text carries the reason in brackets (which call failed,
/// with which error, or which ABI is too old). The web process can still fail to
/// confine itself afterwards, for example under a profile that refuses
/// `landlock_restrict_self`; then it exits before it reports ready.
pub fn web_confinement_supported() -> Result<(), Refused> {
    native::supported()
}

/// Checks the confinement report of the web process's ready line. macOS has no
/// Landlock and no seccomp, so a report of either refuses. Every other target
/// refuses until its adapter checks its own report.
///
/// GNU/Linux: requires a Landlock ABI of 3 or higher. The `seccomp` field is not
/// read: the ready line cannot carry it, and [`verify_web`] observes the filter.
pub fn verify_ready_report(enforcement: &Enforcement) -> io::Result<()> {
    native::verify_ready_report(enforcement)
}

/// Parent-only kernel observation after child image/peer verification: the report
/// is checked as in [`verify_ready_report`], then the child's confinement itself.
/// macOS queries permissions for the critical paths without opening or reading
/// those paths.
///
/// GNU/Linux reads `/proc/<pid>/status` once and requires `NoNewPrivs: 1`,
/// `Seccomp: 2` and more seccomp filters than the calling process has itself: a
/// child inherits its parent's filters, as under a container's profile, so only one
/// more is the web process's own. A missing or unreadable entry refuses. It cannot
/// observe another process's Landlock domain, so `critical` is not used and the
/// Landlock state is the report's alone: the host trusts it as far as it trusts the
/// image it admitted. It cannot tell which filter the process installed either. The
/// caller must hold the child unreaped across this call and check through its own
/// child handle that it is still alive: the process id then names the admitted child
/// and no other process.
pub fn verify_web(pid: u32, enforcement: &Enforcement, critical: &[&Path]) -> io::Result<()> {
    native::verify_web(pid, enforcement, critical)
}

/// OS CSPRNG without opening an ambient random-device path after confinement.
/// Each call uses the OS's bounded getentropy operation; there is no file fallback.
/// macOS and GNU/Linux: `getentropy`. Every other target refuses.
pub fn fill_entropy(bytes: &mut [u8]) -> io::Result<()> {
    native::fill_entropy(bytes)
}
