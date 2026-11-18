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
use crate::instance::setup::{Input, Journal, Paths};

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
        root: root.clone(),
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
