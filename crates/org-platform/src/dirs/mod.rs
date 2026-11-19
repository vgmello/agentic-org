//! Where an instance's per-user state lives by default.
//!
//! The consumer composes the rest (`instances/<name>`, `setup/<name>`, `locator`).
#[cfg(target_os = "macos")]
mod macos;
#[cfg_attr(target_os = "macos", allow(dead_code))]
mod refused;

#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(not(target_os = "macos"))]
use refused as imp;

use crate::Refused;
use std::path::{Path, PathBuf};

/// The per-user application base for the account whose home is `home`.
/// The consumer validates the result as it validates every other path it opens.
pub fn application_base(home: &Path) -> Result<PathBuf, Refused> {
    imp::application_base(home)
}

#[cfg(test)]
mod tests {
    #[test]
    fn refusing_adapter_names_the_capability_and_the_system() {
        let refused = super::refused::application_base(std::path::Path::new("/home/x"));
        let crate::Refused::Unsupported { capability, os } = refused.unwrap_err();
        assert_eq!(capability, "instance locations");
        assert_eq!(os, std::env::consts::OS);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_base_is_application_support() {
        assert_eq!(
            super::application_base(std::path::Path::new("/Users/a")).unwrap(),
            std::path::Path::new("/Users/a/Library/Application Support/agentic.org")
        );
    }
}
