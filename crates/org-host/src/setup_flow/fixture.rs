//! A registered account for real-process tests (`test-support`, debug builds only). A2 owns
//! this module; it is not `crate::test_support`.
//!
//! [`registered_account`] does what setup's steps 2 and 3 do, without a service or a link:
//! under the application base `base` (pass the same path to the binary as
//! `ORG_DEV_ACCOUNT_BASE`) it freezes the setup record, reserves the instance at
//! `<base>/instances/<name>`, records the loopback listener on `port` and prepares the layout.
//! `agentic-org serve --name <name>` with `ORG_DEV_ACCOUNT_BASE=<base>` and
//! `ORG_OFFICE_DEV_KEY_DIR` then starts that instance and creates its registry.
use std::path::{Path, PathBuf};

use crate::instance::registry::Registry;
use crate::instance::setup::{Input, Journal, Paths, Service};

/// Where a registered instance's files are, for tests that wait on its sockets.
#[derive(Clone, Debug)]
pub struct RegisteredAccount {
    pub instance_id: String,
    /// The instance root (`<base>/instances/<name>`).
    pub root: PathBuf,
    /// The host's office home, `<root>/office-state`.
    pub office_home: PathBuf,
    /// The instance's runtime folder (its temp folder under `<base>/temp`), which holds
    /// `host.sock` and the office sockets.
    pub runtime: PathBuf,
}

impl RegisteredAccount {
    pub fn host_socket(&self) -> PathBuf {
        self.runtime.join("host.sock")
    }
}

/// Registers instance `name` (`None` is `default`) under `base` with a loopback listener on
/// `port`. `base` must be an absolute, private (0700) folder the test owns; keep it short, since
/// socket paths under it are limited to 103 bytes. Repeating the call with the same arguments
/// returns the same instance.
pub fn registered_account(
    base: &Path,
    name: Option<&str>,
    port: u16,
) -> Result<RegisteredAccount, String> {
    let temp = base.join("temp");
    if !temp.exists() {
        std::fs::create_dir(&temp).map_err(|e| e.to_string())?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    let paths = Paths::at(base, temp, name).map_err(|e| e.to_string())?;
    let journal = Journal::open(&paths.base, name).map_err(|e| e.to_string())?;
    let root = paths.instance_root(name).map_err(|e| e.to_string())?;
    let input = Input {
        version: 1,
        service: Service::Registered,
        root: root.clone(),
        credentials: None,
        listen: super::loopback(port)?,
    };
    let mut record = journal
        .freeze(input, false, super::release_pin()?)
        .map_err(|e| e.to_string())?;
    let registry = Registry::open(&paths.locator, &paths.user_temp).map_err(|e| e.to_string())?;
    let instance = registry.reserve(name, &root).map_err(|e| e.to_string())?;
    instance
        .set_browser(&record.input.listen)
        .map_err(|e| e.to_string())?;
    let layout = instance.prepare_layout().map_err(|e| e.to_string())?;
    if record.instance_id.is_none() {
        record.instance_id = Some(instance.id().to_owned());
        journal.write(&record).map_err(|e| e.to_string())?;
    }
    Ok(RegisteredAccount {
        instance_id: instance.id().to_owned(),
        root,
        office_home: layout.office_home,
        runtime: instance.temp_folder().to_owned(),
    })
}

/// The administrator [`seed_administrator`] made: what a browser signs in with.
#[derive(Clone, Debug)]
pub struct SeededAdministrator {
    pub login: String,
    pub password: String,
}

/// Seeds one administrator account into the host registry of instance `name` registered under
/// `base` (by [`registered_account`]), as a redeemed registration link leaves it, and returns
/// its sign-in. The registry is created first if the instance never started, exactly as its
/// first start creates it: under the instance and host locks, with the development key folder
/// `dev_key_dir` (pass the same folder to the binary as `ORG_OFFICE_DEV_KEY_DIR`) as its
/// custody. Call it while no host runs the instance.
pub fn seed_administrator(
    base: &Path,
    name: Option<&str>,
    dev_key_dir: &Path,
) -> Result<SeededAdministrator, String> {
    use crate::home::{HostLock, Layout};
    use crate::registered::{create_registry, registry_state, RegistryState};
    let paths = Paths::at(base, base.join("temp"), name).map_err(|e| e.to_string())?;
    let root = paths.instance_root(name).map_err(|e| e.to_string())?;
    let registry = Registry::open(&paths.locator, &paths.user_temp).map_err(|e| e.to_string())?;
    let instance = registry.reserve(name, &root).map_err(|e| e.to_string())?;
    let homes = instance.prepare_layout().map_err(|e| e.to_string())?;
    let layout = Layout::with_runtime(&homes.office_home, instance.temp_folder());
    layout.prepare().map_err(|e| e.to_string())?;
    let _lock = HostLock::acquire(&layout).map_err(|e| e.to_string())?;
    {
        use std::os::unix::fs::DirBuilderExt;
        match std::fs::DirBuilder::new().mode(0o700).create(dev_key_dir) {
            Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return Err(e.to_string()),
            _ => {}
        }
    }
    let custody = crate::host_secrets::store_for(
        Some(dev_key_dir),
        instance.root(),
        instance.id(),
        crate::code_signing::own_signing,
        None,
    )
    .map_err(|e| format!("{e:?}"))?;
    if registry_state(&instance, &homes.office_home).map_err(|e| e.to_string())?
        == RegistryState::Absent
    {
        create_registry(&homes.office_home, &*custody, instance.id()).map_err(|e| e.to_string())?;
    }
    instance
        .record_registry_created()
        .map_err(|e| e.to_string())?;
    let db =
        crate::hostdb::HostDb::open(&homes.office_home, &*custody).map_err(|e| format!("{e:?}"))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    let now = i64::try_from(now).map_err(|e| e.to_string())?;
    super::seed::administrator(&db, super::seed::PASSWORD, now)?;
    Ok(SeededAdministrator {
        login: super::seed::LOGIN.into(),
        password: super::seed::PASSWORD.into(),
    })
}
