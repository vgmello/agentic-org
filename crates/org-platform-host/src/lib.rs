//! The host's operating-system capabilities that need more than the C library.
//!
//! [`org_platform`] holds every operating-system difference that the four executables
//! share, and depends on `libc` only: a crate's link directives reach every executable
//! that links it, and the web process must load nothing but the C library before it
//! confines itself (ADR-0008). A capability that only the host (`agentic-org serve`
//! and its commands) consumes, and whose macOS adapter needs an Apple framework
//! (Security, CoreFoundation, LocalAuthentication), lives here instead. Only `org-host`
//! depends on this crate; `crates/org-platform/tests/libc_only.rs` keeps the other
//! three executables free of it.
//!
//! The rules are `org_platform`'s (ADR-0017 decision 6, Linux host design section 1):
//! one module per capability, `<capability>/mod.rs` as the contract with `macos.rs`,
//! `linux.rs` and `refused.rs` beside it, one non-conditional entry per function, a
//! refusing adapter that returns [`org_platform::Refused`] and is compiled and tested
//! on every target, no domain policy, and platform predicates only in the module
//! declarations at the top of each `mod.rs`. The LNX-5 check exempts this crate as it
//! exempts `org-platform`.

#[cfg(not(unix))]
compile_error!("org-platform-host builds only for Unix targets");

pub mod names;
pub mod presence;
