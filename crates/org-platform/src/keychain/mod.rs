//! Reading a generic password from the user's keychain: a credential another program
//! saved there, such as a vendor tool's sign-in.
//!
//! The consumer keeps the policy: which item (`service`), how many bytes a valid answer may
//! have, how long to wait, and how to parse what comes back. This module only asks the
//! system, and hands the answer over as [`SecretBytes`], which are never printed and are
//! wiped when dropped.
//!
//! # Systems
//!
//! - macOS: `/usr/bin/security find-generic-password -s <service> -w`, with the caller's
//!   environment (the lookup needs the user's own keychain search list). Its output is
//!   read while it runs, so an answer longer than the pipe can hold never stalls it.
//! - Every other Unix target refuses ([`Refused`]), GNU/Linux included, until it has an
//!   adapter.
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as native;

#[cfg_attr(
    target_os = "macos",
    allow(
        dead_code,
        reason = "refusing fallback module is used on unsupported targets and in unit tests"
    )
)]
mod refused;
#[cfg(not(target_os = "macos"))]
use refused as native;

#[cfg_attr(
    not(target_os = "macos"),
    allow(
        dead_code,
        reason = "only the macOS adapter runs a lookup program; its unit tests run everywhere"
    )
)]
mod bounded;

#[cfg(all(test, target_os = "macos"))]
mod adapter_tests;
#[cfg(all(test, not(target_os = "macos")))]
mod refused_target_tests;
#[cfg(test)]
mod tests;

use crate::Refused;
use std::{
    fmt,
    sync::atomic::{compiler_fence, Ordering},
    time::Duration,
};

/// The password of the user's generic-password keychain item for `service`.
///
/// - `Ok(Some(bytes))`: the password exactly as the system printed it (on macOS followed by
///   a newline, which is kept: the caller parses).
/// - `Ok(None)`: no answer. The item is missing or its keychain is locked, the lookup
///   failed or could not start, it took longer than `timeout` (it is then killed), or the
///   answer was longer than `max_bytes`. A long answer is never cut short to fit.
/// - `Err(Refused)`: this build target has no keychain adapter.
///
/// `max_bytes + 1` bytes are reserved before the lookup starts, so the answer never moves
/// in memory (a move would leave a copy that is not wiped). Blocks for at most about
/// `timeout`.
pub fn find_generic_password(
    service: &str,
    max_bytes: usize,
    timeout: Duration,
) -> Result<Option<SecretBytes>, Refused> {
    native::find_generic_password(service, max_bytes, timeout)
}

/// The bytes of a secret. [`fmt::Debug`] prints `<redacted>`, and dropping them overwrites
/// every byte the buffer holds with zeros, spare capacity included. [`SecretBytes::as_slice`]
/// lends them out; nothing hands over an owned copy.
pub struct SecretBytes {
    bytes: Vec<u8>,
}

impl SecretBytes {
    /// The secret's bytes.
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    /// Empty, with room for `capacity` bytes reserved up front, so that filling it with
    /// [`SecretBytes::extend_within_capacity`] never moves the bytes. `None` when the memory
    /// cannot be reserved.
    pub(crate) fn with_capacity(capacity: usize) -> Option<Self> {
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(capacity).ok()?;
        Some(Self { bytes })
    }

    /// `len` zero bytes, for a scratch buffer that is wiped when dropped.
    pub(crate) fn zeroed(len: usize) -> Self {
        Self {
            bytes: vec![0; len],
        }
    }

    /// Appends as much of `more` as the reserved capacity still holds, without ever
    /// reallocating, and returns how many bytes it took.
    pub(crate) fn extend_within_capacity(&mut self, more: &[u8]) -> usize {
        let room = self.bytes.capacity() - self.bytes.len();
        let taken = more.len().min(room);
        self.bytes.extend_from_slice(&more[..taken]);
        taken
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.bytes
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        wipe(&mut self.bytes);
    }
}

/// Overwrites every byte `bytes` holds, spare capacity included, with zeros, and makes the
/// optimizer assume the zeros are read, so it keeps the stores although the buffer is about
/// to be freed (best effort). Without `ptr::write_volatile`: the workspace keeps that to its
/// two zeroing primitives (`tests/rust_hygiene.rs`), and this crate links neither.
fn wipe(bytes: &mut Vec<u8>) {
    bytes.fill(0);
    // Within capacity, so this writes zeros over the spare bytes without reallocating.
    bytes.resize(bytes.capacity(), 0);
    std::hint::black_box(bytes.as_slice());
    compiler_fence(Ordering::SeqCst);
}
