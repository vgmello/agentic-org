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
use crate::home::{HostLock, Layout};
use crate::hostdb::{HostDb, HostDbInit};
use crate::instance::registry::Registry;
use crate::instance::setup::{Journal, Paths};
use crate::{host_log, startup_app_policy, Config, Host, HostError};

fn refused(context: &'static str, why: impl std::fmt::Display) -> HostError {
    HostError::Io {
        context,
        source: io::Error::other(why.to_string()),
    }
}

impl Host {
    /// Starts the registered instance `name` (the default instance when `None`) that setup
    /// reserved for this account. Fails closed where the platform has no instance locations
    /// or protected custody.
    pub fn start_registered(config: Config, name: Option<&str>) -> Result<Self, HostError> {
        let paths =
            crate::human::paths(name).map_err(|why| refused("locating the instance", why))?;
        Self::start_registered_at(config, &paths, name)
    }

    pub(crate) fn start_registered_at(
        mut config: Config,
        paths: &Paths,
        name: Option<&str>,
    ) -> Result<Self, HostError> {
        // Before any key or custody is read (this path opens custody before the ordinary
        // composition is reached).
        crate::harden()?;
        let record = Journal::snapshot(&paths.base, name)
            .map_err(|e| refused("reading the setup record", e))?;
        // Before anything is touched on disk (the record was only read): a release build
        // without a pinned runtime release refuses here, and so does a development build
        // that names no custody of its own, neither a development key folder nor a
        // credential directory in its record.
        config
            .admit_runtime_with(record.credentials.is_some() && keeps_secrets_in_a_directory())?;
        // The record was validated as it was read: a listener that is plain HTTP on a
        // published port with a service mode other than foreground (a record edited by
        // hand) never comes this far. Said once per start, before anything is bound.
        if record.input.listen.scheme() == org_host_control::listen::ListenMode::HttpPublished {
            host_log::log(&crate::setup_flow::published_http_notice(
                &record.input.listen,
            ));
        }
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
        if instance
            .browser()
            .map_err(|e| refused("reading the instance listener", e))?
            .as_ref()
            != Some(&record.input.listen)
        {
            return Err(refused(
                "reading the instance listener",
                "the instance listener is missing or differs from setup",
            ));
        }
        let policy = startup_app_policy(&config, true)?;
        let layout_paths = instance
            .prepare_layout()
            .map_err(|e| refused("preparing the instance layout", e))?;
        let layout = Layout::with_runtime(&layout_paths.office_home, instance.temp_folder());
        layout.check_socket_paths()?;
        layout.prepare()?;
        let lock = HostLock::acquire(&layout)?;
        // Before any custody is opened: a registry the host once created and that is now
        // missing is refused, never recreated empty (ADR-0022).
        let registry_state = registry_state(&instance, &layout_paths.office_home)?;
        #[cfg(debug_assertions)]
        let dev_key_dir = config.dev_key_dir.clone();
        #[cfg(not(debug_assertions))]
        let dev_key_dir: Option<std::path::PathBuf> = None;
        let foreground = record.input.service == crate::instance::setup::Service::Foreground;
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
        // The account's records, held to be compared with the credential directory: it must
        // not be in their volume either (a data folder mounted inside the data volume).
        let records = open_directory(&paths.base).map_err(|source| HostError::Io {
            context: "opening the account's records",
            source,
        })?;
        // Where this system keeps a host's secrets in a directory and the record names one, it
        // is opened here, once, in every build: checked (in the foreground mode as a mounted
        // volume), locked for this host alone, and refused with the directory and the rule it
        // broke. Both views open below it, from its descriptor. No custody exists before this,
        // so a store found empty where the records say keys exist (a lost secrets volume)
        // stops the start before anything could make a fresh key (X5 review, fourth round).
        let release_directory = crate::release_store::open(
            !cfg!(debug_assertions),
            org_platform_host::credentials::protected_store(),
            crate::release_store::Facts {
                recorded: record.credentials.as_deref(),
                data_root: instance.root_descriptor(),
                // In the foreground mode (a container) the directory is a mounted volume.
                foreground,
                expect_items: crate::release_store::records_say_keys_exist(
                    &layout_paths.office_home,
                )
                .map_err(|source| HostError::Io {
                    context: "reading the office folders",
                    source,
                })?,
                records: Some(std::os::fd::AsFd::as_fd(&records)),
            },
        )
        .map_err(|why| refused("opening the credential directory", why))?;
        drop(records);
        if let (None, Some(dir)) = (&release_directory, &dev_key_dir) {
            // A development build whose record names no credential directory keeps its keys
            // in the folder its environment names: made if it is missing, never its parent.
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
        let custody = crate::host_secrets::store_for(
            dev_key_dir.as_deref(),
            instance.root(),
            instance.id(),
            crate::code_signing::own_signing,
            release_directory.as_ref(),
        )
        .map_err(|e| refused("opening protected account custody", format!("{e:?}")))?;
        if registry_state == RegistryState::Absent {
            create_registry(&layout_paths.office_home, &*custody, instance.id())?;
        }
        // Written after the registry is published, and repaired here if a start died between
        // the two: from now on a missing registry is refused.
        instance
            .record_registry_created()
            .map_err(|e| refused("recording the host registry", e))?;

        let instance_name = instance.name().to_owned();
        let host = Self::start_with(
            config,
            Some(instance),
            Some((lock, custody)),
            release_directory,
        )?;
        let db = match &host.service.authentication {
            Some(authentication) => authentication.provisioning_storage().0,
            None => {
                return Err(refused(
                    "opening the host registry",
                    "the registered host has no registry",
                ))
            }
        };
        let admin = Admin::new(
            db,
            &record.setup_operation,
            record.input.listen.origin().clone(),
            policy,
            Arc::new(NativePresence),
        );
        if foreground {
            admin.foreground_instance();
        }
        let installed = host.service.install_admin(Arc::new(admin));
        debug_assert!(installed, "the administrator service is installed once");
        let named = host.service.install_instance_name(&instance_name);
        debug_assert!(named, "the instance name is installed once");
        Ok(host)
    }
}

/// The message a foreground start refuses with when its record names no secrets volume on a
/// system that keeps a host's secrets in a directory.
pub(crate) const FOREGROUND_WITHOUT_VOLUME: &str = "this foreground instance's setup record \
     names no credential directory, and this system keeps a host's secrets in one (the \
     mounted secrets volume); the instance does not start; nothing was written";

/// The message a start refuses with when the registry it once created is gone.
pub(crate) const REGISTRY_MISSING: &str = "host registry missing: restore host-registry.db \
     from a backup of this instance; starting over without one needs the instance removed, \
     which uninstall does not offer yet (FC-022)";

#[derive(Debug, PartialEq, Eq)]
enum RegistryState {
    /// The first start: neither the registry nor the host's marker exists.
    Absent,
    /// The registry exists (its marker may still need writing).
    Present,
}

/// Decides whether this start creates the registry. The caller holds the instance root lock
/// and `host.lock`, so no other host can race these checks. A registry recorded as created
/// but now missing is refused with an explicit recovery message: recreating it empty would
/// reopen the initial administrator link and drop the accounts and custody bound to it.
fn registry_state(
    instance: &crate::instance::registry::Instance,
    office_home: &Path,
) -> Result<RegistryState, HostError> {
    let exists = match std::fs::symlink_metadata(office_home.join("host-registry.db")) {
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(source) => {
            return Err(HostError::Io {
                context: "checking the host registry",
                source,
            })
        }
    };
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
fn create_registry(
    office_home: &Path,
    custody: &dyn crate::host_secrets::HostSecrets,
    instance_id: &str,
) -> Result<(), HostError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|n| i64::try_from(n.as_millis()).ok())
        .ok_or_else(|| refused("creating the host registry", "the clock is unavailable"))?;
    let init = HostDbInit {
        instance_id: instance_id.to_owned(),
        clock_high_ms: now,
    };
    // The error names only a kind (an I/O kind, SQLite code or custody class), never bytes.
    HostDb::create(office_home, custody, &init)
        .map_err(|e| refused("creating the host registry", format!("{e:?}")))?;
    host_log::log("created the host registry at the instance's first start");
    Ok(())
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
