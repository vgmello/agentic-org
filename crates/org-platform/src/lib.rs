//! Every operating-system difference in the workspace, behind small contracts.
//!
//! ADR-0017 decision 6 and the Linux host design (`docs/design/linux-host.md`): each
//! module here is one platform capability. It holds the contract, a macOS adapter
//! (`macos.rs`), a GNU/Linux adapter (`linux.rs`) and a refusing adapter (`refused.rs`),
//! and exports one non-conditional constructor that returns the build target's
//! adapter or the refusing one. No other crate has a `cfg` on `target_os`,
//! `target_vendor`, `target_env` or `target_family`; `tests/platform_conditionals.rs`
//! enforces that (LNX-5).
//!
//! The crate knows nothing about the domain. Consumers keep their policy (which image
//! is trusted, which secret names exist) and pass it in as plain data. It depends on
//! no workspace crate.

#[cfg(not(unix))]
compile_error!(
    "org-platform builds only for Unix targets; macOS and GNU/Linux have adapters and \
     every other Unix target gets the refusing ones (ADR-0017)"
);

mod refusal;
pub mod service;

pub use refusal::Refused;
