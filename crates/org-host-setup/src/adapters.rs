//! The platform adapters setup composes and the runtime release it pins (step 1).

use std::path::{Path, PathBuf};

use org_platform::disk::DiskEncryption;
use org_platform::service::ServiceManager;

use org_host::release::ReleasePin;

/// The platform adapters setup composes.
pub struct Platform {
    pub service: Box<dyn ServiceManager>,
    pub disk: Box<dyn DiskEncryption>,
    pub credentials: CredentialDirectory,
}

/// Where the host of the instance being set up keeps its secrets, as far as setup has to
/// provide it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CredentialDirectory {
    /// Not in a directory setup makes: the Keychain (a development build's key folder in
    /// its place, [`Request::development_keys`](super::Request::development_keys)). The record names none.
    Unused,
    /// In `<base>/<instance id>`: setup makes it (mode 700, with the folders of `base`
    /// that are missing), checks it and records it. `base` is absolute.
    Below(PathBuf),
}

impl CredentialDirectory {
    /// What a host keeps its secrets in on this system, for the account whose home is
    /// `home`: the system's protected store, which is a directory on Linux
    /// (`${XDG_STATE_HOME:-~/.local/state}/agentic-org/credentials`, read here and never
    /// again) and the Keychain on macOS. In every build: a development build on Linux keeps
    /// its secrets in the credential directory its record names too, and needs no
    /// development key folder (X5 review V-a); on macOS it keeps them in that folder.
    pub fn for_host(home: &Path) -> Result<Self, String> {
        use org_platform_host::credentials::{protected_store, ProtectedStore};
        match protected_store().map_err(|e| e.to_string())? {
            ProtectedStore::Keychain => Ok(Self::Unused),
            ProtectedStore::ReleaseDirectory => org_platform::dirs::credential_directory_base(home)
                .map(Self::Below)
                .map_err(|e| e.to_string()),
        }
    }
}

/// The release this setup records. Development builds record a fixed development pin; a
/// release build admits the currently installed release in the account's protected release
/// store (`default_release_store_dir`) and records its verified pin, releasing the store lock
/// before the service host starts.
pub(crate) fn release_pin() -> Result<ReleasePin, String> {
    if cfg!(debug_assertions) {
        use sha2::Digest;
        let digest = sha2::Sha256::digest(
            format!("agentic-org development {}", env!("CARGO_PKG_VERSION")).as_bytes(),
        );
        Ok(ReleasePin {
            release_id: "development".into(),
            sequence: 1,
            manifest_sha256: digest.iter().map(|b| format!("{b:02x}")).collect(),
        })
    } else {
        let store_dir = org_host::default_release_store_dir()?;
        let store = org_host::release::ReleaseStore::open(&store_dir).map_err(|e| {
            format!(
                "opening release store ({}): {e}; install a signed release first with `agentic-org release install <bundle.tar.gz>`; nothing was changed",
                store_dir.display()
            )
        })?;
        let admitted = store.admit_self().map_err(|e| {
            format!(
                "admitting installed release ({}): {e}; run the installed binary from the release store or install with `agentic-org release install <bundle.tar.gz>`; nothing was changed",
                store_dir.display()
            )
        })?;
        Ok(admitted.pin().clone())
    }
}

/// The platform's own adapters: the account's service manager and disk-encryption probe.
/// Systems without them refuse here, before anything is changed. `foreground` composes
/// the manager that registers nothing, which every system has.
pub fn platform(foreground: bool) -> Result<Platform, String> {
    let home = org_host::home::account_dir().ok_or("the account home folder is unavailable")?;
    Ok(Platform {
        service: service_manager(foreground)?,
        disk: org_platform::disk::disk_encryption().map_err(|e| e.to_string())?,
        credentials: CredentialDirectory::for_host(&home)?,
    })
}

/// The account's service manager alone, as [`platform`] composes it: what `release install`
/// needs to move the registered services, so a disk-encryption probe or a credential folder
/// that cannot be determined never stops that.
pub fn service_manager(foreground: bool) -> Result<Box<dyn ServiceManager>, String> {
    use org_platform::service::ServiceMode;
    let home = org_host::home::account_dir().ok_or("the account home folder is unavailable")?;
    let mode = if foreground {
        ServiceMode::Foreground
    } else {
        ServiceMode::Registered
    };
    org_platform::service::service(mode, &home).map_err(|e| e.to_string())
}
