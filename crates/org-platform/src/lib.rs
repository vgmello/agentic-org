//! Every operating-system difference sits here, behind one contract per capability
//! ([ADR-0017](../../../docs/adr/0017-linux-and-container-hosting.md) decision 6,
//! [Linux host design](../../../docs/design/linux-host.md) section 1).
//!
//! Each module exports one non-conditional constructor. It returns the build target's
//! implementation, or the refusing adapter, which reports [`Refused::Unsupported`] and
//! never falls back to another system's behaviour. Platform conditionals stay inside this
//! crate. Consumers keep their policy (which program, which label prefix) and pass it as
//! data. The crate depends on no workspace crate.
//!
//! Implemented today (milestone one): macOS. Linux arrives with Track X; until then every
//! other Unix target gets the refusing adapters. No other crate has a `cfg` on
//! `target_os`, `target_vendor`, `target_env` or `target_family`:
//! `tests/platform_conditionals.rs` enforces that (LNX-5).
//!
//! # How a capability is laid out
//!
//! Each capability is one module, `src/<capability>/mod.rs`, with its adapters beside it:
//!
//! - `mod.rs` is the contract: the public types and functions, their documentation, and
//!   the code that is the same on every system. Its public functions are not
//!   conditional.
//! - `macos.rs` and `linux.rs` are the adapters for macOS and for GNU/Linux (glibc).
//!   They define the same `pub(super)` items.
//! - `refused.rs` is the adapter for every other Unix target, including Linux with
//!   another C library. Every item in it returns [`Refused`] (or an `io::Error` made
//!   from one). It is compiled and tested on every target, so it cannot rot.
//!
//! A capability with both adapters declares them with `adapters!()`, which binds
//! `native` to the build target's adapter, and puts code that only the two real
//! adapters share inside `supported! { … }`. A capability that has only its macOS
//! adapter so far selects it with its own two `cfg` lines until its Linux adapter is
//! written. An adapter file is already compiled for one system only and needs no
//! predicate.

#[cfg(not(unix))]
compile_error!("org-platform builds only for Unix targets");

/// Declares `macos`, `linux` and `refused` beside the calling module and binds
/// `native` to the build target's adapter.
// Unused until the first capability with both adapters lands (X1b).
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

/// Wraps items that only the macOS and GNU/Linux adapters use (code the two share),
/// so a build for a refusing target does not compile them.
// Unused until the first capability with both adapters lands (X1b).
#[allow(unused_macros)]
macro_rules! supported {
    ($($item:item)*) => {
        $(
            #[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
            $item
        )*
    };
}

pub mod dirs;
pub mod disk;
pub mod fs;
pub mod process;
pub mod service;
pub mod socket;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;

/// A capability this build target has no adapter for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    Unsupported {
        capability: &'static str,
        os: &'static str,
    },
}

impl Refused {
    pub(crate) fn unsupported(capability: &'static str) -> Self {
        Self::Unsupported {
            capability,
            os: std::env::consts::OS,
        }
    }
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported { capability, os } => write!(
                f,
                "{capability} is not available on {os} in this release; nothing was changed"
            ),
        }
    }
}

impl std::error::Error for Refused {}

/// For contracts whose functions return `io::Result`: the refusal as an
/// [`std::io::ErrorKind::Unsupported`] error that keeps the [`Refused`] as its source.
impl From<Refused> for std::io::Error {
    fn from(refused: Refused) -> Self {
        std::io::Error::new(std::io::ErrorKind::Unsupported, refused)
    }
}

#[cfg(test)]
mod tests {
    use super::Refused;
    use std::io;

    #[test]
    fn a_refusal_becomes_an_unsupported_io_error_that_keeps_its_reason() {
        let error = io::Error::from(Refused::unsupported("fs"));
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        let source = error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<Refused>())
            .expect("the refusal is the error's source");
        assert_eq!(source, &Refused::unsupported("fs"));
        assert_eq!(error.to_string(), Refused::unsupported("fs").to_string());
    }
}
