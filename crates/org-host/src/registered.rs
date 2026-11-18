//! The service host's registered start (A2, D1): `agentic-org serve --name N`.
//!
//! The host reads the frozen setup record without taking setup's writer lock, reopens the
//! instance setup reserved (its ID must be the record's), admits the runtime release before
//! any custody is opened, takes `host.lock`, creates the host registry if it is absent (the
//! first start after setup), and hands the lock and custody to the ordinary composition. Only
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
            .map_err(|e| refused("reopening the reserved instance", e))?;
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
        #[cfg(debug_assertions)]
        let dev_key_dir = config.dev_key_dir.clone();
        #[cfg(not(debug_assertions))]
        let dev_key_dir: Option<std::path::PathBuf> = None;
        let custody = crate::host_secrets::store_for(
            dev_key_dir.as_deref(),
            instance.root(),
            instance.id(),
            crate::code_signing::own_signing,
        )
        .map_err(|_| refused("opening protected account custody", "custody unavailable"))?;
        create_registry_once(&layout_paths.office_home, &*custody, instance.id())?;

        let host = Self::start_prepared(config, Some(instance), Some((lock, custody)))?;
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
        Ok(host)
    }
}

/// Creates the host registry at the instance's first start. The caller holds `host.lock`, so
/// no other host can race the existence check. An existing registry is opened later as usual.
fn create_registry_once(
    office_home: &Path,
    custody: &dyn crate::host_secrets::HostSecrets,
    instance_id: &str,
) -> Result<bool, HostError> {
    match std::fs::symlink_metadata(office_home.join("host-registry.db")) {
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(HostError::Io {
                context: "checking the host registry",
                source,
            })
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|n| i64::try_from(n.as_millis()).ok())
        .ok_or_else(|| refused("creating the host registry", "the clock is unavailable"))?;
    let init = HostDbInit {
        instance_id: instance_id.to_owned(),
        clock_high_ms: now,
    };
    HostDb::create(office_home, custody, &init)
        .map_err(|_| refused("creating the host registry", "registry creation failed"))?;
    host_log::log("created the host registry at the instance's first start");
    Ok(true)
}

#[cfg(test)]
mod tests;
