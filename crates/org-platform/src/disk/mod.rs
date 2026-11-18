//! Whether the volume holding a path is encrypted at rest (ADR-0016, ENC-3).
//!
//! Agentic Org does not encrypt its data; setup reports this state and never refuses on
//! it. `Unknown` carries the reason so the report can say why it could not tell.
#[cfg(target_os = "macos")]
mod macos;
#[cfg_attr(target_os = "macos", allow(dead_code))]
mod refused;

#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(not(target_os = "macos"))]
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

#[cfg(test)]
mod tests {
    #[test]
    fn refusing_adapter_names_the_capability() {
        let Err(crate::Refused::Unsupported { capability, .. }) = super::refused::disk_encryption()
        else {
            panic!("the refusing adapter must refuse");
        };
        assert_eq!(capability, "disk encryption");
    }
}
