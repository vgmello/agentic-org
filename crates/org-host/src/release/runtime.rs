//! A privately constructed complete pin held for one host lifetime. This is a
//! publisher/image check; account and organization authorization remain separate.
use super::{files::Directory, store::ReleaseStore, Error, ReleasePin};
use org_release::VerifiedManifest;
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
        #[cfg(target_os = "macos")]
        if org_socket::image::own_loaded_cdhash()? != self.cdhash("agentic-org")? {
            return Err(Error::PlatformAdmission);
        }
        #[cfg(not(target_os = "macos"))]
        return Err(Error::UnsupportedTarget);
        #[cfg(target_os = "macos")]
        Ok(())
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
    pub(crate) fn cdhash(&self, name: &str) -> Result<[u8; 20], Error> {
        self.0
            .manifest
            .file(name)
            .and_then(|f| f.macos_cdhash())
            .copied()
            .ok_or(Error::PlatformAdmission)
    }
    /// Release identity the host binds into the web process's first line.
    pub(crate) fn web_binding(&self) -> org_host_control::gateway::WebRelease {
        let pin = self.pin();
        org_host_control::gateway::WebRelease {
            release_id: pin.release_id.clone(),
            manifest_sha256: pin.manifest_sha256.clone(),
            sequence: pin.sequence,
        }
    }
}
