//! A privately constructed complete pin held for one host lifetime. This is a
//! publisher/image check; account and organization authorization remain separate.
use super::{files::Directory, store::ReleaseStore, Error, ReleasePin};
use crate::server_check::Pin;
use org_platform_host::admission::{own_image, ExpectedImage, OwnImageError};
use org_release::{Platform, VerifiedManifest};
use std::{path::PathBuf, sync::Arc};

struct Admitted {
    store: ReleaseStore,
    manifest: VerifiedManifest,
    _tree: Directory,
}
/// Immutable runtime release with a retained protected store lock. No wire,
/// runtime-key constructor or mutable trusted manifest is exposed.
#[derive(Clone)]
pub struct RuntimeRelease(Arc<Admitted>);
impl std::fmt::Debug for RuntimeRelease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeRelease")
            .field("pin", &self.pin())
            .finish_non_exhaustive()
    }
}
impl ReleaseStore {
    /// Verify the current complete pin and this actual loaded host image before
    /// custody. The consumed store lock remains held until every clone drops.
    pub fn admit_self(mut self) -> Result<RuntimeRelease, Error> {
        let pin = self.recover()?.ok_or(Error::InvalidRelease)?;
        let tree = self.releases.output_child(&pin.release_id, false)?;
        let manifest = self.manifest_at(&tree)?;
        if super::store::ReleasePin::of(&manifest) != pin {
            return Err(Error::InvalidRelease);
        }
        let result = RuntimeRelease(Arc::new(Admitted {
            store: self,
            manifest,
            _tree: tree,
        }));
        result.check_self()?;
        Ok(result)
    }
}
impl RuntimeRelease {
    pub fn pin(&self) -> &ReleasePin {
        self.0
            .store
            .pin()
            .expect("private constructor requires a recovered pin")
    }
    pub(crate) fn check_self(&self) -> Result<(), Error> {
        self.0.store.verify_root()?;
        // The loaded image is read before the manifest's hash is required, so a build
        // target without a loaded-image query is refused as that, whatever the manifest.
        // Both identities are the signed manifest's entry for the host: the system compares
        // the one it has (the kernel's CodeDirectory hash of the loaded image, or the
        // SHA-256 of this process's executable), and an image with neither is a mismatch.
        let pin = self.image_pin("agentic-org").ok();
        let expected = ExpectedImage {
            program: self.cli_program(),
            sha256: pin.map(|pin| pin.sha256),
            macos_cdhash: pin.and_then(|pin| pin.macos_cdhash),
            macos_requirement: None,
            macos_signing: crate::code_signing::HARDENED,
            development: false,
        };
        own_image(&expected).map_err(|error| match error {
            OwnImageError::Unsupported(_) => Error::UnsupportedTarget,
            OwnImageError::Unavailable(error) => Error::from(error),
            OwnImageError::Mismatch => Error::PlatformAdmission,
        })
    }
    pub fn office_program(&self) -> PathBuf {
        self.path("agentic-org-office")
    }
    pub fn gateway_program(&self) -> PathBuf {
        self.path("agentic-org-web")
    }
    pub fn cli_program(&self) -> PathBuf {
        self.path("agentic-org")
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0
            .store
            .path
            .join("releases")
            .join(&self.pin().release_id)
            .join(name)
    }
    /// The exact image this release names for the executable `name`: the identities of its
    /// entry in the verified manifest, as plain data for the platform's admission.
    pub(crate) fn image_pin(&self, name: &str) -> Result<Pin, Error> {
        pin_of(&self.0.manifest, name)
    }
    /// Release identity the host binds into the web process's first line.
    pub(crate) fn web_binding(&self) -> org_host_control::web::WebRelease {
        let pin = self.pin();
        org_host_control::web::WebRelease {
            release_id: pin.release_id.clone(),
            manifest_sha256: pin.manifest_sha256.clone(),
            sequence: pin.sequence,
        }
    }
}

/// The pin `manifest` gives the executable `name`. The SHA-256 of the file's bytes is in
/// every entry. A release for macOS also names the CodeDirectory hash of each executable,
/// which that system's admission requires: an entry without it pins nothing there and is
/// refused here, whatever the manifest's own validation allowed. A release for Linux has no
/// such hash.
pub(super) fn pin_of(manifest: &VerifiedManifest, name: &str) -> Result<Pin, Error> {
    let entry = manifest
        .file(name)
        .filter(|entry| entry.role() == org_release::Role::Executable)
        .ok_or(Error::PlatformAdmission)?;
    let macos_cdhash = match manifest.manifest().platform() {
        Platform::Macos => Some(*entry.macos_cdhash().ok_or(Error::PlatformAdmission)?),
        Platform::Linux => None,
    };
    Ok(Pin {
        sha256: *entry.sha256(),
        macos_cdhash,
    })
}
