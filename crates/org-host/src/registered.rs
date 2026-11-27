//! The service host's registered start (A2, D1): `agentic-org serve --name N`.
//!
//! The host reads the frozen setup record without taking setup's writer lock, reopens the
//! instance setup reserved (its ID must be the record's), admits the runtime release before
//! any custody is opened, takes `host.lock`, creates the host registry if it is absent (the
//! first start after setup; a registry the host once created and that is now missing is
//! refused, never recreated), and hands the lock and custody to the ordinary composition. Only
//! then is the administrator link service installed: until it is, the admin calls answer
//! `unavailable`. The host never writes the setup record; setup observes the registry itself.
use std::io;
use std::path::Path;
use std::sync::Arc;

use crate::admin::{Admin, NativePresence};
use crate::browser_listener::{browser_matches_setup, published_http_notice};
use crate::home::{HostLock, Layout};
use crate::hostdb::{HostDbInit, HostStore};
use crate::instance::registry::Registry;
use crate::instance::setup::{Journal, Paths};
use crate::ports::HostStoreOpener;
use crate::startup::startup_app_policy;
use crate::{host_log, Config, Host, HostError};

fn refused(context: &'static str, why: impl std::fmt::Display) -> HostError {
    HostError::Io {
        context,
        source: io::Error::other(why.to_string()),
    }
}

impl Host {
    /// Starts the registered instance `name` with caller-supplied persistence and custody ports.
    ///
    /// Callers must supply either `ports.store` (a pre-opened [`HostStore`]) or
    /// `ports.store_opener` (for example `org-host-sqlite`'s SQLite opener), never neither and
    /// never both. Any custody field left `None` falls back to the local host custody adapter.
    pub fn start_registered_with_ports(
        config: Config,
        name: Option<&str>,
        ports: crate::ports::HostPorts,
    ) -> Result<Self, HostError> {
        let paths =
            crate::human::paths(name).map_err(|why| refused("locating the instance", why))?;
        Self::start_registered_at_with_ports(config, &paths, name, ports)
    }

    pub(crate) fn start_registered_at_with_ports(
        mut config: Config,
        paths: &Paths,
        name: Option<&str>,
        mut ports: crate::ports::HostPorts,
    ) -> Result<Self, HostError> {
        // Before any key or custody is read (this path opens custody before the ordinary
        // composition is reached).
        crate::config::harden()?;
        validate_registered_ports(&ports)?;
        let record = Journal::snapshot(&paths.base, name)
            .map_err(|e| refused("reading the setup record", e))?;
        // Before anything is touched on disk (the record was only read): a release build
        // without a pinned runtime release refuses here, and so does a development build
        // that names no custody of its own, neither a development key folder, a
        // credential directory in its record, nor caller-injected custody ports.
        let explicit_custody = (record.credentials.is_some() && keeps_secrets_in_a_directory())
            || (ports.secrets.is_some() && ports.keys.is_some());
        config.admit_runtime_with(explicit_custody)?;
        let (instance, browser) = reopen_reserved_instance(paths, name, &record)?;
        let policy = startup_app_policy(&config, true)?;
        let layout_paths = instance
            .prepare_layout()
            .map_err(|e| refused("preparing the instance layout", e))?;
        let layout = Layout::with_runtime(&layout_paths.office_home, instance.temp_folder());
        layout.check_socket_paths()?;
        layout.prepare()?;
        let lock = HostLock::acquire(&layout)?;
        let (custody, release_directory) = open_registered_custody_and_store(
            &config,
            paths,
            &record,
            &instance,
            &layout_paths.office_home,
            &mut ports,
        )?;
        let instance_name = instance.name().to_owned();
        // An `https` instance imports the pair setup verified at its first start.
        config.supplied_tls = record.tls.clone();
        config.federation = record.input.federation.clone();
        let host = Self::start_with(
            config,
            Some(instance),
            Some((lock, custody)),
            release_directory,
            ports,
        )?;
        install_registered_admin(&host, &record, &browser, policy, &instance_name)?;
        Ok(host)
    }
}

fn validate_registered_ports(ports: &crate::ports::HostPorts) -> Result<(), HostError> {
    match (&ports.store, &ports.store_opener) {
        (Some(_), Some(_)) => Err(refused(
            "opening the host registry",
            "supply either HostPorts::store or HostPorts::store_opener, not both",
        )),
        (None, None) => Err(refused(
            "opening the host registry",
            "no HostStore or HostStoreOpener configured; compose with org-host-sqlite or inject HostPorts::store",
        )),
        _ => Ok(()),
    }
}

fn reopen_reserved_instance(
    paths: &Paths,
    name: Option<&str>,
    record: &crate::instance::setup::Record,
) -> Result<
    (
        crate::instance::registry::Instance,
        org_host_control::listen::ListenConfig,
    ),
    HostError,
> {
    let id = record.instance_id.clone().ok_or_else(|| {
        refused(
            "reading the setup record",
            "setup has not reserved an instance yet",
        )
    })?;
    let registry = Registry::open(&paths.locator, &paths.user_temp)
        .map_err(|e| refused("opening the instance locator", e))?;
    registry
        .restore_temp_folders()
        .map_err(|e| refused("restoring instance temp folders", e))?;
    // Read-only first, so a name setup never reserved is refused instead of reserved here.
    let (located, _) = registry
        .locate(name, &record.input.root)
        .map_err(|e| refused("locating the reserved instance", e))?;
    if located != id {
        return Err(refused(
            "locating the reserved instance",
            "the reserved instance is not the one setup recorded",
        ));
    }
    let instance = registry
        .reserve(name, &record.input.root)
        .map_err(|e| match e {
            // Another process holds the instance's state lock: its service host, setup or
            // `admin reset`. Name that, not the lock file.
            crate::instance::Error::Busy(_) => refused(
                "reopening the reserved instance",
                format!(
                    "instance {} is in use by another agentic-org process (a running service, \
                     setup or admin reset); stop it first",
                    name.unwrap_or("default")
                ),
            ),
            e => refused("reopening the reserved instance", e),
        })?;
    if instance.id() != id {
        return Err(refused(
            "reopening the reserved instance",
            "the reserved instance is not the one setup recorded",
        ));
    }
    let browser = instance
        .browser()
        .map_err(|e| refused("reading the instance listener", e))?
        .filter(|active| browser_matches_setup(active, &record.input.listen))
        .ok_or_else(|| {
            refused(
                "reading the instance listener",
                "the instance listener is missing or differs from setup",
            )
        })?;
    // Said once per start, before anything is bound, using the active listener (omitted
    // once an http_published instance has opted in to HTTPS).
    if browser.scheme() == org_host_control::listen::ListenMode::HttpPublished {
        host_log::log(&published_http_notice(&browser));
    }
    Ok((instance, browser))
}

fn install_registered_admin(
    host: &Host,
    record: &crate::instance::setup::Record,
    browser: &org_host_control::listen::ListenConfig,
    policy: crate::app_auth::AppPolicy,
    instance_name: &str,
) -> Result<(), HostError> {
    let authentication = host.service.authentication.as_ref().ok_or_else(|| {
        refused(
            "opening the host registry",
            "the registered host has no registry",
        )
    })?;
    let admin = Admin::new(
        authentication.accounts_store(),
        authentication.links_store(),
        &record.setup_operation,
        browser.origin().clone(),
        policy,
        Arc::new(NativePresence),
    );
    if record.input.service == crate::instance::setup::Service::Foreground {
        admin.foreground_instance();
    }
    let installed = host.service.install_admin(Arc::new(admin));
    debug_assert!(installed, "the administrator service is installed once");
    let named = host.service.install_instance_name(instance_name);
    debug_assert!(named, "the instance name is installed once");
    Ok(())
}

fn open_registered_custody_and_store(
    config: &Config,
    paths: &Paths,
    record: &crate::instance::setup::Record,
    instance: &crate::instance::registry::Instance,
    office_home: &Path,
    ports: &mut crate::ports::HostPorts,
) -> Result<
    (
        Arc<dyn crate::host_secrets::HostSecrets>,
        Option<org_platform_host::credentials::ReleaseRoot>,
    ),
    HostError,
> {
    let opener = if ports.store.is_none() {
        ports.store_opener.take()
    } else {
        None
    };
    let uses_local_custody = ports.secrets.is_none() || ports.keys.is_none();
    // Before any custody is opened: a persisted registry the host once created and that is now
    // missing is refused, never recreated empty (ADR-0022).
    let reg_state = match &opener {
        Some(opener) => Some(registry_state_with(instance, office_home, &**opener)?),
        None => None,
    };
    #[cfg(debug_assertions)]
    let dev_key_dir = config.dev_key_dir.clone();
    #[cfg(not(debug_assertions))]
    let dev_key_dir: Option<std::path::PathBuf> = None;
    let foreground = record.input.service == crate::instance::setup::Service::Foreground;
    let release_directory = if uses_local_custody {
        open_local_release_directory(
            paths,
            record,
            instance,
            office_home,
            foreground,
            dev_key_dir.as_deref(),
        )?
    } else {
        None
    };
    let custody = match ports.secrets.take() {
        Some(injected) => injected,
        None => crate::host_secrets::store_for(
            dev_key_dir.as_deref(),
            instance.root(),
            instance.id(),
            crate::code_signing::own_signing,
            release_directory.as_ref(),
        )
        .map_err(|e| refused("opening protected account custody", format!("{e:?}")))?,
    };
    if let (Some(opener), Some(state)) = (opener, reg_state) {
        let store = match state {
            RegistryState::Absent => {
                create_registry_with(office_home, &*custody, instance.id(), &*opener)?
            }
            RegistryState::Present => open_registry_with(office_home, &*custody, &*opener)?,
        };
        // Written after the registry is published, and repaired here if a start died between
        // the two: from now on a missing registry is refused.
        instance
            .record_registry_created()
            .map_err(|e| refused("recording the host registry", e))?;
        ports.store = Some(store);
    }
    Ok((custody, release_directory))
}

fn open_local_release_directory(
    paths: &Paths,
    record: &crate::instance::setup::Record,
    instance: &crate::instance::registry::Instance,
    office_home: &Path,
    foreground: bool,
    dev_key_dir: Option<&Path>,
) -> Result<Option<org_platform_host::credentials::ReleaseRoot>, HostError> {
    // On a system that keeps a host's secrets in a directory, a foreground instance's
    // record names its secrets volume: setup refuses a file without one. A record without
    // one (written by hand, or by an earlier build) does not start, in any build: the
    // volume's checks below would be skipped and the keys kept wherever the build says.
    if foreground && record.credentials.is_none() && keeps_secrets_in_a_directory() {
        return Err(refused(
            "reading the setup record",
            FOREGROUND_WITHOUT_VOLUME,
        ));
    }
    let records = open_directory(&paths.base).map_err(|source| HostError::Io {
        context: "opening the account's records",
        source,
    })?;
    let release_directory = crate::credential_store_root::open(
        !cfg!(debug_assertions),
        org_platform_host::credentials::protected_store(),
        crate::credential_store_root::Facts {
            instance_id: instance.id(),
            recorded: record.credentials.as_deref(),
            data_root: instance.root_descriptor(),
            foreground,
            expect_items: crate::credential_store_root::records_say_keys_exist(office_home)
                .map_err(|source| HostError::Io {
                    context: "reading the office folders",
                    source,
                })?,
            records: Some(std::os::fd::AsFd::as_fd(&records)),
        },
    )
    .map_err(|why| refused("opening the credential directory", why))?;
    drop(records);
    if let (None, Some(dir)) = (&release_directory, dev_key_dir) {
        use std::os::unix::fs::DirBuilderExt;
        match std::fs::DirBuilder::new().mode(0o700).create(dir) {
            Err(source) if source.kind() != io::ErrorKind::AlreadyExists => {
                return Err(HostError::Io {
                    context: "making the development key folder",
                    source,
                });
            }
            _ => {}
        }
    }
    Ok(release_directory)
}

/// The message a foreground start refuses with when its record names no secrets volume on a
/// system that keeps a host's secrets in a directory.
pub(crate) const FOREGROUND_WITHOUT_VOLUME: &str = "this foreground instance's setup record \
     names no credential directory, and this system keeps a host's secrets in one (the \
     mounted secrets volume); the instance does not start; nothing was written";

/// The message a start refuses with when the registry it once created is gone.
pub const REGISTRY_MISSING: &str = "host registry missing: restore host-registry.db \
     from a backup of this instance; starting over without one needs the instance removed, \
     which uninstall does not offer yet (FC-022)";

#[derive(Debug, PartialEq, Eq)]
pub enum RegistryState {
    /// The first start: neither the registry nor the host's marker exists.
    Absent,
    /// The registry exists (its marker may still need writing).
    Present,
}

/// Decides whether this start creates the registry. The caller holds the instance root lock
/// and `host.lock`, so no other host can race these checks. A registry recorded as created
/// but now missing is refused with an explicit recovery message: recreating it empty would
/// reopen the initial administrator link and drop the accounts and custody bound to it.
pub(crate) fn registry_state_with(
    instance: &crate::instance::registry::Instance,
    office_home: &Path,
    opener: &dyn HostStoreOpener,
) -> Result<RegistryState, HostError> {
    let exists = opener.exists(office_home).map_err(|source| HostError::Io {
        context: "checking the host registry",
        source,
    })?;
    let created = instance
        .registry_created()
        .map_err(|e| refused("checking the host registry", e))?;
    match (exists, created) {
        (true, _) => Ok(RegistryState::Present),
        (false, false) => Ok(RegistryState::Absent),
        (false, true) => {
            host_log::log(REGISTRY_MISSING);
            Err(refused("opening the host registry", REGISTRY_MISSING))
        }
    }
}

/// Creates the host registry at the instance's first start, under the same locks.
pub(crate) fn create_registry_with(
    office_home: &Path,
    custody: &dyn crate::host_secrets::HostSecrets,
    instance_id: &str,
    opener: &dyn HostStoreOpener,
) -> Result<Arc<dyn HostStore>, HostError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|n| i64::try_from(n.as_millis()).ok())
        .ok_or_else(|| refused("creating the host registry", "the clock is unavailable"))?;
    let init = HostDbInit {
        instance_id: instance_id.to_owned(),
        clock_high_ms: now,
    };
    // The error names only a kind (an I/O kind, storage fault or custody class), never bytes.
    let store = opener
        .create(office_home, custody, &init)
        .map_err(|e| refused("creating the host registry", format!("{e:?}")))?;
    host_log::log("created the host registry at the instance's first start");
    Ok(store)
}

fn open_registry_with(
    office_home: &Path,
    custody: &dyn crate::host_secrets::HostSecrets,
    opener: &dyn HostStoreOpener,
) -> Result<Arc<dyn HostStore>, HostError> {
    opener
        .open(office_home, custody)
        .map_err(|error| match error {
            crate::hostdb::Error::Outdated => refused(
                "opening protected account registry",
                crate::hostdb::sessions::OUTDATED,
            ),
            _ => refused(
                "opening protected account registry",
                "protected registry unavailable",
            ),
        })
}

/// Whether this system keeps a host's secrets in a directory (GNU/Linux), where a recorded
/// credential directory is opened in every build.
fn keeps_secrets_in_a_directory() -> bool {
    org_platform_host::credentials::protected_store()
        == Ok(org_platform_host::credentials::ProtectedStore::ReleaseDirectory)
}

/// The directory at `path`, held open to be compared: never a file, and not opened for
/// anything else.
fn open_directory(path: &Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY)
        .open(path)
}

#[cfg(test)]
mod tests;
