//! Whether the volume holding a path is encrypted at rest (ADR-0016, ENC-3).
//!
//! Agentic Org does not encrypt its data; setup reports this state and never refuses on
//! it. `Unknown` carries the reason so the report can say why it could not tell.
//!
//! # Systems
//!
//! - macOS: FileVault, through `diskutil info -plist` for the volume.
//! - GNU/Linux: dm-crypt (LUKS) under the filesystem, read from `/proc/self/mountinfo`
//!   and `/sys/dev/block` without privilege. Encryption the kernel does not show as a
//!   device-mapper target (the drive's own, a cloud provider's, fscrypt on a folder) is
//!   not seen; what cannot be followed to a block device is `Unknown`.
//! - Every other Unix target refuses.
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod linux;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use linux as imp;

#[cfg_attr(
    any(target_os = "macos", all(target_os = "linux", target_env = "gnu")),
    allow(
        dead_code,
        reason = "refusing fallback module is used on unsupported targets and in unit tests"
    )
)]
mod refused;
#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
use refused as imp;

use crate::Refused;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncryptionState {
    On,
    Off,
    Unknown(String),
}

pub trait DiskEncryption: Send + Sync {
    /// The state of the volume that holds `path`, or of its nearest existing ancestor
    /// when `path` does not exist yet.
    fn state(&self, path: &Path) -> EncryptionState;
}

/// The build target's disk-encryption probe.
pub fn disk_encryption() -> Result<Box<dyn DiskEncryption>, Refused> {
    imp::disk_encryption()
}

/// What the build target calls the encryption its probe reports, for a report's wording.
/// The consumer keeps the sentences; these are the two system-specific parts of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wording {
    /// The facility's name, as in "Disk encryption (FileVault): on."
    pub name: &'static str,
    /// What a person turns on or checks, as in "turn on <setting>." and "check <setting>."
    pub setting: &'static str,
}

/// The build target's [`Wording`]. A target without a probe has neutral words; its probe
/// refuses before any report is written.
pub fn wording() -> Wording {
    imp::WORDING
}

#[cfg(test)]
mod tests {
    #[test]
    fn refusing_adapter_names_the_capability() {
        let Err(crate::Refused::Unsupported { capability, .. }) = super::refused::disk_encryption()
        else {
            panic!("the refusing adapter must refuse");
        };
        assert_eq!(capability, "disk encryption");
        assert!(!super::refused::WORDING.name.is_empty());
        assert!(!super::wording().setting.is_empty());
    }
}
