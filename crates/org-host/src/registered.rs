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
        // Before anything is touched on disk: a release build without a pinned runtime
        // release refuses here.
        config.admit_runtime()?;
        let record = Journal::snapshot(&paths.base, name)
            .map_err(|e| refused("reading the setup record", e))?;
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
        // Where a release build keeps its secrets in a credential directory, it is opened
        // here, once, from the path setup recorded: checked, locked for this host alone, and
        // refused with the directory and the rule it broke. Both views open below it. No
        // custody exists before this, so a store found empty where the records say keys exist
        // stops the start before anything could make a fresh key.
        let release_directory = crate::release_store::open(
            !cfg!(debug_assertions),
            org_platform_host::credentials::protected_store(),
            crate::release_store::Facts {
                recorded: record.credentials.as_deref(),
                data_root: instance.root_descriptor(),
                // The setup record has no service mode yet: increment X5 adds it, and with it
                // the foreground mode of a container, where the directory must be a mount.
                foreground: false,
                expect_items: crate::release_store::records_say_keys_exist(
                    &layout_paths.office_home,
                )
                .map_err(|source| HostError::Io {
                    context: "reading the office folders",
                    source,
                })?,
            },
        )
        .map_err(|why| refused("opening the credential directory", why))?;
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
        let installed = host.service.install_admin(Arc::new(admin));
        debug_assert!(installed, "the administrator service is installed once");
        let named = host.service.install_instance_name(&instance_name);
        debug_assert!(named, "the instance name is installed once");
        Ok(host)
    }
}

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

#[cfg(test)]
mod tests;
