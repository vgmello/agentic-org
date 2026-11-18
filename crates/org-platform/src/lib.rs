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
//! other Unix target gets the refusing adapters.

#[cfg(not(unix))]
compile_error!("org-platform builds only for Unix targets");

pub mod dirs;
pub mod disk;
pub mod service;
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
