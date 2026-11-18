//! Every operating-system difference in the workspace, behind small contracts.
//!
//! ADR-0017 decision 6 and the Linux host design (`docs/design/linux-host.md`). No
//! other crate has a `cfg` on `target_os`, `target_vendor`, `target_env` or
//! `target_family`; `tests/platform_conditionals.rs` enforces that (LNX-5).
//!
//! The crate knows nothing about the domain. Consumers keep their policy (which image
//! is trusted, which secret names exist) and pass it in as plain data. It depends on
//! no workspace crate.
//!
//! # How a capability is laid out
//!
//! Each capability is one module, `src/<capability>.rs`, with three adapters beside it:
//!
//! - `<capability>.rs` is the contract: the public types and functions, their
//!   documentation, and the code that is the same on every system. Its public
//!   functions are not conditional. They call `native::…`.
//! - `<capability>/macos.rs` and `<capability>/linux.rs` are the adapters for macOS
//!   and for GNU/Linux (glibc). They define the same `pub(super)` items.
//! - `<capability>/refused.rs` is the adapter for every other Unix target, including
//!   Linux with another C library. Every item in it returns [`Refused`] (or an
//!   `io::Error` made from one). It never falls back to another system's behaviour.
//!   It is compiled and tested on every target, so it cannot rot.
//!
//! `adapters!()` at the top of the contract declares the three modules and binds
//! `native` to the build target's. That macro, and the `compile_error!` below, are the
//! only places a new capability needs a platform predicate; an adapter file is already
//! compiled for one system only.
//!
//! A difference found later becomes a new function in the capability it belongs to,
//! or a new capability, with all three adapters, and a row in section 2 of the design.

#[cfg(not(unix))]
compile_error!(
    "org-platform builds only for Unix targets; macOS and GNU/Linux have adapters and \
     every other Unix target gets the refusing ones (ADR-0017)"
);

/// Declares `macos`, `linux` and `refused` beside the calling module and binds
/// `native` to the build target's adapter.
// Unused until the first capability of X1b lands.
#[allow(unused_macros)]
macro_rules! adapters {
    () => {
        #[cfg(target_os = "macos")]
        mod macos;
        #[cfg(target_os = "macos")]
        use macos as native;

        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        mod linux;
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        use linux as native;

        // Unused in a build for macOS or GNU/Linux, where only its tests call it.
        #[cfg_attr(
            any(target_os = "macos", all(target_os = "linux", target_env = "gnu")),
            allow(dead_code)
        )]
        mod refused;
        #[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
        use refused as native;
    };
}

mod refusal;

pub mod dirs;
pub mod fs;
pub mod process;
pub mod service;
pub mod socket;

pub use refusal::Refused;
